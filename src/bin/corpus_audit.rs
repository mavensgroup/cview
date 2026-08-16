// src/bin/corpus_audit.rs
//
// Corpus round-trip fidelity audit for CView.
//
// Supporting Information for the JAC manuscript. This is NOT a throughput
// benchmark: it measures whether CView's parser/writer stack preserves a
// crystal structure across a round trip, over the messy diversity of real
// crystallographic files. Timing is collected as a secondary by-product and
// reported as single-file latency, which is the figure of merit for an
// interactive tool.
//
// Build/run:
//   cargo run --release --features audit --bin corpus_audit -- \
//       --corpus /path/to/cod/cif --sample 1000 --seed 42 --out ./audit_results
//
// Design notes
// ------------
// * Each file is processed in a CHILD PROCESS (`--worker`). This is what makes
//   the run robust: `catch_unwind` cannot contain an abort, a stack overflow,
//   or an infinite loop, but a killed child can. Panics, aborts and timeouts
//   therefore all become data points instead of ending the run.
// * Timings are taken INSIDE the child, so process-spawn overhead is excluded.
// * Every file is processed twice; the first pass is discarded so the reported
//   number is warm-page-cache parse cost, not the machine's disk.
// * No GTK is initialised. `console::do_append` early-returns when its
//   OnceLock is unset and the `log` facade no-ops with no logger installed,
//   so the io/ and physics/ paths are safe to call headlessly.

use std::collections::BTreeMap;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use cview::io;
use cview::model::Structure;
use cview::physics::analysis::symmetry::{self, SYMPREC};
use cview::utils::linalg::{cart_to_frac, frac_to_cart};

// =========================================================================
// Tolerances — fixed by writer precision (CIF {:.6} frac / {:.4} occupancy,
// POSCAR {:15.9}). Stated up front and NOT adjusted after seeing results.
// =========================================================================
const TOL_LENGTH_A: f64 = 1e-5; // Å, absolute, on a/b/c
const TOL_ANGLE_DEG: f64 = 1e-4; // degrees, absolute, on alpha/beta/gamma
const TOL_VOLUME_REL: f64 = 1e-5; // relative, on cell volume
const TOL_OCCUPANCY: f64 = 1e-3; // absolute, on occupancy (round trip A only)
const TOL_POSITION_A: f64 = 1e-4; // Å, absolute, PBC-aware per-site deviation

/// Above this atom count the O(n^2) site-matching is skipped (and noted),
/// to keep a pathological file from eating the per-file timeout.
const POSITION_MATCH_MAX_ATOMS: usize = 5000;

// =========================================================================
// Deterministic RNG (xoshiro256** seeded through SplitMix64).
// Vendored rather than pulled in as a dependency; the sample must be
// reproducible from `--seed` alone by anyone reading the SI.
// =========================================================================
struct Rng(
    [u64; 4],
);

impl Rng {
    fn new(seed: u64) -> Self {
        let mut s = seed;
        let mut next = || {
            s = s.wrapping_add(0x9E37_79B9_7F4A_7C15);
            let mut z = s;
            z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
            z ^ (z >> 31)
        };
        Rng([next(), next(), next(), next()])
    }

    fn next_u64(&mut self) -> u64 {
        let s = &mut self.0;
        let result = s[1].wrapping_mul(5).rotate_left(7).wrapping_mul(9);
        let t = s[1] << 17;
        s[2] ^= s[0];
        s[3] ^= s[1];
        s[1] ^= s[2];
        s[0] ^= s[3];
        s[2] ^= t;
        s[3] = s[3].rotate_left(45);
        result
    }

    /// Unbiased index in [0, n) via Lemire rejection.
    fn below(&mut self, n: u64) -> u64 {
        assert!(n > 0);
        let zone = u64::MAX - (u64::MAX % n);
        loop {
            let v = self.next_u64();
            if v < zone {
                return v % n;
            }
        }
    }

    /// Partial Fisher-Yates: first `k` entries become a uniform sample.
    fn sample_in_place<T>(&mut self, items: &mut Vec<T>, k: usize) {
        let n = items.len();
        let k = k.min(n);
        for i in 0..k {
            let j = i + self.below((n - i) as u64) as usize;
            items.swap(i, j);
        }
        items.truncate(k);
    }
}

// =========================================================================
// Result records
// =========================================================================

#[derive(Serialize, Deserialize, Clone, Default)]
struct TripResult {
    outcome: String,
    /// Every condition that fired, even when a more severe one won the
    /// `outcome` slot. Keeping these makes the taxonomy auditable.
    notes: Vec<String>,
    parse_ms: f64,
    roundtrip_ms: f64,
    sg_original: String,
    sg_final: String,
    max_length_dev_a: f64,
    max_angle_dev_deg: f64,
    volume_rel_dev: f64,
    max_pos_dev_a: f64,
    error_detail: String,
}

#[derive(Serialize, Deserialize, Clone, Default)]
struct FileResult {
    cod_id: String,
    file_bytes: u64,
    n_atoms: usize,
    classification: String,
    has_partial_occ: bool,
    has_oxidation: bool,
    has_refln_block: bool,
    has_semicolon_text: bool,
    has_split_site: bool,
    crystal_system: String,
    sg_declared: String,
    trip_a: TripResult,
    trip_b: TripResult,
}

// =========================================================================
// Geometry helpers
// =========================================================================

