// src/physics/xrd.rs

use crate::model::elements;
use crate::model::structure::Structure;
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

    // 3. Scan HKL Loop
    // Limit range based on d-spacing
    let range = 6;

    for h in -range..=range {
        for k in -range..=range {
            for l in -range..=range {
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

                for atom in &structure.atoms {
                    // Atomic Form Factor (f0)
                    // FIXED: get_atomic_number returns i32, not Option<i32>
                    let z = elements::get_atomic_number(&atom.element);
                    let f0 = if z > 0 { z as f64 } else { 1.0 };

                    // Temperature Factor (Debye-Waller)
                    // exp(-B * (sin(theta)/lambda)^2) -> simplified using g (1/d = 2sin(theta)/lambda)
                    // Common approx: exp( -B * g^2 / 4 )
                    let debye = (-settings.temperature_factor * (g_mag * g_mag) / 4.0).exp();
                    let f_eff = f0 * debye;

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
                        hkl: vec![(h, k, l)],
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
        let has_peak_near = |target: f64| peaks.iter().any(|p| (p.two_theta - target).abs() < 0.15);
        assert!(has_peak_near(47.30), "Si missing (220) peak near 47.30°");
        assert!(has_peak_near(56.12), "Si missing (311) peak near 56.12°");
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
