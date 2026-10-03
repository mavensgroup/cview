// src/physics/analysis/isosurface.rs
//
// Isosurfaces of periodic volumetric data (CHGCAR and friends).
//
// Method: marching tetrahedra on the Freudenthal subdivision. Each grid cube
// is split into six tetrahedra that all share the cube's main diagonal; the
// split is identical in every cube, so neighbouring cubes cut their shared
// faces the same way. Unlike classic marching cubes there are no ambiguous
// cases, so the surface is watertight by construction — no pinholes where two
// cubes resolve a saddle differently. The surface is exact for the linear
// interpolant inside each tetrahedron.
//
// - Vertices are shared: each one is keyed by the grid edge it lies on, so
//   the mesh is indexed, compact and smooth-shaded.
// - Normals come from the density gradient (central differences on the
//   grid, interpolated along the edge), not from triangle facets, so the
//   surface looks smooth even on coarse grids.
// - Periodic: grid indices wrap, so a region may span several cells
//   (`Region`), and a lobe crossing a cell face continues seamlessly.
// - Parallel over z-slabs with rayon; slabs are stitched by vertex key.
//
// Grid convention (VASP): value (i, j, k) sits at fractional coordinates
// (i/nx, j/ny, k/nz) and is stored x-fastest: data[i + nx*(j + ny*k)].

use crate::utils::task::CancelToken;
use nalgebra::{Matrix3, Vector3};
use rayon::prelude::*;
use std::collections::HashMap;
use std::hash::{BuildHasherDefault, Hasher};

/// Periodic scalar field on a regular grid spanning one cell.
pub struct Grid<'a> {
    pub data: &'a [f32],
    pub dims: [usize; 3],
    /// Lattice vectors as rows, Å.
    pub lattice: [[f64; 3]; 3],
}

/// Which grid cubes to mesh, in grid-step units; may extend past one cell
/// (`0..2*nx` shows two cells along a). Upper bounds are exclusive.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Region {
    pub lo: [i64; 3],
    pub hi: [i64; 3],
}

impl Region {
    /// Exactly one cell.
    pub fn cell(dims: [usize; 3]) -> Self {
        Self {
            lo: [0; 3],
            hi: dims.map(|n| n as i64),
        }
    }
}

/// Which side of the isovalue is "inside" (the side normals point away from).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Inside {
    /// ρ > iso — positive lobes, total density.
    Above,
    /// ρ < iso — negative lobes of a difference or spin density (iso < 0).
    Below,
}

/// Indexed triangle mesh. Triangles wind counter-clockwise seen from
/// outside; normals point outward.
#[derive(Clone, Debug, Default)]
pub struct Mesh {
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub indices: Vec<u32>,
}

impl Mesh {
    pub fn triangle_count(&self) -> usize {
        self.indices.len() / 3
    }

    pub fn is_empty(&self) -> bool {
        self.indices.is_empty()
    }

    /// Enclosed volume (Å³) by the divergence theorem. Only meaningful for a
    /// closed surface, i.e. one that does not touch the region's faces.
    pub fn enclosed_volume(&self) -> f64 {
        self.indices
            .chunks_exact(3)
            .map(|t| {
                let p = |k: usize| {
                    let v = self.positions[t[k] as usize];
                    Vector3::new(v[0] as f64, v[1] as f64, v[2] as f64)
                };
                p(0).dot(&p(1).cross(&p(2))) / 6.0
            })
            .sum()
    }

    /// Surface area, Å².
    pub fn area(&self) -> f64 {
        self.indices
            .chunks_exact(3)
            .map(|t| {
                let p = |k: usize| {
                    let v = self.positions[t[k] as usize];
                    Vector3::new(v[0] as f64, v[1] as f64, v[2] as f64)
                };
                0.5 * (p(1) - p(0)).cross(&(p(2) - p(0))).norm()
            })
            .sum()
    }
}

// ---------------------------------------------------------------------------
// Small fast hasher for integer keys (FxHash). std's SipHash dominates the
// run time otherwise; no new dependency for twenty lines.
// ---------------------------------------------------------------------------

#[derive(Default)]
struct FxHasher(u64);

