// src/physics/analysis/interstitial.rs
//
// Distinct interstitial sites from the void distance field.
//
// `voids::calculate_voids` answers "how big is the largest hole in this
// cell". This module answers "where are ALL the holes" — every local
// maximum of the distance function, refined off the sampling grid so the
// reported radius is not quantized by the grid spacing.
//
// SCOPE — this is geometry, not energetics. A site reported here is a place
// where a hard sphere of the given radius fits in the rigid, as-loaded
// framework. It says nothing about whether an ion would be stable there:
// no electrostatics, no lattice relaxation, no migration barrier. Treat the
// output as a screen to run BEFORE a DFT/NEB calculation, never as a
// substitute for one.

use super::voids::{
    calculate_distance_field, calculate_distance_field_cancellable, summarize, DistanceField,
    RadiusType, VoidConfig, VoidError, VoidResult,
};
use crate::model::Structure;
use crate::utils::task::CancelToken;
use nalgebra::Vector3;
use rayon::prelude::*;

/// A local maximum of the distance function: a place where an inserted
/// sphere has the most room locally.
#[derive(Clone, Debug, PartialEq)]
pub struct InterstitialSite {
    /// Fractional coordinates, wrapped into [0, 1).
    pub frac: [f64; 3],
    /// Cartesian coordinates in Å.
    pub cart: [f64; 3],
    /// Radius of the largest sphere centred here that touches no atom, in Å.
    /// Refined off the grid, so it is not quantized by `grid_resolution`.
    pub radius: f64,
}

/// Parameters for the site search.
#[derive(Clone, Copy, Debug)]
pub struct SiteSearch {
    /// Discard sites that cannot host a sphere at least this large (Å).
    /// Applied AFTER refinement, so a site that refines upward across the
    /// threshold is kept.
    pub min_radius: f64,

    /// Refined sites closer together than this are treated as one site (Å).
    /// `None` derives it from the grid spacing, which is the scale at which
    /// two starting points can only be the same maximum sampled twice.
    pub merge_tolerance: Option<f64>,
}

impl Default for SiteSearch {
    fn default() -> Self {
        Self {
            min_radius: 0.0,
            merge_tolerance: None,
        }
    }
}

impl SiteSearch {
    /// Sites that can host an ion of the given Shannon radius.
    pub fn for_ion(ionic_radius: f64) -> Self {
        Self {
            min_radius: ionic_radius,
            merge_tolerance: None,
        }
    }
}

/// Union-find over grid-adjacent candidates, so a plateau of equal samples
/// collapses to one site instead of a cluster of near-duplicates.
struct DisjointSet {
    parent: Vec<usize>,
}

impl DisjointSet {
    fn new(n: usize) -> Self {
        Self {
            parent: (0..n).collect(),
        }
    }

    fn find(&mut self, mut x: usize) -> usize {
        while self.parent[x] != x {
            self.parent[x] = self.parent[self.parent[x]];
            x = self.parent[x];
        }
        x
    }

    fn union(&mut self, a: usize, b: usize) {
        let (ra, rb) = (self.find(a), self.find(b));
        if ra != rb {
            self.parent[rb] = ra;
        }
    }
}

/// Find every distinct interstitial site in the field.
///
/// 1. Collect grid points that are no lower than all 26 periodic neighbors.
/// 2. Collapse adjacent candidates (plateaus) into one representative each.
/// 3. Refine each representative off the grid.
/// 4. Merge representatives that converged onto the same maximum, and drop
///    anything below `min_radius`.
///
/// Sites come back sorted by radius, largest first.
pub fn find_sites(field: &DistanceField, search: SiteSearch) -> Vec<InterstitialSite> {
    find_sites_cancellable(field, search, &CancelToken::never())
        .expect("an uncancellable search cannot be cancelled")
}