fn cell_params(lattice: [[f64; 3]; 3]) -> ([f64; 3], [f64; 3]) {
    let norm = |v: [f64; 3]| (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    let dot = |u: [f64; 3], v: [f64; 3]| u[0] * v[0] + u[1] * v[1] + u[2] * v[2];
    let (av, bv, cv) = (lattice[0], lattice[1], lattice[2]);
    let (a, b, c) = (norm(av), norm(bv), norm(cv));
    let to_deg = 180.0 / std::f64::consts::PI;
    let safe_acos = |x: f64| x.clamp(-1.0, 1.0).acos() * to_deg;
    let alpha = safe_acos(dot(bv, cv) / (b * c));
    let beta = safe_acos(dot(av, cv) / (a * c));
    let gamma = safe_acos(dot(av, bv) / (a * b));
    ([a, b, c], [alpha, beta, gamma])
}

fn cell_volume(l: [[f64; 3]; 3]) -> f64 {
    let cross = [
        l[1][1] * l[2][2] - l[1][2] * l[2][1],
        l[1][2] * l[2][0] - l[1][0] * l[2][2],
        l[1][0] * l[2][1] - l[1][1] * l[2][0],
    ];
    (l[0][0] * cross[0] + l[0][1] * cross[1] + l[0][2] * cross[2]).abs()
}

/// A structure that no downstream code could use: no sites, or a collapsed
/// lattice. `cif::parse` never returns `Err` on malformed content, so this
/// is the only way to detect a failed import.
fn is_degenerate(s: &Structure) -> Option<String> {
    if s.atoms.is_empty() {
        return Some("zero atom sites".to_string());
    }
    let v = cell_volume(s.lattice);
    if !v.is_finite() || v < 1e-6 {
        return Some(format!("degenerate lattice (volume {v:.3e})"));
    }
    let (lens, angs) = cell_params(s.lattice);
    if lens.iter().any(|x| !x.is_finite() || *x <= 0.0) || angs.iter().any(|x| !x.is_finite()) {
        return Some("non-finite cell parameters".to_string());
    }
    if s.atoms
        .iter()
        .any(|a| a.position.iter().any(|x| !x.is_finite()))
    {
        return Some("non-finite atom coordinates".to_string());
    }
    None
}

fn composition(s: &Structure) -> BTreeMap<String, usize> {
    let mut m = BTreeMap::new();
    for a in &s.atoms {
        *m.entry(a.element.clone()).or_insert(0) += 1;
    }
    m
}

fn occupancy_multiset(s: &Structure) -> Vec<f64> {
    let mut v: Vec<f64> = s.atoms.iter().map(|a| a.occupancy).collect();
    v.sort_by(|x, y| x.partial_cmp(y).unwrap_or(std::cmp::Ordering::Equal));
    v
}

fn oxidation_multiset(s: &Structure) -> Vec<Option<i32>> {
    let mut v: Vec<Option<i32>> = s.atoms.iter().map(|a| a.oxidation).collect();
    v.sort();
    v
}

/// Largest per-site displacement, matched order-independently and under
/// periodic boundary conditions. Necessary because the POSCAR writer sorts
/// atoms by element, so index-wise comparison would be meaningless.
///
/// Greedy nearest-neighbour matching within each element group. Returns
/// `None` when the structure is too large to match in reasonable time.
fn max_site_deviation(orig: &Structure, fin: &Structure) -> Option<f64> {
    if orig.atoms.len() > POSITION_MATCH_MAX_ATOMS {
        return None;
    }
    let lat = fin.lattice;
    let mut by_el_o: BTreeMap<&str, Vec<[f64; 3]>> = BTreeMap::new();
    let mut by_el_f: BTreeMap<&str, Vec<[f64; 3]>> = BTreeMap::new();
    for a in &orig.atoms {
        let f = cart_to_frac(a.position, orig.lattice)?;
        by_el_o.entry(a.element.as_str()).or_default().push(f);
    }
    for a in &fin.atoms {
        let f = cart_to_frac(a.position, fin.lattice)?;
        by_el_f.entry(a.element.as_str()).or_default().push(f);
    }

    let mut worst: f64 = 0.0;
    for (el, ofs) in &by_el_o {
        let ffs = by_el_f.get(el)?;
        if ffs.len() != ofs.len() {
            return None;
        }
        let mut used = vec![false; ffs.len()];
        for of in ofs {
            let mut best = f64::INFINITY;
            let mut best_j = usize::MAX;
            for (j, ff) in ffs.iter().enumerate() {
                if used[j] {
                    continue;
                }
                // Minimum image in fractional space, then to Cartesian.
                let mut d = [0.0f64; 3];
                for k in 0..3 {
                    let mut delta = of[k] - ff[k];
                    delta -= delta.round();
                    d[k] = delta;
                }
                let cart = frac_to_cart(d, lat);
                let dist = (cart[0] * cart[0] + cart[1] * cart[1] + cart[2] * cart[2]).sqrt();
                if dist < best {
                    best = dist;
                    best_j = j;
                }
            }
            if best_j == usize::MAX {
                return None;
            }
            used[best_j] = true;
            worst = worst.max(best);
        }
    }
    Some(worst)
}

// =========================================================================
// Comparison
// =========================================================================

struct Compared {
    outcome: Option<String>,
    notes: Vec<String>,
    max_len_dev: f64,
    max_ang_dev: f64,
    vol_rel_dev: f64,
    max_pos_dev: f64,
}

/// Compare first-parsed against final-parsed. `via_poscar` marks round trip B,
/// where occupancy and oxidation loss is format-inherent rather than a defect.
fn compare(orig: &Structure, fin: &Structure, via_poscar: bool) -> Compared {
    let mut notes = Vec::new();
    let mut outcome: Option<String> = None;
    let mut set = |o: &str, notes: &mut Vec<String>| {
        notes.push(o.to_string());
        if outcome.is_none() {
            outcome = Some(o.to_string());
        }
    };

    // --- counts and composition (most severe structural checks) ---
    if orig.atoms.len() != fin.atoms.len() {
        set("COUNT_MISMATCH", &mut notes);
    }
    let (co, cf) = (composition(orig), composition(fin));
    if co != cf {
        set("COMPOSITION_MISMATCH", &mut notes);
    }

    // --- geometry ---
    let (lo, ao) = cell_params(orig.lattice);
    let (lf, af) = cell_params(fin.lattice);
    let max_len_dev = (0..3).map(|i| (lo[i] - lf[i]).abs()).fold(0.0, f64::max);
    let max_ang_dev = (0..3).map(|i| (ao[i] - af[i]).abs()).fold(0.0, f64::max);
    let (vo, vf) = (cell_volume(orig.lattice), cell_volume(fin.lattice));
    let vol_rel_dev = if vo > 0.0 { ((vo - vf) / vo).abs() } else { 0.0 };

    let pos_dev = if co == cf {
        max_site_deviation(orig, fin)
    } else {
        None
    };
    let max_pos_dev = pos_dev.unwrap_or(-1.0);
    if pos_dev.is_none() && co == cf {
        notes.push("POSITION_CHECK_SKIPPED".to_string());
    }

    if max_len_dev > TOL_LENGTH_A
        || max_ang_dev > TOL_ANGLE_DEG
        || vol_rel_dev > TOL_VOLUME_REL
        || pos_dev.map(|d| d > TOL_POSITION_A).unwrap_or(false)
    {
        set("GEOM_MISMATCH", &mut notes);
    }

    // --- occupancy ---
    let (oo, of_) = (occupancy_multiset(orig), occupancy_multiset(fin));
    let orig_has_partial = oo.iter().any(|x| *x < 0.99);
    let occ_differs = oo.len() != of_.len()
        || oo
            .iter()
            .zip(of_.iter())
            .any(|(x, y)| (x - y).abs() > TOL_OCCUPANCY);
    if occ_differs {
        if via_poscar && orig_has_partial {
            // POSCAR has no occupancy field. Format-inherent, not a defect.
            set("LOSS_OCCUPANCY_EXPECTED", &mut notes);
        } else {
            set("LOSS_UNEXPECTED", &mut notes);
            notes.push("occupancy changed where the format could carry it".to_string());
        }
    }

    // --- oxidation ---
    let (xo, xf) = (oxidation_multiset(orig), oxidation_multiset(fin));
    let orig_has_ox = xo.iter().any(|x| x.is_some());
    if xo != xf {
        if via_poscar && orig_has_ox {
            set("LOSS_OXIDATION_EXPECTED", &mut notes);
        } else {
            set("LOSS_UNEXPECTED", &mut notes);
            notes.push("oxidation state changed where the format could carry it".to_string());
        }
    }

    Compared {
        outcome,
        notes,
        max_len_dev,
        max_ang_dev,
        vol_rel_dev,
        max_pos_dev,
    }
}

/// Severity ranking. Each file gets exactly one outcome per round trip: the
/// most severe condition that fired.
fn severity(code: &str) -> u32 {
    match code {
        "PANIC" => 100,
        "TIMEOUT" => 99,
        "CHILD_CRASH" => 98,
        "PARSE_FAIL_ORIG" => 90,
        "PARSE_DEGENERATE_ORIG" => 89,
        "WRITE_FAIL" => 80,
        "PARSE_FAIL_ROUND" => 79,
        "PARSE_DEGENERATE_ROUND" => 78,
        "COUNT_MISMATCH" => 70,
        "COMPOSITION_MISMATCH" => 69,
        "SYMMETRY_CHANGED" => 60,
        // Asymmetric detection failure: the round trip changed whether
        // symmetry could be found at all. A real fidelity failure.
        "SYMMETRY_LOST" => 59,
        "SYMMETRY_GAINED" => 58,
        "GEOM_MISMATCH" => 55,
        "LOSS_UNEXPECTED" => 50,
        // Symmetric failure: moyo could not search either side (typically
        // coincident split-site atoms). Not a round-trip defect.
        "SYMMETRY_UNDETERMINED_BOTH" => 15,
        "LOSS_OCCUPANCY_EXPECTED" => 20,
        "LOSS_OXIDATION_EXPECTED" => 19,
        "PASS" => 0,
        _ => 10,
    }
}

fn worst(a: Option<String>, b: Option<String>) -> Option<String> {
    match (a, b) {
        (None, x) => x,
        (x, None) => x,
        (Some(x), Some(y)) => {
            if severity(&x) >= severity(&y) {
                Some(x)
            } else {
                Some(y)
            }
        }
    }
}

// =========================================================================
// Worker: audit one file, emit one JSON line on stdout
// =========================================================================

fn sg_string(r: &Result<symmetry::SymmetryInfo, String>) -> String {
    match r {
        Ok(i) => i.number.to_string(),
        Err(_) => "NA".to_string(),
    }
}

/// Textual scan of the raw CIF for features that the parsed model does not
/// retain: reflection loops, multi-line text fields, declared space group.
fn scan_raw(path: &Path) -> (bool, bool, String) {
    let text = std::fs::read_to_string(path).unwrap_or_default();
    let mut has_refln = false;
    let mut has_semi = false;
    let mut declared = String::new();
    for line in text.lines() {
        if line.starts_with(';') {
            has_semi = true;
        }
        let t = line.trim_start().to_ascii_lowercase();
        if t.starts_with("_refln") || t.starts_with("_diffrn_refln") {
            has_refln = true;
        }
        if declared.is_empty()
            && (t.starts_with("_symmetry_int_tables_number")
                || t.starts_with("_space_group_it_number"))
        {
            if let Some(v) = line.split_whitespace().nth(1) {
                let v = v.trim_matches(|c| c == '\'' || c == '"');
                if let Ok(n) = v.parse::<i32>() {
                    declared = n.to_string();
                }
            }
        }
    }
    (has_refln, has_semi, declared)
}

/// Two sites within 0.3 Å carrying different elements — a shared/split site.
fn detect_split_site(s: &Structure) -> bool {
    let n = s.atoms.len();
    if n > POSITION_MATCH_MAX_ATOMS {
        return false;
    }
    for i in 0..n {
        for j in (i + 1)..n {
            if s.atoms[i].element == s.atoms[j].element {
                continue;
            }
            let d = [
                s.atoms[i].position[0] - s.atoms[j].position[0],
                s.atoms[i].position[1] - s.atoms[j].position[1],
                s.atoms[i].position[2] - s.atoms[j].position[2],
            ];
            if (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt() < 0.3 {
                return true;
            }
        }
    }
    false
}

/// Classification rule, stated in the SI: a structure counts as *organic or
/// metal-organic* when it contains BOTH carbon and hydrogen; otherwise
/// *inorganic*. Simple and reproducible. Known edge case: hydrated
/// carbonates and hydroxide-bearing carbides fall on the organic side.
fn classify(s: &Structure) -> &'static str {
    let has_c = s.atoms.iter().any(|a| a.element == "C");
    let has_h = s.atoms.iter().any(|a| a.element == "H");
    if has_c && has_h {
        "organic_or_metalorganic"
    } else {
        "inorganic"
    }
}

fn run_worker(path: &str, tmpdir: &Path) -> FileResult {
    let mut res = FileResult {
        cod_id: cod_id_of(Path::new(path)),
        file_bytes: std::fs::metadata(path).map(|m| m.len()).unwrap_or(0),
        ..Default::default()
    };

    let (has_refln, has_semi, declared) = scan_raw(Path::new(path));
    res.has_refln_block = has_refln;
    res.has_semicolon_text = has_semi;
    res.sg_declared = declared;

    std::fs::create_dir_all(tmpdir).ok();
    let a_cif = tmpdir.join("rt_a.cif");
    let b_poscar = tmpdir.join("POSCAR");
    let b_cif = tmpdir.join("rt_b.cif");

    // ---- parse the original twice; keep the second (warm cache) ----
    let _ = io::load_structure(path);
    let t0 = Instant::now();
    let orig_res = io::load_structure(path);
    let parse_ms = t0.elapsed().as_secs_f64() * 1000.0;

    let orig = match orig_res {
        Err(e) => {
            let t = TripResult {
                outcome: "PARSE_FAIL_ORIG".into(),
                parse_ms,
                error_detail: e.to_string(),
                ..Default::default()
            };
            res.trip_a = t.clone();
            res.trip_b = t;
            return res;
        }
        Ok(s) => s,
    };

    res.n_atoms = orig.atoms.len();
    res.classification = classify(&orig).to_string();
    res.has_partial_occ = orig.atoms.iter().any(|a| a.occupancy < 0.99);
    res.has_oxidation = orig.atoms.iter().any(|a| a.oxidation.is_some());
    res.has_split_site = detect_split_site(&orig);

    if let Some(why) = is_degenerate(&orig) {
        let t = TripResult {
            outcome: "PARSE_DEGENERATE_ORIG".into(),
            parse_ms,
            error_detail: why,
            ..Default::default()
        };
        res.trip_a = t.clone();
        res.trip_b = t;
        return res;
    }

    let sym_orig = symmetry::analyze(&orig);
    res.crystal_system = match &sym_orig {
        Ok(i) => i.system.clone(),
        Err(_) => "Undetermined".to_string(),
    };

    // =================== ROUND TRIP A: CIF -> CIF ===================
    res.trip_a = {
        let a_path = a_cif.to_string_lossy().to_string();
        // discarded warm-up pass
        if io::save_structure(&a_path, &orig).is_ok() {
            let _ = io::load_structure(&a_path);
        }
        let t0 = Instant::now();
        let write_res = io::save_structure(&a_path, &orig);
        let reload = write_res.as_ref().ok().map(|_| io::load_structure(&a_path));
        let roundtrip_ms = t0.elapsed().as_secs_f64() * 1000.0;

        finish_trip(
            &orig, &sym_orig, write_res, reload, false, parse_ms, roundtrip_ms,
        )
    };

    // ============ ROUND TRIP B: CIF -> POSCAR -> CIF ============
    res.trip_b = {
        let p_path = b_poscar.to_string_lossy().to_string();
        let c_path = b_cif.to_string_lossy().to_string();

        let chain = |timed: bool| -> (std::io::Result<()>, Option<std::io::Result<Structure>>, f64) {
            let t0 = Instant::now();
            let w1 = io::save_structure(&p_path, &orig);
            if w1.is_err() {
                return (w1, None, t0.elapsed().as_secs_f64() * 1000.0);
            }
            let mid = match io::load_structure(&p_path) {
                Err(e) => {
                    let ms = t0.elapsed().as_secs_f64() * 1000.0;
                    return (Ok(()), Some(Err(e)), ms);
                }
                Ok(s) => s,
            };
            let w2 = io::save_structure(&c_path, &mid);
            if w2.is_err() {
                return (w2, None, t0.elapsed().as_secs_f64() * 1000.0);
            }
            let fin = io::load_structure(&c_path);
            let ms = t0.elapsed().as_secs_f64() * 1000.0;
            let _ = timed;
            (Ok(()), Some(fin), ms)
        };

        let _ = chain(false); // warm-up, discarded
        let (write_res, reload, roundtrip_ms) = chain(true);

        finish_trip(
            &orig, &sym_orig, write_res, reload, true, parse_ms, roundtrip_ms,
        )
    };

    res
}

#[allow(clippy::too_many_arguments)]
fn finish_trip(
    orig: &Structure,
    sym_orig: &Result<symmetry::SymmetryInfo, String>,
    write_res: std::io::Result<()>,
    reload: Option<std::io::Result<Structure>>,
    via_poscar: bool,
    parse_ms: f64,
    roundtrip_ms: f64,
) -> TripResult {
    let mut t = TripResult {
        parse_ms,
        roundtrip_ms,
        sg_original: sg_string(sym_orig),
        max_pos_dev_a: -1.0,
        ..Default::default()
    };

    if let Err(e) = write_res {
        t.outcome = "WRITE_FAIL".into();
        t.error_detail = e.to_string();
        return t;
    }
    let fin = match reload {
        None => {
            t.outcome = "WRITE_FAIL".into();
            return t;
        }
        Some(Err(e)) => {
            t.outcome = "PARSE_FAIL_ROUND".into();
            t.error_detail = e.to_string();
            return t;
        }
        Some(Ok(s)) => s,
    };
    if let Some(why) = is_degenerate(&fin) {
        t.outcome = "PARSE_DEGENERATE_ROUND".into();
        t.error_detail = why;
        return t;
    }

    let cmp = compare(orig, &fin, via_poscar);
    t.max_length_dev_a = cmp.max_len_dev;
    t.max_angle_dev_deg = cmp.max_ang_dev;
    t.volume_rel_dev = cmp.vol_rel_dev;
    t.max_pos_dev_a = cmp.max_pos_dev;
    t.notes = cmp.notes;

    // Space group: parser detection on BOTH sides, never the CIF header.
    let sym_fin = symmetry::analyze(&fin);
    t.sg_final = sg_string(&sym_fin);
    // The smoke run showed every `analyze()` failure was symmetric (moyo
    // cannot search a cell containing coincident split-site atoms). That is a
    // limitation of symmetry detection on disordered structures, not a
    // round-trip defect, so it is graded separately from the ASYMMETRIC case —
    // where the round trip itself made symmetry detectable or undetectable,
    // which IS a fidelity failure.
    let sym_outcome = match (sym_orig, &sym_fin) {
        (Ok(a), Ok(b)) => {
            if a.number != b.number {
                Some("SYMMETRY_CHANGED".to_string())
            } else {
                None
            }
        }
        (Err(_), Err(_)) => Some("SYMMETRY_UNDETERMINED_BOTH".to_string()),
        (Ok(_), Err(_)) => Some("SYMMETRY_LOST".to_string()),
        (Err(_), Ok(_)) => Some("SYMMETRY_GAINED".to_string()),
    };
    if let Some(s) = &sym_outcome {
        t.notes.push(s.clone());
    }

    t.outcome = worst(cmp.outcome, sym_outcome).unwrap_or_else(|| "PASS".to_string());
    t
}

fn cod_id_of(p: &Path) -> String {
    p.file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "unknown".into())
}

