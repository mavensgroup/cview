// src/physics/operations/spacegroup_build.rs
//
// Build a crystal from a space group, cell parameters and the asymmetric
// unit (the "New Structure" workflow of VESTA), and the reverse: describe an
// existing structure as group + cell + unique sites so it can be edited.
//
// Space groups come from moyo's Hall-symbol table (all 530 settings), so the
// operations are the same ones the symmetry analysis uses.
//
// Every entered site is first moved onto its exact special position (the
// average of the images that land on it, i.e. the projection onto the fixed
// space of its site-symmetry group). Typing 0.333 for 1/3 therefore gives the
// same crystal as typing 1/3, instead of three atoms 0.01 Å apart.

use crate::model::elements::get_atomic_number;
use crate::model::{Atom, Structure};
use crate::physics::analysis::symmetry::SYMPREC;
use crate::physics::operations::conversion::build_formula;
use crate::utils::linalg::{cart_to_frac, frac_to_cart, lattice_to_matrix3};
use moyo::base::{AngleTolerance, Cell, Lattice};
use moyo::data::{hall_symbol_entry, HallSymbol, Setting};
use moyo::MoyoDataset;
use nalgebra::{Matrix3, Vector3};
use std::collections::HashMap;

/// Images of one site closer than this (Å) are taken to be the same point:
/// the site sits on a special position and is snapped onto it.
pub const SNAP_TOL: f64 = 0.05;
/// After snapping, images closer than this (Å) are the same atom.
const MERGE_TOL: f64 = 0.01;
/// Distinct atoms of one site closer than this (Å) are almost certainly a
/// typo or a coordinate given to too few digits; reported as a warning.
const CLASH_WARN: f64 = 0.5;

/// Cell parameters in the order a, b, c (Å), α, β, γ (degrees).
pub type CellParams = [f64; 6];

/// One setting (Hall symbol) of a space group.
#[derive(Clone, Debug)]
pub struct SettingChoice {
    pub hall_number: i32,
    pub number: i32,
    pub hm_short: &'static str,
    pub hm_full: &'static str,
    /// moyo/spglib setting code: "", "b", "c1", "1", "2", "H", "R", "ba-c", …
    pub code: &'static str,
}

impl SettingChoice {
    pub fn from_hall(hall_number: i32) -> Option<Self> {
        let e = hall_symbol_entry(hall_number)?;
        Some(Self {
            hall_number,
            number: e.number,
            hm_short: e.hm_short,
            hm_full: e.hm_full,
            code: e.setting,
        })
    }

    /// Human-readable description of the setting, e.g. "unique axis b, cell
    /// choice 1" or "origin choice 2". Empty when the group has one setting.
    pub fn description(&self) -> String {
        let code = self.code;
        if code.is_empty() {
            return String::new();
        }
        match crystal_system(self.number) {
            CrystalSystem::Monoclinic => {
                let axis = code.trim_start_matches('-').chars().next().unwrap_or('b');
                let cell = code.chars().filter(|c| c.is_ascii_digit()).collect::<String>();
                let mut s = format!("unique axis {axis}");
                if !cell.is_empty() {
                    s.push_str(&format!(", cell choice {cell}"));
                }
                if code.starts_with('-') {
                    s.push_str(", reversed");
                }
                s
            }
            _ => match code {
                "1" => "origin choice 1".into(),
                "2" => "origin choice 2".into(),
                "H" => "hexagonal axes".into(),
                "R" => "rhombohedral axes".into(),
                other if other.starts_with('1') || other.starts_with('2') => {
                    let (origin, axes) = other.split_at(1);
                    format!("axes {axes}, origin choice {origin}")
                }
                other => format!("axes {other}"),
            },
        }
    }

    /// "P 1 2_1/c 1  (unique axis b, cell choice 1)".
    pub fn label(&self) -> String {
        let d = self.description();
        let hm = pretty_hm(self.hm_full);
        if d.is_empty() {
            hm
        } else {
            format!("{hm}  ({d})")
        }
    }

    /// Standard short symbol for display ("P2₁/c", "Fm-3m").
    pub fn short_symbol(&self) -> String {
        pretty_hm(self.hm_short).replace(' ', "")
    }
}

/// Hermann–Mauguin symbol with screw axes as subscripts: "P 2_1/c" → "P 2₁/c".
pub fn pretty_hm(hm: &str) -> String {
    const SUB: [char; 10] = ['₀', '₁', '₂', '₃', '₄', '₅', '₆', '₇', '₈', '₉'];
    let mut out = String::with_capacity(hm.len());
    let mut chars = hm.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '_' {
            if let Some(d) = chars.peek().and_then(|d| d.to_digit(10)) {
                out.push(SUB[d as usize]);
                chars.next();
                continue;
            }
        }
        out.push(c);
    }
    out
}

/// All settings of space group `number` (1–230), in table order.
pub fn settings_for(number: i32) -> Vec<SettingChoice> {
    (1..=530)
        .filter_map(SettingChoice::from_hall)
        .filter(|s| s.number == number)
        .collect()
}

/// The setting offered first: unique axis b / cell choice 1, hexagonal axes,
/// origin choice 2 (the ITA standard, as moyo's `Setting::Standard`).
pub fn standard_hall_number(number: i32) -> Option<i32> {
    if !(1..=230).contains(&number) {
        return None;
    }
    Setting::Standard.hall_number(number)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CrystalSystem {
    Triclinic,
    Monoclinic,
    Orthorhombic,
    Tetragonal,
    Trigonal,
    Hexagonal,
    Cubic,
}

impl CrystalSystem {
    pub fn name(self) -> &'static str {
        match self {
            CrystalSystem::Triclinic => "Triclinic",
            CrystalSystem::Monoclinic => "Monoclinic",
            CrystalSystem::Orthorhombic => "Orthorhombic",
            CrystalSystem::Tetragonal => "Tetragonal",
            CrystalSystem::Trigonal => "Trigonal",
            CrystalSystem::Hexagonal => "Hexagonal",
            CrystalSystem::Cubic => "Cubic",
        }
    }
}

