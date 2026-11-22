// src/io/ovf.rs
//
// OOMMF Vector Field files (.ovf, .omf, .ohf): the magnetization written by
// mumax3, OOMMF and Spirit. One site per mesh cell, placed at the cell
// centre, carrying the field vector there as its moment.
//
// Supported: OVF 1.0 and 2.0; rectangular meshes (positions from
// xbase/xstepsize/xnodes) and irregular ones (positions in the data);
// data as text or binary 4/8. Binary byte order comes from the check value
// (OVF 2.0 is little-endian, OVF 1.0 big-endian). Only the first segment
// is read, and only vector fields (valuedim 3).
//
// Cells whose vector is zero (outside the magnet in a mumax3 geometry) are
// left out, so a disc reads as a disc. A field in A/m is divided by its
// largest magnitude, so moments are m = M/Ms_max (unitless) either way.
// Meshes beyond MAX_SITES cells are thinned in-plane.

use crate::model::{Atom, Structure};
use std::io;

/// Above this many cells the mesh is thinned (every k-th cell in x and y).
pub const MAX_SITES: usize = 40_000;

/// Name the mesh sites carry instead of an element.
pub const SITE_NAME: &str = "Cell";

fn bad(msg: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, msg.into())
}

/// True when the file starts like an OVF file.
pub fn sniff(bytes: &[u8]) -> bool {
    let head = String::from_utf8_lossy(&bytes[..bytes.len().min(64)]).to_ascii_lowercase();
    head.starts_with("# oommf")
}

#[derive(Default, Debug)]
struct Header {
    version: u8,
    irregular: bool,
    nodes: [usize; 3],
    base: [f64; 3],
    step: [f64; 3],
    min: [f64; 3],
    max: [f64; 3],
    unit_to_ang: f64,
    valuedim: usize,
    multiplier: f64,
    value_unit: String,
    point_count: usize,
    title: String,
}

/// Å per mesh unit.
fn unit_scale(u: &str) -> f64 {
    match u.trim().to_ascii_lowercase().as_str() {
        "nm" => 10.0,
        "um" | "µm" | "μm" => 1e4,
        "a" | "å" | "angstrom" => 1.0,
        "pm" => 0.01,
        _ => 1e10, // "m", and the OVF default
    }
}

pub fn parse(path: &str) -> io::Result<Structure> {
    let bytes = std::fs::read(path)?;
    parse_bytes(&bytes)
}

pub fn parse_bytes(bytes: &[u8]) -> io::Result<Structure> {
    if !sniff(bytes) {
        return Err(bad("not an OVF file (no '# OOMMF' first line)"));
    }
    let mut h = Header { unit_to_ang: 1e10, valuedim: 3, multiplier: 1.0, ..Default::default() };

    // Header: '#' lines up to "# Begin: Data ...".
    let mut pos = 0usize;
    let data_kind: String = loop {
        let end = bytes[pos..].iter().position(|&b| b == b'\n').map(|e| pos + e).ok_or_else(|| bad("OVF file ends before its data"))?;
        let line = String::from_utf8_lossy(&bytes[pos..end]).trim().to_string();
        pos = end + 1;
        let Some(body) = line.strip_prefix('#') else { continue };
        let body = body.trim();
        let lower = body.to_ascii_lowercase();
        if lower.starts_with("oommf") {
            h.version = if lower.contains("2.0") { 2 } else { 1 };
            continue;
        }
        if let Some(kind) = lower.strip_prefix("begin: data") {
            break kind.trim().to_string();
        }
        let Some((key, val)) = body.split_once(':') else { continue };
        let (key, val) = (key.trim().to_ascii_lowercase(), val.trim());
        let num = || val.split_whitespace().next().and_then(|v| v.parse::<f64>().ok()).unwrap_or(0.0);
        let axis = |k: &str| match k.chars().next() {
            Some('x') => Some(0),
            Some('y') => Some(1),
            Some('z') => Some(2),
            _ => None,
        };
        match key.as_str() {
            "meshtype" => h.irregular = val.eq_ignore_ascii_case("irregular"),
            "meshunit" => h.unit_to_ang = unit_scale(val),
            "valuedim" => h.valuedim = num() as usize,
            "valuemultiplier" => h.multiplier = num(),
            "valueunit" | "valueunits" => h.value_unit = val.split_whitespace().next().unwrap_or("").to_string(),
            "pointcount" => h.point_count = num() as usize,
            "title" => h.title = val.to_string(),
            k if k.len() < 2 => {}
            k if k[1..].starts_with("nodes") => {
                if let Some(a) = axis(k) {
                    h.nodes[a] = num() as usize;
                }
            }
            k if k[1..].starts_with("base") => {
                if let Some(a) = axis(k) {
                    h.base[a] = num();
                }
            }
            k if k[1..].starts_with("stepsize") => {
                if let Some(a) = axis(k) {
                    h.step[a] = num();
                }
            }
            k if &k[1..] == "min" => {
                if let Some(a) = axis(k) {
                    h.min[a] = num();
                }
            }
            k if &k[1..] == "max" => {
                if let Some(a) = axis(k) {
                    h.max[a] = num();
                }
            }
            _ => {}
        }
    };
    if h.version == 1 {
        h.valuedim = 3; // OVF 1.0 is always a 3-vector field
    }
    if h.valuedim != 3 {
        return Err(bad(format!("OVF field has {} component(s); only vector fields (3) are shown", h.valuedim)));
    }

    let n_nodes = if h.irregular { h.point_count } else { h.nodes.iter().product() };
    if n_nodes == 0 {
        return Err(bad("OVF header gives no mesh size (xnodes/ynodes/znodes or pointcount)"));
    }
    let per_node = if h.irregular { 6 } else { 3 };
    let count = n_nodes * per_node;

    let values: Vec<f64> = if let Some(width) = data_kind.strip_prefix("binary") {
        let width: usize = width.trim().parse().map_err(|_| bad("OVF binary data width"))?;
        read_binary(&bytes[pos..], width, count)?
    } else if data_kind.starts_with("text") {
        let text = String::from_utf8_lossy(&bytes[pos..]);
        let v: Vec<f64> = text
            .lines()
            .take_while(|l| !l.trim_start().starts_with('#'))
            .flat_map(|l| l.split_whitespace().map(str::to_string).collect::<Vec<_>>())
            .filter_map(|t| t.parse().ok())
            .take(count)
            .collect();
        if v.len() < count {
            return Err(bad(format!("OVF text data has {} of {} values", v.len(), count)));
        }
        v
    } else {
        return Err(bad(format!("OVF data format '{}' is not supported", data_kind)));
    };

    build(&h, &values)
}

