// src/physics/analysis/orbitals.rs
//
// The orbitals of a DFT+U shell, from its on-site density matrix (see
// `io::ldau`), and 3D glyphs to draw them.
//
// - Basis: VASP's real spherical harmonics, m = -l..l in VASP's order
//   (p: y z x; d: xy yz z² xz x²−y²; f: y(3x²−y²) xyz yz² z³ xz² z(x²−y²)
//   x(x²−3y²)), unit-normalised, in the Cartesian frame of the cell. The f
//   order and signs were checked against a CHGCAR spin density (a Ce³⁺
//   polaron: overlap 1.000 between the fitted and the printed eigenvector).
// - Occupied orbitals: eigenvectors of each spin's matrix, with their
//   occupations. A localised electron (Ce³⁺ 4f¹, an eg hole) shows as one
//   eigenvalue near 1 among small ones.
// - Shell spin density: ρ↑ − ρ↓ of the shell, Σ (n↑ − n↓)_mm' Y_m Y_m'.
//   It shows orbital order even when no single eigenvector stands out.
// - Glyphs: an isosurface of R(r)·A(Ω) on a small local grid, with a
//   Slater-type radial part R = r^l e^(−r) (squared for a density), so lobes
//   look like the textbook orbital rather than a polar plot. Positive and
//   negative parts are separate meshes: the wavefunction's phase, or the
//   sign of the spin density. Coordinates are in units of the radial decay
//   length, centred on the atom; the caller scales and places them.

use crate::io::ldau::OnsiteMatrix;
use crate::physics::analysis::isosurface::{isosurface, Grid, Inside, Mesh, Region};
use crate::utils::task::CancelToken;
use nalgebra::{DMatrix, DVector, SymmetricEigen, Vector3};
use std::f64::consts::PI;

/// VASP's real spherical harmonics of degree `l` (0–3) at the unit vector
/// `d`, m = −l..l.
pub fn real_harmonics(l: usize, d: Vector3<f64>) -> Vec<f64> {
    let (x, y, z) = (d.x, d.y, d.z);
    match l {
        0 => vec![0.5 / PI.sqrt()],
        1 => {
            let c = (3.0 / (4.0 * PI)).sqrt();
            vec![c * y, c * z, c * x]
        }
        2 => {
            let a = 0.5 * (15.0 / PI).sqrt();
            vec![
                a * x * y,
                a * y * z,
                0.25 * (5.0 / PI).sqrt() * (3.0 * z * z - 1.0),
                a * x * z,
                0.25 * (15.0 / PI).sqrt() * (x * x - y * y),
            ]
        }
        3 => {
            let c3 = 0.25 * (35.0 / (2.0 * PI)).sqrt();
            let c2 = 0.5 * (105.0 / PI).sqrt();
            let c1 = 0.25 * (21.0 / (2.0 * PI)).sqrt();
            let c0 = 0.25 * (7.0 / PI).sqrt();
            vec![
                c3 * y * (3.0 * x * x - y * y),
                c2 * x * y * z,
                c1 * y * (5.0 * z * z - 1.0),
                c0 * z * (5.0 * z * z - 3.0),
                c1 * x * (5.0 * z * z - 1.0),
                0.5 * c2 * z * (x * x - y * y),
                c3 * x * (x * x - 3.0 * y * y),
            ]
        }
        _ => Vec::new(),
    }
}

/// Spectroscopic letter of a shell.
pub fn shell_letter(l: usize) -> char {
    ['s', 'p', 'd', 'f'].get(l).copied().unwrap_or('?')
}

/// One eigenvector of a spin block of an on-site density matrix.
#[derive(Clone, Debug, PartialEq)]
pub struct Orbital {
    pub atom: usize,
    pub l: usize,
    /// 0 = up (or unpolarised), 1 = down.
    pub spin: usize,
    pub occupation: f64,
    /// Coefficients on the real harmonics, unit norm; the largest is positive.
    pub coeffs: Vec<f64>,
}

