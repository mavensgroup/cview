// src/io/chgcar.rs
// VASP CHGCAR file parser
// Supports: non-spin-polarized, spin-polarized (two grids), charge density difference

use std::fs::File;
use std::io::{self, BufRead, BufReader};

/// A single atom parsed from the CHGCAR header
#[derive(Clone, Debug)]
pub struct ChgcarAtom {
    /// Element symbol (e.g. "Ba", "Ti", "O")
    pub element: String,
    /// Fractional coordinates [0, 1)
    pub frac_coords: [f64; 3],
}

/// Volumetric charge density data parsed from a CHGCAR file.
#[derive(Clone, Debug)]
pub struct ChgcarData {
    /// Lattice vectors [a, b, c] each as [x, y, z]
    pub lattice: [[f64; 3]; 3],
    /// Grid dimensions [nx, ny, nz]
    pub grid: [usize; 3],
    /// Total charge density (ρ_up + ρ_down), flat array in Fortran order (x fastest)
    /// Raw VASP values (electrons, NOT divided by volume)
    pub charge_total: Vec<f64>,
    /// Magnetization density (ρ_up - ρ_down), present only for spin-polarized calculations
    pub charge_mag: Option<Vec<f64>>,
    /// Whether this is a spin-polarized calculation
    pub spin_polarized: bool,
    /// Atoms parsed from the CHGCAR header
    pub atoms: Vec<ChgcarAtom>,
    /// Species names in order (e.g. ["Ba", "Ti", "O"])
    pub species: Vec<String>,
}

impl ChgcarData {
    /// Returns the total number of grid points
    pub fn n_points(&self) -> usize {
        self.grid[0] * self.grid[1] * self.grid[2]
    }

    /// Get spin-up density: (total + mag) / 2
    pub fn charge_up(&self) -> Option<Vec<f64>> {
        let mag = self.charge_mag.as_ref()?;
        Some(
            self.charge_total
                .iter()
                .zip(mag.iter())
                .map(|(t, m)| (t + m) * 0.5)
                .collect(),
        )
    }

    /// Get spin-down density: (total - mag) / 2
    pub fn charge_down(&self) -> Option<Vec<f64>> {
        let mag = self.charge_mag.as_ref()?;
        Some(
            self.charge_total
                .iter()
                .zip(mag.iter())
                .map(|(t, m)| (t - m) * 0.5)
                .collect(),
        )
    }

    /// Flat index from (ix, iy, iz) — Fortran column-major order (x fastest)
    pub fn index(&self, ix: usize, iy: usize, iz: usize) -> usize {
        ix + self.grid[0] * (iy + self.grid[1] * iz)
    }

    /// Convert CHGCAR header data to a CView `Structure` for 3D visualization.
    pub fn to_structure(&self) -> crate::model::structure::Structure {
        let atoms = self
            .atoms
            .iter()
            .enumerate()
            .map(|(i, a)| crate::model::structure::Atom {
                element: a.element.clone(),
                position: a.frac_coords,
                original_index: i,
            })
            .collect();
        crate::model::structure::Structure {
            lattice: self.lattice,
            atoms,
            formula: String::new(),
            is_periodic: true,
        }
    }

    /// Volume of the unit cell in ų
    pub fn cell_volume(&self) -> f64 {
        let a = self.lattice[0];
        let b = self.lattice[1];
        let c = self.lattice[2];
        // Triple product  a · (b × c)
        (a[0] * (b[1] * c[2] - b[2] * c[1]) - a[1] * (b[0] * c[2] - b[2] * c[0])
            + a[2] * (b[0] * c[1] - b[1] * c[0]))
            .abs()
    }

    /// Return density values normalized to e/ų for the total channel.
    /// VASP stores charge × volume; dividing gives physical density.
    pub fn normalized_total(&self) -> Vec<f64> {
        let vol = self.cell_volume();
        if vol.abs() < 1e-30 {
            return self.charge_total.clone();
        }
        self.charge_total.iter().map(|&v| v / vol).collect()
    }

    /// Return density values normalized to e/ų for the magnetization channel.
    pub fn normalized_mag(&self) -> Option<Vec<f64>> {
        let vol = self.cell_volume();
        if vol.abs() < 1e-30 {
            return self.charge_mag.clone();
        }
        self.charge_mag
            .as_ref()
            .map(|m| m.iter().map(|&v| v / vol).collect())
    }
}

