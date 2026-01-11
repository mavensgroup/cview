// src/physics/xrd.rs
use crate::model::elements;
use crate::model::structure::Structure;
use std::cmp::Ordering;
use std::collections::HashMap;
use std::f64::consts::PI;

#[derive(Debug, Clone)]
pub struct XRDSettings {
    pub wavelength: f64,
    pub min_2theta: f64,
    pub max_2theta: f64,
    pub smoothing: f64,
    pub temperature_factor: f64, // B-factor
}

impl Default for XRDSettings {
    fn default() -> Self {
        Self {
            wavelength: 1.5406, // Cu Kα
            min_2theta: 10.0,
            max_2theta: 90.0,
            smoothing: 0.2,
            temperature_factor: 1.0, // Default B-factor
        }
    }
}

#[derive(Debug, Clone)]
pub struct XRDPattern {
    pub two_theta: f64,
    pub intensity: f64,
    pub hkl: Vec<(i32, i32, i32)>, // List of equivalent indices
    pub d_spacing: f64,
    pub multiplicity: u32,
}

/// Main calculation: Returns discrete peaks with proper physics
pub fn calculate_pattern(structure: &Structure, settings: &XRDSettings) -> Vec<XRDPattern> {
    // 1. Reciprocal Lattice Vectors
    let a = structure.lattice[0];
    let b = structure.lattice[1];
    let c = structure.lattice[2];

    let v_cross = cross(b, c);
    let volume = dot(a, v_cross);
    if volume.abs() < 1e-10 {
        return Vec::new();
    }

    let inv_vol = 1.0 / volume;
    let a_star = scale(cross(b, c), inv_vol);
    let b_star = scale(cross(c, a), inv_vol);
    let c_star = scale(cross(a, b), inv_vol);

    // 2. Generate all reflections and group by d-spacing
    let theta_max_rad = (settings.max_2theta / 2.0).to_radians();
    // Safety check for very small angles or invalid settings
    if theta_max_rad.sin().abs() < 1e-6 {
        return Vec::new();
    }

    let d_min = settings.wavelength / (2.0 * theta_max_rad.sin());

    // Calculate maximum Miller indices needed
    let h_max = ((magnitude(a_star) / d_min).ceil() as i32 + 1).max(2);
    let k_max = ((magnitude(b_star) / d_min).ceil() as i32 + 1).max(2);
    let l_max = ((magnitude(c_star) / d_min).ceil() as i32 + 1).max(2);

    // Group reflections by their unique d-spacing (handle multiplicity)
    let mut reflection_groups: HashMap<String, Vec<(i32, i32, i32)>> = HashMap::new();

    for h in -h_max..=h_max {
        for k in -k_max..=k_max {
            for l in -l_max..=l_max {
                if h == 0 && k == 0 && l == 0 {
                    continue;
                }

                // Calculate reciprocal lattice vector
                let g = [
                    h as f64 * a_star[0] + k as f64 * b_star[0] + l as f64 * c_star[0],
                    h as f64 * a_star[1] + k as f64 * b_star[1] + l as f64 * c_star[1],
                    h as f64 * a_star[2] + k as f64 * b_star[2] + l as f64 * c_star[2],
                ];

                let g_mag = magnitude(g);
                if g_mag < 1e-10 {
                    continue;
                }

                let d_spacing = 1.0 / g_mag;

                // Check if this reflection is physically observable
                let sin_theta = settings.wavelength / (2.0 * d_spacing);
                if sin_theta > 1.0 || sin_theta < 0.0 {
                    continue;
                }

                let two_theta = 2.0 * sin_theta.asin().to_degrees();
                if two_theta < settings.min_2theta || two_theta > settings.max_2theta {
                    continue;
                }

                // Group by d-spacing (rounded to avoid floating point issues)
                let d_key = format!("{:.5}", d_spacing);
                reflection_groups
                    .entry(d_key)
                    .or_insert_with(Vec::new)
                    .push((h, k, l));
            }
        }
    }

    // 3. Calculate intensity for each unique reflection
    let mut peaks = Vec::new();

    for (_d_key, hkl_list) in reflection_groups.iter() {
        // Use the first reflection to get geometric parameters
        let (h0, k0, l0) = hkl_list[0];

        let g = [
            h0 as f64 * a_star[0] + k0 as f64 * b_star[0] + l0 as f64 * c_star[0],
            h0 as f64 * a_star[1] + k0 as f64 * b_star[1] + l0 as f64 * c_star[1],
            h0 as f64 * a_star[2] + k0 as f64 * b_star[2] + l0 as f64 * c_star[2],
        ];

        let g_mag = magnitude(g);
        let d_spacing = 1.0 / g_mag;
        let sin_theta = settings.wavelength / (2.0 * d_spacing);
        let theta_rad = sin_theta.asin();
        let two_theta = 2.0 * theta_rad.to_degrees();

        // Calculate structure factor for this (hkl)
        // s = sin(θ)/λ for form factor calculation
        let s = sin_theta / settings.wavelength;
        let s_squared = s * s;

        let mut f_real = 0.0;
        let mut f_imag = 0.0;

        for atom in &structure.atoms {
            // Get atomic form factor (Cromer-Mann approximation)
            let coeffs = elements::get_cromer_mann_coeffs(&atom.element);
            let f0 = calculate_atomic_form_factor(s, &coeffs);

            // Debye-Waller temperature factor
            let b_factor = settings.temperature_factor;
            let debye_waller = (-b_factor * s_squared).exp();

            // Phase calculation: 2π(h·x + k·y + l·z)
            let phase = 2.0
                * PI
                * (h0 as f64 * atom.position[0]
                    + k0 as f64 * atom.position[1]
                    + l0 as f64 * atom.position[2]);

            let f_atom = f0 * debye_waller;
            f_real += f_atom * phase.cos();
            f_imag += f_atom * phase.sin();
        }

        // Structure factor squared
        let f_squared = f_real * f_real + f_imag * f_imag;

        // Check for systematic absences (structure factor = 0)
        if f_squared < 1e-6 {
            continue;
        }

        // Lorentz-Polarization factor for powder diffraction
        // LP = (1 + cos²(2θ)) / (sin²(θ)·cos(θ))
        let cos_2theta = (2.0 * theta_rad).cos();
        let sin_theta = theta_rad.sin();
        let cos_theta = theta_rad.cos();

        if sin_theta.abs() < 1e-10 || cos_theta.abs() < 1e-10 {
            continue;
        }

        let lp_factor = (1.0 + cos_2theta * cos_2theta) / (sin_theta * sin_theta * cos_theta);

        // Multiplicity = number of symmetrically equivalent reflections
        let multiplicity = hkl_list.len() as u32;

        // Final intensity with all corrections
        let intensity = f_squared * lp_factor * multiplicity as f64;

        if intensity > 0.01 {
            // Lower threshold slightly
            peaks.push(XRDPattern {
                two_theta,
                intensity,
                hkl: hkl_list.clone(),
                d_spacing,
                multiplicity,
            });
        }
    }

    // 4. Sort by angle and normalize
    peaks.sort_by(|a, b| {
        a.two_theta
            .partial_cmp(&b.two_theta)
            .unwrap_or(Ordering::Equal)
    });

    // Normalize to 100 for strongest peak
    let max_intensity = peaks.iter().map(|p| p.intensity).fold(0.0_f64, f64::max);

    if max_intensity > 1e-6 {
        for peak in &mut peaks {
            peak.intensity = (peak.intensity / max_intensity) * 100.0;
        }
    }

    peaks
}