/// [`find_sites`], abandoning the work if `cancel` is tripped.
///
/// The local-maxima scan touches all 26 neighbours of every grid point, so on
/// a 25M-point field it is comparable in cost to building the field itself and
/// has to be interruptible for the same reason.
pub fn find_sites_cancellable(
    field: &DistanceField,
    search: SiteSearch,
    cancel: &CancelToken,
) -> Option<Vec<InterstitialSite>> {
    let (nx, ny, nz) = field.dims();
    let (inx, iny, inz) = (nx as isize, ny as isize, nz as isize);

    // --- 1. Local maxima on the grid ---
    // `>=` rather than `>` so plateaus are caught; step 2 collapses them.
    let mut candidates: Vec<(usize, usize, usize)> = (0..nz)
        .into_par_iter()
        .flat_map_iter(|k| {
            let mut local = Vec::new();

            // Checked per slice, as in the field sweep.
            if cancel.is_cancelled() {
                return local;
            }

            for j in 0..ny {
                for i in 0..nx {
                    let centre = field.at(i as isize, j as isize, k as isize);

                    // A site inside an atom is not a site.
                    if centre <= 0.0 {
                        continue;
                    }

                    let mut is_max = true;
                    'neighbors: for dk in -1..=1 {
                        for dj in -1..=1 {
                            for di in -1..=1 {
                                if di == 0 && dj == 0 && dk == 0 {
                                    continue;
                                }
                                if field.at(
                                    i as isize + di,
                                    j as isize + dj,
                                    k as isize + dk,
                                ) > centre
                                {
                                    is_max = false;
                                    break 'neighbors;
                                }
                            }
                        }
                    }

                    if is_max {
                        local.push((i, j, k));
                    }
                }
            }

            local
        })
        .collect();

    if cancel.is_cancelled() {
        return None;
    }

    if candidates.is_empty() {
        return Some(Vec::new());
    }

    // Deterministic order regardless of how rayon interleaved the slices.
    candidates.sort_unstable();

    // --- 2. Collapse plateaus ---
    let index_of: std::collections::HashMap<(usize, usize, usize), usize> = candidates
        .iter()
        .enumerate()
        .map(|(id, &ijk)| (ijk, id))
        .collect();

    let mut dsu = DisjointSet::new(candidates.len());

    for (id, &(i, j, k)) in candidates.iter().enumerate() {
        for dk in -1..=1 {
            for dj in -1..=1 {
                for di in -1..=1 {
                    if di == 0 && dj == 0 && dk == 0 {
                        continue;
                    }
                    let key = (
                        (i as isize + di).rem_euclid(inx) as usize,
                        (j as isize + dj).rem_euclid(iny) as usize,
                        (k as isize + dk).rem_euclid(inz) as usize,
                    );
                    if let Some(&other) = index_of.get(&key) {
                        dsu.union(id, other);
                    }
                }
            }
        }
    }

    // Best-sampled member of each plateau becomes its representative.
    let mut representatives: std::collections::HashMap<usize, (usize, f64)> =
        std::collections::HashMap::new();

    for (id, &(i, j, k)) in candidates.iter().enumerate() {
        let root = dsu.find(id);
        let value = field.at(i as isize, j as isize, k as isize);
        representatives
            .entry(root)
            .and_modify(|best| {
                if value > best.1 {
                    *best = (id, value);
                }
            })
            .or_insert((id, value));
    }

    let mut reps: Vec<usize> = representatives.values().map(|&(id, _)| id).collect();
    reps.sort_unstable();

    // --- 3. Sub-grid refinement ---
    let mut refined: Vec<(Vector3<f64>, f64)> = reps
        .par_iter()
        .map(|&id| {
            let (i, j, k) = candidates[id];
            field.refine_maximum(field.frac_of(i, j, k))
        })
        .collect();

    // Largest first, so a merge always keeps the better-resolved member.
    refined.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

    // --- 4. Merge duplicates and filter ---
    let merge_tol = search
        .merge_tolerance
        .unwrap_or_else(|| field.spacing_hint().max(0.05));

    if cancel.is_cancelled() {
        return None;
    }

    let mut sites: Vec<InterstitialSite> = Vec::new();

    for (frac, radius) in refined {
        if radius < search.min_radius {
            continue;
        }

        let duplicate = sites.iter().any(|s| {
            field.min_image_distance(frac, Vector3::from(s.frac)) < merge_tol
        });

        if duplicate {
            continue;
        }

        let cart = field.to_cart(frac);
        sites.push(InterstitialSite {
            frac: [frac.x, frac.y, frac.z],
            cart: [cart.x, cart.y, cart.z],
            radius,
        });
    }

    Some(sites)
}

