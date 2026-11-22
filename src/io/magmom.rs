// src/io/magmom.rs
//
// Magnetic moments for formats that keep them apart from the geometry.
//
// VASP: a POSCAR/CONTCAR has no moments. The INCAR beside it gives the
// starting moments (`MAGMOM`), the OUTCAR the computed ones (the
// `magnetization (x/y/z)` tables, one per ionic step). vasprun.xml carries
// the expanded `MAGMOM` in its <parameters>. Moments are converted to
// Cartesian with `SAXIS` (VASP wiki, "SAXIS"); a collinear moment m becomes
// m·ŝ, i.e. (0, 0, m) for the default SAXIS = (0, 0, 1).
//
// Quantum ESPRESSO: pw.x prints "Magnetic moment per site" blocks during
// the SCF; `qe.rs` reads those with `qe_site_moments`.

use std::path::Path;

pub type Moments = Vec<[f64; 3]>;

/// What an INCAR (or vasprun <parameters>) says about magnetism.
#[derive(Debug, Default, Clone)]
pub struct VaspMagnetism {
    /// MAGMOM values, `N*x` repeats expanded.
    pub magmom: Option<Vec<f64>>,
    pub ispin: Option<i32>,
    pub noncollinear: bool,
    pub saxis: Option<[f64; 3]>,
}

impl VaspMagnetism {
    /// Whether VASP would use spin at all: ISPIN = 2, or a non-collinear run.
    /// With ISPIN = 1 (the default) MAGMOM is ignored.
    pub fn spin_polarised(&self) -> bool {
        self.noncollinear || self.ispin == Some(2)
    }

    /// Cartesian moments for `n` atoms, or `None` when MAGMOM is missing,
    /// ignored (ISPIN = 1), or does not have N (collinear) / 3N
    /// (non-collinear) values.
    pub fn moments(&self, n: usize) -> Option<Moments> {
        if !self.spin_polarised() || n == 0 {
            return None;
        }
        let vals = self.magmom.as_ref()?;
        let saxis = self.saxis.unwrap_or([0.0, 0.0, 1.0]);
        if self.noncollinear {
            (vals.len() == 3 * n).then(|| {
                vals.chunks(3)
                    .map(|c| spinor_to_cartesian([c[0], c[1], c[2]], saxis))
                    .collect()
            })
        } else {
            (vals.len() == n).then(|| vals.iter().map(|&m| spinor_to_cartesian([0.0, 0.0, m], saxis)).collect())
        }
    }
}

/// VASP spinor frame → Cartesian, given SAXIS. α is the azimuth of SAXIS,
/// β its polar angle; SAXIS = (0, 0, 1) is the identity.
pub fn spinor_to_cartesian(m: [f64; 3], saxis: [f64; 3]) -> [f64; 3] {
    let [sx, sy, sz] = saxis;
    let alpha = sy.atan2(sx);
    let beta = (sx * sx + sy * sy).sqrt().atan2(sz);
    let (sa, ca) = alpha.sin_cos();
    let (sb, cb) = beta.sin_cos();
    [
        cb * ca * m[0] - sa * m[1] + sb * ca * m[2],
        cb * sa * m[0] + ca * m[1] + sb * sa * m[2],
        -sb * m[0] + cb * m[2],
    ]
}

/// Expand a MAGMOM value list: `2*5.0 3*0 1.5` → [5, 5, 0, 0, 0, 1.5].
/// `None` when a token is not a number.
pub fn expand_values(s: &str) -> Option<Vec<f64>> {
    let mut out = Vec::new();
    for tok in s.split_whitespace() {
        match tok.split_once('*') {
            Some((n, v)) => {
                let n: usize = n.parse().ok()?;
                let v: f64 = v.parse().ok()?;
                out.extend(std::iter::repeat(v).take(n));
            }
            None => out.push(tok.parse().ok()?),
        }
    }
    Some(out)
}

fn fortran_bool(v: &str) -> bool {
    let v = v.trim().trim_start_matches('.').to_ascii_uppercase();
    v.starts_with('T')
}