pub fn parse(path: &str) -> io::Result<ChgcarData> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);
    let mut lines = reader.lines();

    // ---- Line 1: comment ----
    let _comment = lines
        .next()
        .ok_or_else(|| io_err("Unexpected EOF: comment line"))??;

    // ---- Line 2: scale factor ----
    let scale_line = lines
        .next()
        .ok_or_else(|| io_err("Unexpected EOF: scale factor"))??;
    let scale: f64 = scale_line
        .trim()
        .parse()
        .map_err(|_| io_err("Cannot parse scale factor"))?;

    // ---- Lines 3-5: lattice vectors ----
    let mut lattice = [[0.0f64; 3]; 3];
    for row in &mut lattice {
        let line = lines
            .next()
            .ok_or_else(|| io_err("Unexpected EOF: lattice"))??;
        let vals = parse_floats(&line, 3)?;
        row[0] = vals[0] * scale;
        row[1] = vals[1] * scale;
        row[2] = vals[2] * scale;
    }

    // ---- Line 6: species names (optional in older VASP format) ----
    let species_or_counts = lines
        .next()
        .ok_or_else(|| io_err("Unexpected EOF: species/counts"))??;

    // Detect whether this line is species names or atom counts
    let (species_names, counts_line) = if species_or_counts
        .trim()
        .chars()
        .next()
        .map(|c| c.is_alphabetic())
        .unwrap_or(false)
    {
        let names: Vec<String> = species_or_counts
            .split_whitespace()
            .map(|s| s.to_string())
            .collect();
        let cl = lines
            .next()
            .ok_or_else(|| io_err("Unexpected EOF: atom counts"))??;
        (names, cl)
    } else {
        // Old VASP format without species line — generate placeholder names
        let n_species = species_or_counts.split_whitespace().count();
        let names: Vec<String> = (0..n_species).map(|i| format!("X{}", i + 1)).collect();
        (names, species_or_counts.clone())
    };

    // Parse atom counts
    let atom_counts: Vec<usize> = counts_line
        .split_whitespace()
        .map(|s| s.parse::<usize>().map_err(|_| io_err("Invalid atom count")))
        .collect::<io::Result<_>>()?;
    let total_atoms: usize = atom_counts.iter().sum();

    // ---- Selective dynamics / Coordinate type ----
    let coord_or_selective = lines
        .next()
        .ok_or_else(|| io_err("Unexpected EOF: coord type"))??;

    let is_cartesian = if coord_or_selective.trim().to_lowercase().starts_with('s') {
        // Selective dynamics: skip this line, next is the actual coord type
        let actual_coord = lines
            .next()
            .ok_or_else(|| io_err("Unexpected EOF: coord type after selective"))??;
        let low = actual_coord.trim().to_lowercase();
        low.starts_with('c') || low.starts_with('k')
    } else {
        let low = coord_or_selective.trim().to_lowercase();
        low.starts_with('c') || low.starts_with('k')
    };

    // ---- Atom positions — parse them for overlay ----
    let mut atoms = Vec::with_capacity(total_atoms);
    let mut species_idx = 0;
    let mut count_in_species = 0;

    for _ in 0..total_atoms {
        let line = lines
            .next()
            .ok_or_else(|| io_err("Unexpected EOF: atom positions"))??;
        let vals = parse_floats(&line, 3)?;

        // Determine which species this atom belongs to
        while species_idx < atom_counts.len() && count_in_species >= atom_counts[species_idx] {
            species_idx += 1;
            count_in_species = 0;
        }
        let element = if species_idx < species_names.len() {
            species_names[species_idx].clone()
        } else {
            format!("X{}", species_idx + 1)
        };
        count_in_species += 1;

        let frac_coords = if is_cartesian {
            cart_to_frac_inline(&[vals[0], vals[1], vals[2]], &lattice)
        } else {
            [vals[0], vals[1], vals[2]]
        };

        atoms.push(ChgcarAtom {
            element,
            frac_coords,
        });
    }

    // ---- Blank line before grid ----
    let grid_line = skip_blanks_get_line(&mut lines)?;

    // ---- Grid dimensions ----
    let grid_vals = parse_ints(&grid_line, 3)?;
    let grid = [grid_vals[0], grid_vals[1], grid_vals[2]];
    let n_points = grid[0] * grid[1] * grid[2];

    // ---- Read charge density values ----
    let charge_total = read_density_values(&mut lines, n_points)?;

    // ---- Check for spin-polarized second grid ----
    let charge_mag = try_read_second_grid(&mut lines, grid, n_points);
    let spin_polarized = charge_mag.is_some();

    Ok(ChgcarData {
        lattice,
        grid,
        charge_total,
        charge_mag,
        spin_polarized,
        atoms,
        species: species_names,
    })
}