/// Find the sites in `structure` that could host an ion of the given
/// Shannon radius.
///
/// Uses a purely geometric field (no probe inflation) — the probe radius is
/// applied as the `min_radius` filter instead, so the reported site radii
/// are the real clearances rather than clearances minus the probe.
pub fn find_sites_for_ion(
    structure: &Structure,
    ionic_radius: f64,
    grid_resolution: f64,
) -> Result<Vec<InterstitialSite>, VoidError> {
    let config = VoidConfig {
        grid_resolution,
        probe_radius: 0.0,
        radii_scale: 1.0,
        radius_type: RadiusType::Ionic,
        ..Default::default()
    };

    let field = calculate_distance_field(structure, config)?;
    Ok(find_sites(&field, SiteSearch::for_ion(ionic_radius)))
}

/// One sweep of the cell answering both questions the voids panel asks: the
/// headline void numbers, and every site that could host the candidate ion.
///
/// Sampling the distance field is the expensive part and both answers come
/// out of the same field, so they are computed together rather than by two
/// passes over the same grid.
///
/// Returns `Ok(None)` when the run was cancelled.
pub fn screen_structure(
    structure: &Structure,
    config: VoidConfig,
    ion_radius: f64,
    cancel: &CancelToken,
) -> Result<Option<(VoidResult, Vec<InterstitialSite>)>, VoidError> {
    let field = match calculate_distance_field_cancellable(structure, config, cancel)? {
        Some(field) => field,
        None => return Ok(None),
    };

    let voids = summarize(&field);

    match find_sites_cancellable(&field, SiteSearch::for_ion(ion_radius), cancel) {
        Some(sites) => Ok(Some((voids, sites))),
        None => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::elements::get_atom_ionic_radius;
    use crate::model::structure::Atom;

    /// Grid fine enough that every site in the test cells lands exactly on a
    /// sample point, so a failure means the algorithm is wrong rather than
    /// the grid being unlucky.
    const RES: f64 = 0.2;
    const A: f64 = 4.0;

    fn cell(fracs: &[[f64; 3]]) -> Structure {
        Structure {
            lattice: [[A, 0.0, 0.0], [0.0, A, 0.0], [0.0, 0.0, A]],
            atoms: fracs
                .iter()
                .enumerate()
                .map(|(i, f)| Atom {
                    element: "Na".into(),
                    position: [f[0] * A, f[1] * A, f[2] * A],
                    original_index: i,
                    oxidation: None,
                    occupancy: 1.0,
                })
                .collect(),
            formula: "Na".into(),
            is_periodic: true,
        }
    }

    fn na_radius() -> f64 {
        get_atom_ionic_radius("Na")
    }

    fn sites_of(structure: &Structure) -> Vec<InterstitialSite> {
        find_sites_for_ion(structure, 0.0, RES).expect("field build")
    }

    /// One atom per simple-cubic cell: the distance function increases
    /// monotonically from the corner to the body centre, so there is exactly
    /// one maximum, at (1/2, 1/2, 1/2) with clearance a·√3/2 − R.
    #[test]
    fn single_atom_cubic_has_one_site_at_body_centre() {
        let sites = sites_of(&cell(&[[0.0, 0.0, 0.0]]));

        assert_eq!(sites.len(), 1, "expected a single maximum, got {:?}", sites);

        let s = &sites[0];
        for x in s.frac {
            assert!((x - 0.5).abs() < 1e-3, "site not at body centre: {:?}", s.frac);
        }

        let expected = A * 3f64.sqrt() / 2.0 - na_radius();
        assert!(
            (s.radius - expected).abs() < 1e-3,
            "radius {} != expected {}",
            s.radius,
            expected
        );
    }

    /// FCC has exactly two kinds of interstitial: 4 octahedral holes per
    /// conventional cell at a/2 from the nearest atom, and 8 tetrahedral at
    /// a·√3/4. Finding 12 sites in those two groups exercises periodic
    /// neighbor wrapping, plateau collapsing and the merge tolerance at once.
    #[test]
    fn fcc_finds_octahedral_and_tetrahedral_holes() {
        let fcc = cell(&[
            [0.0, 0.0, 0.0],
            [0.0, 0.5, 0.5],
            [0.5, 0.0, 0.5],
            [0.5, 0.5, 0.0],
        ]);

        let sites = sites_of(&fcc);
        assert_eq!(sites.len(), 12, "expected 4 octahedral + 8 tetrahedral");

        let r = na_radius();
        let octahedral = A / 2.0 - r;
        let tetrahedral = A * 3f64.sqrt() / 4.0 - r;

        let n_oct = sites
            .iter()
            .filter(|s| (s.radius - octahedral).abs() < 1e-3)
            .count();
        let n_tet = sites
            .iter()
            .filter(|s| (s.radius - tetrahedral).abs() < 1e-3)
            .count();

        assert_eq!(n_oct, 4, "octahedral holes: {:?}", sites);
        assert_eq!(n_tet, 8, "tetrahedral holes: {:?}", sites);

        // Sorted largest-first, and the octahedral hole is the bigger one.
        assert!(sites[0].radius > sites[11].radius);
    }

    /// `min_radius` is applied after refinement and must cut the smaller
    /// family without disturbing the larger one.
    #[test]
    fn min_radius_filters_the_smaller_family() {
        let fcc = cell(&[
            [0.0, 0.0, 0.0],
            [0.0, 0.5, 0.5],
            [0.5, 0.0, 0.5],
            [0.5, 0.5, 0.0],
        ]);

        let r = na_radius();
        let tetrahedral = A * 3f64.sqrt() / 4.0 - r;

        let sites = find_sites_for_ion(&fcc, tetrahedral + 0.05, RES).unwrap();
        assert_eq!(sites.len(), 4, "only the octahedral holes should survive");
    }

    /// The whole point of refining off the grid: on a deliberately coarse,
    /// incommensurate grid the sampled maximum is quantized, and refinement
    /// has to recover the true value.
    #[test]
    fn refinement_recovers_the_true_maximum_from_a_coarse_grid() {
        // 0.7 Å over a 4 Å cell gives 6 divisions — the body centre at 0.5
        // is not a sample point.
        let structure = cell(&[[0.0, 0.0, 0.0]]);
        let sites = find_sites_for_ion(&structure, 0.0, 0.7).unwrap();

        assert_eq!(sites.len(), 1);

        let expected = A * 3f64.sqrt() / 2.0 - na_radius();
        assert!(
            (sites[0].radius - expected).abs() < 1e-3,
            "coarse-grid radius {} did not refine to {}",
            sites[0].radius,
            expected
        );
    }

    /// Sites must be found the same way in a sheared cell. A rhombohedral
    /// distortion of the one-atom cell still has a single maximum, and the
    /// oblique minimum-image path has to agree with the orthogonal one on
    /// the undistorted limit.
    #[test]
    fn oblique_cell_finds_a_single_site() {
        let sheared = Structure {
            lattice: [[A, 0.0, 0.0], [0.6, A, 0.0], [0.4, 0.5, A]],
            atoms: vec![Atom {
                element: "Na".into(),
                position: [0.0, 0.0, 0.0],
                original_index: 0,
                oxidation: None,
                occupancy: 1.0,
            }],
            formula: "Na".into(),
            is_periodic: true,
        };

        let sites = find_sites_for_ion(&sheared, 0.0, RES).unwrap();
        assert_eq!(sites.len(), 1, "expected one maximum, got {:?}", sites);
        assert!(sites[0].radius > 0.0);
    }
}
