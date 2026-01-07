// src/physics/xrd.rs
use std::f64::consts::PI;
use std::cmp::Ordering;
// Corrected import path based on your file tree (src/model/structure.rs)
use crate::model::structure::Structure;

/// Settings for the XRD simulation
#[derive(Debug, Clone)]
pub struct XRDSettings {
    pub wavelength: f64,
    pub min_2theta: f64,
    pub max_2theta: f64,
    pub smoothing: f64, // Gaussian broadening factor (sigma)
}

impl Default for XRDSettings {
    fn default() -> Self {
        Self {
            wavelength: 1.5406, // Cu K-alpha default
            min_2theta: 10.0,
            max_2theta: 90.0,
            smoothing: 0.2,
        }
    }
}

/// Represents a single diffraction peak
/// Renamed from 'XrdPeak' to 'XRDPattern' to match your UI imports
#[derive(Debug, Clone)]
pub struct XRDPattern {
    pub two_theta: f64,
    pub intensity: f64,
    pub hkl: (i32, i32, i32),
    pub d_spacing: f64,
}

/// Helper: Vector Dot Product
fn dot(u: [f64; 3], v: [f64; 3]) -> f64 {
    u[0] * v[0] + u[1] * v[1] + u[2] * v[2]
}

/// Helper: Vector Cross Product
fn cross(u: [f64; 3], v: [f64; 3]) -> [f64; 3] {
    [
        u[1] * v[2] - u[2] * v[1],
        u[2] * v[0] - u[0] * v[2],
        u[0] * v[1] - u[1] * v[0],
    ]
}

/// Helper: Vector Scalar Multiplication
fn scale(v: [f64; 3], s: f64) -> [f64; 3] {
    [v[0] * s, v[1] * s, v[2] * s]
}

/// Helper: Vector Magnitude
fn magnitude(v: [f64; 3]) -> f64 {
    dot(v, v).sqrt()
}

/// Approximates atomic scattering factor using Atomic Number (Z).
fn get_atomic_scattering_factor(element: &str) -> f64 {
    match element {
        "H" => 1.0, "He" => 2.0,
        "Li" => 3.0, "Be" => 4.0, "B" => 5.0, "C" => 6.0, "N" => 7.0, "O" => 8.0, "F" => 9.0,
        "Na" => 11.0, "Mg" => 12.0, "Al" => 13.0, "Si" => 14.0, "P" => 15.0, "S" => 16.0, "Cl" => 17.0,
        "K" => 19.0, "Ca" => 20.0, "Ti" => 22.0, "V" => 23.0, "Cr" => 24.0, "Mn" => 25.0, "Fe" => 26.0,
        "Co" => 27.0, "Ni" => 28.0, "Cu" => 29.0, "Zn" => 30.0, "Ga" => 31.0, "Ge" => 32.0, "As" => 33.0,
        "Zr" => 40.0, "Nb" => 41.0, "Mo" => 42.0, "Ag" => 47.0, "Sn" => 50.0, "Sb" => 51.0,
        "Au" => 79.0, "Pb" => 82.0,
        _ => 6.0, // Default to Carbon-like if unknown
    }
}

/// Main XRD Calculation Function
/// Accepts settings struct to control wavelength and range
pub fn calculate_pattern(structure: &Structure, settings: &XRDSettings) -> Vec<XRDPattern> {
    let mut peaks = Vec::new();

    // 1. Extract Real Lattice Vectors
    let a = structure.lattice[0];
    let b = structure.lattice[1];
    let c = structure.lattice[2];

    // 2. Calculate Reciprocal Lattice Vectors (a*, b*, c*)
    // Formula: a* = 2pi * (b x c) / V
    let volume = dot(a, cross(b, c));

    // Avoid division by zero for invalid structures
    if volume.abs() < 1e-9 { return peaks; }

    let pre_factor = 2.0 * PI / volume;

    let a_star = scale(cross(b, c), pre_factor);
    let b_star = scale(cross(c, a), pre_factor);
    let c_star = scale(cross(a, b), pre_factor);

    // 3. Determine Loop Bounds based on max 2-theta
    // Max |G| = 4pi * sin(theta_max/2) / lambda
    let theta_max_rad = settings.max_2theta.to_radians() / 2.0;
    let max_g = 4.0 * PI * theta_max_rad.sin() / settings.wavelength;

    // Estimate index range (simplified, safer to over-scan slightly)
    let min_recip_len = [magnitude(a_star), magnitude(b_star), magnitude(c_star)]
        .iter().fold(f64::INFINITY, |a, &b| a.min(b));

    let range = (max_g / min_recip_len).ceil() as i32 + 1;

    for h in -range..=range {
        for k in -range..=range {
            for l in -range..=range {
                if h == 0 && k == 0 && l == 0 { continue; }

                // 4. Calculate Scattering Vector G
                let g_vec = [
                    h as f64 * a_star[0] + k as f64 * b_star[0] + l as f64 * c_star[0],
                    h as f64 * a_star[1] + k as f64 * b_star[1] + l as f64 * c_star[1],
                    h as f64 * a_star[2] + k as f64 * b_star[2] + l as f64 * c_star[2],
                ];

                let g_mag = magnitude(g_vec);

                // 5. Calculate d-spacing and Theta
                let d_spacing = 2.0 * PI / g_mag;
                let sin_theta = settings.wavelength / (2.0 * d_spacing);

                if sin_theta > 1.0 { continue; }

                let theta_rad = sin_theta.asin();
                let two_theta_deg = 2.0 * theta_rad.to_degrees();

                // Filter by requested range
                if two_theta_deg < settings.min_2theta || two_theta_deg > settings.max_2theta {
                    continue;
                }

                // 6. Calculate Structure Factor F_hkl
                let mut f_real = 0.0;
                let mut f_imag = 0.0;

                for atom in &structure.atoms {
                    let f_j = get_atomic_scattering_factor(&atom.element);
                    let phase = dot(g_vec, atom.position); // G . r
                    f_real += f_j * phase.cos();
                    f_imag += f_j * phase.sin();
                }

                let intensity_raw = f_real.powi(2) + f_imag.powi(2);

                // 7. Lorentz-Polarization Factor
                let cos_2theta = (2.0 * theta_rad).cos();
                let sin_theta_sq = sin_theta.powi(2);
                let cos_theta = theta_rad.cos();

                let lp_factor = if sin_theta_sq > 1e-6 {
                    (1.0 + cos_2theta.powi(2)) / (sin_theta_sq * cos_theta)
                } else {
                    0.0
                };

                let final_intensity = intensity_raw * lp_factor;

                if final_intensity > 1e-3 {
                    peaks.push(XRDPattern {
                        two_theta: two_theta_deg,
                        intensity: final_intensity,
                        hkl: (h, k, l),
                        d_spacing,
                    });
                }
            }
        }
    }

    // Sort by angle
    peaks.sort_by(|a, b| a.two_theta.partial_cmp(&b.two_theta).unwrap_or(Ordering::Equal));

    peaks
}