/// `count` values after the check value, in the byte order the check value
/// is in.
fn read_binary(data: &[u8], width: usize, count: usize) -> io::Result<Vec<f64>> {
    let need = width * (count + 1);
    if data.len() < need {
        return Err(bad(format!("OVF binary data is {} bytes, {} needed", data.len(), need)));
    }
    match width {
        4 => {
            let le = f32::from_le_bytes(data[0..4].try_into().unwrap());
            let be = f32::from_be_bytes(data[0..4].try_into().unwrap());
            let little = if le == 1_234_567.0 {
                true
            } else if be == 1_234_567.0 {
                false
            } else {
                return Err(bad("OVF binary 4 check value is not 1234567"));
            };
            Ok(data[4..need]
                .chunks_exact(4)
                .map(|c| {
                    let b: [u8; 4] = c.try_into().unwrap();
                    (if little { f32::from_le_bytes(b) } else { f32::from_be_bytes(b) }) as f64
                })
                .collect())
        }
        8 => {
            let le = f64::from_le_bytes(data[0..8].try_into().unwrap());
            let be = f64::from_be_bytes(data[0..8].try_into().unwrap());
            let little = if le == 123_456_789_012_345.0 {
                true
            } else if be == 123_456_789_012_345.0 {
                false
            } else {
                return Err(bad("OVF binary 8 check value is not 123456789012345"));
            };
            Ok(data[8..need]
                .chunks_exact(8)
                .map(|c| {
                    let b: [u8; 8] = c.try_into().unwrap();
                    if little { f64::from_le_bytes(b) } else { f64::from_be_bytes(b) }
                })
                .collect())
        }
        w => Err(bad(format!("OVF binary width {} is not supported", w))),
    }
}

