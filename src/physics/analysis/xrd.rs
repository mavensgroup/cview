// src/physics/xrd.rs
use std::f64::consts::PI;
use std::cmp::Ordering;
use crate::model::structure::Structure;
use crate::model::elements;

#[derive(Debug, Clone)]
pub struct XRDSettings {
    pub wavelength: f64,
    pub min_2theta: f64,
    pub max_2theta: f64,
    pub smoothing: f64, // Matches your UI code
}

impl Default for XRDSettings {
    fn default() -> Self {
        Self {
            wavelength: 1.5406,
            min_2theta: 10.0,
            max_2theta: 90.0,
            smoothing: 0.2,
        }
    }
}

#[derive(Debug, Clone)]
pub struct XRDPattern {
    pub two_theta: f64,
    pub intensity: f64,
    pub hkl: (i32, i32, i32),
    pub d_spacing: f64,
}

/// Main calculation: Returns discrete peaks.
/// The UI using Plotters will handle the continuous curve generation.
pub fn calculate_pattern(structure: &Structure, settings: &XRDSettings) -> Vec<XRDPattern> {
    let mut peaks = Vec::new();

    // 1. Reciprocal Lattice
    let a = structure.lattice[0];
    let b = structure.lattice[1];
    let c = structure.lattice[2];

    let v_cross = cross(b, c);
    let volume = dot(a, v_cross);
    if volume.abs() < 1e-6 { return peaks; }

    let inv_vol = 1.0 / volume;
    let a_star = scale(cross(b, c), inv_vol);
    let b_star = scale(cross(c, a), inv_vol);
    let c_star = scale(cross(a, b), inv_vol);

    // 2. Search Bounds
    let theta_max = (settings.max_2theta / 2.0).to_radians();
    let max_s = theta_max.sin() / settings.wavelength;
    let limit = 12;

    for h in -limit..=limit {
        for k in -limit..=limit {
            for l in -limit..=limit {
                if h == 0 && k == 0 && l == 0 { continue; }

                // 3. d-spacing and Theta
                let gx = h as f64 * a_star[0] + k as f64 * b_star[0] + l as f64 * c_star[0];
                let gy = h as f64 * a_star[1] + k as f64 * b_star[1] + l as f64 * c_star[1];
                let gz = h as f64 * a_star[2] + k as f64 * b_star[2] + l as f64 * c_star[2];

                let g_sq = gx*gx + gy*gy + gz*gz;
                let d_spacing = 1.0 / g_sq.sqrt();
                let s = 1.0 / (2.0 * d_spacing);

                if s > max_s { continue; }

                let sin_theta = s * settings.wavelength;
                if sin_theta > 1.0 { continue; }
                let two_theta = 2.0 * sin_theta.asin().to_degrees();

                if two_theta < settings.min_2theta { continue; }

                // 4. Structure Factor (with Cromer-Mann)
                let mut f_real = 0.0;
                let mut f_imag = 0.0;

                for atom in &structure.atoms {
                    let coeffs = elements::get_cromer_mann_coeffs(&atom.element);
                    let f0 = calculate_atomic_form_factor(s, &coeffs);
                    let phase = 2.0 * PI * (gx * atom.position[0] + gy * atom.position[1] + gz * atom.position[2]);

                    f_real += f0 * phase.cos();
                    f_imag += f0 * phase.sin();
                }

                let intensity_raw = f_real.powi(2) + f_imag.powi(2);

                // 5. LP Factor
                let theta_rad = (two_theta / 2.0).to_radians();
                let lp = (1.0 + (2.0*theta_rad).cos().powi(2)) / (theta_rad.sin().powi(2) * theta_rad.cos());

                let final_intensity = intensity_raw * lp;

                if final_intensity > 0.5 {
                    peaks.push(XRDPattern {
                        two_theta,
                        intensity: final_intensity,
                        hkl: (h, k, l),
                        d_spacing,
                    });
                }
            }
        }
    }

    // Sort and Normalize
    peaks.sort_by(|a, b| a.two_theta.partial_cmp(&b.two_theta).unwrap_or(Ordering::Equal));
    let max_i = peaks.iter().map(|p| p.intensity).fold(0.0, f64::max);
    if max_i > 1e-6 {
        for p in &mut peaks { p.intensity = (p.intensity / max_i) * 100.0; }
    }

    peaks
}

fn calculate_atomic_form_factor(s: f64, c: &[f64; 9]) -> f64 {
    let s2 = s * s;
    c[0]*(-c[1]*s2).exp() + c[2]*(-c[3]*s2).exp() + c[4]*(-c[5]*s2).exp() + c[6]*(-c[7]*s2).exp() + c[8]
}

// Math Helpers
fn dot(u: [f64; 3], v: [f64; 3]) -> f64 { u[0]*v[0] + u[1]*v[1] + u[2]*v[2] }
fn cross(u: [f64; 3], v: [f64; 3]) -> [f64; 3] { [ u[1]*v[2] - u[2]*v[1], u[2]*v[0] - u[0]*v[2], u[0]*v[1] - u[1]*v[0] ] }
fn scale(v: [f64; 3], s: f64) -> [f64; 3] { [v[0]*s, v[1]*s, v[2]*s] }