pub fn crystal_system(number: i32) -> CrystalSystem {
    match number {
        1..=2 => CrystalSystem::Triclinic,
        3..=15 => CrystalSystem::Monoclinic,
        16..=74 => CrystalSystem::Orthorhombic,
        75..=142 => CrystalSystem::Tetragonal,
        143..=167 => CrystalSystem::Trigonal,
        168..=194 => CrystalSystem::Hexagonal,
        _ => CrystalSystem::Cubic,
    }
}

/// How each cell parameter is set for a setting: free, a copy of another
/// parameter, or fixed at a value.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Param {
    Free,
    SameAs(usize),
    Fixed(f64),
}

/// The metric constraints of the lattice for `hall_number`, indexed like
/// [`CellParams`].
pub fn param_rules(hall_number: i32) -> [Param; 6] {
    use Param::*;
    let Some(s) = SettingChoice::from_hall(hall_number) else {
        return [Free; 6];
    };
    let right = Fixed(90.0);
    match crystal_system(s.number) {
        CrystalSystem::Triclinic => [Free; 6],
        CrystalSystem::Monoclinic => {
            let axis = s.code.trim_start_matches('-').chars().next().unwrap_or('b');
            let mut r = [Free, Free, Free, right, right, right];
            let free_angle = match axis {
                'a' => 3,
                'c' => 5,
                _ => 4,
            };
            r[free_angle] = Free;
            r
        }
        CrystalSystem::Orthorhombic => [Free, Free, Free, right, right, right],
        CrystalSystem::Tetragonal => [Free, SameAs(0), Free, right, right, right],
        CrystalSystem::Trigonal | CrystalSystem::Hexagonal if s.code == "R" => {
            [Free, SameAs(0), SameAs(0), Free, SameAs(3), SameAs(3)]
        }
        CrystalSystem::Trigonal | CrystalSystem::Hexagonal => {
            [Free, SameAs(0), Free, right, right, Fixed(120.0)]
        }
        CrystalSystem::Cubic => [Free, SameAs(0), SameAs(0), right, right, right],
    }
}

/// `params` with the constraints of `hall_number` applied (dependent values
/// copied, fixed angles set).
pub fn constrain(hall_number: i32, params: CellParams) -> CellParams {
    let rules = param_rules(hall_number);
    let mut out = params;
    for (i, r) in rules.iter().enumerate() {
        match *r {
            Param::Free => {}
            Param::SameAs(j) => out[i] = params[j],
            Param::Fixed(v) => out[i] = v,
        }
    }
    out
}

/// Lattice vectors (rows) for cell parameters, in the usual orientation:
/// a along x, b in the xy plane. Shared with the CIF reader.
pub fn lattice_from_params(p: CellParams) -> Result<[[f64; 3]; 3], String> {
    let [a, b, c, alpha, beta, gamma] = p;
    if !(a > 0.0 && b > 0.0 && c > 0.0) {
        return Err("Cell lengths must be positive.".into());
    }
    if [alpha, beta, gamma].iter().any(|x| !(*x > 0.0 && *x < 180.0)) {
        return Err("Cell angles must lie between 0° and 180°.".into());
    }
    let (ca, cb, cg) = (
        alpha.to_radians().cos(),
        beta.to_radians().cos(),
        gamma.to_radians().cos(),
    );
    let sg = gamma.to_radians().sin();
    let v2 = 1.0 - ca * ca - cb * cb - cg * cg + 2.0 * ca * cb * cg;
    if v2 <= 1e-10 {
        return Err("These angles do not form a cell (zero or imaginary volume).".into());
    }
    let v = v2.sqrt();
    Ok([
        [a, 0.0, 0.0],
        [b * cg, b * sg, 0.0],
        [c * cb, c * (ca - cb * cg) / sg, c * v / sg],
    ])
}