/// Read the magnetism tags from INCAR text. Handles `#`/`!` comments, `;`
/// between tags on one line and `\` line continuations.
pub fn parse_incar(text: &str) -> VaspMagnetism {
    // Join continuations first, then split into `TAG = value` statements.
    let mut joined = String::with_capacity(text.len());
    for line in text.lines() {
        let line = line.split(['#', '!']).next().unwrap_or("");
        match line.trim_end().strip_suffix('\\') {
            Some(head) => {
                joined.push_str(head);
                joined.push(' ');
            }
            None => {
                joined.push_str(line);
                joined.push('\n');
            }
        }
    }
    let mut out = VaspMagnetism::default();
    for stmt in joined.split(['\n', ';']) {
        let Some((tag, val)) = stmt.split_once('=') else { continue };
        match tag.trim().to_ascii_uppercase().as_str() {
            "MAGMOM" => out.magmom = expand_values(val),
            "ISPIN" => out.ispin = val.trim().parse().ok(),
            "LNONCOLLINEAR" => out.noncollinear = fortran_bool(val),
            "LSORBIT" => out.noncollinear |= fortran_bool(val), // implies non-collinear
            "SAXIS" => {
                let v: Vec<f64> = val.split_whitespace().filter_map(|t| t.parse().ok()).collect();
                if v.len() == 3 {
                    out.saxis = Some([v[0], v[1], v[2]]);
                }
            }
            _ => {}
        }
    }
    out
}

/// Every set of computed moments in an OUTCAR, in file order (one per ionic
/// step, the last one is the final result). Each set has `n` entries. A
/// collinear run prints only `magnetization (x)`, which is then m_z.
pub fn parse_outcar(text: &str, n: usize, saxis: Option<[f64; 3]>) -> Vec<Moments> {
    // Tables come as (x) [then (y), (z) for non-collinear runs]. Group them:
    // a new (x) starts a new set.
    let mut sets: Vec<[Option<Vec<f64>>; 3]> = Vec::new();
    let lines: Vec<&str> = text.lines().collect();
    let mut i = 0;
    while i < lines.len() {
        let t = lines[i].trim();
        let axis = match t {
            "magnetization (x)" => Some(0),
            "magnetization (y)" => Some(1),
            "magnetization (z)" => Some(2),
            _ => None,
        };
        let Some(axis) = axis else {
            i += 1;
            continue;
        };
        // Header, dashes, then one row per ion: index s p d [f] tot.
        let mut vals = Vec::new();
        let mut k = i + 1;
        let mut seen_rule = false;
        while let Some(l) = lines.get(k) {
            let l = l.trim();
            if l.starts_with("---") {
                if seen_rule {
                    break;
                }
                seen_rule = true;
            } else if seen_rule {
                let cols: Vec<&str> = l.split_whitespace().collect();
                if cols.first().is_some_and(|c| c.parse::<usize>().is_ok()) {
                    match cols.last().and_then(|c| c.parse::<f64>().ok()) {
                        Some(v) => vals.push(v),
                        None => break,
                    }
                } else {
                    break;
                }
            }
            k += 1;
        }
        if axis == 0 {
            sets.push([None, None, None]);
        }
        if let Some(set) = sets.last_mut() {
            set[axis] = Some(vals);
        }
        i = k;
    }

    let saxis = saxis.unwrap_or([0.0, 0.0, 1.0]);
    sets.into_iter()
        .filter_map(|[x, y, z]| {
            let x = x.filter(|v| v.len() == n)?;
            match (y.filter(|v| v.len() == n), z.filter(|v| v.len() == n)) {
                (Some(y), Some(z)) => Some(
                    (0..n)
                        .map(|a| spinor_to_cartesian([x[a], y[a], z[a]], saxis))
                        .collect(),
                ),
                _ => Some(x.iter().map(|&m| spinor_to_cartesian([0.0, 0.0, m], saxis)).collect()),
            }
        })
        .collect()
}

/// A file named `name` (any case) in the directory of `path`.
fn sibling(path: &str, name: &str) -> Option<std::path::PathBuf> {
    let dir = Path::new(path).parent().filter(|d| !d.as_os_str().is_empty()).unwrap_or(Path::new("."));
    std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .find(|p| p.is_file() && p.file_name().is_some_and(|f| f.to_string_lossy().eq_ignore_ascii_case(name)))
}

/// The INCAR beside `path`, parsed, if there is one.
pub fn sibling_incar(path: &str) -> Option<VaspMagnetism> {
    let text = std::fs::read_to_string(sibling(path, "INCAR")?).ok()?;
    Some(parse_incar(&text))
}

