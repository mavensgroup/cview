// src/io.rs
pub mod chgcar;
pub mod cif;
pub mod pdb;
pub mod poscar;
pub mod qe;
pub mod sprkkr;
pub mod xrd_exp;
pub mod xyz;

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

pub fn load_structure(path: &str) -> io::Result<Structure> {
    let p = path.to_lowercase();

    // Check extension-based formats first
    if p.ends_with(".cif") {
        return cif::parse(path);
    }
    if p.ends_with(".xyz") {
        return xyz::parse(path);
    }
    if p.ends_with(".vasp") {
        return poscar::parse(path);
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
        return poscar::parse(path);
    }
    if filename.starts_with("chgcar") {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "CHGCAR files contain volumetric data, not structures. \
             Use Analysis → Charge Density to visualize them.",
        ));
    }

    // Fallback: try POSCAR parser (most permissive for VASP-family files)
    poscar::parse(path)
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
        // Check filename for POSCAR/CONTCAR/VASP patterns
        let filename = Path::new(path)
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_lowercase();

        if p.ends_with(".vasp") || filename.starts_with("poscar") || filename.starts_with("contcar")
        {
            poscar::write(path, structure)
        } else {
            // Default fallback
            poscar::write(path, structure)
        }
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