/// Eigenvectors of every spin block with occupation ≥ `threshold`, most
/// occupied first.
pub fn occupied_orbitals(m: &OnsiteMatrix, threshold: f64) -> Vec<Orbital> {
    let mut out = Vec::new();
    for (spin, mat) in m.spins.iter().enumerate() {
        let sym = (mat + mat.transpose()) * 0.5;
        let eig = SymmetricEigen::new(sym);
        for (k, &occ) in eig.eigenvalues.iter().enumerate() {
            if occ < threshold {
                continue;
            }
            let mut v: Vec<f64> = eig.eigenvectors.column(k).iter().copied().collect();
            let big = v.iter().copied().fold(0.0f64, |a, b| if b.abs() > a.abs() { b } else { a });
            if big < 0.0 {
                v.iter_mut().for_each(|c| *c = -*c);
            }
            out.push(Orbital { atom: m.atom, l: m.l, spin, occupation: occ, coeffs: v });
        }
    }
    out.sort_by(|a, b| b.occupation.total_cmp(&a.occupation));
    out
}

/// n↑ − n↓ of the shell (zero for an unpolarised run).
pub fn spin_matrix(m: &OnsiteMatrix) -> DMatrix<f64> {
    match m.spins.as_slice() {
        [up, down] => ((up - down) + (up - down).transpose()) * 0.5,
        [one] => DMatrix::zeros(one.nrows(), one.ncols()),
        _ => DMatrix::zeros(0, 0),
    }
}

/// Shell moment tr(n↑ − n↓), μB.
pub fn shell_moment(m: &OnsiteMatrix) -> f64 {
    spin_matrix(m).trace()
}

/// What a glyph draws.
#[derive(Clone, Debug)]
pub enum Shape {
    /// ψ(Ω) = Σ c_m Y_m: lobes coloured by phase.
    Wavefunction(Vec<f64>),
    /// ρ(Ω) = Σ n_mm' Y_m Y_m': lobes coloured by sign.
    Density(DMatrix<f64>),
}

/// Positive and negative lobes of a glyph, centred on the origin.
pub struct Glyph {
    pub pos: Mesh,
    pub neg: Mesh,
}

/// Grid points per axis for a glyph.
const GLYPH_GRID: usize = 56;

fn half_width(l: usize) -> f64 {
    2.0 * l as f64 + 4.0
}

fn radial(l: usize, r: f64) -> f64 {
    r.powi(l as i32) * (-r).exp()
}

/// The glyph's field on its local grid (x fastest), and its largest |value|.
fn sample(l: usize, shape: &Shape) -> (Vec<f32>, f64) {
    let n = GLYPH_GRID;
    let hw = half_width(l);
    let mut data = vec![0f32; n * n * n];
    let mut max = 0.0f64;
    for k in 0..n {
        for j in 0..n {
            for i in 0..n {
                let p = Vector3::new(i as f64, j as f64, k as f64) * (2.0 * hw / n as f64) - Vector3::repeat(hw);
                let r = p.norm();
                let v = if r < 1e-9 {
                    0.0
                } else {
                    let y = DVector::from_vec(real_harmonics(l, p / r));
                    match shape {
                        Shape::Wavefunction(c) => radial(l, r) * y.dot(&DVector::from_column_slice(c)),
                        Shape::Density(nm) => radial(l, r).powi(2) * (y.transpose() * nm * &y)[(0, 0)],
                    }
                };
                max = max.max(v.abs());
                data[i + n * (j + n * k)] = v as f32;
            }
        }
    }
    (data, max)
}

/// Largest |value| of a glyph's field, to give several glyphs one isovalue.
pub fn glyph_peak(l: usize, shape: &Shape) -> f64 {
    sample(l, shape).1
}

