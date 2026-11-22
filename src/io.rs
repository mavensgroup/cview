// src/io.rs
pub mod chgcar;
pub mod cif;
pub mod lammps_dump;
pub mod magmom;
pub mod ovf;
pub mod pdb;
pub mod poscar;
pub mod qe;
pub mod sprkkr;
pub mod vasprun;
pub mod xrd_exp;
pub mod xyz;

use crate::model::trajectory::{FrameChoice, Trajectory};
use crate::model::Structure;
use std::io;
use std::path::Path;

/// SPR-KKR potentials: `.pot`, and the `.pot_new` (or `.pot_old`...) an SCF run
/// writes. VASP's POTCAR has no extension, so it is not caught here.
fn is_sprkkr_potential(lower_path: &str) -> bool {
    Path::new(lower_path)
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e == "pot" || e.starts_with("pot_"))
}

/// What opening a file gives the main view: one structure, plus all frames
/// when the file holds a relaxation or MD run.
pub struct Document {
    pub structure: Structure,
    /// Present only when the file has more than one frame.
    pub trajectory: Option<Trajectory>,
    /// Index of the frame `structure` was taken from.
    pub frame: usize,
}

/// Load `path` for display. Multi-frame files (vasprun.xml, QE relax/MD
/// output, LAMMPS dump) are read in full, and `choice` picks the frame shown.
/// An OVF file brings the rest of its numbered series (`ovf::load_document`).
pub fn load_document(path: &str, choice: FrameChoice) -> io::Result<Document> {
    if is_ovf_path(&path.to_lowercase()) {
        return ovf::load_document(path, choice);
    }
    let traj = if lammps_dump::sniff(path) {
        Some(lammps_dump::parse_trajectory(path)?)
    } else if vasprun::sniff(path) {
        Some(vasprun::parse_trajectory(path)?)
    } else if is_qe_path(&path.to_lowercase()) {
        qe::parse_trajectory(path)?
    } else {
        None
    };
    match traj {
        Some(t) => {
            let frame = t.index_of(choice);
            let structure = t
                .structure_at(frame)
                .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "file has no frames"))?;
            let trajectory = (t.len() > 1).then_some(t);
            Ok(Document { structure, trajectory, frame })
        }
        None => load_structure(path).map(|structure| Document { structure, trajectory: None, frame: 0 }),
    }
}

fn is_ovf_path(lower: &str) -> bool {
    [".ovf", ".omf", ".ohf"].iter().any(|e| lower.ends_with(e))
}

fn is_qe_path(lower: &str) -> bool {
    [".in", ".pwi", ".qe", ".out", ".log", ".pwo"].iter().any(|e| lower.ends_with(e))
}

pub fn load_structure(path: &str) -> io::Result<Structure> {
    let p = path.to_lowercase();

    // LAMMPS dumps go by content: `.dat`, `.dump`, `.lammpstrj` or no extension.
    if lammps_dump::sniff(path) {
        return lammps_dump::parse(path);
    }

    if vasprun::sniff(path) {
        return vasprun::parse(path);
    }

    // Check extension-based formats first
    if p.ends_with(".cif") || p.ends_with(".mcif") {
        return cif::parse(path);
    }
    // OOMMF vector fields (mumax3, OOMMF, Spirit): a magnetization texture.
    if is_ovf_path(&p) {
        return ovf::parse(path);
    }
    if p.ends_with(".xyz") {
        return xyz::parse(path);
    }
    if p.ends_with(".vasp") {
        return poscar_with_moments(path);
    }
    if p.ends_with(".pdb") || p.ends_with(".ent") {
        return pdb::parse(path);
    }
    if p.ends_with(".in")
        || p.ends_with(".pwi")
        || p.ends_with(".qe")
        || p.ends_with(".out")
        || p.ends_with(".log")
    {
        return qe::parse(path);
    }
    if p.ends_with(".inp") || is_sprkkr_potential(&p) || p.ends_with(".sys") {
        return sprkkr::parse(path);
    }

    // For extensionless files, check the filename itself (case-insensitive).
    // This handles POSCAR, CONTCAR, POSCAR_relaxed, CONTCAR.1, etc.
    let filename = Path::new(path)
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_lowercase();

    if filename.starts_with("poscar") || filename.starts_with("contcar") {
        return poscar_with_moments(path);
    }
    if filename.starts_with("chgcar") {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "CHGCAR files contain volumetric data, not structures. \
             Use Analysis → Charge Density to visualize them.",
        ));
    }

    // Fallback: try POSCAR parser (most permissive for VASP-family files)
    poscar_with_moments(path)
}