/// Compute charge density difference between two CHGCAR files.
/// Both grids must have identical dimensions.
pub fn compute_difference(a: &ChgcarData, b: &ChgcarData) -> io::Result<ChgcarData> {
    if a.grid != b.grid {
        return Err(io_err(&format!(
            "Grid mismatch: {:?} vs {:?}",
            a.grid, b.grid
        )));
    }
    let charge_total: Vec<f64> = a
        .charge_total
        .iter()
        .zip(b.charge_total.iter())
        .map(|(x, y)| x - y)
        .collect();

    let charge_mag = match (&a.charge_mag, &b.charge_mag) {
        (Some(ma), Some(mb)) => Some(
            ma.iter()
                .zip(mb.iter())
                .map(|(x, y)| x - y)
                .collect::<Vec<f64>>(),
        ),
        _ => None,
    };

    Ok(ChgcarData {
        lattice: a.lattice,
        grid: a.grid,
        charge_total,
        charge_mag,
        spin_polarized: a.spin_polarized && b.spin_polarized,
        atoms: a.atoms.clone(),
        species: a.species.clone(),
    })
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

fn io_err(msg: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, msg)
}

fn parse_floats(line: &str, expected: usize) -> io::Result<Vec<f64>> {
    let vals: Vec<f64> = line
        .split_whitespace()
        .take(expected)
        .map(|s| {
            s.parse::<f64>()
                .map_err(|_| io_err(&format!("Cannot parse float: {}", s)))
        })
        .collect::<io::Result<_>>()?;
    if vals.len() < expected {
        return Err(io_err(&format!(
            "Expected {} floats, got {}",
            expected,
            vals.len()
        )));
    }
    Ok(vals)
}

fn parse_ints(line: &str, expected: usize) -> io::Result<Vec<usize>> {
    let vals: Vec<usize> = line
        .split_whitespace()
        .take(expected)
        .map(|s| {
            s.parse::<usize>()
                .map_err(|_| io_err(&format!("Cannot parse int: {}", s)))
        })
        .collect::<io::Result<_>>()?;
    if vals.len() < expected {
        return Err(io_err("Not enough integers in grid line"));
    }
    Ok(vals)
}

/// Skip blank lines and return the first non-blank line.
fn skip_blanks_get_line<I>(lines: &mut I) -> io::Result<String>
where
    I: Iterator<Item = io::Result<String>>,
{
    loop {
        let line = lines
            .next()
            .ok_or_else(|| io_err("Unexpected EOF while skipping blanks"))??;
        if !line.trim().is_empty() {
            return Ok(line);
        }
    }
}

fn read_density_values<I>(lines: &mut I, n_points: usize) -> io::Result<Vec<f64>>
where
    I: Iterator<Item = io::Result<String>>,
{
    let mut data = Vec::with_capacity(n_points);
    while data.len() < n_points {
        let line = lines
            .next()
            .ok_or_else(|| io_err("Unexpected EOF reading charge density"))??;
        for token in line.split_whitespace() {
            if data.len() >= n_points {
                break;
            }
            let v: f64 = token
                .parse()
                .map_err(|_| io_err(&format!("Cannot parse density value: {}", token)))?;
            data.push(v);
        }
    }
    Ok(data)
}

/// After reading the first grid, VASP appends augmentation data then an
/// optional second grid for spin-polarized data. We scan forward until we
/// find a line of exactly three integers matching our grid dimensions.
fn try_read_second_grid<I>(lines: &mut I, grid: [usize; 3], n_points: usize) -> Option<Vec<f64>>
where
    I: Iterator<Item = io::Result<String>>,
{
    for line_result in lines.by_ref() {
        let line = match line_result {
            Ok(l) => l,
            Err(_) => return None,
        };

        let tokens: Vec<&str> = line.split_whitespace().collect();
        if tokens.len() == 3 {
            let parsed: Option<Vec<usize>> =
                tokens.iter().map(|s| s.parse::<usize>().ok()).collect();
            if let Some(dims) = parsed {
                if dims[0] == grid[0] && dims[1] == grid[1] && dims[2] == grid[2] {
                    match read_density_values(lines, n_points) {
                        Ok(data) => return Some(data),
                        Err(_) => return None,
                    }
                }
            }
        }
    }
    None
}

/// Inline Cartesian → fractional conversion.
fn cart_to_frac_inline(cart: &[f64; 3], lattice: &[[f64; 3]; 3]) -> [f64; 3] {
    use nalgebra::{Matrix3, Vector3};
    let lat = Matrix3::new(
        lattice[0][0],
        lattice[0][1],
        lattice[0][2],
        lattice[1][0],
        lattice[1][1],
        lattice[1][2],
        lattice[2][0],
        lattice[2][1],
        lattice[2][2],
    );
    match lat.transpose().try_inverse() {
        Some(inv) => {
            let c = Vector3::new(cart[0], cart[1], cart[2]);
            let f = inv * c;
            [f.x, f.y, f.z]
        }
        None => [cart[0], cart[1], cart[2]],
    }
}
