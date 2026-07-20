use crate::model::{Atom, Structure};
use std::fs::File;
use std::io::Write;
use std::io::{self, BufRead}; // Ensure Write is imported

pub fn parse(path: &str) -> io::Result<Structure> {
    let file = File::open(path)?;
    let reader = io::BufReader::new(file);
    let mut lines = reader.lines();

    // 1. Number of Atoms
    let n_atoms_str = lines
        .next()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "Empty XYZ file"))??;
    let n_atoms: usize = n_atoms_str
        .trim()
        .parse()
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "Invalid atom count"))?;

    // 2. Comment Line (Try to find "Lattice=...")
    let comment = lines.next().unwrap_or(Ok(String::new()))?;

    // Default Lattice (20.0 Angstrom Identity) — non-periodic
    let mut lattice = [[20.0, 0.0, 0.0], [0.0, 20.0, 0.0], [0.0, 0.0, 20.0]];
    let mut is_periodic = false;

    // Parse Extended XYZ Lattice if present
    // Format: Lattice="ax ay az bx by bz cx cy cz"
    if let Some(start) = comment.find("Lattice=\"") {
        let remainder = &comment[start + 9..];
        if let Some(end) = remainder.find('"') {
            let lat_str = &remainder[..end];
            let parts: Vec<f64> = lat_str
                .split_whitespace()
                .filter_map(|s| s.parse().ok())
                .collect();

            if parts.len() == 9 {
                lattice = [
                    [parts[0], parts[1], parts[2]],
                    [parts[3], parts[4], parts[5]],
                    [parts[6], parts[7], parts[8]],
                ];
                is_periodic = true; // Extended XYZ with explicit lattice → periodic
            }
        }
    }

    // 3. Atoms
    //
    // Respect n_atoms strictly: XYZ trajectory files concatenate multiple
    // frames, each with its own count+comment header. Reading to EOF would
    // merge frames and scale linearly with trajectory length; stopping at
    // n_atoms loads just the first frame.
    let mut atoms = Vec::with_capacity(n_atoms);
    for (i, line) in lines.enumerate() {
        if atoms.len() >= n_atoms {
            break;
        }
        let line = line?;
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 4 {
            continue;
        }

        let el = parts[0].to_string();
        let x: f64 = parts[1]
            .parse()
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "Invalid X"))?;
        let y: f64 = parts[2]
            .parse()
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "Invalid Y"))?;
        let z: f64 = parts[3]
            .parse()
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "Invalid Z"))?;

        atoms.push(Atom {
            element: el,
            position: [x, y, z],
            original_index: i,
            oxidation: None,
            occupancy: 1.0,
        });
    }

    Ok(Structure {
        lattice,
        atoms,
        formula: "XYZ Import".to_string(),
        is_periodic,
    })
}

pub fn write(path: &str, structure: &Structure) -> io::Result<()> {
    if structure.atoms.iter().any(|a| a.occupancy < 0.99) {
        crate::utils::console::log_warn(
            "XYZ format has no occupancy field — partial occupancies are discarded on export",
        );
    }
    let mut file = std::fs::File::create(path)?;

    // 1. Number of atoms
    writeln!(file, "{}", structure.atoms.len())?;

    // 2. Comment line: Write Lattice in Extended XYZ format
    // Lattice="ax ay az bx by bz cx cy cz"
    let l = structure.lattice;
    writeln!(file, "Lattice=\"{:.9} {:.9} {:.9} {:.9} {:.9} {:.9} {:.9} {:.9} {:.9}\" Properties=species:S:1:pos:R:3",
        l[0][0], l[0][1], l[0][2],
        l[1][0], l[1][1], l[1][2],
        l[2][0], l[2][1], l[2][2]
    )?;

    // 3. Atom lines
    for atom in &structure.atoms {
        writeln!(
            file,
            "{:<4} {:15.9} {:15.9} {:15.9}",
            atom.element, atom.position[0], atom.position[1], atom.position[2]
        )?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    struct TmpFile(std::path::PathBuf);
    impl TmpFile {
        fn new(contents: &str) -> Self {
            let mut p = std::env::temp_dir();
            let n = COUNTER.fetch_add(1, Ordering::Relaxed);
            p.push(format!("cview_xyz_{}_{}.xyz", std::process::id(), n));
            std::fs::write(&p, contents).unwrap();
            TmpFile(p)
        }
        fn path(&self) -> &str {
            self.0.to_str().unwrap()
        }
    }
    impl Drop for TmpFile {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }

    fn approx(a: f64, b: f64) {
        assert!((a - b).abs() < 1e-6, "{a} != {b}");
    }

    #[test]
    fn plain_xyz_is_non_periodic_with_default_box() {
        let f = TmpFile::new("2\nwater fragment\nO 0.0 0.0 0.0\nH 0.96 0.0 0.0\n");
        let s = parse(f.path()).unwrap();
        assert!(!s.is_periodic);
        assert_eq!(s.atoms.len(), 2);
        assert_eq!(s.atoms[0].element, "O");
        // Default 20 Å identity box when no Lattice= is present.
        approx(s.lattice[0][0], 20.0);
        approx(s.atoms[1].position[0], 0.96);
    }

    #[test]
    fn extended_xyz_lattice_is_periodic() {
        let f = TmpFile::new(
            "1\nLattice=\"3.0 0.0 0.0 0.0 3.0 0.0 0.0 0.0 3.0\" Properties=species:S:1:pos:R:3\n\
             Po 0.0 0.0 0.0\n",
        );
        let s = parse(f.path()).unwrap();
        assert!(s.is_periodic);
        approx(s.lattice[0][0], 3.0);
        approx(s.lattice[1][1], 3.0);
        approx(s.lattice[2][2], 3.0);
    }

    #[test]
    fn multiframe_trajectory_reads_only_first_frame() {
        // Two concatenated frames of 1 atom each; parser must stop at n_atoms.
        let f = TmpFile::new("1\nframe 1\nH 0.0 0.0 0.0\n1\nframe 2\nH 1.0 1.0 1.0\n");
        let s = parse(f.path()).unwrap();
        assert_eq!(s.atoms.len(), 1);
        approx(s.atoms[0].position[0], 0.0);
    }

    #[test]
    fn write_then_parse_roundtrips() {
        let original = Structure {
            lattice: [[4.0, 0.0, 0.0], [0.0, 4.0, 0.0], [0.0, 0.0, 4.0]],
            atoms: vec![Atom {
                element: "Fe".into(),
                position: [1.0, 2.0, 3.0],
                original_index: 0,
                oxidation: None,
                occupancy: 1.0,
            }],
            formula: String::new(),
            is_periodic: true,
        };
        let f = TmpFile::new("");
        write(f.path(), &original).unwrap();
        let s = parse(f.path()).unwrap();
        // Writer emits Lattice=, so round-trip is periodic.
        assert!(s.is_periodic);
        assert_eq!(s.atoms[0].element, "Fe");
        approx(s.atoms[0].position[2], 3.0);
    }
}