/// Calculate atomic form factor using Cromer-Mann 9-parameter approximation
fn calculate_atomic_form_factor(s: f64, coeffs: &[f64; 9]) -> f64 {
    let s_squared = s * s;
    coeffs[0] * (-coeffs[1] * s_squared).exp()
        + coeffs[2] * (-coeffs[3] * s_squared).exp()
        + coeffs[4] * (-coeffs[5] * s_squared).exp()
        + coeffs[6] * (-coeffs[7] * s_squared).exp()
        + coeffs[8]
}

// ============================================================================
// Vector Math Utilities
// ============================================================================

#[inline]
fn dot(u: [f64; 3], v: [f64; 3]) -> f64 {
    u[0] * v[0] + u[1] * v[1] + u[2] * v[2]
}

#[inline]
fn cross(u: [f64; 3], v: [f64; 3]) -> [f64; 3] {
    [
        u[1] * v[2] - u[2] * v[1],
        u[2] * v[0] - u[0] * v[2],
        u[0] * v[1] - u[1] * v[0],
    ]
}

#[inline]
fn scale(v: [f64; 3], s: f64) -> [f64; 3] {
    [v[0] * s, v[1] * s, v[2] * s]
}

#[inline]
fn magnitude(v: [f64; 3]) -> f64 {
    (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt()
}
