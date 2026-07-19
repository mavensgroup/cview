// src/physics/xrd.rs

use crate::model::elements;
use crate::model::structure::Structure;
use crate::utils::console;
use nalgebra::Vector3;
use std::cmp::Ordering;
use std::f64::consts::PI;

#[derive(Debug, Clone)]
pub struct XRDSettings {
    pub wavelength: f64, // e.g. 1.5406 for Cu K-alpha
    pub min_2theta: f64,
    pub max_2theta: f64,
    pub smoothing: f64,          // For potential future Gaussian broadening
    pub temperature_factor: f64, // Debye-Waller B-factor (approx 1.0)
}

impl Default for XRDSettings {
    fn default() -> Self {
        Self {
            wavelength: 1.5406,
            min_2theta: 10.0,
            max_2theta: 90.0,
            smoothing: 0.2,
            temperature_factor: 1.0,
        }
    }
}

#[derive(Debug, Clone)]
pub struct XRDPattern {
    pub two_theta: f64,
    pub intensity: f64,
    pub hkl: Vec<(i32, i32, i32)>,
    pub d_spacing: f64,
    pub multiplicity: u32,
}

/// Crystallographic convention: first non-zero index is positive.
/// Friedel's law makes (h,k,l) and (-h,-k,-l) symmetry-equivalent in intensity,
/// so we collapse them onto a single canonical label.
fn canonical_hkl(hkl: (i32, i32, i32)) -> (i32, i32, i32) {
    let (h, k, l) = hkl;
    let flip = h < 0 || (h == 0 && k < 0) || (h == 0 && k == 0 && l < 0);
    if flip { (-h, -k, -l) } else { (h, k, l) }
}