// =========================================================================
// Parent: enumerate, drive children, aggregate
// =========================================================================

struct Args {
    corpus: PathBuf,
    sample: usize,
    seed: u64,
    out: PathBuf,
    timeout_s: u64,
    manifest: Option<PathBuf>,
}

fn parse_args() -> Result<Args, String> {
    let mut corpus = None;
    let mut sample = 0usize;
    let mut seed = 42u64;
    let mut out = PathBuf::from("./audit_results");
    let mut timeout_s = 60u64;
    let mut manifest = None;

    let argv: Vec<String> = std::env::args().collect();
    let mut i = 1;
    while i < argv.len() {
        let take = |i: &mut usize| -> Result<String, String> {
            *i += 1;
            argv.get(*i)
                .cloned()
                .ok_or_else(|| format!("missing value after {}", argv[*i - 1]))
        };
        match argv[i].as_str() {
            "--corpus" => corpus = Some(PathBuf::from(take(&mut i)?)),
            "--sample" => sample = take(&mut i)?.parse().map_err(|e| format!("{e}"))?,
            "--seed" => seed = take(&mut i)?.parse().map_err(|e| format!("{e}"))?,
            "--out" => out = PathBuf::from(take(&mut i)?),
            "--timeout" => timeout_s = take(&mut i)?.parse().map_err(|e| format!("{e}"))?,
            "--manifest" => manifest = Some(PathBuf::from(take(&mut i)?)),
            other => return Err(format!("unknown argument: {other}")),
        }
        i += 1;
    }
    Ok(Args {
        corpus: corpus.ok_or("--corpus is required")?,
        sample,
        seed,
        out,
        timeout_s,
        manifest,
    })
}