/// Cell parameters of a lattice (rows = vectors).
pub fn params_from_lattice(lat: [[f64; 3]; 3]) -> CellParams {
    let len = |v: [f64; 3]| (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    let ang = |u: [f64; 3], v: [f64; 3]| {
        let d = u[0] * v[0] + u[1] * v[1] + u[2] * v[2];
        (d / (len(u) * len(v))).clamp(-1.0, 1.0).acos().to_degrees()
    };
    [
        len(lat[0]),
        len(lat[1]),
        len(lat[2]),
        ang(lat[1], lat[2]),
        ang(lat[0], lat[2]),
        ang(lat[0], lat[1]),
    ]
}

/// One symmetry operation in fractional coordinates: x' = R x + t.
#[derive(Clone, Copy, Debug)]
pub struct SymOp {
    pub rotation: Matrix3<i32>,
    pub translation: Vector3<f64>,
}

impl SymOp {
    fn apply(&self, x: [f64; 3]) -> [f64; 3] {
        let r = self.rotation.map(|e| e as f64);
        let y = r * Vector3::from(x) + self.translation;
        [y.x, y.y, y.z]
    }
}

/// Every operation of the setting modulo lattice translations of the
/// conventional cell, centring translations included.
pub fn operations(hall_number: i32) -> Result<Vec<SymOp>, String> {
    let hs = HallSymbol::from_hall_number(hall_number)
        .ok_or_else(|| format!("Unknown space-group setting (Hall number {hall_number})."))?;
    let coset = hs.traverse();
    let mut ops = Vec::with_capacity(coset.len() * hs.centering.order());
    for t in hs.centering.lattice_points() {
        for op in &coset {
            ops.push(SymOp {
                rotation: op.rotation,
                translation: (op.translation + t).map(|e| e.rem_euclid(1.0)),
            });
        }
    }
    Ok(ops)
}

// ---------------------------------------------------------------------------
// Wyckoff positions
// ---------------------------------------------------------------------------

/// Parameter values given to the free coordinates of a newly chosen Wyckoff
/// position. Chosen to lie on no special position in any setting (tested),
/// so a fresh site shows the multiplicity of the position it was put on.
pub const GENERIC_PARAMS: [f64; 3] = [0.434, 0.139, 0.413];

/// One Wyckoff position of a setting: coordinates = linear · (x, y, z) + origin.
#[derive(Clone, Debug)]
pub struct Wyckoff {
    pub letter: char,
    /// Atoms per conventional cell.
    pub multiplicity: usize,
    pub site_symmetry: &'static str,
    /// Representative coordinates as in International Tables: "x,1/4,z".
    pub coordinates: &'static str,
    linear: Matrix3<f64>,
    pinv: Matrix3<f64>,
    origin: Vector3<f64>,
}

impl Wyckoff {
    fn parse(row: &crate::model::wyckoff_table::WyckoffRow) -> Option<Self> {
        let (_, mult, letter, sym, coords) = *row;
        let (linear, origin) = parse_wyckoff_coords(coords)?;
        let linear = linear.map(|e| e as f64);
        Some(Self {
            letter,
            multiplicity: mult as usize,
            site_symmetry: sym,
            coordinates: coords,
            pinv: linear.pseudo_inverse(1e-9).ok()?,
            linear,
            origin,
        })
    }

    /// Which of x, y, z are free parameters of this position.
    pub fn free(&self) -> [bool; 3] {
        std::array::from_fn(|j| (0..3).any(|i| self.linear[(i, j)] != 0.0))
    }

    /// The coordinates for parameter values `p` (unused entries ignored).
    pub fn position(&self, p: [f64; 3]) -> [f64; 3] {
        let v = self.linear * Vector3::from(p) + self.origin;
        wrap([v.x, v.y, v.z])
    }

    /// "4a".
    pub fn name(&self) -> String {
        format!("{}{}", self.multiplicity, self.letter)
    }

    /// "4a   m-3m   (0, 0, 0)".
    pub fn label(&self) -> String {
        format!("{}   {}   ({})", self.name(), self.site_symmetry, self.coordinates.replace(',', ", "))
    }

    /// Parameters that put this position's representative on `frac` (or a
    /// lattice translate of it), if it lies on it.
    pub fn params_for(&self, frac: [f64; 3]) -> Option<[f64; 3]> {
        let f = Vector3::from(frac);
        for t in lattice_shifts() {
            let r = f + t - self.origin;
            let v = self.pinv * r;
            if (self.linear * v - r).amax() < 1e-4 {
                let free = self.free();
                // Integer coefficients: whole-cell shifts of a parameter move
                // the point by a lattice vector, so wrapping is safe.
                return Some(std::array::from_fn(|j| {
                    if free[j] {
                        wrap([v[j], 0.0, 0.0])[0]
                    } else {
                        0.0
                    }
                }));
            }
        }
        None
    }

    /// Parameters placing this position on any symmetry copy of `frac`.
    pub fn fit_orbit(&self, frac: [f64; 3], ops: &[SymOp]) -> Option<[f64; 3]> {
        ops.iter().find_map(|op| self.params_for(wrap(op.apply(frac))))
    }
}

fn lattice_shifts() -> impl Iterator<Item = Vector3<f64>> {
    (0..27).map(|k| {
        let d = |n: i32| [0.0, -1.0, 1.0][n as usize];
        Vector3::new(d(k % 3), d((k / 3) % 3), d(k / 9))
    })
}

/// Parse a coordinate triplet such as "x,-x+1/2,2z+1/4" into the linear part
/// (integer coefficients of x, y, z per coordinate) and the origin.
fn parse_wyckoff_coords(s: &str) -> Option<(Matrix3<i32>, Vector3<f64>)> {
    let s = s.replace(' ', "");
    let terms: Vec<&str> = s.split(',').collect();
    if terms.len() != 3 {
        return None;
    }
    let mut lin = Matrix3::zeros();
    let mut org = Vector3::zeros();
    for (i, term) in terms.iter().enumerate() {
        let mut tokens: Vec<(i32, String)> = Vec::new();
        let mut sign = 1;
        let mut tok = String::new();
        for c in term.chars() {
            if c == '+' || c == '-' {
                if !tok.is_empty() {
                    tokens.push((sign, std::mem::take(&mut tok)));
                }
                sign = if c == '-' { -1 } else { 1 };
            } else {
                tok.push(c);
            }
        }
        if !tok.is_empty() {
            tokens.push((sign, tok));
        }
        for (sign, t) in tokens {
            let last = t.chars().last()?;
            if let Some(j) = "xyz".find(last) {
                let coeff = if t.len() == 1 { 1 } else { t[..t.len() - 1].parse::<i32>().ok()? };
                lin[(i, j)] += sign * coeff;
            } else {
                org[i] += sign as f64 * parse_coord(&t)?;
            }
        }
    }
    Some((lin, org))
}

/// Wyckoff positions of a setting, in International Tables order (general
/// position first, 'a' last).
pub fn wyckoff_positions(hall_number: i32) -> Vec<Wyckoff> {
    crate::model::wyckoff_table::WYCKOFF_TABLE
        .iter()
        .filter(|r| r.0 as i32 == hall_number)
        .filter_map(Wyckoff::parse)
        .collect()
}

/// The most special Wyckoff position `frac` lies on (lowest multiplicity),
/// with its parameters. `orbit_size`, when known, limits the search to the
/// positions of that multiplicity.
pub fn identify_wyckoff<'a>(
    positions: &'a [Wyckoff],
    ops: &[SymOp],
    frac: [f64; 3],
    orbit_size: Option<usize>,
) -> Option<(&'a Wyckoff, [f64; 3])> {
    let mut order: Vec<&Wyckoff> = positions
        .iter()
        .filter(|w| orbit_size.map_or(true, |n| w.multiplicity == n))
        .collect();
    order.sort_by_key(|w| w.multiplicity);
    order
        .into_iter()
        .find_map(|w| w.fit_orbit(frac, ops).map(|p| (w, p)))
}