/// Mesh the lobes at `fraction` of `reference` (or of the glyph's own peak).
pub fn glyph(l: usize, shape: &Shape, fraction: f64, reference: Option<f64>, cancel: &CancelToken) -> Option<Glyph> {
    let (data, max) = sample(l, shape);
    let level = (fraction * reference.unwrap_or(max)).max(1e-12) as f32;
    let hw = half_width(l);
    let side = 2.0 * hw;
    let grid = Grid {
        data: &data,
        dims: [GLYPH_GRID; 3],
        lattice: [[side, 0.0, 0.0], [0.0, side, 0.0], [0.0, 0.0, side]],
    };
    let region = Region::cell(grid.dims);
    let shift = |mut m: Mesh| {
        for p in &mut m.positions {
            for c in p.iter_mut() {
                *c -= hw as f32;
            }
        }
        m
    };
    let pos = shift(isosurface(&grid, level, Inside::Above, region, cancel)?);
    let neg = shift(isosurface(&grid, -level, Inside::Below, region, cancel)?);
    Some(Glyph { pos, neg })
}

/// Which glyphs to draw for a set of atoms.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum GlyphKind {
    /// Eigenvectors with occupation ≥ threshold, coloured by phase.
    Orbitals,
    /// The shell's ρ↑ − ρ↓ where |tr(n↑ − n↓)| ≥ threshold, coloured by sign,
    /// all at one isovalue so weaker shells look smaller.
    SpinDensity,
}

/// How glyphs are chosen and drawn.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GlyphParams {
    pub kind: GlyphKind,
    /// Occupation (orbitals) or |shell moment| in μB (spin density).
    pub threshold: f64,
    /// Isovalue as a fraction of the peak.
    pub fraction: f64,
    /// Reach of the largest lobe from its atom, Å.
    pub size: f64,
}

/// All glyphs of a view, merged into two meshes in Å.
pub struct GlyphSet {
    pub pos: Mesh,
    pub neg: Mesh,
    /// One line per atom drawn (for Structure Info).
    pub report: String,
    /// Atoms with a glyph (not counting periodic images).
    pub atoms: usize,
    /// The first shell drawn, e.g. "Ce f", for keys.
    pub label: Option<String>,
}

fn append(dst: &mut Mesh, src: &Mesh, scale: f32, at: Vector3<f64>) {
    let base = dst.positions.len() as u32;
    let o = [at.x as f32, at.y as f32, at.z as f32];
    dst.positions.extend(src.positions.iter().map(|p| [p[0] * scale + o[0], p[1] * scale + o[1], p[2] * scale + o[2]]));
    dst.normals.extend_from_slice(&src.normals);
    dst.indices.extend(src.indices.iter().map(|i| i + base));
}

fn empty_mesh() -> Mesh {
    Mesh { positions: Vec::new(), normals: Vec::new(), indices: Vec::new() }
}