/// Every moment set in the OUTCAR beside `path` that has `n` atoms.
pub fn sibling_outcar(path: &str, n: usize, saxis: Option<[f64; 3]>) -> Vec<Moments> {
    sibling(path, "OUTCAR")
        .and_then(|p| std::fs::read_to_string(p).ok())
        .map(|t| parse_outcar(&t, n, saxis))
        .unwrap_or_default()
}

/// Moments for a POSCAR-family file with `n` atoms, from the files beside
/// it, and which file they came from. A CONTCAR is a result, so the OUTCAR's
/// final moments come first; for anything else (a POSCAR is an input) the
/// INCAR's starting moments come first. The other is the fallback.
pub fn vasp_sidecar_moments(path: &str, n: usize) -> Option<(Moments, &'static str)> {
    let incar = sibling_incar(path);
    let saxis = incar.as_ref().and_then(|i| i.saxis);
    let from_incar = || incar.as_ref().and_then(|i| i.moments(n)).map(|m| (m, "INCAR MAGMOM"));
    let from_outcar = || sibling_outcar(path, n, saxis).pop().map(|m| (m, "OUTCAR"));
    let is_contcar = Path::new(path)
        .file_name()
        .is_some_and(|f| f.to_string_lossy().to_ascii_lowercase().starts_with("contcar"));
    if is_contcar {
        from_outcar().or_else(from_incar)
    } else {
        from_incar().or_else(from_outcar)
    }
}

/// `MAGMOM` values in POSCAR atom order (`poscar::write_order`), with
/// `N*x` for runs. Collinear moments are written as their component along
/// the magnetic axis; anything else as Cartesian (x, y, z) triples for a
/// non-collinear run. Atoms without a moment get 0.
fn magmom_values(structure: &crate::model::Structure, collinear: bool, axis: [f64; 3]) -> String {
    let mut vals: Vec<f64> = Vec::new();
    for i in super::poscar::write_order(structure) {
        let m = structure.atoms[i].magmom.unwrap_or([0.0; 3]);
        if collinear {
            vals.push(m[0] * axis[0] + m[1] * axis[1] + m[2] * axis[2]);
        } else {
            vals.extend(m);
        }
    }
    let fmt = |v: f64| {
        let s = format!("{:.3}", v);
        let s = s.trim_end_matches('0').trim_end_matches('.');
        if s == "-0" { "0".to_string() } else { s.to_string() }
    };
    let mut out: Vec<String> = Vec::new();
    let mut k = 0;
    while k < vals.len() {
        let v = fmt(vals[k]);
        let mut run = 1;
        while k + run < vals.len() && fmt(vals[k + run]) == v {
            run += 1;
        }
        out.push(if run > 1 { format!("{run}*{v}") } else { v });
        k += run;
    }
    out.join(" ")
}

/// INCAR magnetism tags for `structure` as a POSCAR would list its atoms, or
/// `None` without moments: `ISPIN = 2` + `MAGMOM` for a collinear structure,
/// `LNONCOLLINEAR = .TRUE.` + 3N `MAGMOM` values otherwise.
pub fn vasp_incar_tags(structure: &crate::model::Structure) -> Option<String> {
    let axis = structure.magnetic_axis()?;
    if structure.moments_collinear() {
        let mut out = String::new();
        if axis[2] < 1.0 - 1e-6 {
            out.push_str(&format!(
                "# Moments are collinear along ({:.4}, {:.4}, {:.4}); MAGMOM gives each atom's\n\
                 # component along it. With spin-orbit coupling also set SAXIS to that axis.\n",
                axis[0], axis[1], axis[2]
            ));
        }
        out.push_str(&format!("ISPIN = 2\nMAGMOM = {}\n", magmom_values(structure, true, axis)));
        Some(out)
    } else {
        Some(format!(
            "LNONCOLLINEAR = .TRUE.\nMAGMOM = {}\n",
            magmom_values(structure, false, axis)
        ))
    }
}