/// Main calculation: Returns discrete peaks with proper physics (Structure Factor)
pub fn calculate_pattern(structure: &Structure, settings: &XRDSettings) -> Vec<XRDPattern> {
    // 1. Calculate Real Lattice Vectors (a1, a2, a3)
    let a1 = Vector3::from(structure.lattice[0]);
    let a2 = Vector3::from(structure.lattice[1]);
    let a3 = Vector3::from(structure.lattice[2]);

    // Calculate Volume of the unit cell
    let volume = a1.dot(&a2.cross(&a3)).abs();

    // Safety check for invalid volume
    if volume < 1e-6 {
        return vec![];
    }

    // 2. Calculate Reciprocal Lattice Vectors (b1, b2, b3)
    // Using Crystallography definition (without 2*PI factor here, we add it in phase)
    // b1 = (a2 x a3) / V
    let b1 = a2.cross(&a3) / volume;
    let b2 = a3.cross(&a1) / volume;
    let b3 = a1.cross(&a2) / volume;

    let mut raw_peaks: Vec<XRDPattern> = Vec::new();

    // Cromer-Mann coefficients per unique species. Atoms of the same element
    // share f0 at a given reflection, so the expansion is evaluated once per
    // species per reflection rather than once per atom.
    let mut species: Vec<(&str, [f64; 9])> = Vec::new();
    let species_of: Vec<usize> = structure
        .atoms
        .iter()
        .map(
            |a| match species.iter().position(|(e, _)| *e == a.element) {
                Some(i) => i,
                None => {
                    species.push((&a.element, elements::get_cromer_mann_coeffs(&a.element)));
                    species.len() - 1
                }
            },
        )
        .collect();
    let mut f0_by_species = vec![0.0; species.len()];

    // 3. Scan HKL Loop
    // The largest reciprocal vector reachable in the angular window is
    // g_max = 2 sin(theta_max) / lambda. Since h = g . a1 (crystallographic
    // convention, b_i . a_j = delta_ij), |h| <= g_max * |a1| — so the loop
    // bounds must scale with the real-space cell, per axis. A fixed cube
    // silently truncates large cells anisotropically.
    let theta_max = (settings.max_2theta / 2.0).to_radians();
    let g_max = 2.0 * theta_max.sin() / settings.wavelength;
    const MAX_RANGE: i32 = 50;
    let range_of = |a: &Vector3<f64>| ((g_max * a.norm()).ceil() as i32 + 1).max(1);
    let (want_h, want_k, want_l) = (range_of(&a1), range_of(&a2), range_of(&a3));
    let (range_h, range_k, range_l) = (
        want_h.min(MAX_RANGE),
        want_k.min(MAX_RANGE),
        want_l.min(MAX_RANGE),
    );
    // The cap guards against a runaway loop on pathological cells, but it
    // truncates the pattern — say so instead of failing silently.
    if want_h > MAX_RANGE || want_k > MAX_RANGE || want_l > MAX_RANGE {
        console::log_warn(&format!(
            "XRD: hkl enumeration capped at ±{MAX_RANGE} (cell would need ±{}); high-angle peaks will be missing from the pattern",
            want_h.max(want_k).max(want_l)
        ));
    }

    for h in -range_h..=range_h {
        for k in -range_k..=range_k {
            for l in -range_l..=range_l {
                // Skip the origin
                if h == 0 && k == 0 && l == 0 {
                    continue;
                }

                // Construct Reciprocal Vector g = h*b1 + k*b2 + l*b3
                let g = b1.scale(h as f64) + b2.scale(k as f64) + b3.scale(l as f64);

                // Magnitude of g is 1/d
                let g_mag = g.norm();
                if g_mag < 1e-6 {
                    continue;
                }

                let d = 1.0 / g_mag;

                // Bragg's Law: lambda = 2d sin(theta)
                // -> sin(theta) = lambda / 2d
                let sin_theta = settings.wavelength / (2.0 * d);

                // If sin_theta > 1, diffraction is impossible for this wavelength
                if sin_theta > 1.0 {
                    continue;
                }

                let theta = sin_theta.asin();
                let two_theta_deg = 2.0 * theta * 180.0 / PI;

                // Filter by angular range
                if two_theta_deg < settings.min_2theta || two_theta_deg > settings.max_2theta {
                    continue;
                }

                // 4. Structure Factor Calculation (F_hkl)
                // F = Sum( f_j * exp(2*pi*i * (g . r_j)) )
                let mut f_real = 0.0;
                let mut f_imag = 0.0;

                // Cromer-Mann argument: s = sin(theta)/lambda = g/2 (since g = 1/d)
                let s2 = (g_mag / 2.0) * (g_mag / 2.0);

                // Temperature Factor (Debye-Waller)
                // exp(-B * (sin(theta)/lambda)^2) = exp(-B * g^2 / 4); atom-independent
                let debye = (-settings.temperature_factor * (g_mag * g_mag) / 4.0).exp();

                // Atomic Form Factor: f0(s) = sum_i a_i exp(-b_i s^2) + c
                // (Cromer-Mann analytic expansion, ITC Vol. C, Table 6.1.1.4)
                for (f0, (_, cm)) in f0_by_species.iter_mut().zip(&species) {
                    *f0 = cm[0] * (-cm[1] * s2).exp()
                        + cm[2] * (-cm[3] * s2).exp()
                        + cm[4] * (-cm[5] * s2).exp()
                        + cm[6] * (-cm[7] * s2).exp()
                        + cm[8];
                }

                for (atom, &sp) in structure.atoms.iter().zip(&species_of) {
                    let f_eff = f0_by_species[sp] * debye;

                    // Phase = 2 * PI * (g_vector dot position_vector)
                    let pos = Vector3::from(atom.position);
                    let phase = 2.0 * PI * g.dot(&pos);

                    f_real += f_eff * phase.cos();
                    f_imag += f_eff * phase.sin();
                }

                // Intensity is magnitude squared of Structure Factor
                let intensity_sq = f_real * f_real + f_imag * f_imag;

                // 5. Lorentz-Polarization Factor (LP)
                // Standard for powder diffraction: (1 + cos^2(2theta)) / (sin^2(theta) * cos(theta))
                let cos_2theta = (2.0 * theta).cos();
                let cos_theta = theta.cos();
                let sin_theta_sq = sin_theta * sin_theta;

                // Avoid division by zero at theta=0 or theta=90
                if sin_theta_sq < 1e-6 || cos_theta.abs() < 1e-6 {
                    continue;
                }

                let lp = (1.0 + cos_2theta * cos_2theta) / (sin_theta_sq * cos_theta);

                let final_intensity = intensity_sq * lp;

                // Threshold to ignore extremely weak peaks
                if final_intensity > 1e-4 {
                    raw_peaks.push(XRDPattern {
                        two_theta: two_theta_deg,
                        intensity: final_intensity,
                        hkl: vec![canonical_hkl((h, k, l))],
                        d_spacing: d,
                        multiplicity: 1,
                    });
                }
            }
        }
    }

    // 6. Sort by 2Theta
    raw_peaks.sort_by(|a, b| {
        a.two_theta
            .partial_cmp(&b.two_theta)
            .unwrap_or(Ordering::Equal)
    });

    // 7. Merge overlapping peaks (multiplicity)
    let mut merged_peaks: Vec<XRDPattern> = Vec::new();

    for peak in raw_peaks {
        match merged_peaks.last_mut() {
            // If peaks are within 0.05 degrees, consider them the same peak
            Some(last) if (peak.two_theta - last.two_theta).abs() < 0.05 => {
                last.intensity += peak.intensity;
                last.multiplicity += 1;
                // Add index to list if unique (limit to 6 to save UI space)
                if last.hkl.len() < 6 && !last.hkl.contains(&peak.hkl[0]) {
                    last.hkl.push(peak.hkl[0]);
                }
            }
            _ => merged_peaks.push(peak),
        }
    }

    // 8. Normalize intensities (0 to 100)
    let max_i = merged_peaks.iter().map(|p| p.intensity).fold(0.0, f64::max);

    if max_i > 0.0 {
        for p in &mut merged_peaks {
            p.intensity = (p.intensity / max_i) * 100.0;
        }
    }

    merged_peaks
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::structure::{Atom, Structure};

    fn make_structure(lat: [[f64; 3]; 3], atoms: Vec<(&str, [f64; 3])>) -> Structure {
        Structure {
            lattice: lat,
            atoms: atoms
                .into_iter()
                .enumerate()
                .map(|(i, (e, p))| Atom {
                    element: e.to_string(),
                    position: p,
                    original_index: i,
                    oxidation: None,
                })
                .collect(),
            formula: String::new(),
            is_periodic: true,
        }
    }
    /// Convert fractional → Cartesian for the test inputs.
    fn frac_to_cart(lat: [[f64; 3]; 3], frac: [f64; 3]) -> [f64; 3] {
        let a = Vector3::from(lat[0]);
        let b = Vector3::from(lat[1]);
        let c = Vector3::from(lat[2]);
        let r = a * frac[0] + b * frac[1] + c * frac[2];
        [r.x, r.y, r.z]
    }

    /// Si (Fd-3m, a = 5.4309 Å, Cu Kα). Reference peaks (ICDD 00-027-1402):
    ///   (111) 28.44°, (220) 47.30°, (311) 56.12°
    #[test]
    fn test_si_xrd_peaks() {
        let a = 5.4309;
        let lat = [[a, 0.0, 0.0], [0.0, a, 0.0], [0.0, 0.0, a]];
        // 8 Si atoms in the conventional cell (diamond cubic)
        let frac = [
            [0.0, 0.0, 0.0],
            [0.5, 0.5, 0.0],
            [0.5, 0.0, 0.5],
            [0.0, 0.5, 0.5],
            [0.25, 0.25, 0.25],
            [0.75, 0.75, 0.25],
            [0.75, 0.25, 0.75],
            [0.25, 0.75, 0.75],
        ];
        let atoms: Vec<_> = frac.iter().map(|f| ("Si", frac_to_cart(lat, *f))).collect();
        let s = make_structure(lat, atoms);

        let settings = XRDSettings::default();
        let peaks = calculate_pattern(&s, &settings);
        assert!(!peaks.is_empty(), "Si produced no XRD peaks");

        // Find the strongest peak — must be (111) at ~28.44°
        let strongest = peaks
            .iter()
            .max_by(|a, b| a.intensity.partial_cmp(&b.intensity).unwrap())
            .unwrap();
        assert!(
            (strongest.two_theta - 28.44).abs() < 0.15,
            "Si strongest peak should be (111) at 28.44°, got {:.3}°",
            strongest.two_theta
        );

        // (220) and (311) must exist within tolerance
        let peak_near = |target: f64| {
            peaks
                .iter()
                .find(|p| (p.two_theta - target).abs() < 0.15)
                .unwrap_or_else(|| panic!("Si missing peak near {target}°"))
        };
        let p220 = peak_near(47.30);
        let p311 = peak_near(56.12);

        // Relative intensities (ICDD 00-027-1402: I(220) ≈ 55, I(311) ≈ 30 vs
        // I(111) = 100). With a constant f0 = Z these high-angle peaks come out
        // far too strong; only an angle-dependent form factor lands them in
        // these bands (wide, since the B-factor here is a generic default).
        assert!(
            p220.intensity > 30.0 && p220.intensity < 80.0,
            "Si (220) relative intensity should be ~55, got {:.1}",
            p220.intensity
        );
        assert!(
            p311.intensity > 12.0 && p311.intensity < 55.0,
            "Si (311) relative intensity should be ~30, got {:.1}",
            p311.intensity
        );
    }

    /// A large cell (a = 13.7 Å, chibaite-sized) needs hkl indices up to ~12
    /// to cover 2θ ≤ 90° at Cu Kα. A fixed ±6 enumeration cube silently drops
    /// the upper half of the pattern; this guards the dynamic per-axis range.
    #[test]
    fn test_large_cell_high_angle_peaks() {
        let a = 13.7;
        let lat = [[a, 0.0, 0.0], [0.0, a, 0.0], [0.0, 0.0, a]];
        let s = make_structure(lat, vec![("C", [0.0, 0.0, 0.0])]);
        let settings = XRDSettings::default();
        let peaks = calculate_pattern(&s, &settings);

        let max_index = peaks
            .iter()
            .flat_map(|p| p.hkl.iter())
            .map(|&(h, k, l)| h.abs().max(k.abs()).max(l.abs()))
            .max()
            .unwrap_or(0);
        assert!(
            max_index > 6,
            "Large cell should produce reflections with hkl index > 6, max was {max_index}"
        );

        let max_2theta = peaks.iter().map(|p| p.two_theta).fold(0.0, f64::max);
        assert!(
            max_2theta > 80.0,
            "Large cell should have peaks near the top of the 2θ range, max was {max_2theta:.1}°"
        );
    }

    /// α-quartz (P3₁21, a = 4.9133 Å, c = 5.4053 Å, Cu Kα).
    /// Reference (ICDD 01-085-0865): (101) 26.64°, (100) 20.86°.
    /// Tests non-cubic reciprocal-lattice metric.
    #[test]
    fn test_quartz_xrd_peaks() {
        let a = 4.9133;
        let c = 5.4053;
        // Hexagonal lattice vectors
        let lat = [
            [a, 0.0, 0.0],
            [-a / 2.0, a * (3.0_f64).sqrt() / 2.0, 0.0],
            [0.0, 0.0, c],
        ];
        // Si at (0.4697, 0, 0.3333) and symmetry equivalents (3a Wyckoff)
        // O at (0.4133, 0.2672, 0.2144) and equivalents (6c Wyckoff)
        // Minimal asymmetric unit is enough for peak position checks.
        let frac = [
            ("Si", [0.4697, 0.0, 1.0 / 3.0]),
            ("Si", [0.0, 0.4697, 2.0 / 3.0]),
            ("Si", [-0.4697, -0.4697, 0.0]),
            ("O", [0.4133, 0.2672, 0.2144]),
            ("O", [-0.2672, 0.1461, 0.5478]),
            ("O", [-0.1461, -0.4133, 0.8811]),
            ("O", [0.2672, 0.4133, -0.2144]),
            ("O", [0.1461, -0.2672, 0.4522]),
            ("O", [-0.4133, -0.1461, 0.1189]),
        ];
        let atoms: Vec<_> = frac
            .iter()
            .map(|(e, f)| (*e, frac_to_cart(lat, *f)))
            .collect();
        let s = make_structure(lat, atoms);

        let settings = XRDSettings::default();
        let peaks = calculate_pattern(&s, &settings);
        assert!(!peaks.is_empty(), "Quartz produced no XRD peaks");

        let has_peak_near = |target: f64| peaks.iter().any(|p| (p.two_theta - target).abs() < 0.20);
        assert!(
            has_peak_near(26.64),
            "Quartz missing (101) peak near 26.64°"
        );
        assert!(
            has_peak_near(20.86),
            "Quartz missing (100) peak near 20.86°"
        );
    }

    /// Sanity: Bragg's law constraint — no peak should violate λ ≤ 2d.
    #[test]
    fn test_xrd_bragg_law_consistency() {
        let a = 5.4309;
        let lat = [[a, 0.0, 0.0], [0.0, a, 0.0], [0.0, 0.0, a]];
        let s = make_structure(lat, vec![("Si", [0.0, 0.0, 0.0])]);
        let settings = XRDSettings::default();
        let peaks = calculate_pattern(&s, &settings);
        for p in &peaks {
            let sin_theta = settings.wavelength / (2.0 * p.d_spacing);
            assert!(
                sin_theta <= 1.0 + 1e-9,
                "Bragg violation: sin(θ) = {}",
                sin_theta
            );
            assert!(p.two_theta >= settings.min_2theta && p.two_theta <= settings.max_2theta);
        }
    }
}