impl Hasher for FxHasher {
    fn write(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.write_u64(b as u64);
        }
    }
    fn write_u64(&mut self, n: u64) {
        self.0 = (self.0.rotate_left(5) ^ n).wrapping_mul(0x51_7c_c1_b7_27_22_0a_95);
    }
    fn write_i64(&mut self, n: i64) {
        self.write_u64(n as u64);
    }
    fn write_u8(&mut self, n: u8) {
        self.write_u64(n as u64);
    }
    fn finish(&self) -> u64 {
        self.0
    }
}

type FxMap<K, V> = HashMap<K, V, BuildHasherDefault<FxHasher>>;

/// A vertex lies on the grid edge from `p` to `p + dir`, where `dir` is a
/// 0/1 step mask (bit 0 = x, 1 = y, 2 = z), or exactly on grid point `p`
/// when `dir` is 0. Every edge of the Freudenthal
/// tetrahedra has this form, so the key is unique and shared by all
/// tetrahedra (and cubes, and slabs) that touch the edge.
type EdgeKey = ([i64; 3], u8);

/// The six tetrahedra of a cube, as corner masks (bit 0 = +x, 1 = +y,
/// 2 = +z). Each walks from corner 0 to corner 7 along one axis order, so
/// all share the diagonal 0–7 and every cube splits its faces identically.
const TETS: [[u8; 4]; 6] = [
    [0, 1, 3, 7],
    [0, 1, 5, 7],
    [0, 2, 3, 7],
    [0, 2, 6, 7],
    [0, 4, 5, 7],
    [0, 4, 6, 7],
];

fn bits(mask: u8) -> [i64; 3] {
    [(mask & 1) as i64, ((mask >> 1) & 1) as i64, ((mask >> 2) & 1) as i64]
}

struct Field<'a> {
    grid: &'a Grid<'a>,
    n: [i64; 3],
    /// Columns are lattice vectors: cart = m * frac.
    m: Matrix3<f64>,
    /// Gradient in fractional coordinates to Cartesian: m^{-T}.
    grad_to_cart: Matrix3<f64>,
}

impl Field<'_> {
    #[inline]
    fn value(&self, p: [i64; 3]) -> f32 {
        let i = p[0].rem_euclid(self.n[0]) as usize;
        let j = p[1].rem_euclid(self.n[1]) as usize;
        let k = p[2].rem_euclid(self.n[2]) as usize;
        self.grid.data[i + self.grid.dims[0] * (j + self.grid.dims[1] * k)]
    }

    /// d(rho)/d(frac) at a grid point, by central differences.
    fn grad_frac(&self, p: [i64; 3]) -> Vector3<f64> {
        let mut g = Vector3::zeros();
        for a in 0..3 {
            let mut up = p;
            let mut dn = p;
            up[a] += 1;
            dn[a] -= 1;
            // d/du (index units) times n = d/dfrac.
            g[a] = (self.value(up) - self.value(dn)) as f64 * 0.5 * self.n[a] as f64;
        }
        g
    }

    fn cart(&self, u: [f64; 3]) -> Vector3<f64> {
        self.m * Vector3::new(u[0] / self.n[0] as f64, u[1] / self.n[1] as f64, u[2] / self.n[2] as f64)
    }
}

struct Chunk {
    /// The z-planes this slab shares with its neighbours.
    z0: i64,
    z1: i64,
    keys: Vec<EdgeKey>,
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    /// Indices into this chunk's vertices.
    triangles: Vec<[u32; 3]>,
}