fn collect_cifs(dir: &Path) -> Vec<PathBuf> {
    let mut v = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else {
            continue;
        };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().map(|x| x == "cif").unwrap_or(false) {
                v.push(p);
            }
        }
    }
    v.sort();
    v
}

fn machine_metadata() -> serde_json::Value {
    let read = |p: &str| std::fs::read_to_string(p).unwrap_or_default();
    let cpuinfo = read("/proc/cpuinfo");
    let cpu = cpuinfo
        .lines()
        .find(|l| l.starts_with("model name"))
        .and_then(|l| l.split(':').nth(1))
        .map(|s| s.trim().to_string())
        .unwrap_or_default();
    let cores = cpuinfo.lines().filter(|l| l.starts_with("processor")).count();
    let mem = read("/proc/meminfo")
        .lines()
        .find(|l| l.starts_with("MemTotal"))
        .map(|l| l.trim().to_string())
        .unwrap_or_default();
    let os = read("/etc/os-release")
        .lines()
        .find(|l| l.starts_with("PRETTY_NAME="))
        .and_then(|l| l.split('=').nth(1))
        .map(|s| s.trim_matches('"').to_string())
        .unwrap_or_default();
    let rustc = Command::new("rustc")
        .arg("--version")
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default();
    let kernel = read("/proc/sys/kernel/osrelease").trim().to_string();

    serde_json::json!({
        "cpu_model": cpu,
        "logical_cores": cores,
        "memory": mem,
        "os": os,
        "kernel": kernel,
        "rustc": rustc,
        "cview_version": env!("CARGO_PKG_VERSION"),
        "build_profile": "release",
        "symprec": SYMPREC,
        "tolerances": {
            "cell_length_A": TOL_LENGTH_A,
            "cell_angle_deg": TOL_ANGLE_DEG,
            "volume_relative": TOL_VOLUME_REL,
            "occupancy_absolute": TOL_OCCUPANCY,
            "site_position_A": TOL_POSITION_A,
        }
    })
}

