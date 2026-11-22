// src/io/ovf.rs
//
// OOMMF Vector Field files (.ovf, .omf, .ohf): the magnetization written by
// mumax3, OOMMF and Spirit. One site per mesh cell, placed at the cell
// centre, carrying the field vector there as its moment.
//
// Supported: OVF 1.0 and 2.0; rectangular meshes (positions from
// xbase/xstepsize/xnodes) and irregular ones (positions in the data);
// data as text or binary 4/8. Binary byte order comes from the check value
// (OVF 2.0 is little-endian, OVF 1.0 big-endian). Only vector fields
// (valuedim 3).
//
// Snapshots: mumax3 and OOMMF write one file per snapshot (m000000.ovf,
// m000001.ovf, ...); Spirit can put many in one file as segments. Opening
// any file of a numbered series, or a file with several segments, gives a
// trajectory of all of them (`load_document`), which the Trajectory Player
// plays; the main view shows the snapshot that was opened.
//
// Cells whose vector is zero (outside the magnet in a mumax3 geometry) are
// left out, so a disc reads as a disc. A field in A/m is divided by its
// largest magnitude, so moments are m = M/Ms_max (unitless) either way.
// Meshes beyond MAX_SITES cells are thinned in-plane. In a series, the
// cells kept and the scale come from the opened snapshot and apply to all.

use crate::io::Document;
use crate::model::trajectory::{Frame, FrameChoice, Trajectory};
use crate::model::{Atom, Structure};
use crate::utils::console::{log_info, log_warn};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Above this many cells the mesh is thinned (every k-th cell in x and y).
pub const MAX_SITES: usize = 40_000;

/// A series keeps at most about this many cell moments in memory in all
/// (24 bytes each): long series are thinned further per snapshot.
pub const SERIES_BUDGET: usize = 6_000_000;

/// Fewest cells per snapshot a long series is thinned to.
const SERIES_MIN_SITES: usize = 2_000;

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

#[derive(Default, Debug, Clone)]
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
    /// "Desc: Total simulation time: ... s" (mumax3, OOMMF).
    time: Option<f64>,
    /// "Desc: Iteration: ..." (OOMMF).
    iteration: Option<i64>,
}

impl Header {
    fn new(version: u8) -> Self {
        Header { version, unit_to_ang: 1e10, valuedim: 3, multiplier: 1.0, ..Default::default() }
    }

    fn n_nodes(&self) -> usize {
        if self.irregular {
            self.point_count
        } else {
            self.nodes.iter().product()
        }
    }

    /// Floats per node in the data: position + vector for irregular meshes.
    fn per_node(&self) -> usize {
        if self.irregular {
            6
        } else {
            3
        }
    }
}