/// A unique site of the asymmetric unit.
#[derive(Clone, Debug, PartialEq)]
pub struct Site {
    pub label: String,
    pub element: String,
    pub frac: [f64; 3],
    pub occupancy: f64,
    pub oxidation: Option<i32>,
    /// Wyckoff letter the site was entered on (or found on), when known.
    pub wyckoff: Option<char>,
}

/// What became of one entered site.
#[derive(Clone, Debug)]
pub struct SiteResult {
    /// Atoms the site generates in the conventional cell.
    pub multiplicity: usize,
    /// The position after snapping onto its special position.
    pub frac: [f64; 3],
    /// How far snapping moved it (Å).
    pub moved: f64,
    /// The Wyckoff position it ended up on, e.g. "4a".
    pub wyckoff: Option<String>,
}

#[derive(Clone, Debug)]
pub struct Built {
    pub structure: Structure,
    pub sites: Vec<SiteResult>,
    /// Things worth telling the user that did not stop the build.
    pub warnings: Vec<String>,
}

/// Wrap into [0, 1), sending values that round to 1 to 0.
fn wrap(x: [f64; 3]) -> [f64; 3] {
    x.map(|v| {
        let w = v.rem_euclid(1.0);
        if w >= 1.0 - 1e-9 {
            0.0
        } else {
            w
        }
    })
}

/// `x` moved by whole cells to lie as close as possible to `near`.
fn unwrap_near(x: [f64; 3], near: [f64; 3]) -> [f64; 3] {
    [
        x[0] - (x[0] - near[0]).round(),
        x[1] - (x[1] - near[1]).round(),
        x[2] - (x[2] - near[2]).round(),
    ]
}

/// Minimum-image distance (Å) between two fractional points. Rounding each
/// component is exact for the short distances this is used for.
fn min_image_dist(lat: &[[f64; 3]; 3], a: [f64; 3], b: [f64; 3]) -> f64 {
    let d = [a[0] - b[0], a[1] - b[1], a[2] - b[2]].map(|v| v - v.round());
    let c = frac_to_cart(d, *lat);
    (c[0] * c[0] + c[1] * c[1] + c[2] * c[2]).sqrt()
}

/// Element symbol with standard capitalisation ("fe" → "Fe"), or None if it
/// is not an element.
pub fn normalize_element(s: &str) -> Option<String> {
    let t = s.trim();
    let mut chars = t.chars();
    let first = chars.next()?;
    if !first.is_ascii_alphabetic() || t.len() > 2 || !chars.all(|c| c.is_ascii_alphabetic()) {
        return None;
    }
    let sym: String = first
        .to_uppercase()
        .chain(t.chars().skip(1).flat_map(|c| c.to_lowercase()))
        .collect();
    (get_atomic_number(&sym) > 0).then_some(sym)
}

/// A fractional coordinate as typed: "0.25", "-0.1", "1/3", "-2/3", "+1/4".
pub fn parse_coord(s: &str) -> Option<f64> {
    let t = s.trim().replace(' ', "");
    if t.is_empty() {
        return None;
    }
    if let Some((n, d)) = t.split_once('/') {
        let n: f64 = n.parse().ok()?;
        let d: f64 = d.parse().ok()?;
        if d == 0.0 {
            return None;
        }
        let v = n / d;
        return v.is_finite().then_some(v);
    }
    t.parse::<f64>().ok().filter(|v| v.is_finite())
}