fn percentile(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return f64::NAN;
    }
    let idx = ((sorted.len() - 1) as f64 * p).round() as usize;
    sorted[idx.min(sorted.len() - 1)]
}

fn csv_escape(s: &str) -> String {
    if s.contains(',') || s.contains('"') || s.contains('\n') {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

fn main() {
    // ---- worker mode ----
    let argv: Vec<String> = std::env::args().collect();
    if argv.len() >= 2 && argv[1] == "--worker" {
        let path = argv[2].clone();
        let tmpdir = PathBuf::from(argv[3].clone());

        // Capture panic text; the parent turns a missing JSON line into PANIC.
        let msg = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
        let m2 = msg.clone();
        std::panic::set_hook(Box::new(move |info| {
            let text = format!("{info}");
            *m2.lock().unwrap() = text;
        }));

        let path2 = path.clone();
        let result = std::panic::catch_unwind(move || run_worker(&path2, &tmpdir));
        match result {
            Ok(r) => {
                println!("{}", serde_json::to_string(&r).unwrap());
            }
            Err(_) => {
                let detail = msg.lock().unwrap().clone();
                let t = TripResult {
                    outcome: "PANIC".into(),
                    error_detail: detail.clone(),
                    ..Default::default()
                };
                let r = FileResult {
                    cod_id: cod_id_of(Path::new(&path)),
                    file_bytes: std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0),
                    trip_a: t.clone(),
                    trip_b: t,
                    ..Default::default()
                };
                println!("{}", serde_json::to_string(&r).unwrap());
            }
        }
        return;
    }

    // ---- parent mode ----
    let args = match parse_args() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("corpus_audit: {e}");
            eprintln!(
                "usage: corpus_audit --corpus DIR [--sample N] [--seed S] \
                 [--out DIR] [--timeout SEC] [--manifest FILE]"
            );
            std::process::exit(2);
        }
    };

    std::fs::create_dir_all(&args.out).expect("cannot create output directory");
    let tmproot = std::env::temp_dir().join(format!("cview_audit_{}", std::process::id()));
    std::fs::create_dir_all(&tmproot).ok();

    // Build the file list.
    let mut files: Vec<PathBuf> = match &args.manifest {
        Some(m) => {
            let text = std::fs::read_to_string(m).expect("cannot read manifest");
            text.lines()
                .map(|l| l.trim())
                .filter(|l| !l.is_empty() && !l.starts_with('#'))
                .map(|id| args.corpus.join(format!("{id}.cif")))
                .collect()
        }
        None => collect_cifs(&args.corpus),
    };
    files.sort();

    if args.sample > 0 && args.sample < files.len() {
        let mut rng = Rng::new(args.seed);
        rng.sample_in_place(&mut files, args.sample);
        files.sort();
    }

    eprintln!("corpus_audit: {} files to process", files.len());

    let exe = std::env::current_exe().expect("current_exe");
    let mut results: Vec<FileResult> = Vec::with_capacity(files.len());

    for (n, f) in files.iter().enumerate() {
        if n % 25 == 0 {
            eprintln!("  [{}/{}] {}", n, files.len(), f.display());
        }
        let tmpdir = tmproot.join(format!("f{n}"));
        let mut child = Command::new(&exe)
            .arg("--worker")
            .arg(f)
            .arg(&tmpdir)
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("failed to spawn worker");

        // Poll for completion so a hung parse cannot stall the whole run.
        let deadline = Instant::now() + Duration::from_secs(args.timeout_s);
        let mut timed_out = false;
        loop {
            match child.try_wait() {
                Ok(Some(_)) => break,
                Ok(None) => {
                    if Instant::now() > deadline {
                        let _ = child.kill();
                        let _ = child.wait();
                        timed_out = true;
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(5));
                }
                Err(_) => break,
            }
        }

        let mut out = String::new();
        if let Some(mut so) = child.stdout.take() {
            use std::io::Read;
            let _ = so.read_to_string(&mut out);
        }
        std::fs::remove_dir_all(&tmpdir).ok();

        let id = cod_id_of(f);
        let bytes = std::fs::metadata(f).map(|m| m.len()).unwrap_or(0);

        let parsed: Option<FileResult> = out
            .lines()
            .last()
            .and_then(|l| serde_json::from_str::<FileResult>(l).ok());

        match parsed {
            Some(r) => results.push(r),
            None => {
                // No JSON line: the child died. Either killed on timeout, or
                // it aborted / overflowed the stack (which catch_unwind cannot
                // intercept). Both are GUI crashes and must be reported.
                let code = if timed_out { "TIMEOUT" } else { "CHILD_CRASH" };
                let t = TripResult {
                    outcome: code.into(),
                    error_detail: if timed_out {
                        format!("exceeded {} s wall clock", args.timeout_s)
                    } else {
                        "worker terminated without producing a result".into()
                    },
                    ..Default::default()
                };
                results.push(FileResult {
                    cod_id: id,
                    file_bytes: bytes,
                    trip_a: t.clone(),
                    trip_b: t,
                    ..Default::default()
                });
            }
        }
    }

    std::fs::remove_dir_all(&tmproot).ok();
    results.sort_by(|a, b| a.cod_id.cmp(&b.cod_id));

    write_outputs(&args, &results);
    eprintln!("corpus_audit: wrote results to {}", args.out.display());
}

