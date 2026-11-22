// src/physics/operations/magnetic_order.rs
//
// A commensurate magnetic order from a propagation vector, as neutron
// diffraction describes it: atom j of the chemical cell, translated by the
// lattice vector R, carries
//
//     m_j(R) = m_j · cos(2π k·R + φ_j)
//
// with k in reciprocal-lattice units. For k components of 0 and ½ this is a
// pattern of ±m_j (ferro-, A-, C-, G-type antiferromagnets); other rational
// k give an amplitude-modulated (spin-density-wave) pattern. The smallest
// cell that holds the order is n_i = denominator of k_i along each axis.
//
// Only single-k collinear or amplitude-modulated orders: a helix needs two
// components in quadrature, and an incommensurate k has no finite cell.

use crate::model::structure::{Atom, Structure};
use crate::physics::operations::conversion::build_formula;
use crate::utils::linalg::{cart_to_frac, frac_to_cart};

/// Longest period handled along one axis.
pub const MAX_PERIOD: usize = 12;

/// Common orders, as propagation vectors on a cell with one magnetic atom
/// per primitive (pseudo)cubic cell, e.g. the B site of an ABO₃ perovskite.
pub const PRESETS: [(&str, [f64; 3]); 4] = [
    ("Ferromagnetic, k = (0, 0, 0)", [0.0, 0.0, 0.0]),
    ("A-type, k = (0, 0, ½)", [0.0, 0.0, 0.5]),
    ("C-type, k = (½, ½, 0)", [0.5, 0.5, 0.0]),
    ("G-type, k = (½, ½, ½)", [0.5, 0.5, 0.5]),
];

/// The period along one axis for the k component `k`: the smallest n with
/// n·k an integer. Err for an irrational-looking or too long period.
pub fn period(k: f64) -> Result<usize, String> {
    (1..=MAX_PERIOD)
        .find(|&n| {
            let x = k * n as f64;
            (x - x.round()).abs() < 1e-6
        })
        .ok_or_else(|| {
            format!(
                "k component {} is not commensurate within {} cells (use a fraction such as 1/2 or 1/3)",
                k, MAX_PERIOD
            )
        })
}

/// The magnetic cell as multiples of the chemical cell along a, b, c.
pub fn cell_multiples(k: [f64; 3]) -> Result<[usize; 3], String> {
    Ok([period(k[0])?, period(k[1])?, period(k[2])?])
}

/// The structure on the n₁×n₂×n₃ magnetic cell of `k`, with moment
/// `moments[j]·cos(2π k·R + phase_deg[j])` on each image of atom j (and no
/// moment where `moments[j]` is `None` or the modulation is zero). k = 0
/// keeps the cell and the atom order: it just sets the moments.
pub fn build(
    s: &Structure,
    k: [f64; 3],
    moments: &[Option<[f64; 3]>],
    phase_deg: &[f64],
) -> Result<Structure, String> {
    if !s.is_periodic {
        return Err("A magnetic order needs a periodic cell".into());
    }
    if moments.len() != s.atoms.len() || phase_deg.len() != s.atoms.len() {
        return Err("one moment and one phase per atom are needed".into());
    }
    let n = cell_multiples(k)?;
    let lattice = [0, 1, 2].map(|i| s.lattice[i].map(|x| x * n[i] as f64));

    let mut atoms = Vec::with_capacity(s.atoms.len() * n[0] * n[1] * n[2]);
    for (j, atom) in s.atoms.iter().enumerate() {
        let f = cart_to_frac(atom.position, s.lattice).ok_or("singular cell")?;
        // Image (a, b, c) of the wrapped atom is the original moved by
        // R = (a, b, c) − floor(f).
        let w = f.map(f64::floor);
        for a in 0..n[0] {
            for b in 0..n[1] {
                for c in 0..n[2] {
                    let cell = [a as f64, b as f64, c as f64];
                    let r = [0, 1, 2].map(|i| cell[i] - w[i]);
                    let phase = 2.0 * std::f64::consts::PI * (k[0] * r[0] + k[1] * r[1] + k[2] * r[2])
                        + phase_deg[j].to_radians();
                    let amp = clean(phase.cos());
                    let magmom = moments[j]
                        .map(|m| m.map(|x| clean(x * amp)))
                        .filter(|m| m.iter().any(|x| x.abs() > 1e-9));
                    let frac = [0, 1, 2].map(|i| (f[i] - w[i] + cell[i]) / n[i] as f64);
                    atoms.push(Atom {
                        position: frac_to_cart(frac, lattice),
                        original_index: atoms.len(),
                        magmom,
                        ..atom.clone()
                    });
                }
            }
        }
    }
    if n == [1, 1, 1] {
        // Same cell: put the atoms back where they were (not wrapped), so
        // this is a pure moment edit.
        for (new, old) in atoms.iter_mut().zip(&s.atoms) {
            new.position = old.position;
        }
    }
    Ok(Structure {
        formula: build_formula(&atoms),
        atoms,
        lattice,
        is_periodic: true,
    })
}

/// Round cos() noise (6e-17 for cos π/2) to the value it stands for.
fn clean(x: f64) -> f64 {
    let r = x.round();
    if (x - r).abs() < 1e-9 {
        r
    } else if x.abs() < 1e-9 {
        0.0
    } else {
        x
    }
}