/// Moments from a pw.x "Magnetic moment per site" block starting after
/// line `start` (the header). Accepts both layouts:
///   `atom   1 (R=0.177)  charge=  6.6036  magn=  2.1817  constr=  0.0000`
///   `atom:    1    charge:   13.9531    magn:    2.2061    constr:    0.0000`
/// and three `magn` values for non-collinear runs. Returns the moments and
/// the index of the first line after the block.
pub fn qe_site_moments(lines: &[&str], start: usize) -> (Moments, usize) {
    let mut out = Vec::new();
    let mut k = start + 1;
    while let Some(l) = lines.get(k) {
        let t = l.trim();
        if !t.starts_with("atom") {
            break;
        }
        let Some(after) = t.split_once("magn").map(|(_, r)| r.trim_start_matches(['=', ':'])) else {
            break;
        };
        let vals: Vec<f64> = after
            .split("constr")
            .next()
            .unwrap_or("")
            .split_whitespace()
            .map_while(|v| v.parse().ok())
            .collect();
        match vals.as_slice() {
            [m] => out.push([0.0, 0.0, *m]),
            [x, y, z, ..] => out.push([*x, *y, *z]),
            _ => break,
        }
        k += 1;
    }
    (out, k)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: [f64; 3], b: [f64; 3]) {
        for k in 0..3 {
            assert!((a[k] - b[k]).abs() < 1e-9, "{:?} vs {:?}", a, b);
        }
    }

    #[test]
    fn incar_collinear_with_repeats_comments_and_semicolons() {
        let m = parse_incar("SYSTEM = FeO\nISPIN = 2 ; ENCUT = 500\nMAGMOM = 2*5.0 \\\n  2*0  # O\n");
        assert_eq!(m.magmom, Some(vec![5.0, 5.0, 0.0, 0.0]));
        let v = m.moments(4).unwrap();
        close(v[1], [0.0, 0.0, 5.0]);
        assert!(m.moments(3).is_none(), "count must match");
    }

    #[test]
    fn incar_magmom_is_ignored_without_spin() {
        let m = parse_incar("MAGMOM = 5 5\n");
        assert!(m.moments(2).is_none());
    }

    #[test]
    fn incar_noncollinear_and_saxis() {
        let m = parse_incar("LSORBIT = .TRUE.\nMAGMOM = 0 0 2  1 0 0\n");
        let v = m.moments(2).unwrap();
        close(v[0], [0.0, 0.0, 2.0]);
        close(v[1], [1.0, 0.0, 0.0]);
        // SAXIS along x: the spinor z axis becomes Cartesian x.
        let m = parse_incar("ISPIN = 2\nSAXIS = 1 0 0\nMAGMOM = 3\n");
        close(m.moments(1).unwrap()[0], [3.0, 0.0, 0.0]);
    }

    const OUTCAR: &str = "
 magnetization (x)

# of ion       s       p       d       tot
------------------------------------------
    1       -0.010  -0.030   2.000   1.960
    2       -0.010  -0.030  -2.000  -2.040
--------------------------------------------------
tot         -0.020  -0.060   0.000  -0.080

 magnetization (x)

# of ion       s       p       d       tot
------------------------------------------
    1       -0.010  -0.030   2.200   2.160
    2       -0.010  -0.030  -2.200  -2.240