fn write_outputs(args: &Args, results: &[FileResult]) {
    // ---------- corpus_manifest.txt ----------
    let mut man = String::new();
    man.push_str("# COD IDs audited, one per line.\n");
    man.push_str(&format!(
        "# seed={} sample={} corpus={}\n",
        args.seed,
        args.sample,
        args.corpus.display()
    ));
    for r in results {
        man.push_str(&r.cod_id);
        man.push('\n');
    }
    std::fs::write(args.out.join("corpus_manifest.txt"), man).ok();

    // ---------- per_file.csv ----------
    let mut csv = String::new();
    csv.push_str(
        "cod_id,round_trip,outcome,classification,n_atoms,file_bytes,parse_ms,roundtrip_ms,\
         sg_original,sg_final,sg_declared,has_partial_occ,has_oxidation,has_refln_block,\
         has_semicolon_text,has_split_site,crystal_system,max_length_dev_A,max_angle_dev_deg,\
         volume_rel_dev,max_site_dev_A,notes,error_detail\n",
    );
    for r in results {
        for (tag, t) in [("A", &r.trip_a), ("B", &r.trip_b)] {
            csv.push_str(&format!(
                "{},{},{},{},{},{},{:.4},{:.4},{},{},{},{},{},{},{},{},{},{:.3e},{:.3e},{:.3e},{:.3e},{},{}\n",
                csv_escape(&r.cod_id),
                tag,
                csv_escape(&t.outcome),
                csv_escape(&r.classification),
                r.n_atoms,
                r.file_bytes,
                t.parse_ms,
                t.roundtrip_ms,
                csv_escape(&t.sg_original),
                csv_escape(&t.sg_final),
                csv_escape(&r.sg_declared),
                r.has_partial_occ,
                r.has_oxidation,
                r.has_refln_block,
                r.has_semicolon_text,
                r.has_split_site,
                csv_escape(&r.crystal_system),
                t.max_length_dev_a,
                t.max_angle_dev_deg,
                t.volume_rel_dev,
                t.max_pos_dev_a,
                csv_escape(&t.notes.join("; ")),
                csv_escape(&t.error_detail),
            ));
        }
    }
    std::fs::write(args.out.join("per_file.csv"), csv).ok();

    // ---------- aggregates ----------
    // Plain `fn` items rather than closures: closure return-type inference
    // cannot express the "borrow lives as long as the argument" relationship.
    fn ta(r: &FileResult) -> &TripResult {
        &r.trip_a
    }
    fn tb(r: &FileResult) -> &TripResult {
        &r.trip_b
    }
    fn all(_: &FileResult) -> bool {
        true
    }
    fn inorg(r: &FileResult) -> bool {
        r.classification == "inorganic"
    }
    fn org(r: &FileResult) -> bool {
        r.classification == "organic_or_metalorganic"
    }

    type Sel = fn(&FileResult) -> bool;
    type Trip = for<'a> fn(&'a FileResult) -> &'a TripResult;

    let tally = |sel: Sel, trip: Trip| -> BTreeMap<String, usize> {
        let mut m = BTreeMap::new();
        for r in results.iter().filter(|r| sel(r)) {
            *m.entry(trip(r).outcome.clone()).or_insert(0) += 1;
        }
        m
    };

    let mut parse_times: Vec<f64> = results
        .iter()
        .filter(|r| r.trip_a.parse_ms > 0.0)
        .map(|r| r.trip_a.parse_ms)
        .collect();
    parse_times.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let mut rt_a: Vec<f64> = results
        .iter()
        .filter(|r| r.trip_a.roundtrip_ms > 0.0)
        .map(|r| r.trip_a.roundtrip_ms)
        .collect();
    rt_a.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let mut rt_b: Vec<f64> = results
        .iter()
        .filter(|r| r.trip_b.roundtrip_ms > 0.0)
        .map(|r| r.trip_b.roundtrip_ms)
        .collect();
    rt_b.sort_by(|a, b| a.partial_cmp(b).unwrap());

    let slowest = results
        .iter()
        .max_by(|a, b| a.trip_a.parse_ms.partial_cmp(&b.trip_a.parse_ms).unwrap());

    let timing = serde_json::json!({
        "parse_ms": {
            "median": percentile(&parse_times, 0.50),
            "p95": percentile(&parse_times, 0.95),
            "max": parse_times.last().copied().unwrap_or(f64::NAN),
            "n": parse_times.len(),
        },
        "roundtrip_A_ms": {
            "median": percentile(&rt_a, 0.50),
            "p95": percentile(&rt_a, 0.95),
            "max": rt_a.last().copied().unwrap_or(f64::NAN),
        },
        "roundtrip_B_ms": {
            "median": percentile(&rt_b, 0.50),
            "p95": percentile(&rt_b, 0.95),
            "max": rt_b.last().copied().unwrap_or(f64::NAN),
        },
        "slowest_file": slowest.map(|r| serde_json::json!({
            "cod_id": r.cod_id,
            "parse_ms": r.trip_a.parse_ms,
            "file_bytes": r.file_bytes,
            "n_atoms": r.n_atoms,
            "has_refln_block": r.has_refln_block,
        })),
    });

    let sg_declared_vs_detected = {
        let (mut agree, mut differ, mut unknown) = (0, 0, 0);
        for r in results {
            if r.sg_declared.is_empty() || r.trip_a.sg_original == "NA" {
                unknown += 1;
            } else if r.sg_declared == r.trip_a.sg_original {
                agree += 1;
            } else {
                differ += 1;
            }
        }
        serde_json::json!({ "agree": agree, "differ": differ, "not_comparable": unknown })
    };

    let crystal_systems: BTreeMap<String, usize> = {
        let mut m = BTreeMap::new();
        for r in results {
            *m.entry(r.crystal_system.clone()).or_insert(0usize) += 1;
        }
        m
    };

    let summary = serde_json::json!({
        "run": {
            "seed": args.seed,
            "requested_sample": args.sample,
            "files_audited": results.len(),
            "corpus": args.corpus.display().to_string(),
            "per_file_timeout_s": args.timeout_s,
        },
        "machine": machine_metadata(),
        "outcomes": {
            "round_trip_A_cif_cif": {
                "all": tally(all, ta),
                "inorganic": tally(inorg, ta),
                "organic_or_metalorganic": tally(org, ta),
            },
            "round_trip_B_cif_poscar_cif": {
                "all": tally(all, tb),
                "inorganic": tally(inorg, tb),
                "organic_or_metalorganic": tally(org, tb),
            },
        },
        "classification_counts": {
            "inorganic": results.iter().filter(|r| inorg(r)).count(),
            "organic_or_metalorganic": results.iter().filter(|r| org(r)).count(),
        },
        "feature_counts": {
            "partial_occupancy": results.iter().filter(|r| r.has_partial_occ).count(),
            "oxidation_states": results.iter().filter(|r| r.has_oxidation).count(),
            "split_sites": results.iter().filter(|r| r.has_split_site).count(),
            "refln_block": results.iter().filter(|r| r.has_refln_block).count(),
            "semicolon_text": results.iter().filter(|r| r.has_semicolon_text).count(),
        },
        "crystal_systems": crystal_systems,
        "declared_vs_detected_space_group": sg_declared_vs_detected,
        "timing": timing,
    });
    std::fs::write(
        args.out.join("summary.json"),
        serde_json::to_string_pretty(&summary).unwrap(),
    )
    .ok();

    // ---------- summary.md ----------
    let mut md = String::new();
    md.push_str("# CView corpus round-trip fidelity audit\n\n");
    md.push_str(&format!(
        "Files audited: **{}** (seed {}, per-file timeout {} s)\n\n",
        results.len(),
        args.seed,
        args.timeout_s
    ));

    let table = |m: &BTreeMap<String, usize>, total: usize, out: &mut String| {
        out.push_str("| Outcome | Files | % |\n|---|---:|---:|\n");
        let mut rows: Vec<(&String, &usize)> = m.iter().collect();
        rows.sort_by(|a, b| severity(b.0).cmp(&severity(a.0)));
        for (k, v) in rows {
            let pct = if total > 0 {
                100.0 * *v as f64 / total as f64
            } else {
                0.0
            };
            out.push_str(&format!("| `{k}` | {v} | {pct:.1} |\n"));
        }
        out.push('\n');
    };

    let n_all = results.len();
    let n_in = results.iter().filter(|r| inorg(r)).count();
    let n_or = results.iter().filter(|r| org(r)).count();

    md.push_str("## Round trip A — CIF → CIF\n\n### All files\n\n");
    table(&tally(all, ta), n_all, &mut md);
    md.push_str("### Inorganic only\n\n");
    table(&tally(inorg, ta), n_in, &mut md);
    md.push_str("### Organic / metal-organic only\n\n");
    table(&tally(org, ta), n_or, &mut md);

    md.push_str("## Round trip B — CIF → POSCAR → CIF\n\n### All files\n\n");
    table(&tally(all, tb), n_all, &mut md);
    md.push_str("### Inorganic only\n\n");
    table(&tally(inorg, tb), n_in, &mut md);
    md.push_str("### Organic / metal-organic only\n\n");
    table(&tally(org, tb), n_or, &mut md);

    md.push_str("## Timing (single-file latency, warm cache, release build)\n\n");
    md.push_str("| Quantity | Median | p95 | Max |\n|---|---:|---:|---:|\n");
    md.push_str(&format!(
        "| `load_structure` parse (ms) | {:.2} | {:.2} | {:.2} |\n",
        percentile(&parse_times, 0.50),
        percentile(&parse_times, 0.95),
        parse_times.last().copied().unwrap_or(f64::NAN)
    ));
    md.push_str(&format!(
        "| Round trip A total (ms) | {:.2} | {:.2} | {:.2} |\n",
        percentile(&rt_a, 0.50),
        percentile(&rt_a, 0.95),
        rt_a.last().copied().unwrap_or(f64::NAN)
    ));
    md.push_str(&format!(
        "| Round trip B total (ms) | {:.2} | {:.2} | {:.2} |\n\n",
        percentile(&rt_b, 0.50),
        percentile(&rt_b, 0.95),
        rt_b.last().copied().unwrap_or(f64::NAN)
    ));
    if let Some(s) = slowest {
        md.push_str(&format!(
            "Worst-case parse: COD **{}** — {:.1} ms, {} bytes, {} atoms, reflection block: {}.\n\n",
            s.cod_id, s.trip_a.parse_ms, s.file_bytes, s.n_atoms, s.has_refln_block
        ));
    }

    md.push_str("## Notable failures\n\n");
    for (tag, f) in [
        ("A", ta as Trip),
        ("B", tb as Trip),
    ] {
        let mut by_code: BTreeMap<String, Vec<&FileResult>> = BTreeMap::new();
        for r in results {
            let o = &f(r).outcome;
            if o != "PASS" {
                by_code.entry(o.clone()).or_default().push(r);
            }
        }
        if by_code.is_empty() {
            continue;
        }
        md.push_str(&format!("### Round trip {tag}\n\n"));
        let mut codes: Vec<&String> = by_code.keys().collect();
        codes.sort_by(|a, b| severity(b).cmp(&severity(a)));
        for code in codes {
            let v = &by_code[code];
            md.push_str(&format!("**`{}`** — {} file(s)\n\n", code, v.len()));
            for r in v.iter().take(25) {
                let t = f(r);
                md.push_str(&format!(
                    "- COD {} — sg {}→{}, max Δlen {:.2e} Å, max Δsite {:.2e} Å{}\n",
                    r.cod_id,
                    t.sg_original,
                    t.sg_final,
                    t.max_length_dev_a,
                    t.max_pos_dev_a,
                    if t.error_detail.is_empty() {
                        String::new()
                    } else {
                        format!(" — {}", t.error_detail)
                    }
                ));
            }
            if v.len() > 25 {
                md.push_str(&format!("- … and {} more (see per_file.csv)\n", v.len() - 25));
            }
            md.push('\n');
        }
    }

    md.push_str("## Method notes\n\n");
    md.push_str(&format!(
        "- Space group compared as `symmetry::analyze()` on the **parsed original** vs the \
         **parsed final**, never against the CIF header. SYMPREC = {SYMPREC:e}.\n"
    ));
    md.push_str(
        "- Classification rule: a structure is *organic / metal-organic* if it contains both \
         C and H; otherwise *inorganic*. Hydrated carbonates fall on the organic side.\n",
    );
    md.push_str(
        "- POSCAR carries neither occupancy nor oxidation state; loss of these in round trip B \
         is format-inherent and reported as `LOSS_*_EXPECTED`, not as failure.\n",
    );
    md.push_str(&format!(
        "- Tolerances: cell length {TOL_LENGTH_A:e} Å, angle {TOL_ANGLE_DEG:e}°, volume \
         {TOL_VOLUME_REL:e} relative, site position {TOL_POSITION_A:e} Å, occupancy \
         {TOL_OCCUPANCY:e}.\n"
    ));
    md.push_str(
        "- Each file runs in an isolated child process; panics, aborts and hangs are recorded \
         rather than ending the run. Timings are the second of two passes (warm page cache).\n",
    );

    std::fs::write(args.out.join("summary.md"), md).ok();

    let _ = std::io::stdout().flush();
}
