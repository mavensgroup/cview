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
    let _n_atoms: usize = n_atoms_str
        .trim()
        .parse()
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "Invalid atom count"))?;

    // 2. Comment Line (Try to find "Lattice=...")
    let comment = lines.next().unwrap_or(Ok(String::new()))?;

    // Default Lattice (20.0 Angstrom Identity)
    let mut lattice = [[20.0, 0.0, 0.0], [0.0, 20.0, 0.0], [0.0, 0.0, 20.0]];

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
            }
        }
    }

    // 3. Atoms
    let mut atoms = Vec::new();
    for (i, line) in lines.enumerate() {
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
        });
    }

    Ok(Structure {
        lattice,
        atoms,
        formula: "XYZ Import".to_string(),
    })
}

pub fn write(path: &str, structure: &Structure) -> io::Result<()> {
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