--------------------------------------------------
tot         -0.020  -0.060   0.000  -0.080
";

    #[test]
    fn outcar_collinear_keeps_every_step() {
        let sets = parse_outcar(OUTCAR, 2, None);
        assert_eq!(sets.len(), 2);
        close(sets[1][1], [0.0, 0.0, -2.24]);
        assert!(parse_outcar(OUTCAR, 3, None).is_empty());
    }

    #[test]
    fn outcar_noncollinear_combines_xyz() {
        let block = |axis: &str, v: f64| {
            format!(
                " magnetization ({})\n\n# of ion  s  p  d  tot\n----------\n    1  0 0 0 {}\n----------\ntot 0 0 0 0\n",
                axis, v
            )
        };
        let text = format!("{}{}{}", block("x", 1.0), block("y", 2.0), block("z", 3.0));
        let sets = parse_outcar(&text, 1, None);
        assert_eq!(sets.len(), 1);
        close(sets[0][0], [1.0, 2.0, 3.0]);
    }

    #[test]
    fn qe_both_layouts() {
        let new = [
            "     Magnetic moment per site  (integrated on atomic sphere of radius R)",
            "     atom   1 (R=0.177)  charge=  6.6036  magn=  2.1817  constr=    0.0000",
            "     atom   2 (R=0.177)  charge=  6.6036  magn= -2.1817  constr=    0.0000",
            "",
        ];
        let (m, next) = qe_site_moments(&new, 0);
        assert_eq!(next, 3);
        close(m[1], [0.0, 0.0, -2.1817]);

        let old = [
            "     Magnetic moment per site:",
            "     atom:    1    charge:   13.9531    magn:    2.2061    constr:    0.0000",
        ];
        close(qe_site_moments(&old, 0).0[0], [0.0, 0.0, 2.2061]);

        let nc = [
            "     Magnetic moment per site  (integrated on atomic sphere of radius R)",
            "     atom    1 (R=0.183)  charge= 14.1061  magn=   0.1000   0.0000   2.2096  constr=    0.0000   0.0000   0.0000",
        ];
        close(qe_site_moments(&nc, 0).0[0], [0.1, 0.0, 2.2096]);
    }

    fn structure(atoms: &[(&str, Option<[f64; 3]>)]) -> crate::model::Structure {
        crate::model::Structure {
            lattice: [[4.0, 0.0, 0.0], [0.0, 4.0, 0.0], [0.0, 0.0, 4.0]],
            atoms: atoms
                .iter()
                .enumerate()
                .map(|(i, (el, m))| crate::model::Atom {
                    element: el.to_string(),
                    position: [i as f64 * 0.5, 0.0, 0.0],
                    original_index: i,
                    oxidation: None,
                    occupancy: 1.0,
                    magmom: *m,
                })
                .collect(),
            formula: String::new(),
            is_periodic: true,
        }
    }

    #[test]
    fn magmom_follows_poscar_order_and_compresses_runs() {
        // Interleaved Fe/O: the POSCAR groups Fe first, then O.
        let up = Some([0.0, 0.0, 3.6]);
        let down = Some([0.0, 0.0, -3.6]);
        let s = structure(&[("O", None), ("Fe", up), ("O", None), ("Fe", up), ("Fe", down), ("Fe", down)]);
        let tags = vasp_incar_tags(&s).unwrap();
        assert_eq!(tags, "ISPIN = 2\nMAGMOM = 2*3.6 2*-3.6 2*0\n");
        // And it reads back to the same moments in POSCAR order.
        let m = parse_incar(&tags).moments(6).unwrap();
        assert_eq!(m[0], [0.0, 0.0, 3.6]);
        assert_eq!(m[3], [0.0, 0.0, -3.6]);
        assert_eq!(m[5], [0.0, 0.0, 0.0]);
    }

    #[test]
    fn noncollinear_and_in_plane_collinear_tags() {
        let s = structure(&[("Mn", Some([1.0, 0.0, 0.0])), ("Mn", Some([0.0, 1.0, 0.0]))]);
        assert_eq!(vasp_incar_tags(&s).unwrap(), "LNONCOLLINEAR = .TRUE.\nMAGMOM = 1 3*0 1 0\n");
        let s = structure(&[("Mn", Some([-2.0, 0.0, 0.0])), ("Mn", Some([2.0, 0.0, 0.0]))]);
        let tags = vasp_incar_tags(&s).unwrap();
        assert!(tags.starts_with("# Moments are collinear along (1.0000, 0.0000, 0.0000)"), "{tags}");
        assert!(tags.ends_with("ISPIN = 2\nMAGMOM = -2 2\n"), "{tags}");
        assert!(vasp_incar_tags(&structure(&[("Cu", None)])).is_none());
    }

    #[test]
    fn saving_a_poscar_writes_an_incar_that_reads_back() {
        let dir = std::env::temp_dir().join(format!("cview_magsave_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let s = structure(&[("Ni", Some([0.0, 0.0, 1.7])), ("O", None), ("Ni", Some([0.0, 0.0, -1.7]))]);
        let path = dir.join("POSCAR");
        crate::io::save_structure(path.to_str().unwrap(), &s).unwrap();
        assert!(dir.join("INCAR").exists());
        let back = crate::io::load_structure(path.to_str().unwrap()).unwrap();
        let mz: Vec<f64> = back.atoms.iter().map(|a| a.magmom.unwrap()[2]).collect();
        assert_eq!(mz, vec![1.7, -1.7, 0.0], "Ni, Ni, O in POSCAR order");

        // A second save does not touch the INCAR that is now there.
        std::fs::write(dir.join("INCAR"), "ENCUT = 500\n").unwrap();
        crate::io::save_structure(path.to_str().unwrap(), &s).unwrap();
        assert_eq!(std::fs::read_to_string(dir.join("INCAR")).unwrap(), "ENCUT = 500\n");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