/// One snapshot: its header and the raw data values.
struct Segment {
    h: Header,
    values: Vec<f64>,
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

/// The first number in `s`.
fn first_number<T: std::str::FromStr>(s: &str) -> Option<T> {
    s.split_whitespace().find_map(|t| t.parse().ok())
}

fn read_header_line(h: &mut Header, body: &str) {
    let Some((key, val)) = body.split_once(':') else { return };
    let (key, val) = (key.trim().to_ascii_lowercase(), val.trim());
    let num = || first_number::<f64>(val).unwrap_or(0.0);
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
        "desc" => {
            if let Some((k, v)) = val.split_once(':') {
                let k = k.trim().to_ascii_lowercase();
                if k.contains("simulation time") {
                    h.time = first_number(v);
                } else if k == "iteration" {
                    h.iteration = first_number(v);
                }
            }
        }
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
}

/// Every segment of an OVF file, in order.
fn parse_segments(bytes: &[u8]) -> io::Result<Vec<Segment>> {
    if !sniff(bytes) {
        return Err(bad("not an OVF file (no '# OOMMF' first line)"));
    }
    let first = bytes.iter().position(|&b| b == b'\n').unwrap_or(bytes.len());
    let version = if String::from_utf8_lossy(&bytes[..first]).contains("2.0") { 2 } else { 1 };

    let mut segments = Vec::new();
    let mut pos = first.min(bytes.len());
    loop {
        // Header: '#' lines up to "# Begin: Data ...".
        let mut h = Header::new(version);
        let mut data_kind = None;
        while pos < bytes.len() {
            let end = bytes[pos..].iter().position(|&b| b == b'\n').map_or(bytes.len(), |e| pos + e);
            let line = String::from_utf8_lossy(&bytes[pos..end]).trim().to_string();
            pos = (end + 1).min(bytes.len());
            let Some(body) = line.strip_prefix('#') else { continue };
            let body = body.trim();
            let lower = body.to_ascii_lowercase();
            if let Some(kind) = lower.strip_prefix("begin: data") {
                data_kind = Some(kind.trim().to_string());
                break;
            }
            read_header_line(&mut h, body);
        }
        let Some(data_kind) = data_kind else {
            if segments.is_empty() {
                return Err(bad("OVF file ends before its data"));
            }
            return Ok(segments);
        };

        if h.version == 1 {
            h.valuedim = 3; // OVF 1.0 is always a 3-vector field
        }
        if h.valuedim != 3 {
            return Err(bad(format!("OVF field has {} component(s); only vector fields (3) are shown", h.valuedim)));
        }
        let n_nodes = h.n_nodes();
        if n_nodes == 0 {
            return Err(bad("OVF header gives no mesh size (xnodes/ynodes/znodes or pointcount)"));
        }
        let count = n_nodes * h.per_node();

        let values: Vec<f64> = if let Some(width) = data_kind.strip_prefix("binary") {
            let width: usize = width.trim().parse().map_err(|_| bad("OVF binary data width"))?;
            let v = read_binary(&bytes[pos..], width, count)?;
            pos += width * (count + 1);
            v
        } else if data_kind.starts_with("text") {
            let mut v = Vec::with_capacity(count);
            while pos < bytes.len() {
                let end = bytes[pos..].iter().position(|&b| b == b'\n').map_or(bytes.len(), |e| pos + e);
                let line = String::from_utf8_lossy(&bytes[pos..end]);
                if line.trim_start().starts_with('#') {
                    break; // "# End: Data Text": the header scan takes it from here
                }
                pos = (end + 1).min(bytes.len());
                for t in line.split_whitespace() {
                    if v.len() == count {
                        break;
                    }
                    if let Ok(x) = t.parse() {
                        v.push(x);
                    }
                }
            }
            if v.len() < count {
                return Err(bad(format!("OVF text data has {} of {} values", v.len(), count)));
            }
            v
        } else {
            return Err(bad(format!("OVF data format '{}' is not supported", data_kind)));
        };
        segments.push(Segment { h, values });
    }
}

/// The cells shown and how vectors are scaled, fixed from one snapshot and
/// applied to every snapshot of a series.
struct Mesh {
    irregular: bool,
    nodes: [usize; 3],
    n_nodes: usize,
    /// Data index of each kept cell.
    cells: Vec<usize>,
    /// Cartesian position of each kept cell, Å.
    positions: Vec<[f64; 3]>,
    lattice: [[f64; 3]; 3],
    /// Applied to every raw vector.
    factor: f64,
    stride: usize,
    /// Cells before thinning-out empty ones / after thinning in-plane.
    total: usize,
    empty: usize,
    /// Set when vectors were divided by the largest |M|.
    largest: Option<f64>,
    value_unit: String,
    version: u8,
    title: String,
}

impl Mesh {
    fn new(seg: &Segment, max_sites: usize) -> io::Result<Mesh> {
        let h = &seg.h;
        let values = &seg.values;
        let s = h.unit_to_ang;
        // Sites: (data index, position in mesh units, vector).
        let mut sites: Vec<(usize, [f64; 3], [f64; 3])> = Vec::new();
        let mut stride = 1usize;
        if h.irregular {
            for (i, c) in values.chunks_exact(6).enumerate() {
                sites.push((i, [c[0], c[1], c[2]], [c[3], c[4], c[5]]));
            }
        } else {
            let [nx, ny, nz] = h.nodes;
            // Thin in-plane (x and y) until the mesh fits.
            while (nx.div_ceil(stride)) * (ny.div_ceil(stride)) * nz > max_sites && stride < nx.max(ny) {
                stride += 1;
            }
            for k in 0..nz {
                for j in (0..ny).step_by(stride) {
                    for i in (0..nx).step_by(stride) {
                        let n = i + nx * (j + ny * k);
                        let p = [
                            h.base[0] + i as f64 * h.step[0],
                            h.base[1] + j as f64 * h.step[1],
                            h.base[2] + k as f64 * h.step[2],
                        ];
                        sites.push((n, p, [values[3 * n], values[3 * n + 1], values[3 * n + 2]]));
                    }
                }
            }
        }

        let norm = |v: [f64; 3]| (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
        let total = sites.len();
        sites.retain(|(_, _, v)| norm(*v) > 0.0);
        let empty = total - sites.len();
        if sites.is_empty() {
            return Err(bad("every cell of the OVF field is zero"));
        }

        // m stays as it is; a magnetization (A/m) is divided by its largest
        // magnitude.
        let largest = sites.iter().map(|(_, _, v)| norm(*v) * h.multiplier.abs()).fold(0.0, f64::max);
        let normalise = largest > 10.0 || h.value_unit.eq_ignore_ascii_case("a/m");
        let factor = if normalise { h.multiplier / largest } else { h.multiplier };

        // The box: the mesh extent (xmin..xmax) when given, else the node span.
        let mut lo = h.min;
        let mut hi = h.max;
        for a in 0..3 {
            if hi[a] <= lo[a] {
                let (mn, mx) = sites.iter().map(|(_, p, _)| p[a]).fold((f64::MAX, f64::MIN), |(l, u), x| (l.min(x), u.max(x)));
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

        Ok(Mesh {
            irregular: h.irregular,
            nodes: h.nodes,
            n_nodes: h.n_nodes(),
            cells: sites.iter().map(|(n, _, _)| *n).collect(),
            positions: sites.iter().map(|(_, p, _)| [(p[0] - lo[0]) * s, (p[1] - lo[1]) * s, (p[2] - lo[2]) * s]).collect(),
            lattice,
            factor,
            stride,
            total,
            empty,
            largest: normalise.then_some(largest),
            value_unit: h.value_unit.clone(),
            version: h.version,
            title: h.title.clone(),
        })
    }

    /// Whether `seg` is a snapshot on this mesh.
    fn fits(&self, seg: &Segment) -> bool {
        seg.h.irregular == self.irregular && seg.h.n_nodes() == self.n_nodes && (self.irregular || seg.h.nodes == self.nodes)
    }

    /// The scaled vector at every kept cell of `seg` (which must `fit`).
    fn vectors(&self, seg: &Segment) -> Vec<[f64; 3]> {
        let (per, off) = if self.irregular { (6, 3) } else { (3, 0) };
        let f = self.factor;
        self.cells
            .iter()
            .map(|&n| {
                let o = per * n + off;
                [seg.values[o] * f, seg.values[o + 1] * f, seg.values[o + 2] * f]
            })
            .collect()
    }

    fn describe(&self) -> String {
        format!(
            "OVF {}: {} cell(s){}{}{}",
            if self.version == 2 { "2.0" } else { "1.0" },
            self.cells.len(),
            if self.empty > 0 { format!(", {} empty cell(s) left out", self.empty) } else { String::new() },
            if self.stride > 1 {
                format!(
                    ", every {}th cell in x and y shown ({} of {}×{})",
                    self.stride, self.total, self.nodes[0], self.nodes[1]
                )
            } else {
                String::new()
            },
            match self.largest {
                Some(l) => format!(", divided by the largest |M| = {:.4e} {}", l, self.value_unit),
                None => String::new(),
            },
        )
    }

    fn structure(&self, seg: &Segment) -> Structure {
        let atoms = self
            .positions
            .iter()
            .zip(self.vectors(seg))
            .enumerate()
            .map(|(i, (p, v))| Atom {
                element: SITE_NAME.to_string(),
                position: *p,
                original_index: i,
                oxidation: None,
                occupancy: 1.0,
                magmom: Some(v),
            })
            .collect();
        Structure {
            lattice: self.lattice,
            atoms,
            formula: if self.title.is_empty() { "OVF".to_string() } else { format!("OVF {}", self.title) },
            is_periodic: true,
        }
    }
}

pub fn parse(path: &str) -> io::Result<Structure> {
    let bytes = std::fs::read(path)?;
    parse_bytes(&bytes)
}

/// The first snapshot in `bytes`.
pub fn parse_bytes(bytes: &[u8]) -> io::Result<Structure> {
    let segs = parse_segments(bytes)?;
    let mesh = Mesh::new(&segs[0], MAX_SITES)?;
    log_info(&mesh.describe());
    Ok(mesh.structure(&segs[0]))
}

/// The files of the numbered series `path` belongs to (same directory,
/// same name up to a trailing number, same extension), in numeric order,
/// with their numbers. Just `path` when its name ends in no number.
fn series_members(path: &Path) -> Vec<(PathBuf, Option<u64>)> {
    let alone = vec![(path.to_path_buf(), None)];
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else { return alone };
    let Some((stem, ext)) = name.rsplit_once('.') else { return alone };
    let prefix = stem.trim_end_matches(|c: char| c.is_ascii_digit());
    if prefix.len() == stem.len() {
        return alone;
    }
    let dir = match path.parent() {
        Some(d) if !d.as_os_str().is_empty() => d.to_path_buf(),
        _ => PathBuf::from("."),
    };
    let Ok(entries) = std::fs::read_dir(&dir) else { return alone };
    let mut found: Vec<(u64, String, PathBuf)> = entries
        .flatten()
        .filter_map(|e| {
            let n = e.file_name().into_string().ok()?;
            let (s, x) = n.rsplit_once('.')?;
            let digits = s.strip_prefix(prefix)?;
            if !x.eq_ignore_ascii_case(ext) || digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            let num = digits.parse().ok()?;
            Some((num, n.clone(), dir.join(&n)))
        })
        .collect();
    found.sort();
    if !found.iter().any(|(_, n, _)| n == name) {
        return alone;
    }
    found.into_iter().map(|(num, _, p)| (p, Some(num))).collect()
}

/// Open `path` for display. A file of a numbered series, or with several
/// segments, comes with a trajectory of every snapshot; the opened file's
/// snapshot is the one shown (for a lone multi-segment file, `choice`).
pub fn load_document(path: &str, choice: FrameChoice) -> io::Result<Document> {
    let path = Path::new(path);
    let members = series_members(path);
    let segs = parse_segments(&std::fs::read(path)?)?;
    if members.len() == 1 && segs.len() == 1 {
        let mesh = Mesh::new(&segs[0], MAX_SITES)?;
        log_info(&mesh.describe());
        return Ok(Document { structure: mesh.structure(&segs[0]), trajectory: None, frame: 0 });
    }

    let opened = members.iter().position(|(p, _)| p.file_name() == path.file_name()).unwrap_or(0);
    let estimate = members.len() * segs.len();
    let max_sites = (SERIES_BUDGET / estimate.max(1)).clamp(SERIES_MIN_SITES, MAX_SITES);
    let mesh = Mesh::new(&segs[0], max_sites)?;
    let positions = Arc::new(mesh.positions.clone());
    let mut traj = Trajectory {
        species: vec![SITE_NAME.to_string(); mesh.cells.len()],
        frames: Vec::with_capacity(estimate),
        is_periodic: true,
        format: "OVF",
    };

    let mut opened_frame = 0;
    let mut skipped: Vec<String> = Vec::new();
    let add = |traj: &mut Trajectory, segs: &[Segment], number: Option<u64>| -> bool {
        let mut any = false;
        for (k, seg) in segs.iter().enumerate() {
            if !mesh.fits(seg) {
                continue;
            }
            any = true;
            traj.frames.push(Frame {
                lattice: mesh.lattice,
                positions: positions.clone(),
                energy: None,
                max_force: None,
                step: seg.h.iteration.or(number.map(|n| n as i64)).or(Some(k as i64)),
                time: seg.h.time,
                magmoms: Some(mesh.vectors(seg)),
            });
        }
        any
    };
    for (i, (p, number)) in members.iter().enumerate() {
        let name = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        if i == opened {
            opened_frame = traj.frames.len();
            add(&mut traj, &segs, *number);
            continue;
        }
        match std::fs::read(p).map_err(|e| e.to_string()).and_then(|b| parse_segments(&b).map_err(|e| e.to_string())) {
            Ok(other) => {
                if !add(&mut traj, &other, *number) {
                    skipped.push(format!("{name} (different mesh)"));
                }
            }
            Err(e) => skipped.push(format!("{name} ({e})")),
        }
    }

    log_info(&mesh.describe());
    if members.len() > 1 {
        let first = members[0].0.file_name().unwrap_or_default().to_string_lossy();
        let last = members[members.len() - 1].0.file_name().unwrap_or_default().to_string_lossy();
        log_info(&format!(
            "OVF series: {} snapshot(s) from {} file(s), {} … {}",
            traj.len(),
            members.len() - skipped.len(),
            first,
            last
        ));
    }
    if !skipped.is_empty() {
        let shown: Vec<&str> = skipped.iter().take(5).map(String::as_str).collect();
        log_warn(&format!(
            "OVF series: {} file(s) left out: {}{}",
            skipped.len(),
            shown.join(", "),
            if skipped.len() > 5 { ", …" } else { "" }
        ));
    }

    let frame = if members.len() == 1 { traj.index_of(choice) } else { opened_frame };
    let mut structure = traj.structure_at(frame).ok_or_else(|| bad("OVF series has no snapshots"))?;
    if !mesh.title.is_empty() {
        structure.formula = format!("OVF {}", mesh.title);
    }
    let trajectory = (traj.len() > 1).then_some(traj);
    Ok(Document { structure, trajectory, frame })
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

    /// A 2×2×1 OVF 2.0 text snapshot with `m` in every cell, at time `t`.
    fn snapshot(m: [f64; 3], t: f64) -> String {
        let mut s = header(" OVF 2.0", &format!("# Desc: Total simulation time:  {t:e}  s\n# valuedim: 3\n"), "Text");
        for _ in 0..4 {
            s.push_str(&format!("{} {} {}\n", m[0], m[1], m[2]));
        }
        s.push_str("# End: Data Text\n# End: Segment\n");
        s
    }

    #[test]
    fn every_segment_of_a_spirit_style_file_is_read() {
        let mut t = snapshot([0.0, 0.0, 1.0], 0.0);
        // Spirit appends segments after the first.
        t.push_str(&snapshot([1.0, 0.0, 0.0], 1e-12)[t.find('\n').unwrap() + 1..]);
        let segs = parse_segments(t.as_bytes()).unwrap();
        assert_eq!(segs.len(), 2);
        assert_eq!(segs[1].h.time, Some(1e-12));
        let mesh = Mesh::new(&segs[0], MAX_SITES).unwrap();
        close(mesh.vectors(&segs[1])[3], [1.0, 0.0, 0.0]);
        // Binary segments one after another too.
        let mut b = Vec::new();
        for z in [1.0f32, -1.0] {
            b.extend(header(" OVF 2.0", "# valuedim: 3\n", "Binary 4").into_bytes());
            b.extend(1_234_567.0f32.to_le_bytes());
            for _ in 0..4 {
                for v in [0.0f32, 0.0, z] {
                    b.extend(v.to_le_bytes());
                }
            }
            b.extend(b"\n# End: Data Binary 4\n# End: Segment\n");
        }
        let segs = parse_segments(&b).unwrap();
        assert_eq!(segs.len(), 2);
        close(Mesh::new(&segs[0], MAX_SITES).unwrap().vectors(&segs[1])[0], [0.0, 0.0, -1.0]);
    }

    #[test]
    fn a_numbered_series_loads_as_a_trajectory_showing_the_opened_file() {
        let dir = std::env::temp_dir().join(format!("cview_ovf_series_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for i in 0..3 {
            let m = [0.0, (i as f64) * 0.5, 1.0];
            std::fs::write(dir.join(format!("m{:06}.ovf", i)), snapshot(m, i as f64 * 1e-10)).unwrap();
        }
        // Not part of the series: another quantity, and an unnumbered file.
        std::fs::write(dir.join("B_ext000000.ovf"), snapshot([1.0, 0.0, 0.0], 0.0)).unwrap();
        std::fs::write(dir.join("m.ovf"), snapshot([1.0, 0.0, 0.0], 0.0)).unwrap();

        let opened = dir.join("m000001.ovf");
        let names: Vec<_> = series_members(&opened).iter().map(|(p, n)| (p.file_name().unwrap().to_owned(), *n)).collect();
        assert_eq!(names.len(), 3);
        assert_eq!(names[2], ("m000002.ovf".into(), Some(2)));

        let doc = load_document(opened.to_str().unwrap(), FrameChoice::First).unwrap();
        let t = doc.trajectory.expect("three snapshots");
        assert_eq!(t.len(), 3);
        assert_eq!(doc.frame, 1);
        assert_eq!(t.frames[2].step, Some(2));
        assert_eq!(t.frames[2].time, Some(2e-10));
        assert!(Arc::ptr_eq(&t.frames[0].positions, &t.frames[2].positions));
        close(doc.structure.atoms[0].magmom.unwrap(), [0.0, 0.5, 1.0]);

        // An unnumbered file stays a single structure.
        let doc = load_document(dir.join("m.ovf").to_str().unwrap(), FrameChoice::First).unwrap();
        assert!(doc.trajectory.is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