/// A coordinate for display: common fractions as "1/3", "3/4", else up to
/// six decimals with trailing zeros dropped.
pub fn format_coord(v: f64) -> String {
    if v.abs() < 1e-7 {
        return "0".into();
    }
    for d in [2i64, 3, 4, 6, 8, 12] {
        let n = (v * d as f64).round();
        if (v - n / d as f64).abs() < 1e-6 {
            let (n, d) = (n as i64, d);
            let g = gcd(n.abs(), d);
            let (n, d) = (n / g, d / g);
            return if d == 1 { n.to_string() } else { format!("{n}/{d}") };
        }
    }
    let s = format!("{v:.6}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

fn gcd(a: i64, b: i64) -> i64 {
    if b == 0 {
        a.max(1)
    } else {
        gcd(b, a % b)
    }
}

/// Expand `sites` with the operations of `hall_number` into the full
/// conventional cell with cell parameters `params` (constraints applied).
pub fn build(hall_number: i32, params: CellParams, sites: &[Site]) -> Result<Built, String> {
    let ops = operations(hall_number)?;
    let params = constrain(hall_number, params);
    let lattice = lattice_from_params(params)?;
    let mut warnings = Vec::new();

    let mut atoms: Vec<Atom> = Vec::new();
    // Which entered site each atom came from.
    let mut atom_site: Vec<usize> = Vec::new();
    let mut results = Vec::with_capacity(sites.len());
    let positions = wyckoff_positions(hall_number);

    for (si, site) in sites.iter().enumerate() {
        let x0 = wrap(site.frac);

        // Snap onto the special position: average the images that land
        // within SNAP_TOL of the site (identity always does).
        let mut acc = [0.0; 3];
        let mut n = 0usize;
        for op in &ops {
            let y = unwrap_near(op.apply(x0), x0);
            if min_image_dist(&lattice, y, x0) < SNAP_TOL {
                for k in 0..3 {
                    acc[k] += y[k];
                }
                n += 1;
            }
        }
        let x = wrap(acc.map(|v| v / n as f64));
        let moved = min_image_dist(&lattice, x, x0);

        // The orbit.
        let mut orbit: Vec<[f64; 3]> = Vec::new();
        for op in &ops {
            let y = wrap(op.apply(x));
            if orbit.iter().all(|o| min_image_dist(&lattice, *o, y) >= MERGE_TOL) {
                orbit.push(y);
            }
        }

        let closest = orbit
            .iter()
            .enumerate()
            .flat_map(|(i, a)| orbit[i + 1..].iter().map(move |b| (*a, *b)))
            .map(|(a, b)| min_image_dist(&lattice, a, b))
            .fold(f64::INFINITY, f64::min);
        if closest < CLASH_WARN {
            warnings.push(format!(
                "{}: its symmetry copies are only {:.3} Å apart. If it should sit on a \
                 special position, enter the coordinates exactly (fractions like 1/3 work).",
                site.label, closest
            ));
        }

        // Clashes with sites entered earlier.
        let mut kept = 0usize;
        let mut dup_of: Option<usize> = None;
        let mut shares_with: Vec<usize> = Vec::new();
        for y in &orbit {
            let hit = atoms
                .iter()
                .zip(&atom_site)
                .find(|(a, _)| {
                    let f = cart_to_frac(a.position, lattice).unwrap_or([0.0; 3]);
                    min_image_dist(&lattice, f, *y) < MERGE_TOL
                })
                .map(|(a, &s)| (a.element.clone(), s));
            match hit {
                Some((el, other)) if el == site.element => {
                    dup_of = Some(other);
                    continue;
                }
                Some((_, other)) if !shares_with.contains(&other) => shares_with.push(other),
                _ => {}
            }
            atoms.push(Atom {
                element: site.element.clone(),
                position: frac_to_cart(*y, lattice),
                original_index: atoms.len(),
                oxidation: site.oxidation,
                occupancy: site.occupancy,
            });
            atom_site.push(si);
            kept += 1;
        }
        if let Some(other) = dup_of {
            warnings.push(format!(
                "{} lands on the same positions as {}; the duplicate atoms were left out.",
                site.label, sites[other].label
            ));
        }
        for other in shares_with {
            let total = site.occupancy + sites[other].occupancy;
            if total > 1.0 + 1e-4 {
                warnings.push(format!(
                    "{} and {} share a position with total occupancy {:.3} (more than 1).",
                    site.label, sites[other].label, total
                ));
            }
        }

        let wyckoff = identify_wyckoff(&positions, &ops, x, Some(orbit.len())).map(|(w, _)| w.name());
        results.push(SiteResult { multiplicity: kept, frac: x, moved, wyckoff });
    }

    let formula = build_formula(&atoms);
    Ok(Built {
        structure: Structure { lattice, atoms, formula, is_periodic: true },
        sites: results,
        warnings,
    })
}

/// An existing structure as group + cell + unique sites.
#[derive(Clone, Debug)]
pub struct Description {
    pub hall_number: i32,
    pub params: CellParams,
    pub sites: Vec<Site>,
    /// Why the structure is described in P1 rather than its own group.
    pub note: Option<String>,
}

/// Describe `structure` for editing: its space group in the standard
/// setting, the standardized conventional cell and one site per orbit.
/// Falls back to P1 with every atom when that is not possible (partial
/// occupancy, a molecule, or no symmetry found).
pub fn describe(structure: &Structure) -> Description {
    match describe_symmetric(structure) {
        Ok(d) => d,
        Err(note) => describe_p1(structure, note),
    }
}

fn site_labels(elements: &[String]) -> Vec<String> {
    let mut count: HashMap<&str, usize> = HashMap::new();
    elements
        .iter()
        .map(|e| {
            let n = count.entry(e.as_str()).or_insert(0);
            *n += 1;
            format!("{e}{n}")
        })
        .collect()
}

fn describe_p1(structure: &Structure, note: String) -> Description {
    let elements: Vec<String> = structure.atoms.iter().map(|a| a.element.clone()).collect();
    let labels = site_labels(&elements);
    let sites = structure
        .atoms
        .iter()
        .zip(labels)
        .map(|(a, label)| Site {
            label,
            element: a.element.clone(),
            frac: wrap(cart_to_frac(a.position, structure.lattice).unwrap_or([0.0; 3])),
            occupancy: a.occupancy,
            oxidation: a.oxidation,
            wyckoff: Some('a'),
        })
        .collect();
    Description {
        hall_number: 1,
        params: params_from_lattice(structure.lattice),
        sites,
        note: Some(note),
    }
}

fn describe_symmetric(structure: &Structure) -> Result<Description, String> {
    if !structure.is_periodic {
        return Err("The structure is not periodic, so it is listed in P1.".into());
    }
    if structure.atoms.iter().any(|a| a.occupancy < 0.999) {
        return Err("Partially occupied sites are not used in the symmetry search, so the \
                    structure is listed in P1."
            .into());
    }

    // Species ids for moyo; an element with mixed oxidation states keeps no
    // oxidation hint.
    let mut ids: HashMap<String, i32> = HashMap::new();
    let mut ox: HashMap<String, Option<i32>> = HashMap::new();
    let mut positions = Vec::with_capacity(structure.atoms.len());
    let mut numbers = Vec::with_capacity(structure.atoms.len());
    for a in &structure.atoms {
        let next = ids.len() as i32 + 1;
        let id = *ids.entry(a.element.clone()).or_insert(next);
        ox.entry(a.element.clone())
            .and_modify(|o| {
                if *o != a.oxidation {
                    *o = None
                }
            })
            .or_insert(a.oxidation);
        let f = cart_to_frac(a.position, structure.lattice).ok_or("Singular lattice")?;
        positions.push(Vector3::from(f));
        numbers.push(id);
    }
    let element_of: HashMap<i32, String> = ids.iter().map(|(e, i)| (*i, e.clone())).collect();

    let cell = Cell::new(Lattice::new(lattice_to_matrix3(structure.lattice)), positions, numbers);
    let fail = |_| "No symmetry could be determined, so the structure is listed in P1.".to_string();
    let ds = MoyoDataset::new(&cell, SYMPREC, AngleTolerance::Default, Setting::Standard, true)
        .map_err(fail)?;
    // Orbits of the standardized cell itself, in the same setting.
    let std = &ds.std_cell;
    let ds2 = MoyoDataset::new(
        std,
        SYMPREC,
        AngleTolerance::Default,
        Setting::HallNumber(ds.hall_number),
        true,
    )
    .map_err(fail)?;

    let reps: Vec<usize> = (0..std.positions.len()).filter(|&i| ds2.orbits[i] == i).collect();
    let elements: Vec<String> = reps
        .iter()
        .map(|&i| element_of.get(&std.numbers[i]).cloned().unwrap_or_else(|| "X".into()))
        .collect();
    let labels = site_labels(&elements);
    let ops = operations(ds.hall_number)?;
    let wyckoffs = wyckoff_positions(ds.hall_number);
    let sites = reps
        .iter()
        .zip(elements)
        .zip(labels)
        .map(|((&i, element), label)| {
            let p = std.positions[i];
            let rep = wrap([p.x, p.y, p.z]);
            // Report the orbit member that has the tabulated form, so the
            // editor shows e.g. Bi on 6c (0, 0, z) with z = 0.4.
            let letter = ds2.wyckoffs.get(i).copied();
            let w = letter.and_then(|l| wyckoffs.iter().find(|w| w.letter == l));
            // Of the copies that fit, the one with the smallest parameters
            // (Bi at z = 0.40 rather than 0.60), as structures are usually quoted.
            let on_form = w.and_then(|w| {
                ops.iter()
                    .filter_map(|op| w.params_for(wrap(op.apply(rep))))
                    .min_by(|a, b| {
                        let key = |p: &[f64; 3]| p.iter().sum::<f64>();
                        key(a).total_cmp(&key(b))
                    })
                    .map(|p| w.position(p))
            });
            Site {
                label,
                oxidation: ox.get(&element).copied().flatten(),
                element,
                frac: on_form.unwrap_or(rep),
                occupancy: 1.0,
                wyckoff: on_form.and(letter),
            }
        })
        .collect();

    // moyo keeps lattice vectors as columns.
    let b = std.lattice.basis;
    let lat = [
        [b[(0, 0)], b[(1, 0)], b[(2, 0)]],
        [b[(0, 1)], b[(1, 1)], b[(2, 1)]],
        [b[(0, 2)], b[(1, 2)], b[(2, 2)]],
    ];
    Ok(Description {
        hall_number: ds.hall_number,
        params: constrain(ds.hall_number, params_from_lattice(lat)),
        sites,
        note: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn site(label: &str, el: &str, frac: [f64; 3]) -> Site {
        Site {
            label: label.into(),
            element: el.into(),
            frac,
            occupancy: 1.0,
            oxidation: None,
            wyckoff: None,
        }
    }

    fn count(s: &Structure, el: &str) -> usize {
        s.atoms.iter().filter(|a| a.element == el).count()
    }

    #[test]
    fn group_orders_include_centring() {
        let order = |n| operations(standard_hall_number(n).unwrap()).unwrap().len();
        assert_eq!(order(1), 1);
        assert_eq!(order(14), 4);
        assert_eq!(order(166), 36); // R-3m, hexagonal axes: 12 × 3
        assert_eq!(order(194), 24);
        assert_eq!(order(225), 192); // Fm-3m: 48 × 4
        assert_eq!(order(229), 96); // Im-3m: 48 × 2
    }

    #[test]
    fn rock_salt() {
        let h = standard_hall_number(225).unwrap();
        let b = build(
            h,
            [5.64, 0.0, 0.0, 90.0, 0.0, 0.0],
            &[site("Na1", "Na", [0.0; 3]), site("Cl1", "Cl", [0.5, 0.5, 0.5])],
        )
        .unwrap();
        assert_eq!(count(&b.structure, "Na"), 4);
        assert_eq!(count(&b.structure, "Cl"), 4);
        assert!(b.warnings.is_empty(), "{:?}", b.warnings);
        assert!((b.structure.lattice[2][2] - 5.64).abs() < 1e-9);
    }

    #[test]
    fn perovskite_and_round_trip() {
        let h = standard_hall_number(221).unwrap();
        let b = build(
            h,
            [4.0, 4.0, 4.0, 90.0, 90.0, 90.0],
            &[
                site("Ba1", "Ba", [0.0; 3]),
                site("Ti1", "Ti", [0.5; 3]),
                site("O1", "O", [0.5, 0.5, 0.0]),
            ],
        )
        .unwrap();
        assert_eq!(b.structure.atoms.len(), 5);
        assert_eq!(b.sites[2].multiplicity, 3);

        let d = describe(&b.structure);
        assert!(d.note.is_none());
        assert_eq!(SettingChoice::from_hall(d.hall_number).unwrap().number, 221);
        assert_eq!(d.sites.len(), 3);
        let again = build(d.hall_number, d.params, &d.sites).unwrap();
        assert_eq!(again.structure.atoms.len(), 5);
    }

    #[test]
    fn rounded_special_position_is_snapped() {
        // Graphite-like 2c/2d sites typed to three decimals.
        let h = standard_hall_number(194).unwrap();
        let b = build(
            h,
            [2.46, 2.46, 6.7, 90.0, 90.0, 120.0],
            &[site("C1", "C", [0.0, 0.0, 0.25]), site("C2", "C", [0.333, 0.667, 0.25])],
        )
        .unwrap();
        assert_eq!(b.sites[1].multiplicity, 2, "{:?}", b.warnings);
        assert!((b.sites[1].frac[0] - 1.0 / 3.0).abs() < 1e-9);
        assert!((b.sites[1].frac[1] - 2.0 / 3.0).abs() < 1e-9);
        assert_eq!(b.structure.atoms.len(), 4);
    }

    #[test]
    fn general_position_multiplicity() {
        let h = standard_hall_number(225).unwrap();
        let b = build(h, [5.0, 5.0, 5.0, 90.0, 90.0, 90.0], &[site("X", "Si", [0.07, 0.29, 0.17])])
            .unwrap();
        assert_eq!(b.sites[0].multiplicity, 192);
    }

    #[test]
    fn duplicate_site_is_dropped_with_warning() {
        let h = standard_hall_number(229).unwrap();
        let b = build(
            h,
            [2.87, 2.87, 2.87, 90.0, 90.0, 90.0],
            &[site("Fe1", "Fe", [0.0; 3]), site("Fe2", "Fe", [0.5; 3])],
        )
        .unwrap();
        assert_eq!(b.structure.atoms.len(), 2);
        assert_eq!(b.sites[1].multiplicity, 0);
        assert_eq!(b.warnings.len(), 1);
    }

    #[test]
    fn constraints_follow_the_setting() {
        let mono_b = standard_hall_number(14).unwrap();
        let p = constrain(mono_b, [5.0, 6.0, 7.0, 80.0, 100.0, 110.0]);
        assert_eq!(p, [5.0, 6.0, 7.0, 90.0, 100.0, 90.0]);

        let r_rhombo = settings_for(166).into_iter().find(|s| s.code == "R").unwrap();
        let p = constrain(r_rhombo.hall_number, [5.0, 1.0, 1.0, 60.0, 1.0, 1.0]);
        assert_eq!(p, [5.0, 5.0, 5.0, 60.0, 60.0, 60.0]);

        let hex = standard_hall_number(166).unwrap();
        let p = constrain(hex, [4.0, 1.0, 30.0, 1.0, 1.0, 1.0]);
        assert_eq!(p, [4.0, 4.0, 30.0, 90.0, 90.0, 120.0]);
    }

    #[test]
    fn rhombohedral_axes_give_the_primitive_cell() {
        let r = settings_for(166).into_iter().find(|s| s.code == "R").unwrap();
        let b = build(r.hall_number, [5.0, 0.0, 0.0, 60.0, 0.0, 0.0], &[site("Bi", "Bi", [0.0; 3])])
            .unwrap();
        assert_eq!(b.structure.atoms.len(), 1);
    }

    #[test]
    fn bad_cells_are_rejected() {
        assert!(lattice_from_params([1.0, 1.0, 1.0, 120.0, 120.0, 120.0]).is_err());
        assert!(lattice_from_params([0.0, 1.0, 1.0, 90.0, 90.0, 90.0]).is_err());
    }

    #[test]
    fn coordinates_parse_and_format() {
        assert_eq!(parse_coord("1/3"), Some(1.0 / 3.0));
        assert_eq!(parse_coord(" -1/4 "), Some(-0.25));
        assert_eq!(parse_coord("0.125"), Some(0.125));
        assert_eq!(parse_coord("1/0"), None);
        assert_eq!(parse_coord("x"), None);
        assert_eq!(format_coord(1.0 / 3.0), "1/3");
        assert_eq!(format_coord(0.75), "3/4");
        assert_eq!(format_coord(0.0), "0");
        assert_eq!(format_coord(0.1234), "0.1234");
    }

    #[test]
    fn elements_normalise() {
        assert_eq!(normalize_element("fe").as_deref(), Some("Fe"));
        assert_eq!(normalize_element(" O ").as_deref(), Some("O"));
        assert_eq!(normalize_element("Xx"), None);
        assert_eq!(normalize_element("Fe3+"), None);
    }

    #[test]
    fn every_setting_builds() {
        for hall in 1..=530 {
            let rules = param_rules(hall);
            let p = constrain(hall, [5.0, 6.0, 7.0, 90.0, 100.0, 90.0]);
            let p = if matches!(rules[3], Param::Free) && matches!(rules[4], Param::SameAs(3)) {
                [p[0], p[1], p[2], 70.0, 70.0, 70.0]
            } else {
                p
            };
            let b = build(hall, p, &[site("A", "Si", [0.07, 0.29, 0.17])])
                .unwrap_or_else(|e| panic!("hall {hall}: {e}"));
            let order = operations(hall).unwrap().len();
            assert_eq!(b.sites[0].multiplicity, order, "hall {hall}");
        }
    }

    #[test]
    fn every_wyckoff_position_has_its_multiplicity() {
        let mut n = 0;
        for hall in 1..=530 {
            let ops = operations(hall).unwrap();
            let positions = wyckoff_positions(hall);
            assert!(!positions.is_empty(), "hall {hall}");
            assert_eq!(positions[0].multiplicity, ops.len(), "hall {hall}: general first");
            // A small cell: the generic values must stay clear of snapping
            // even where symmetry copies are closest in ångström.
            let rules = param_rules(hall);
            let p = constrain(hall, [3.0, 3.5, 4.0, 90.0, 100.0, 90.0]);
            let p = if matches!(rules[3], Param::Free) && matches!(rules[4], Param::SameAs(3)) {
                [p[0], p[1], p[2], 70.0, 70.0, 70.0]
            } else {
                p
            };
            for w in &positions {
                let frac = w.position(GENERIC_PARAMS);
                let b = build(hall, p, &[site("A", "Si", frac)]).unwrap();
                let r = &b.sites[0];
                assert_eq!(r.multiplicity, w.multiplicity, "hall {hall} {}", w.label());
                assert_eq!(r.wyckoff.as_deref(), Some(w.name().as_str()), "hall {hall} {}", w.label());
                assert!(r.moved < 1e-9, "hall {hall} {} moved {}", w.label(), r.moved);
                n += 1;
            }
        }
        assert_eq!(n, 3467);
    }

    #[test]
    fn wyckoff_coordinates_parse() {
        let (l, o) = parse_wyckoff_coords("-x+2z,y,z").unwrap();
        assert_eq!(l, Matrix3::new(-1, 0, 2, 0, 1, 0, 0, 0, 1));
        assert_eq!(o, Vector3::zeros());
        let (l, o) = parse_wyckoff_coords("x,-x+1/2,1/4").unwrap();
        assert_eq!(l, Matrix3::new(1, 0, 0, -1, 0, 0, 0, 0, 0));
        assert_eq!(o, Vector3::new(0.0, 0.5, 0.25));
    }

    #[test]
    fn describe_reports_wyckoff_letters() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/excmples/Bi2Se3.cif");
        let Ok(s) = crate::io::load_structure(path) else { return };
        let d = describe(&s);
        let pos = wyckoff_positions(d.hall_number);
        let names: Vec<String> = d
            .sites
            .iter()
            .map(|site| {
                let w = pos.iter().find(|w| Some(w.letter) == site.wyckoff).expect("letter");
                let p = w.params_for(site.frac);
                assert!(p.is_some(), "{} not on {}", site.label, w.label());
                // Quoted the usual way: z below 1/2 (Bi z ≈ 0.40, not 0.60).
                assert!(p.unwrap()[2] < 0.5, "{} z = {}", site.label, p.unwrap()[2]);
                format!("{}:{}", site.element, w.name())
            })
            .collect();
        // Bi2Se3: Bi 6c, Se 3a and 6c.
        assert!(names.contains(&"Bi:6c".to_string()), "{names:?}");
        assert!(names.contains(&"Se:3a".to_string()), "{names:?}");
    }

    #[test]
    fn labels_describe_settings() {
        let s = SettingChoice::from_hall(standard_hall_number(14).unwrap()).unwrap();
        assert!(s.label().contains("unique axis b"), "{}", s.label());
        assert_eq!(s.short_symbol(), "P2₁/c");
        let fd3m: Vec<_> = settings_for(227);
        assert_eq!(fd3m.len(), 2);
        assert!(fd3m.iter().any(|s| s.description() == "origin choice 2"));
    }
}

#[cfg(test)]
mod example_files {
    use super::*;

    /// Every example structure survives describe → build with the same
    /// composition and the same number of atoms per conventional cell.
    #[test]
    fn examples_round_trip() {
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/excmples");
        let Ok(entries) = std::fs::read_dir(dir) else { return };
        for e in entries.flatten() {
            let path = e.path();
            let name = path.file_name().unwrap().to_string_lossy().to_string();
            let ok_ext = name.ends_with(".cif") || name.ends_with(".vasp") || name == "POSCAR"
                || name.ends_with(".pwi");
            if !ok_ext {
                continue;
            }
            let Ok(s) = crate::io::load_structure(path.to_str().unwrap()) else { continue };
            let d = describe(&s);
            let b = build(d.hall_number, d.params, &d.sites)
                .unwrap_or_else(|e| panic!("{name}: {e}"));
            let sg = SettingChoice::from_hall(d.hall_number).unwrap();
            eprintln!(
                "{name}: {} → {} sites, {} atoms, {}  {:?}  note={:?}",
                s.atoms.len(),
                d.sites.len(),
                b.structure.atoms.len(),
                sg.label(),
                b.warnings,
                d.note
            );
            assert!(b.warnings.is_empty(), "{name}: {:?}", b.warnings);
            if d.note.is_none() {
                let again = crate::physics::analysis::symmetry::analyze(&b.structure).unwrap();
                assert_eq!(again.number, sg.number, "{name}");
            } else {
                assert_eq!(b.structure.atoms.len(), s.atoms.len(), "{name}");
            }
        }
    }
}