fn build(h: &Header, values: &[f64]) -> io::Result<Structure> {
    let s = h.unit_to_ang;
    // Sites: (position in mesh units, vector).
    let mut sites: Vec<([f64; 3], [f64; 3])> = Vec::new();
    let mut stride = 1usize;
    if h.irregular {
        for c in values.chunks_exact(6) {
            sites.push(([c[0], c[1], c[2]], [c[3], c[4], c[5]]));
        }
    } else {
        let [nx, ny, nz] = h.nodes;
        // Thin in-plane (x and y) until the mesh fits.
        while (nx.div_ceil(stride)) * (ny.div_ceil(stride)) * nz > MAX_SITES && stride < nx.max(ny) {
            stride += 1;
        }
        for k in 0..nz {
            for j in (0..ny).step_by(stride) {
                for i in (0..nx).step_by(stride) {
                    let o = 3 * (i + nx * (j + ny * k));
                    let p = [
                        h.base[0] + i as f64 * h.step[0],
                        h.base[1] + j as f64 * h.step[1],
                        h.base[2] + k as f64 * h.step[2],
                    ];
                    sites.push((p, [values[o], values[o + 1], values[o + 2]]));
                }
            }
        }
    }

    let norm = |v: [f64; 3]| (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    let total = sites.len();
    sites.retain(|(_, v)| norm(*v) > 0.0);
    let empty = total - sites.len();
    if sites.is_empty() {
        return Err(bad("every cell of the OVF field is zero"));
    }

    // m stays as it is; a magnetization (A/m) is divided by its largest
    // magnitude.
    let largest = sites.iter().map(|(_, v)| norm(*v) * h.multiplier.abs()).fold(0.0, f64::max);
    let normalise = largest > 10.0 || h.value_unit.eq_ignore_ascii_case("a/m");
    let factor = if normalise { h.multiplier / largest } else { h.multiplier };

    // The box: the mesh extent (xmin..xmax) when given, else the node span.
    let mut lo = h.min;
    let mut hi = h.max;
    for a in 0..3 {
        if hi[a] <= lo[a] {
            let (mn, mx) = sites.iter().map(|(p, _)| p[a]).fold((f64::MAX, f64::MIN), |(l, u), x| (l.min(x), u.max(x)));
            let pad = if h.irregular { 0.0 } else { h.step[a] / 2.0 };
            lo[a] = mn - pad;
            hi[a] = mx + pad;
            if hi[a] - lo[a] <= 0.0 {
                hi[a] = lo[a] + 1e-9_f64.max(h.step[a]);
            }
        }
    }
    let lattice = [
        [(hi[0] - lo[0]) * s, 0.0, 0.0],
        [0.0, (hi[1] - lo[1]) * s, 0.0],
        [0.0, 0.0, (hi[2] - lo[2]) * s],
    ];

    let atoms: Vec<Atom> = sites
        .iter()
        .enumerate()
        .map(|(i, (p, v))| Atom {
            element: SITE_NAME.to_string(),
            position: [(p[0] - lo[0]) * s, (p[1] - lo[1]) * s, (p[2] - lo[2]) * s],
            original_index: i,
            oxidation: None,
            occupancy: 1.0,
            magmom: Some(v.map(|x| x * factor)),
        })
        .collect();

    use crate::utils::console::log_info;
    log_info(&format!(
        "OVF {}: {} cell(s){}{}{}",
        if h.version == 2 { "2.0" } else { "1.0" },
        atoms.len(),
        if empty > 0 { format!(", {} empty cell(s) left out", empty) } else { String::new() },
        if stride > 1 {
            format!(", every {}th cell in x and y shown ({} of {}×{})", stride, total, h.nodes[0], h.nodes[1])
        } else {
            String::new()
        },
        if normalise { format!(", divided by the largest |M| = {:.4e} {}", largest, h.value_unit) } else { String::new() },
    ));

    Ok(Structure {
        lattice,
        atoms,
        formula: if h.title.is_empty() { "OVF".to_string() } else { format!("OVF {}", h.title) },
        is_periodic: true,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header(version: &str, extra: &str, data: &str) -> String {
        format!(
            "# OOMMF{version}\n# Segment count: 1\n# Begin: Segment\n# Begin: Header\n# Title: m\n\
             # meshtype: rectangular\n# meshunit: m\n# xmin: 0\n# ymin: 0\n# zmin: 0\n\
             # xmax: 2e-09\n# ymax: 2e-09\n# zmax: 1e-09\n{extra}\
             # xbase: 5e-10\n# ybase: 5e-10\n# zbase: 5e-10\n# xnodes: 2\n# ynodes: 2\n# znodes: 1\n\
             # xstepsize: 1e-09\n# ystepsize: 1e-09\n# zstepsize: 1e-09\n# End: Header\n# Begin: Data {data}\n"
        )
    }

    fn close(a: [f64; 3], b: [f64; 3]) {
        assert!((0..3).all(|k| (a[k] - b[k]).abs() < 1e-6), "{:?} vs {:?}", a, b);
    }

    #[test]
    fn ovf2_text_positions_units_and_empty_cells() {
        let mut t = header(" OVF 2.0", "# valuedim: 3\n", "Text");
        // Cell (1,1) is empty: outside the magnet.
        t.push_str("0 0 1\n1 0 0\n0 1 0\n0 0 0\n# End: Data Text\n# End: Segment\n");
        let s = parse_bytes(t.as_bytes()).unwrap();
        assert_eq!(s.atoms.len(), 3);
        assert_eq!(s.atoms[0].element, SITE_NAME);
        // 1 nm cells, centres at 0.5 nm = 5 Å; box 20 × 20 × 10 Å.
        close(s.atoms[0].position, [5.0, 5.0, 5.0]);
        close(s.atoms[1].position, [15.0, 5.0, 5.0]);
        close(s.atoms[2].position, [5.0, 15.0, 5.0]);
        assert!((s.lattice[0][0] - 20.0).abs() < 1e-9 && (s.lattice[2][2] - 10.0).abs() < 1e-9);
        close(s.atoms[1].magmom.unwrap(), [1.0, 0.0, 0.0]);
    }

    #[test]
    fn ovf2_binary4_little_endian_like_mumax3() {
        let mut b = header(" OVF 2.0", "# valuedim: 3\n# valueunits: 1 1 1\n", "Binary 4").into_bytes();
        b.extend(1_234_567.0f32.to_le_bytes());
        for v in [0.0f32, 0.0, 1.0, 0.0, 0.0, -1.0, 0.6, 0.8, 0.0, -0.6, 0.0, 0.8] {
            b.extend(v.to_le_bytes());
        }
        b.extend(b"\n# End: Data Binary 4\n# End: Segment\n");
        let s = parse_bytes(&b).unwrap();
        assert_eq!(s.atoms.len(), 4);
        close(s.atoms[1].magmom.unwrap(), [0.0, 0.0, -1.0]);
        close(s.atoms[2].magmom.unwrap(), [0.6, 0.8, 0.0]);
    }

    #[test]
    fn ovf1_binary8_big_endian_magnetization_is_normalised() {
        let mut b = header(": rectangular mesh v1.0", "# valueunit: A/m\n# valuemultiplier: 1\n", "Binary 8").into_bytes();
        b.extend(123_456_789_012_345.0f64.to_be_bytes());
        let ms = 8.0e5;
        for v in [0.0, 0.0, ms, 0.0, 0.0, -ms, ms / 2.0, 0.0, 0.0, 0.0, ms, 0.0] {
            b.extend(f64::to_be_bytes(v));
        }
        b.extend(b"\n# End: Data Binary 8\n");
        let s = parse_bytes(&b).unwrap();
        close(s.atoms[0].magmom.unwrap(), [0.0, 0.0, 1.0]);
        close(s.atoms[2].magmom.unwrap(), [0.5, 0.0, 0.0]);
    }

    #[test]
    fn irregular_text_takes_positions_from_the_data() {
        let t = "# OOMMF: irregular mesh v1.0\n# Begin: Header\n# meshtype: irregular\n# meshunit: nm\n\
                 # pointcount: 2\n# End: Header\n# Begin: Data Text\n0 0 0  0 0 1\n0.3 0 0  0 1 0\n# End: Data Text\n";
        let s = parse_bytes(t.as_bytes()).unwrap();
        assert_eq!(s.atoms.len(), 2);
        close(s.atoms[1].position, [3.0, 0.0, 0.0]);
        close(s.atoms[1].magmom.unwrap(), [0.0, 1.0, 0.0]);
    }

    #[test]
    fn scalar_fields_and_bad_check_values_are_refused() {
        let t = header(" OVF 2.0", "# valuedim: 1\n", "Text") + "1 2 3 4\n";
        assert!(parse_bytes(t.as_bytes()).is_err());
        let mut b = header(" OVF 2.0", "# valuedim: 3\n", "Binary 4").into_bytes();
        b.extend([0u8; 52]);
        assert!(parse_bytes(&b).is_err());
    }

    #[test]
    fn large_meshes_are_thinned() {
        let n = 300usize; // 90 000 cells
        let mut t = format!(
            "# OOMMF OVF 2.0\n# Begin: Header\n# meshunit: m\n# valuedim: 3\n# xbase: 0\n# ybase: 0\n# zbase: 0\n\
             # xnodes: {n}\n# ynodes: {n}\n# znodes: 1\n# xstepsize: 1e-9\n# ystepsize: 1e-9\n# zstepsize: 1e-9\n\
             # End: Header\n# Begin: Data Text\n"
        );
        for _ in 0..n * n {
            t.push_str("0 0 1\n");
        }
        let s = parse_bytes(t.as_bytes()).unwrap();
        assert!(s.atoms.len() <= MAX_SITES, "{}", s.atoms.len());
        assert!(s.atoms.len() > MAX_SITES / 4);
    }
}