/// Sum of the moments, μB, weighted by occupancy.
pub fn net_moment(s: &Structure) -> [f64; 3] {
    let mut t = [0.0; 3];
    for a in &s.atoms {
        if let Some(m) = a.magmom {
            for i in 0..3 {
                t[i] += m[i] * a.occupancy;
            }
        }
    }
    t
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cubic(atoms: &[(&str, [f64; 3])], a: f64) -> Structure {
        let lattice = [[a, 0.0, 0.0], [0.0, a, 0.0], [0.0, 0.0, a]];
        Structure {
            lattice,
            atoms: atoms
                .iter()
                .enumerate()
                .map(|(i, (el, f))| Atom {
                    element: el.to_string(),
                    position: frac_to_cart(*f, lattice),
                    original_index: i,
                    oxidation: None,
                    occupancy: 1.0,
                    magmom: None,
                })
                .collect(),
            formula: String::new(),
            is_periodic: true,
        }
    }

    fn perovskite() -> Structure {
        cubic(
            &[
                ("La", [0.0, 0.0, 0.0]),
                ("Fe", [0.5, 0.5, 0.5]),
                ("O", [0.5, 0.5, 0.0]),
                ("O", [0.5, 0.0, 0.5]),
                ("O", [0.0, 0.5, 0.5]),
            ],
            3.9,
        )
    }

    fn fe_moment(s: &Structure) -> Vec<Option<[f64; 3]>> {
        s.atoms.iter().map(|a| (a.element == "Fe").then_some([0.0, 0.0, 4.0])).collect()
    }

    #[test]
    fn periods_from_k() {
        assert_eq!(cell_multiples([0.0, 0.5, 1.0 / 3.0]).unwrap(), [1, 2, 3]);
        assert!(period(0.123456).is_err());
    }

    #[test]
    fn g_type_alternates_along_every_axis_and_cancels() {
        let s = perovskite();
        let n = s.atoms.len();
        let out = build(&s, [0.5, 0.5, 0.5], &fe_moment(&s), &vec![0.0; n]).unwrap();
        assert_eq!(out.atoms.len(), 8 * n);
        let fe: Vec<&Atom> = out.atoms.iter().filter(|a| a.element == "Fe").collect();
        assert_eq!(fe.len(), 8);
        // Every Fe–Fe nearest-neighbour pair (3.9 Å apart) is antiparallel.
        for a in &fe {
            for b in &fe {
                let d: f64 = (0..3).map(|i| (a.position[i] - b.position[i]).powi(2)).sum::<f64>().sqrt();
                if (d - 3.9).abs() < 1e-6 {
                    assert_eq!(a.magmom.unwrap()[2], -b.magmom.unwrap()[2]);
                }
            }
        }
        assert_eq!(net_moment(&out), [0.0, 0.0, 0.0]);
        assert!(out.atoms.iter().filter(|a| a.element != "Fe").all(|a| a.magmom.is_none()));
    }

    #[test]
    fn a_type_is_ferro_in_plane_and_alternates_along_c() {
        let s = perovskite();
        let n = s.atoms.len();
        let out = build(&s, [0.0, 0.0, 0.5], &fe_moment(&s), &vec![0.0; n]).unwrap();
        assert_eq!(out.atoms.len(), 2 * n);
        assert!((out.lattice[2][2] - 7.8).abs() < 1e-12);
        let fe: Vec<&Atom> = out.atoms.iter().filter(|a| a.element == "Fe").collect();
        let (lo, hi) = if fe[0].position[2] < fe[1].position[2] { (fe[0], fe[1]) } else { (fe[1], fe[0]) };
        assert_eq!(lo.magmom.unwrap()[2], 4.0);
        assert_eq!(hi.magmom.unwrap()[2], -4.0);
    }

    #[test]
    fn k_zero_keeps_the_cell_and_order_and_phase_flips_a_site() {
        // Rock-salt NiO conventional cell, AFM-II via k = (½,½,½) needs the
        // three face-centred Ni flipped relative to the corner one.
        let s = cubic(
            &[
                ("Ni", [0.0, 0.0, 0.0]),
                ("Ni", [0.5, 0.5, 0.0]),
                ("Ni", [0.5, 0.0, 0.5]),
                ("Ni", [0.0, 0.5, 0.5]),
            ],
            4.17,
        );
        let m = vec![Some([1.0, 1.0, 1.0]); 4];
        let same = build(&s, [0.0; 3], &m, &[0.0, 180.0, 0.0, 0.0]).unwrap();
        assert_eq!(same.atoms.len(), 4);
        assert_eq!(same.lattice, s.lattice);
        assert_eq!(same.atoms[1].position, s.atoms[1].position);
        assert_eq!(same.atoms[1].magmom, Some([-1.0, -1.0, -1.0]));

        // AFM-II: ferromagnetic (111) sheets, alternating — the sign is
        // (−1)^(x+y+z) in units of a/… for every Ni of the 2×2×2 cell.
        let out = build(&s, [0.5; 3], &m, &[0.0, 180.0, 180.0, 180.0]).unwrap();
        assert_eq!(out.atoms.len(), 32);
        for a in &out.atoms {
            let f = cart_to_frac(a.position, s.lattice).unwrap();
            let sum = (f[0] + f[1] + f[2]).round() as i64;
            let want = if sum.rem_euclid(2) == 0 { 1.0 } else { -1.0 };
            assert_eq!(a.magmom.unwrap()[0], want, "at {:?}", f);
        }
    }

    #[test]
    fn third_period_gives_a_modulated_wave() {
        let s = cubic(&[("Cr", [0.0, 0.0, 0.0])], 2.9);
        let out = build(&s, [1.0 / 3.0, 0.0, 0.0], &[Some([0.0, 0.0, 2.0])], &[0.0]).unwrap();
        let mz: Vec<f64> = out.atoms.iter().map(|a| a.magmom.unwrap()[2]).collect();
        assert_eq!(mz.len(), 3);
        assert!((mz[0] - 2.0).abs() < 1e-12 && (mz[1] + 1.0).abs() < 1e-12 && (mz[2] + 1.0).abs() < 1e-12);
    }
}