/// Glyphs for `blocks`, placed at `placements` (atom index, Cartesian Å; an
/// atom may appear several times as periodic images). `names` gives each
/// atom's element.
pub fn build_glyph_set(
    blocks: &[OnsiteMatrix],
    names: &[String],
    placements: &[(usize, Vector3<f64>)],
    p: &GlyphParams,
    cancel: &CancelToken,
) -> Option<GlyphSet> {
    let GlyphParams { kind, threshold, fraction, size } = *p;
    // What to draw per atom.
    let mut shapes: Vec<(usize, usize, Shape, String)> = Vec::new();
    for b in blocks {
        let name = names.get(b.atom).cloned().unwrap_or_else(|| "?".into());
        let shell = format!("{}{}", name, shell_letter(b.l));
        match kind {
            GlyphKind::Orbitals => {
                for o in occupied_orbitals(b, threshold) {
                    let spin = match (b.spins.len(), o.spin) {
                        (1, _) => "",
                        (_, 0) => " ↑",
                        _ => " ↓",
                    };
                    let comps: Vec<String> = o.coeffs.iter().map(|c| format!("{c:+.2}")).collect();
                    let line = format!(
                        "  {}{:<4} {}{spin}  n = {:.3}  c(m = −{l}..{l}) = [{}]",
                        name,
                        b.atom + 1,
                        shell,
                        o.occupation,
                        comps.join(" "),
                        l = b.l
                    );
                    shapes.push((b.atom, b.l, Shape::Wavefunction(o.coeffs), line));
                }
            }
            GlyphKind::SpinDensity => {
                let m = shell_moment(b);
                if m.abs() >= threshold {
                    let line = format!("  {}{:<4} {}  m = {:+.3} μB", name, b.atom + 1, shell, m);
                    shapes.push((b.atom, b.l, Shape::Density(spin_matrix(b)), line));
                }
            }
        }
    }
    let reference = match kind {
        GlyphKind::SpinDensity => Some(shapes.iter().map(|(_, l, sh, _)| glyph_peak(*l, sh)).fold(0.0, f64::max)),
        GlyphKind::Orbitals => None,
    };
    let mut units: Vec<(usize, Glyph)> = Vec::new();
    for (atom, l, shape, _) in &shapes {
        units.push((*atom, glyph(*l, shape, fraction, reference, cancel)?));
    }
    // One scale for all: the largest lobe reaches `size`.
    let reach = units
        .iter()
        .flat_map(|(_, g)| g.pos.positions.iter().chain(&g.neg.positions))
        .map(|p| (p[0] * p[0] + p[1] * p[1] + p[2] * p[2]).sqrt())
        .fold(0f32, f32::max);
    let scale = if reach > 0.0 { (size as f32) / reach } else { 1.0 };
    let (mut pos, mut neg) = (empty_mesh(), empty_mesh());
    for (atom, g) in &units {
        for (_, at) in placements.iter().filter(|(i, _)| i == atom) {
            append(&mut pos, &g.pos, scale, *at);
            append(&mut neg, &g.neg, scale, *at);
        }
    }
    let mut drawn: Vec<usize> = shapes.iter().map(|s| s.0).collect();
    drawn.dedup();
    let title = match kind {
        GlyphKind::Orbitals => format!("Occupied orbitals (n ≥ {threshold:.2}), from the DFT+U occupation matrices"),
        GlyphKind::SpinDensity => format!("Shell spin densities (|m| ≥ {threshold:.2} μB), from the DFT+U occupation matrices"),
    };
    let lines: Vec<&str> = shapes.iter().map(|s| s.3.as_str()).collect();
    let report = if lines.is_empty() {
        format!("{title}: none above the threshold\n")
    } else {
        format!("{title}:\n{}\n", lines.join("\n"))
    };
    let label = shapes.first().map(|s| format!("{} {}", names.get(s.0).map_or("?", String::as_str), shell_letter(s.1)));
    Some(GlyphSet { pos, neg, report, atoms: drawn.len(), label })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sphere(n: usize) -> Vec<Vector3<f64>> {
        (0..n)
            .map(|k| {
                let t = (k as f64 + 0.5) / n as f64;
                let phi = (1.0 - 2.0 * t).acos();
                let th = PI * (1.0 + 5f64.sqrt()) * k as f64;
                Vector3::new(th.cos() * phi.sin(), th.sin() * phi.sin(), phi.cos())
            })
            .collect()
    }

    #[test]
    fn harmonics_are_orthonormal() {
        let pts = sphere(40_000);
        for l in 0..=3 {
            let n = 2 * l + 1;
            let mut g = DMatrix::<f64>::zeros(n, n);
            for p in &pts {
                let y = DVector::from_vec(real_harmonics(l, *p));
                g += &y * y.transpose();
            }
            g *= 4.0 * PI / pts.len() as f64;
            assert!((g - DMatrix::identity(n, n)).abs().max() < 2e-3, "l = {l}");
        }
    }

    #[test]
    fn vasp_order_puts_z_cubed_in_the_middle() {
        let y = real_harmonics(3, Vector3::z());
        let i = y.iter().enumerate().max_by(|a, b| a.1.abs().total_cmp(&b.1.abs())).unwrap().0;
        assert_eq!(i, 3);
        let y = real_harmonics(2, Vector3::new(1.0, 1.0, 0.0).normalize());
        assert!(y[0] > 0.5 && y[4].abs() < 1e-12); // d_xy along [110], d_x²−y² zero
    }

    fn one_electron(l: usize, coeffs: &[f64]) -> OnsiteMatrix {
        let n = 2 * l + 1;
        let c = DVector::from_column_slice(coeffs).normalize();
        let up = &c * c.transpose() * 0.98 + DMatrix::identity(n, n) * 0.02;
        let down = DMatrix::identity(n, n) * 0.02;
        OnsiteMatrix { atom: 4, l, spins: vec![up, down] }
    }

    #[test]
    fn finds_the_localised_electron() {
        let m = one_electron(3, &[0.0, 0.0, 0.0, -1.0, 0.0, 0.0, 0.0]);
        let o = occupied_orbitals(&m, 0.5);
        assert_eq!(o.len(), 1);
        assert_eq!((o[0].atom, o[0].spin), (4, 0));
        assert!((o[0].occupation - 1.0).abs() < 1e-9);
        assert!((o[0].coeffs[3] - 1.0).abs() < 1e-9); // sign fixed: largest positive
        assert!((shell_moment(&m) - 0.98).abs() < 1e-9);
    }

    fn params(kind: GlyphKind, threshold: f64) -> GlyphParams {
        GlyphParams { kind, threshold, fraction: 0.3, size: 1.5 }
    }

    #[test]
    fn glyph_sets_place_scale_and_report() {
        let b = one_electron(3, &[0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0]);
        let names = vec!["O".to_string(); 4].into_iter().chain(["Ce".to_string()]).collect::<Vec<_>>();
        let at = Vector3::new(10.0, 0.0, 0.0);
        let placements = [(4usize, at), (4, at + Vector3::new(0.0, 5.0, 0.0)), (1, Vector3::zeros())];
        let set = build_glyph_set(&[b.clone()], &names, &placements, &params(GlyphKind::Orbitals, 0.5), &CancelToken::never()).unwrap();
        assert_eq!((set.atoms, set.label.as_deref()), (1, Some("Ce f")));
        assert!(set.report.contains("Ce5") && set.report.contains("Cef ↑") && set.report.contains("n = 1.000"));
        // Two images, each reaching 1.5 Å from its atom.
        let reach = set.pos.positions.iter().chain(&set.neg.positions).map(|p| {
            let d = Vector3::new(p[0] as f64, p[1] as f64, p[2] as f64) - at;
            let d2 = d - Vector3::new(0.0, 5.0, 0.0);
            d.norm().min(d2.norm())
        });
        let r = reach.fold(0.0, f64::max);
        assert!((r - 1.5).abs() < 1e-3, "{r}");
        // Spin density: a shell below the threshold is skipped.
        let set = build_glyph_set(&[b], &names, &placements, &params(GlyphKind::SpinDensity, 1.5), &CancelToken::never()).unwrap();
        assert_eq!(set.atoms, 0);
        assert!(set.pos.is_empty() && set.report.contains("none"));
    }

    #[test]
    fn z_cubed_glyph_points_along_z() {
        let c = vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0];
        let g = glyph(3, &Shape::Wavefunction(c), 0.3, None, &CancelToken::never()).unwrap();
        // f_z³ has its big lobes on ±z (+ phase up, − phase down); the
        // cone-shaped bands around the equator are smaller.
        let norm = |p: &[f32; 3]| (p[0] * p[0] + p[1] * p[1] + p[2] * p[2]).sqrt();
        let far = |m: &Mesh| *m.positions.iter().max_by(|a, b| norm(a).total_cmp(&norm(b))).unwrap();
        let (p, q) = (far(&g.pos), far(&g.neg));
        assert!(p[2] / norm(&p) > 0.95 && q[2] / norm(&q) < -0.95, "{p:?} {q:?}");
        let top = g.pos.positions.iter().map(|p| p[2]).fold(f32::MIN, f32::max);
        let bottom = g.neg.positions.iter().map(|p| p[2]).fold(f32::MAX, f32::min);
        assert!(top > 2.0 && bottom < -2.0);
        // A density glyph of the same orbital has one sign only.
        let d = one_electron(3, &[0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0]);
        let g = glyph(3, &Shape::Density(spin_matrix(&d)), 0.3, None, &CancelToken::never()).unwrap();
        assert!(!g.pos.is_empty() && g.neg.is_empty());
    }
}
