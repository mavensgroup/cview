// src/model/trajectory.rs
//
// A sequence of configurations of the same atoms: a geometry optimisation
// (vasprun.xml, QE relax/vc-relax output) or an MD run (LAMMPS dump).
//
// The main view shows one frame as an ordinary `Structure`; the GPU
// trajectory player reads the frames directly. Atom count and species are
// the same in every frame; readers stop at the first frame where they are
// not, and say so.

use crate::model::{Atom, Structure};

#[derive(Clone, Debug)]
pub struct Frame {
    /// Lattice vectors as rows, Å. Changes between frames in vc-relax and NPT.
    pub lattice: [[f64; 3]; 3],
    /// Cartesian positions, Å, one per entry of `Trajectory::species`.
    pub positions: Vec<[f64; 3]>,
    /// Total energy, eV, when the file reports one for this geometry.
    pub energy: Option<f64>,
    /// Largest force on any atom, eV/Å, when the file reports forces.
    pub max_force: Option<f64>,
    /// MD timestep or ionic step number as written in the file.
    pub step: Option<i64>,
}

#[derive(Clone, Debug)]
pub struct Trajectory {
    /// Element (or `Type n` label) per atom.
    pub species: Vec<String>,
    pub frames: Vec<Frame>,
    pub is_periodic: bool,
    /// Human-readable source format, e.g. "LAMMPS dump".
    pub format: &'static str,
}

/// Which frame of a multi-frame file the main view shows on open.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum FrameChoice {
    #[default]
    First,
    Last,
}

impl Trajectory {
    pub fn len(&self) -> usize {
        self.frames.len()
    }

    pub fn is_empty(&self) -> bool {
        self.frames.is_empty()
    }

    pub fn index_of(&self, choice: FrameChoice) -> usize {
        match choice {
            FrameChoice::First => 0,
            FrameChoice::Last => self.frames.len().saturating_sub(1),
        }
    }

    /// Frame `i` as a `Structure` for the main view.
    pub fn structure_at(&self, i: usize) -> Option<Structure> {
        let f = self.frames.get(i)?;
        let atoms = f
            .positions
            .iter()
            .zip(&self.species)
            .enumerate()
            .map(|(k, (p, el))| Atom {
                element: el.clone(),
                position: *p,
                original_index: k,
                oxidation: None,
                occupancy: 1.0,
            })
            .collect();
        let formula = match f.step {
            Some(s) => format!("{} step {}", self.format, s),
            None => format!("{} frame {}", self.format, i + 1),
        };
        Some(Structure {
            lattice: f.lattice,
            atoms,
            formula,
            is_periodic: self.is_periodic,
        })
    }

    /// Append `frame` if it has the trajectory's atoms. Returns false (and
    /// adds nothing) when the atom count differs, so a reader can stop there.
    pub fn push_checked(&mut self, frame: Frame) -> bool {
        if frame.positions.len() != self.species.len() {
            return false;
        }
        self.frames.push(frame);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_choice_and_structure() {
        let f = |x: f64| Frame {
            lattice: [[5.0, 0.0, 0.0], [0.0, 5.0, 0.0], [0.0, 0.0, 5.0]],
            positions: vec![[x, 0.0, 0.0]],
            energy: None,
            max_force: None,
            step: None,
        };
        let mut t = Trajectory {
            species: vec!["Fe".into()],
            frames: vec![],
            is_periodic: true,
            format: "test",
        };
        assert!(t.push_checked(f(0.0)));
        assert!(t.push_checked(f(1.0)));
        assert!(!t.push_checked(Frame { positions: vec![], ..f(2.0) }));
        assert_eq!(t.index_of(FrameChoice::Last), 1);
        let s = t.structure_at(1).unwrap();
        assert_eq!(s.atoms[0].position, [1.0, 0.0, 0.0]);
        assert_eq!(s.atoms[0].element, "Fe");
    }
}