/// Mesh the cubes with lower corner z in `z0..z1` (and x, y over `region`).
fn mesh_slab(f: &Field, region: &Region, z0: i64, z1: i64, iso: f32, inside: Inside, cancel: &CancelToken) -> Option<Chunk> {
    let mut map: FxMap<EdgeKey, u32> = FxMap::default();
    let mut chunk = Chunk {
        z0,
        z1,
        keys: Vec::new(),
        positions: Vec::new(),
        normals: Vec::new(),
        triangles: Vec::new(),
    };
    let is_in = |v: f32| match inside {
        Inside::Above => v > iso,
        Inside::Below => v < iso,
    };
    // Normals point from inside to outside: down the gradient for Above.
    let outward = match inside {
        Inside::Above => -1.0,
        Inside::Below => 1.0,
    };

    let mut vertex = |chunk: &mut Chunk, p: [i64; 3], a: u8, b: u8, va: f32, vb: f32| -> u32 {
        // Order the edge from the lower corner to the higher one.
        let (lo, hi, vlo, vhi) = if a & !b == 0 { (a, b, va, vb) } else { (b, a, vb, va) };
        let base = {
            let o = bits(lo);
            [p[0] + o[0], p[1] + o[1], p[2] + o[2]]
        };
        let mut dir = hi & !lo;
        let mut t = ((iso - vlo) / (vhi - vlo)).clamp(0.0, 1.0) as f64;
        let mut base = base;
        // A crossing on a grid point (rho == iso there) is keyed by the point
        // itself (dir 0), so every edge meeting it shares one vertex instead
        // of stacking coincident copies; the triangles this collapses are
        // dropped when the slabs are stitched.
        if t <= 1e-6 {
            dir = 0;
            t = 0.0;
        } else if t >= 1.0 - 1e-6 {
            let d = bits(dir);
            base = [base[0] + d[0], base[1] + d[1], base[2] + d[2]];
            dir = 0;
            t = 0.0;
        }
        let key = (base, dir);
        if let Some(&id) = map.get(&key) {
            return id;
        }
        let d = bits(dir);
        let u = [0, 1, 2].map(|k| base[k] as f64 + t * d[k] as f64);
        let pos = f.cart(u);
        let end = [base[0] + d[0], base[1] + d[1], base[2] + d[2]];
        let g = f.grad_frac(base) * (1.0 - t) + f.grad_frac(end) * t;
        let n = (f.grad_to_cart * g) * outward;
        let n = n.try_normalize(1e-30).unwrap_or_else(Vector3::zeros);
        let id = chunk.positions.len() as u32;
        chunk.keys.push(key);
        chunk.positions.push([pos.x as f32, pos.y as f32, pos.z as f32]);
        chunk.normals.push([n.x as f32, n.y as f32, n.z as f32]);
        map.insert(key, id);
        id
    };

    for z in z0..z1 {
        if cancel.is_cancelled() {
            return None;
        }
        for y in region.lo[1]..region.hi[1] {
            for x in region.lo[0]..region.hi[0] {
                let p = [x, y, z];
                let mut v = [0f32; 8];
                let mut any_in = false;
                let mut any_out = false;
                for (m, slot) in v.iter_mut().enumerate() {
                    let o = bits(m as u8);
                    *slot = f.value([x + o[0], y + o[1], z + o[2]]);
                    if is_in(*slot) {
                        any_in = true;
                    } else {
                        any_out = true;
                    }
                }
                if !(any_in && any_out) {
                    continue;
                }
                for tet in &TETS {
                    let ins: Vec<u8> = tet.iter().copied().filter(|&c| is_in(v[c as usize])).collect();
                    let outs: Vec<u8> = tet.iter().copied().filter(|&c| !is_in(v[c as usize])).collect();
                    let mut e = |a: u8, b: u8| vertex(&mut chunk, p, a, b, v[a as usize], v[b as usize]);
                    match (ins.len(), outs.len()) {
                        (1, 3) => {
                            let t = [e(ins[0], outs[0]), e(ins[0], outs[1]), e(ins[0], outs[2])];
                            chunk.triangles.push(t);
                        }
                        (3, 1) => {
                            let t = [e(outs[0], ins[0]), e(outs[0], ins[1]), e(outs[0], ins[2])];
                            chunk.triangles.push(t);
                        }
                        (2, 2) => {
                            // Quad around the tetrahedron: a-c, a-d, b-d, b-c.
                            let (a, b, c, d) = (ins[0], ins[1], outs[0], outs[1]);
                            let q = [e(a, c), e(a, d), e(b, d), e(b, c)];
                            chunk.triangles.push([q[0], q[1], q[2]]);
                            chunk.triangles.push([q[0], q[2], q[3]]);
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    // Wind every triangle counter-clockwise seen from outside, using the
    // outward vertex normals as the reference.
    for t in chunk.triangles.iter_mut() {
        let p = |k: usize| {
            let v = chunk.positions[t[k] as usize];
            Vector3::new(v[0] as f64, v[1] as f64, v[2] as f64)
        };
        let face = (p(1) - p(0)).cross(&(p(2) - p(0)));
        let nsum: Vector3<f64> = (0..3)
            .map(|k| {
                let n = chunk.normals[t[k] as usize];
                Vector3::new(n[0] as f64, n[1] as f64, n[2] as f64)
            })
            .sum();
        if face.dot(&nsum) < 0.0 {
            t.swap(1, 2);
        }
    }
    Some(chunk)
}

/// Extract the isosurface `rho = iso` over `region`. Returns `None` if
/// `cancel` fires (checked once per grid layer).
pub fn isosurface(grid: &Grid, iso: f32, inside: Inside, region: Region, cancel: &CancelToken) -> Option<Mesh> {
    let n = grid.dims.map(|d| d as i64);
    assert_eq!(grid.data.len(), grid.dims.iter().product::<usize>(), "grid data does not match its dimensions");
    let l = grid.lattice;
    let m = Matrix3::new(l[0][0], l[1][0], l[2][0], l[0][1], l[1][1], l[2][1], l[0][2], l[1][2], l[2][2]);
    let grad_to_cart = m.try_inverse()?.transpose();
    let f = Field { grid, n, m, grad_to_cart };

    // z-slabs: a few per thread, so uneven surfaces still balance.
    let (z_lo, z_hi) = (region.lo[2], region.hi[2]);
    let span = (z_hi - z_lo).max(0);
    let pieces = (rayon::current_num_threads() as i64 * 4).clamp(1, span.max(1));
    let step = (span + pieces - 1) / pieces.max(1);
    let slabs: Vec<(i64, i64)> = (0..pieces)
        .map(|s| (z_lo + s * step, (z_lo + (s + 1) * step).min(z_hi)))
        .filter(|(a, b)| a < b)
        .collect();

    let chunks: Vec<Chunk> = slabs
        .par_iter()
        .map(|&(a, b)| mesh_slab(&f, &region, a, b, iso, inside, cancel))
        .collect::<Option<Vec<_>>>()?;

    // Stitch. A vertex can only have been made by two slabs if it lies on
    // a plane they share (z = z0 or z1, on an edge with no z step); only
    // those go through the map, everything else is copied straight across.
    let shared = |c: &Chunk, key: &EdgeKey| key.1 & 4 == 0 && (key.0[2] == c.z0 || key.0[2] == c.z1);
    let n_verts: usize = chunks.iter().map(|c| c.positions.len()).sum();
    let n_tris: usize = chunks.iter().map(|c| c.triangles.len()).sum();
    let mut global: FxMap<EdgeKey, u32> = FxMap::default();
    let mut mesh = Mesh {
        positions: Vec::with_capacity(n_verts),
        normals: Vec::with_capacity(n_verts),
        indices: Vec::with_capacity(n_tris * 3),
    };
    for c in &chunks {
        let remap: Vec<u32> = c
            .keys
            .iter()
            .enumerate()
            .map(|(i, key)| {
                let mut add = || {
                    mesh.positions.push(c.positions[i]);
                    mesh.normals.push(c.normals[i]);
                    (mesh.positions.len() - 1) as u32
                };
                if shared(c, key) {
                    *global.entry(*key).or_insert_with(add)
                } else {
                    add()
                }
            })
            .collect();
        for t in &c.triangles {
            let t = t.map(|i| remap[i as usize]);
            // Drop triangles collapsed by vertices landing on grid points.
            if t[0] != t[1] && t[1] != t[2] && t[0] != t[2] {
                mesh.indices.extend_from_slice(&t);
            }
        }
    }

    // A vertex where the gradient vanished gets the mean of its faces.
    if mesh.normals.iter().any(|n| n == &[0.0; 3]) {
        let mut acc = vec![Vector3::<f64>::zeros(); mesh.positions.len()];
        for t in mesh.indices.chunks_exact(3) {
            let p = |k: usize| {
                let v = mesh.positions[t[k] as usize];
                Vector3::new(v[0] as f64, v[1] as f64, v[2] as f64)
            };
            let face = (p(1) - p(0)).cross(&(p(2) - p(0)));
            for &i in t {
                acc[i as usize] += face;
            }
        }
        for (n, a) in mesh.normals.iter_mut().zip(acc) {
            if n == &[0.0; 3] {
                if let Some(a) = a.try_normalize(1e-30) {
                    *n = [a.x as f32, a.y as f32, a.z as f32];
                }
            }
        }
    }
    Some(mesh)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap as StdMap;

    /// Sample `f(cart)` on an n³ grid in `lattice`.
    fn sample(n: usize, lattice: [[f64; 3]; 3], f: impl Fn(Vector3<f64>) -> f64) -> Vec<f32> {
        let l = lattice;
        let m = Matrix3::new(l[0][0], l[1][0], l[2][0], l[0][1], l[1][1], l[2][1], l[0][2], l[1][2], l[2][2]);
        let mut out = vec![0f32; n * n * n];
        for k in 0..n {
            for j in 0..n {
                for i in 0..n {
                    let c = m * Vector3::new(i as f64 / n as f64, j as f64 / n as f64, k as f64 / n as f64);
                    out[i + n * (j + n * k)] = f(c) as f32;
                }
            }
        }
        out
    }

    fn cube(a: f64) -> [[f64; 3]; 3] {
        [[a, 0.0, 0.0], [0.0, a, 0.0], [0.0, 0.0, a]]
    }

    /// Each undirected edge used by exactly two triangles, with vertices
    /// identified by `id` (lets a test identify periodic images).
    fn closed_under(mesh: &Mesh, id: impl Fn([f32; 3]) -> [i64; 3]) -> bool {
        let ids: Vec<[i64; 3]> = mesh.positions.iter().map(|p| id(*p)).collect();
        let mut count: StdMap<([i64; 3], [i64; 3]), u32> = StdMap::new();
        for t in mesh.indices.chunks_exact(3) {
            for (a, b) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
                let (a, b) = (ids[a as usize], ids[b as usize]);
                let k = if a < b { (a, b) } else { (b, a) };
                *count.entry(k).or_default() += 1;
            }
        }
        !count.is_empty() && count.values().all(|&c| c == 2)
    }

    fn exact(p: [f32; 3]) -> [i64; 3] {
        p.map(|x| (x as f64 * 1e5).round() as i64)
    }

    #[test]
    fn sphere_is_closed_and_has_the_right_volume_area_and_normals() {
        let lat = cube(10.0);
        let c = Vector3::new(5.0, 5.0, 5.0);
        let r = 3.0;
        let data = sample(48, lat, |p| r - (p - c).norm());
        let grid = Grid { data: &data, dims: [48; 3], lattice: lat };
        let mesh = isosurface(&grid, 0.0, Inside::Above, Region::cell(grid.dims), &CancelToken::never()).unwrap();

        assert!(closed_under(&mesh, exact), "sphere inside the cell must be watertight");
        let v = mesh.enclosed_volume();
        let v0 = 4.0 / 3.0 * std::f64::consts::PI * r * r * r;
        assert!((v - v0).abs() / v0 < 0.01, "volume {v} vs {v0}");
        let a0 = 4.0 * std::f64::consts::PI * r * r;
        assert!((mesh.area() - a0).abs() / a0 < 0.01, "area {} vs {a0}", mesh.area());
        for (p, n) in mesh.positions.iter().zip(&mesh.normals) {
            let radial = (Vector3::new(p[0] as f64, p[1] as f64, p[2] as f64) - c).normalize();
            let nn = Vector3::new(n[0] as f64, n[1] as f64, n[2] as f64);
            assert!(radial.dot(&nn) > 0.98, "normal not outward-radial");
        }
    }

    #[test]
    fn triclinic_cell_volume() {
        let lat = [[10.0, 0.0, 0.0], [3.0, 9.0, 0.0], [2.0, 1.5, 9.5]];
        let m = Matrix3::new(10.0, 3.0, 2.0, 0.0, 9.0, 1.5, 0.0, 0.0, 9.5);
        let c = m * Vector3::new(0.5, 0.5, 0.5);
        let r = 2.5;
        let data = sample(56, lat, |p| r - (p - c).norm());
        let grid = Grid { data: &data, dims: [56; 3], lattice: lat };
        let mesh = isosurface(&grid, 0.0, Inside::Above, Region::cell(grid.dims), &CancelToken::never()).unwrap();
        let v0 = 4.0 / 3.0 * std::f64::consts::PI * r.powi(3);
        assert!((mesh.enclosed_volume() - v0).abs() / v0 < 0.01, "volume {} vs {v0}", mesh.enclosed_volume());
    }

    #[test]
    fn lobe_across_the_cell_corner_is_seamless() {
        // A sphere centred on the cell corner: every cell face cuts it.
        let a = 8.0;
        let lat = cube(a);
        let r = 2.0;
        let data = sample(41, lat, |p| {
            let d = p.map(|x| x - a * (x / a).round()); // minimum image
            r - d.norm()
        });
        let grid = Grid { data: &data, dims: [41; 3], lattice: lat };
        let mesh = isosurface(&grid, 0.0, Inside::Above, Region::cell(grid.dims), &CancelToken::never()).unwrap();
        // Open within one cell, closed once opposite faces are identified.
        assert!(!closed_under(&mesh, exact));
        let wrapped = |p: [f32; 3]| p.map(|x| ((x as f64).rem_euclid(a) * 1e5).round() as i64 % (a * 1e5) as i64);
        assert!(closed_under(&mesh, wrapped), "pieces on opposite faces must match");
        // The eight pieces add up to one sphere.
        let v0 = 4.0 / 3.0 * std::f64::consts::PI * r.powi(3);
        let two = Region { lo: [-20; 3], hi: [21; 3] };
        let whole = isosurface(&grid, 0.0, Inside::Above, two, &CancelToken::never()).unwrap();
        assert!(closed_under(&whole, exact), "a region centred on the corner holds the whole lobe");
        assert!((whole.enclosed_volume() - v0).abs() / v0 < 0.02, "{} vs {v0}", whole.enclosed_volume());
    }

    #[test]
    fn crossings_on_grid_points_share_one_vertex() {
        // r = 2.0 on a 0.2 Å grid: many grid points lie exactly on the surface.
        let lat = cube(8.0);
        let c = Vector3::new(4.0, 4.0, 4.0);
        let data = sample(40, lat, |p| 2.0 - (p - c).norm());
        let grid = Grid { data: &data, dims: [40; 3], lattice: lat };
        let mesh = isosurface(&grid, 0.0, Inside::Above, Region::cell(grid.dims), &CancelToken::never()).unwrap();
        let mut seen = std::collections::HashSet::new();
        for p in &mesh.positions {
            assert!(seen.insert(exact(*p)), "two vertices at {p:?}");
        }
        for t in mesh.indices.chunks_exact(3) {
            assert!(t[0] != t[1] && t[1] != t[2] && t[0] != t[2]);
        }
        assert!(closed_under(&mesh, exact));
    }

    #[test]
    fn negative_lobe_points_outward_too() {
        let lat = cube(10.0);
        let c = Vector3::new(5.0, 5.0, 5.0);
        let data = sample(40, lat, |p| (p - c).norm() - 3.0); // negative inside
        let grid = Grid { data: &data, dims: [40; 3], lattice: lat };
        let mesh = isosurface(&grid, 0.0, Inside::Below, Region::cell(grid.dims), &CancelToken::never()).unwrap();
        assert!(mesh.enclosed_volume() > 0.0, "counter-clockwise from outside");
        let p = mesh.positions[0];
        let n = mesh.normals[0];
        let radial = Vector3::new(p[0] as f64, p[1] as f64, p[2] as f64) - c;
        assert!(radial.dot(&Vector3::new(n[0] as f64, n[1] as f64, n[2] as f64)) > 0.0);
    }

    #[test]
    fn saddle_heavy_field_stays_watertight() {
        // Periodic cosines: every cube near the surface is a saddle case,
        // where classic marching cubes can leave holes.
        let a = 6.0;
        let lat = cube(a);
        let k = 2.0 * std::f64::consts::PI / a;
        let data = sample(31, lat, |p| (k * p.x).cos() + (k * p.y).cos() + (k * p.z).cos());
        let grid = Grid { data: &data, dims: [31; 3], lattice: lat };
        let mesh = isosurface(&grid, 0.0, Inside::Above, Region::cell(grid.dims), &CancelToken::never()).unwrap();
        let wrapped = |p: [f32; 3]| p.map(|x| ((x as f64).rem_euclid(a) * 1e5).round() as i64 % (a * 1e5) as i64);
        assert!(closed_under(&mesh, wrapped));
        // The nodal surface cos+cos+cos = 0 approximates the Schwarz P
        // minimal surface (2.345 a² per cell) closely.
        assert!((mesh.area() / (a * a) - 2.345).abs() < 0.05, "area/a^2 = {}", mesh.area() / (a * a));
    }

    #[test]
    fn empty_and_cancelled() {
        let lat = cube(5.0);
        let data = vec![1.0f32; 8 * 8 * 8];
        let grid = Grid { data: &data, dims: [8; 3], lattice: lat };
        let none = isosurface(&grid, 2.0, Inside::Above, Region::cell(grid.dims), &CancelToken::never()).unwrap();
        assert!(none.is_empty());
        let t = CancelToken::new();
        t.cancel();
        assert!(isosurface(&grid, 0.5, Inside::Above, Region::cell(grid.dims), &t).is_none());
    }
}