/// A POSCAR/CONTCAR, with magnetic moments from the INCAR or OUTCAR in the
/// same directory when they match its atom count.
fn poscar_with_moments(path: &str) -> io::Result<Structure> {
    let mut s = poscar::parse(path)?;
    if let Some((moments, source)) = magmom::vasp_sidecar_moments(path, s.atoms.len()) {
        for (a, m) in s.atoms.iter_mut().zip(moments) {
            a.magmom = Some(m);
        }
        crate::utils::console::log_info(&format!("Magnetic moments from the {} beside the file", source));
    }
    Ok(s)
}

pub fn save_structure(path: &str, structure: &Structure) -> io::Result<()> {
    let p = path.to_lowercase();

    if p.ends_with(".cif") {
        cif::write(path, structure)
    } else if p.ends_with(".xyz") {
        xyz::write(path, structure)
    } else if p.ends_with(".pdb") || p.ends_with(".ent") {
        pdb::write(path, structure)
    } else if p.ends_with(".in") || p.ends_with(".qe") {
        qe::write(path, structure)
    } else if p.ends_with(".inp") || is_sprkkr_potential(&p) || p.ends_with(".sys") {
        sprkkr::write(path, structure)
    } else {
        // POSCAR (named, .vasp, or the fallback for anything else).
        poscar::write(path, structure)?;
        write_magmom_beside(path, structure);
        Ok(())
    }
}

/// After a POSCAR with moments is saved: put the magnetic INCAR tags in an
/// INCAR beside it when there is none, so VASP gets the same order. An
/// existing INCAR is never changed; the tags go to Structure Info instead.
fn write_magmom_beside(path: &str, structure: &Structure) {
    use crate::utils::console;
    let Some(tags) = magmom::vasp_incar_tags(structure) else { return };
    let dir = Path::new(path).parent().filter(|d| !d.as_os_str().is_empty()).unwrap_or(Path::new("."));
    let incar = dir.join("INCAR");
    if incar.exists() {
        console::log_info(&format!(
            "Not changing the existing {}; the magnetic tags for this POSCAR are in Structure Info",
            incar.display()
        ));
        console::info_report(&format!("INCAR magnetic tags for {}:\n{}", path, tags));
        return;
    }
    let text = format!(
        "# Magnetic tags written by CView to go with {}\n# (atom order as in that POSCAR). Add the rest of your settings.\n{}",
        Path::new(path).file_name().unwrap_or_default().to_string_lossy(),
        tags
    );
    match std::fs::write(&incar, text) {
        Ok(()) => console::log_info(&format!("Wrote the magnetic moments to {}", incar.display())),
        Err(e) => console::log_warn(&format!("Could not write {}: {}", incar.display(), e)),
    }
}

#[cfg(test)]
mod dispatch_tests {
    use super::is_sprkkr_potential;

    #[test]
    fn sprkkr_potentials_are_recognised_by_extension() {
        for ok in ["fealfe2.pot", "fealfe2.pot_new", "a/b/x_scf.pot_old", "x.pot_0"] {
            assert!(is_sprkkr_potential(ok), "{ok}");
        }
        for no in ["potcar", "POTCAR".to_lowercase().as_str(), "x.potato", "x.pdb", "pot", "x.pot.bak"] {
            assert!(!is_sprkkr_potential(no), "{no}");
        }
    }
}
