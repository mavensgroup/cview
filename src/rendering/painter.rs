// src/rendering/painter.rs
// Publication-quality vector exports + optimized screen rendering
// Draw order: Polyhedra (background) → Bonds → Atoms (foreground)
// All unwraps eliminated, NaN-safe

use super::primitives::*;
use super::scene::RenderAtom;
use crate::config::ColorMode;
use crate::model::elements::{ColorScheme, get_atom_cov, get_covalent_radius, get_element_color};
use crate::physics::bond_valence::get_ideal_oxidation_state;
use crate::physics::operations::miller_algo::MillerMath;
use crate::rendering::occupancy::PartialSites;
use crate::rendering::polyhedra;
use crate::rendering::polyhedra_lighting;
use crate::state::TabState;
use crate::utils::spatial_grid::SpatialGrid;
use gtk4::cairo;
use std::cmp::Ordering;
use std::f64::consts::PI;

// ============================================================================
// HELPER FUNCTIONS
// ============================================================================

/// Map BVS deviation to color gradient
/// Green (good) → Yellow (warning) → Orange → Red (bad)
fn get_bvs_color(
    bvs_calculated: f64,
    bvs_ideal: f64,
    threshold_good: f64,
    threshold_warn: f64,
) -> (f64, f64, f64) {
    // Unknown ideal state - use neutral gray
    if bvs_ideal < 0.1 {
        return (0.65, 0.65, 0.65);
    }

    let deviation = (bvs_calculated - bvs_ideal).abs();

    if deviation < threshold_good {
        // Excellent agreement: Pure green
        (0.15, 0.75, 0.15)
    } else if deviation < threshold_warn {
        // Warning zone: Green → Yellow → Orange gradient
        let t = (deviation - threshold_good) / (threshold_warn - threshold_good);
        let r = 0.15 + 0.80 * t;
        let g = 0.75 - 0.15 * t;
        let b = 0.15 * (1.0 - t);
        (r, g, b)
    } else {
        // Error zone: Red with intensity based on severity
        let excess = (deviation - threshold_warn).min(0.5);
        let intensity = 1.0 - excess * 0.4;
        (0.85 * intensity, 0.12, 0.12)
    }
}

// ============================================================================
// UNIT CELL DRAWING
// ============================================================================

pub fn draw_unit_cell(cr: &cairo::Context, corners: &[[f64; 2]], is_export: bool) {
    if corners.len() != 8 {
        return;
    }

    cr.set_source_rgb(0.5, 0.5, 0.5);

    // Publication quality: Thicker lines for exports
    cr.set_line_width(if is_export { 2.5 } else { 1.5 });

    let edges = [
        (0, 1),
        (0, 2),
        (0, 4),
        (1, 3),
        (1, 5),
        (2, 3),
        (2, 6),
        (4, 5),
        (4, 6),
        (7, 6),
        (7, 5),
        (7, 3),
    ];

    for (start, end) in edges {
        let p1 = corners[start];
        let p2 = corners[end];
        cr.move_to(p1[0], p1[1]);
        cr.line_to(p2[0], p2[1]);
        cr.stroke()
            .expect("Failed to stroke unit cell - reduce complexity");
    }
}

// ============================================================================
// POLYHEDRA RENDERING  (Lambertian shading via polyhedra_lighting module)
// ============================================================================

/// Scale for property-coloured polyhedra. Drawn once per frame, after the atoms,
/// so exports carry it too: colour without a scale is meaningless.
pub struct PolyLegend {
    title: &'static str,
    unit: &'static str,
    min: f64,
    max: f64,
    integer: bool,
    colormap: crate::rendering::colormap::Colormap,
}

fn property_value(
    prop: crate::config::PolyProperty,
    cn: usize,
    m: &polyhedra::PolyhedronMetrics,
) -> Option<f64> {
    use crate::config::PolyProperty as P;
    match prop {
        P::Coordination => Some(cn as f64),
        P::MeanBondLength => Some(m.mean_bond_length),
        P::BaurDistortion => Some(m.baur_distortion),
        P::QuadraticElongation => m.quadratic_elongation,
        P::AngleVariance => m.bond_angle_variance,
        P::Volume => Some(m.volume),
    }
}

/// One face to draw, with the depth it sorts at.
struct FaceItem {
    depth: f64,
    sv: [[f64; 3]; 3],
    cv: [[f64; 3]; 3],
    center_cart: [f64; 3],
    color: (f64, f64, f64),
    alpha: f64,
    edges: [bool; 3],
}

/// Everything the polyhedra contribute to a frame. The faces are not drawn
/// here: they are sorted together with the atoms and bonds, so an opaque face
/// can hide what is behind it, including its own central atom.
struct PolyPlan {
    faces: Vec<FaceItem>,
    legend: Option<PolyLegend>,
    /// Depth at which a polyhedron's central atom sorts: inside the polyhedron,
    /// after its back faces and before its front faces.
    centre_depth: std::collections::HashMap<usize, f64>,
    /// Same depth for the bonds from the centre to its vertices.
    inner_bond_depth: std::collections::HashMap<(usize, usize), f64>,
}

/// Plan the polyhedra: faces with depth keys, colour scale, and the depths at
/// which each polyhedron's interior (centre atom, centre bonds) sorts.
fn plan_polyhedra(
    atoms: &[RenderAtom],
    tab: &TabState,
    color_scheme: ColorScheme,
) -> Option<PolyPlan> {
    use crate::config::PolyhedraColorMode as Mode;
    use crate::rendering::colormap::colormap_rgb;

    let settings = match &tab.style.polyhedra_settings {
        Some(s) if s.show_polyhedra => s,
        _ => return None,
    };
    let style = &tab.style.polyhedra_style;

    let built = polyhedra::build_for_tab(atoms, tab)?;

    // Property colouring: one value per polyhedron, scaled to the range of
    // the polyhedra on screen.
    let prop = match &settings.color_mode {
        Mode::Property(p) => Some(*p),
        _ => None,
    };
    let values: Vec<Option<f64>> = built
        .iter()
        .map(|poly| {
            prop.and_then(|p| property_value(p, poly.coordination_number, &poly.metrics(atoms)))
        })
        .collect();
    let (vmin, vmax) = values
        .iter()
        .flatten()
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), &v| {
            (lo.min(v), hi.max(v))
        });
    let legend = prop.filter(|_| vmin.is_finite()).map(|p| PolyLegend {
        title: p.label(),
        unit: p.unit(),
        min: vmin,
        max: vmax,
        integer: matches!(p, crate::config::PolyProperty::Coordination),
        colormap: style.colormap,
    });

    let mut items: Vec<FaceItem> = Vec::new();
    let mut centre_depth = std::collections::HashMap::new();
    let mut inner_bond_depth = std::collections::HashMap::new();
    // Keeps back faces strictly behind, and front faces strictly in front of,
    // the polyhedron's interior in the shared depth sort.
    const INTERIOR_GAP: f64 = 0.05;

    for (pi, poly) in built.iter().enumerate() {
        let color = match &settings.color_mode {
            Mode::Custom(r, g, b) => {
                polyhedra_lighting::desaturate((*r, *g, *b), style.desaturation)
            }
            Mode::Property(_) => match values[pi] {
                Some(v) => {
                    let t = if vmax - vmin < 1e-12 {
                        0.5
                    } else {
                        (v - vmin) / (vmax - vmin)
                    };
                    colormap_rgb(style.colormap, t)
                }
                // Undefined for this polyhedron (no reference shape): neutral grey.
                None => (0.62, 0.62, 0.62),
            },
            Mode::Element => {
                let elem = &atoms[poly.center_idx].element;
                let base = tab
                    .style
                    .element_colors
                    .get(elem)
                    .copied()
                    .unwrap_or_else(|| get_element_color(elem, color_scheme));
                polyhedra_lighting::desaturate(base, style.desaturation)
            }
        };
        // The vertex centroid is always inside the polyhedron; the cation can
        // sit outside it (off-centre Ti in a ferroelectric), which would flip
        // the lighting normals and the front/back test.
        let nv = poly.neighbor_indices.len().max(1) as f64;
        let mean = |f: &dyn Fn(&RenderAtom) -> [f64; 3]| {
            let mut c = [0.0; 3];
            for &i in &poly.neighbor_indices {
                let p = f(&atoms[i]);
                for k in 0..3 {
                    c[k] += p[k] / nv;
                }
            }
            c
        };
        let center_cart = mean(&|a| a.cart_pos);
        let center_screen = mean(&|a| a.screen_pos);
        let interior_z = center_screen[2];
        centre_depth.insert(poly.center_idx, interior_z);
        for &vtx in &poly.neighbor_indices {
            let key = (poly.center_idx.min(vtx), poly.center_idx.max(vtx));
            inner_bond_depth.insert(key, interior_z);
        }
        let flags = polyhedra::feature_edge_flags(poly, atoms);

        for (fi, face) in poly.faces.iter().enumerate() {
            let sv = face.screen_vertices(atoms);
            let sc = face.screen_center(atoms);
            let cv: [[f64; 3]; 3] = [
                atoms[face.vertex_atom_indices[0]].cart_pos,
                atoms[face.vertex_atom_indices[1]].cart_pos,
                atoms[face.vertex_atom_indices[2]].cart_pos,
            ];

            // Facing: the outward normal in screen space, against the view
            // direction. Larger z is farther, so a face turned toward the
            // viewer has an outward normal with z < 0.
            let e1 = [sv[1][0] - sv[0][0], sv[1][1] - sv[0][1], sv[1][2] - sv[0][2]];
            let e2 = [sv[2][0] - sv[0][0], sv[2][1] - sv[0][1], sv[2][2] - sv[0][2]];
            let mut n = [
                e1[1] * e2[2] - e1[2] * e2[1],
                e1[2] * e2[0] - e1[0] * e2[2],
                e1[0] * e2[1] - e1[1] * e2[0],
            ];
            let out = [
                sc[0] - center_screen[0],
                sc[1] - center_screen[1],
                sc[2] - center_screen[2],
            ];
            if n[0] * out[0] + n[1] * out[1] + n[2] * out[2] < 0.0 {
                n = [-n[0], -n[1], -n[2]];
            }
            let front = n[2] < 0.0;

            let depth = if front {
                sc[2].min(interior_z - INTERIOR_GAP)
            } else {
                sc[2].max(interior_z + INTERIOR_GAP)
            };
            items.push(FaceItem {
                depth,
                sv,
                cv,
                center_cart,
                color,
                alpha: style.opacity * if front { 1.0 } else { style.back_face_opacity },
                edges: flags[fi],
            });
        }
    }

    Some(PolyPlan {
        faces: items,
        legend,
        centre_depth,
        inner_bond_depth,
    })
}

/// Colour bar for property-coloured polyhedra, bottom-right of the canvas.
fn draw_poly_legend(cr: &cairo::Context, legend: &PolyLegend, background: (f64, f64, f64)) {
    use crate::rendering::colormap::colormap_rgb;
    let Ok((x0, y0, x1, y1)) = cr.clip_extents() else {
        return;
    };
    let (w, h) = (x1 - x0, y1 - y0);
    let (bar_w, bar_h, margin) = (150.0, 10.0, 14.0);
    if w < bar_w + 2.0 * margin || h < 70.0 {
        return;
    }
    // Text colour from the background luminance.
    let lum = 0.299 * background.0 + 0.587 * background.1 + 0.114 * background.2;
    let ink = if lum > 0.5 { (0.1, 0.1, 0.1) } else { (0.92, 0.92, 0.92) };

    let bx = x1 - margin - bar_w;
    let by = y1 - margin - 16.0 - bar_h;
    cr.select_font_face("sans-serif", cairo::FontSlant::Normal, cairo::FontWeight::Normal);
    cr.set_font_size(11.0);

    let fmt = |v: f64| {
        if legend.integer {
            format!("{:.0}{}", v, legend.unit)
        } else if v.abs() >= 100.0 {
            format!("{:.1}{}", v, legend.unit)
        } else {
            format!("{:.3}{}", v, legend.unit)
        }
    };

    // All polyhedra share one value: a gradient would imply a range that is
    // not there, so state the value instead.
    if legend.max - legend.min < 1e-9 {
        let text = format!("{} = {} (uniform)", legend.title, fmt(legend.min));
        let width = cr.text_extents(&text).map(|e| e.width()).unwrap_or(bar_w);
        cr.set_source_rgb(ink.0, ink.1, ink.2);
        cr.move_to(x1 - margin - width, by + bar_h);
        let _ = cr.show_text(&text);
        return;
    }

    cr.set_source_rgb(ink.0, ink.1, ink.2);
    cr.move_to(bx, by - 6.0);
    let _ = cr.show_text(legend.title);

    let steps = 64;
    for i in 0..steps {
        let t = i as f64 / (steps - 1) as f64;
        let (r, g, b) = colormap_rgb(legend.colormap, t);
        cr.set_source_rgb(r, g, b);
        cr.rectangle(bx + bar_w * i as f64 / steps as f64, by, bar_w / steps as f64 + 0.5, bar_h);
        let _ = cr.fill();
    }
    cr.set_source_rgba(ink.0, ink.1, ink.2, 0.6);
    cr.set_line_width(0.8);
    cr.rectangle(bx, by, bar_w, bar_h);
    let _ = cr.stroke();

    cr.set_source_rgb(ink.0, ink.1, ink.2);
    cr.move_to(bx, by + bar_h + 12.0);
    let _ = cr.show_text(&fmt(legend.min));
    let max_text = fmt(legend.max);
    let ext = cr.text_extents(&max_text).map(|e| e.width()).unwrap_or(0.0);
    cr.move_to(bx + bar_w - ext, by + bar_h + 12.0);
    let _ = cr.show_text(&max_text);
}

// ============================================================================
// MAIN STRUCTURE DRAWING
// ============================================================================
pub fn draw_structure(
    cr: &cairo::Context,
    atoms: &[RenderAtom],
    tab: &TabState,
    scale: f64,
    is_export: bool,
    color_scheme: ColorScheme,
) {
    // Bond detection tolerance
    let tolerance = if tab.view.bond_cutoff < 0.1 || tab.view.bond_cutoff > 2.0 {
        1.15
    } else {
        tab.view.bond_cutoff
    };

    // Whether to show ghost atoms visually. Ghost atoms are always present in
    // the atoms slice (needed for polyhedra/bond detection at cell boundaries),
    // but we skip rendering them when the user has "Show Full Unit Cell" off.
    let show_ghosts = tab.view.show_full_unit_cell;

    // Species sharing a partially occupied site are drawn once, as a pie on
    // the majority species' sphere; the other members are skipped for atoms
    // and bonds alike.
    let sites = tab
        .structure
        .as_ref()
        .map(PartialSites::build)
        .unwrap_or_default();

    // Separate lists for depth-sorted rendering
    let mut render_atoms: Vec<(usize, &RenderAtom)> = Vec::with_capacity(atoms.len());
    let mut render_bonds: Vec<RenderBond> = Vec::with_capacity(atoms.len() * 2);

    // ========================================================================
    // STEP 1: Collect Atoms (skip coord-only ghosts and invisible ghosts)
    // ========================================================================
    for (idx, atom) in atoms.iter().enumerate() {
        if atom.is_coord_only {
            continue;
        }
        if atom.is_ghost && !show_ghosts {
            continue;
        }
        if sites.is_hidden(atom.original_index) {
            continue;
        }
        render_atoms.push((idx, atom));
    }

    // ========================================================================
    // STEP 2: Collect Bonds (skip bonds involving coord-only or hidden ghosts)
    //
    // Uses a spatial grid to avoid the O(N²) nested scan. Grid cell size =
    // max bond distance (4 Å), so each query visits a 3×3×3 block at most.
    // Atoms filtered out at grid build time are never returned as neighbors,
    // so the inner loop doesn't need to re-check is_coord_only / is_ghost.
    // ========================================================================
    if tab.view.show_bonds {
        const MAX_BOND_DIST: f64 = 4.0;

        let grid = SpatialGrid::build(atoms, MAX_BOND_DIST, |a| {
            !a.is_coord_only && !(a.is_ghost && !show_ghosts) && !sites.is_hidden(a.original_index)
        });
        let mut neighbors: Vec<usize> = Vec::with_capacity(64);

        for (i, r1) in atoms.iter().enumerate() {
            if r1.is_coord_only
                || (r1.is_ghost && !show_ghosts)
                || sites.is_hidden(r1.original_index)
            {
                continue;
            }
            let rad1 = get_atom_cov(&r1.element);

            neighbors.clear();
            grid.query(r1.cart_pos, MAX_BOND_DIST, &mut neighbors);

            for &j in &neighbors {
                // Enforce unique (i, j) ordering so each pair is emitted once.
                if j <= i {
                    continue;
                }
                let r2 = &atoms[j];
                // Grid filter already excluded coord_only and hidden ghosts.

                // Calculate CARTESIAN distance
                let dx = r2.cart_pos[0] - r1.cart_pos[0];
                let dy = r2.cart_pos[1] - r1.cart_pos[1];
                let dz = r2.cart_pos[2] - r1.cart_pos[2];
                let dist = (dx * dx + dy * dy + dz * dz).sqrt();

                // Grid query already enforced dist ≤ MAX_BOND_DIST, so no
                // redundant check needed here.

                let rad2 = get_atom_cov(&r2.element);
                let max_bond_dist = (rad1 + rad2) * tolerance;
                let min_bond_dist = 0.4;

                if dist > min_bond_dist && dist < max_bond_dist {
                    let raw_r1 = get_covalent_radius(&r1.element);
                    let raw_r2 = get_covalent_radius(&r2.element);

                    let mult1 = tab.override_radius_scale(r1.original_index);
                    let mult2 = tab.override_radius_scale(r2.original_index);
                    let r1_px = raw_r1 * tab.style.atom_scale * mult1 * scale;
                    let r2_px = raw_r2 * tab.style.atom_scale * mult2 * scale;

                    let v_x = r2.screen_pos[0] - r1.screen_pos[0];
                    let v_y = r2.screen_pos[1] - r1.screen_pos[1];
                    let v_z = r2.screen_pos[2] - r1.screen_pos[2];
                    let full_screen_dist = (v_x * v_x + v_y * v_y + v_z * v_z).sqrt();

                    let off1 = r1_px * 0.95;
                    let off2 = r2_px * 0.95;

                    if full_screen_dist > (off1 + off2) {
                        let t1 = off1 / full_screen_dist;
                        let t2 = off2 / full_screen_dist;

                        let start = [
                            r1.screen_pos[0] + v_x * t1,
                            r1.screen_pos[1] + v_y * t1,
                            r1.screen_pos[2] + v_z * t1,
                        ];
                        let end = [
                            r2.screen_pos[0] - v_x * t2,
                            r2.screen_pos[1] - v_y * t2,
                            r2.screen_pos[2] - v_z * t2,
                        ];

                        render_bonds.push(RenderBond {
                            start,
                            end,
                            radius: tab.style.bond_radius * scale,
                            atoms: (i, j),
                        });
                    }
                }
            }
        }
    }

    // ========================================================================
    // STEP 3: Plan the polyhedra (faces, and where their interiors sort)
    // ========================================================================
    let plan = plan_polyhedra(atoms, tab, color_scheme);

    // ========================================================================
    // STEP 4: Atom drawing (called from the shared depth-ordered pass below)
    // ========================================================================
    let mut cache_access = tab.style.atom_cache.borrow_mut();

    // Per-atom override beats every color mode — this is exactly what the
    // user just set in the Atom Instances dialog, so respect it everywhere
    // including BVS view.
    let atom_rgb = |index: usize, element: &str| -> (f64, f64, f64) {
        if let Some(c) = tab.override_color(index) {
            return c;
        }
        let default_rgb = get_element_color(element, color_scheme);
        match tab.style.color_mode {
            ColorMode::Element => tab
                .style
                .element_colors
                .get(element)
                .copied()
                .unwrap_or(default_rgb),
            ColorMode::BondValence => {
                if let Some(bvs_value) = tab.bvs_cache.get(index) {
                    let ideal = get_ideal_oxidation_state(element);
                    get_bvs_color(
                        *bvs_value,
                        ideal,
                        tab.style.bvs_threshold_good,
                        tab.style.bvs_threshold_warn,
                    )
                } else {
                    (0.7, 0.7, 0.7)
                }
            }
            _ => default_rgb,
        }
    };

    let mut draw_atom = |atom: &RenderAtom| {
        let raw_r = get_covalent_radius(&atom.element);
        let override_rgb = tab.override_color(atom.original_index);
        let rgb = atom_rgb(atom.original_index, &atom.element);

        // Species sharing this site with their occupancies, when it is
        // partially occupied. `atom` is the majority species.
        let site: Option<Vec<(usize, &str, f64)>> = match (&tab.structure, sites.members(atom.original_index)) {
            (Some(s), Some(members)) => Some(
                members
                    .iter()
                    .map(|&m| (m, s.atoms[m].element.as_str(), s.atoms[m].occupancy))
                    .collect(),
            ),
            _ => None,
        };

        let radius_mult = tab.override_radius_scale(atom.original_index);
        let target_atom_cov = raw_r * tab.style.atom_scale * radius_mult * scale;

        // Selection glow — keyed on per-instance unique_id so only the clicked
        // ghost copy lights up, not every symmetry-equivalent corner.
        if tab.interaction.selected.contains_key(&atom.unique_id) {
            cr.save().ok();
            let highlight_radius = target_atom_cov + 4.0;
            cr.set_source_rgba(1.0, 0.85, 0.0, 0.8);
            cr.arc(
                atom.screen_pos[0],
                atom.screen_pos[1],
                highlight_radius,
                0.0,
                2.0 * PI,
            );
            cr.fill().ok();
            cr.restore().ok();
        }

        // Draw Atom (Vector vs Sprite)
        // BVS view and per-atom color overrides both use the vector path —
        // sprite cache is keyed by element+material, so a per-atom color
        // change wouldn't get a fresh sprite without a more invasive cache-key
        // rework. Vector draw is fast enough for the override case (typically
        // a few atoms, not all of them).
        if let Some(members) = &site {
            let slices: Vec<((f64, f64, f64), f64)> = members
                .iter()
                .map(|&(m, element, occ)| (atom_rgb(m, element), occ))
                .collect();
            draw_atom_pie(
                cr,
                atom.screen_pos[0],
                atom.screen_pos[1],
                target_atom_cov,
                &slices,
            );
        } else if is_export
            || matches!(tab.style.color_mode, ColorMode::BondValence)
            || override_rgb.is_some()
        {
            draw_atom_vector(
                cr,
                atom.screen_pos[0],
                atom.screen_pos[1],
                target_atom_cov,
                rgb,
            );
        } else {
            use crate::rendering::sprite_cache::SpriteCache;

            // Cache one sprite per on-screen size bucket, not one per element.
            // A 128 px sprite blitted down to a 16 px atom pushes Cairo's
            // Filter::Good onto its full resampling path (~58 us per atom);
            // matching the sprite to the atom keeps every blit at a scale
            // factor in (0.5, 1.0], which is ~50x cheaper.
            let sprite_px = SpriteCache::size_bucket(target_atom_cov * 2.0);
            let cache_key = SpriteCache::make_key(
                &atom.element,
                sprite_px,
                tab.style.metallic,
                tab.style.roughness,
                tab.style.transmission,
            );

            let sprite = cache_access.get_or_insert(cache_key, || {
                create_atom_sprite(
                    rgb.0,
                    rgb.1,
                    rgb.2,
                    tab.style.metallic,
                    tab.style.roughness,
                    tab.style.transmission,
                    sprite_px,
                )
            });

            let sprite_size = sprite_px as f64;
            cr.save().ok();
            cr.translate(atom.screen_pos[0], atom.screen_pos[1]);
            let scale_factor = (target_atom_cov * 2.0) / sprite_size;
            cr.scale(scale_factor, scale_factor);
            cr.set_source_surface(&sprite, -sprite_size / 2.0, -sprite_size / 2.0)
                .ok();
            cr.paint().ok();
            cr.restore().ok();
        }

        // ====================================================================
        // ENGRAVED BILLIARD LABELS
        // ====================================================================
        if tab.style.show_labels && target_atom_cov > 12.0 {
            // 1. Determine Contrast & Engraving Colors
            let lum = 0.299 * rgb.0 + 0.587 * rgb.1 + 0.114 * rgb.2;
            let (text_col, shadow_col) = if lum > 0.65 {
                // Bright Atom: Black text with white highlight (stamped in)
                ((0.0, 0.0, 0.0, 0.8), (1.0, 1.0, 1.0, 0.4))
            } else {
                // Dark Atom: White text with dark shadow (embossed)
                ((1.0, 1.0, 1.0, 0.9), (0.0, 0.0, 0.0, 0.5))
            };

            // 2. Font Settings (Smaller to avoid curvature distortion issues)
            // Reduced from 0.9 to 0.6 to keep text in the "flat" center zone
            // A shared site names every species on it, majority first.
            let label = match &site {
                Some(members) => members.iter().map(|m| m.1).collect::<Vec<_>>().join("/"),
                None => atom.element.clone(),
            };
            let font_size = target_atom_cov * 0.6 / (label.chars().count() as f64 / 2.0).max(1.0);
            cr.select_font_face("Sans", cairo::FontSlant::Normal, cairo::FontWeight::Bold);
            cr.set_font_size(font_size);

            if let Ok(extents) = cr.text_extents(&label) {
                let x_off = extents.width() / 2.0 + extents.x_bearing();
                let y_off = extents.height() / 2.0 + extents.y_bearing();

                let base_x = atom.screen_pos[0] - x_off;
                let base_y = atom.screen_pos[1] - y_off;

                // 3. Draw "Engraving" Shadow (Offset slightly down-right)
                cr.set_source_rgba(shadow_col.0, shadow_col.1, shadow_col.2, shadow_col.3);
                cr.move_to(base_x + 1.0, base_y + 1.0);
                cr.show_text(&label).ok();

                // 4. Draw Main Text
                cr.set_source_rgba(text_col.0, text_col.1, text_col.2, text_col.3);
                cr.move_to(base_x, base_y);
                cr.show_text(&label).ok();
            }

            // 5. Heavy Gloss Overlay (Bakes the text under the shine)
            let grad = cairo::RadialGradient::new(
                atom.screen_pos[0] - target_atom_cov * 0.3,
                atom.screen_pos[1] - target_atom_cov * 0.3,
                target_atom_cov * 0.1,
                atom.screen_pos[0],
                atom.screen_pos[1],
                target_atom_cov,
            );
            // Stronger shine to reinforce spherical shape over the text
            grad.add_color_stop_rgba(0.0, 1.0, 1.0, 1.0, 0.5);
            grad.add_color_stop_rgba(1.0, 1.0, 1.0, 1.0, 0.0);

            cr.set_source(&grad).ok();
            cr.arc(
                atom.screen_pos[0],
                atom.screen_pos[1],
                target_atom_cov,
                0.0,
                2.0 * PI,
            );
            cr.fill().ok();
        }
    };

    // ========================================================================
    // STEP 5: One back-to-front pass over faces, bonds and atoms
    //
    // Sorting them together (rather than polyhedra, then bonds, then atoms)
    // is what lets an opaque front face hide the central atom and the bonds
    // inside a polyhedron. Larger z is farther. At equal depth a face is
    // drawn before a bond and a bond before an atom.
    // ========================================================================
    enum Item {
        Face(usize),
        Bond(usize),
        Atom(usize),
    }
    let mut order: Vec<(f64, u8, Item)> = Vec::new();
    if let Some(plan) = &plan {
        for (i, f) in plan.faces.iter().enumerate() {
            order.push((f.depth, 0, Item::Face(i)));
        }
    }
    for (i, b) in render_bonds.iter().enumerate() {
        let key = (b.atoms.0.min(b.atoms.1), b.atoms.0.max(b.atoms.1));
        let depth = plan
            .as_ref()
            .and_then(|p| p.inner_bond_depth.get(&key).copied())
            .unwrap_or((b.start[2] + b.end[2]) / 2.0);
        order.push((depth, 1, Item::Bond(i)));
    }
    for (i, (idx, a)) in render_atoms.iter().enumerate() {
        let depth = plan
            .as_ref()
            .and_then(|p| p.centre_depth.get(idx).copied())
            .unwrap_or(a.screen_pos[2]);
        order.push((depth, 2, Item::Atom(i)));
    }
    order.sort_by(|a, b| {
        b.0.partial_cmp(&a.0)
            .unwrap_or(Ordering::Equal)
            .then(a.1.cmp(&b.1))
    });

    let poly_style = &tab.style.polyhedra_style;
    for (_, _, item) in &order {
        match item {
            Item::Face(i) => {
                if let Some(plan) = &plan {
                    let f = &plan.faces[*i];
                    polyhedra_lighting::draw_shaded_face(
                        cr,
                        &polyhedra_lighting::FaceDraw {
                            screen_verts: &f.sv,
                            cart_verts: f.cv,
                            poly_center_cart: f.center_cart,
                            color: f.color,
                            alpha: f.alpha,
                            edge_flags: f.edges,
                            style: poly_style,
                        },
                    );
                }
            }
            Item::Bond(i) => {
                let bond = &render_bonds[*i];
                draw_cylinder_impostor(
                    cr,
                    bond.start,
                    bond.end,
                    bond.radius,
                    tab.style.bond_color,
                    tab.style.metallic,
                    tab.style.roughness,
                    tab.style.transmission,
                );
            }
            Item::Atom(i) => draw_atom(render_atoms[*i].1),
        }
    }

    // Colour scale for property-coloured polyhedra, over everything else.
    if let Some(legend) = plan.as_ref().and_then(|p| p.legend.as_ref()) {
        draw_poly_legend(cr, legend, tab.style.background_color);
    }
}

// ============================================================================
// COORDINATE AXES DRAWING
// ============================================================================

pub fn draw_axes(cr: &cairo::Context, tab: &TabState, width: f64, height: f64) {
    let hud_size = (width * 0.12).clamp(60.0, 150.0);
    let hud_cx = hud_size * 0.6;
    let hud_cy = height - hud_size * 0.6;

    let rotation_matrix = tab.view.rotation_matrix();

    let rotate_vec = |v: [f64; 3]| -> [f64; 3] {
        let r = rotation_matrix * nalgebra::Vector3::new(v[0], v[1], v[2]);
        [r.x, r.y, r.z]
    };

    let axes_data = [
        ([1.0, 0.0, 0.0], (0.85, 0.2, 0.2), tab.view.show_axes[0]), // X Red
        ([0.0, 1.0, 0.0], (0.2, 0.7, 0.2), tab.view.show_axes[1]),  // Y Green
        ([0.0, 0.0, 1.0], (0.2, 0.4, 0.85), tab.view.show_axes[2]), // Z Blue
    ];

    let mut sorted_axes: Vec<_> = axes_data
        .iter()
        .map(|(v, c, show)| (rotate_vec(*v), c, show))
        .collect();

    // Sort by depth (NaN-safe)
    sorted_axes.sort_by(|(a, _, _), (b, _, _)| b[2].partial_cmp(&a[2]).unwrap_or(Ordering::Equal));

    let shaft_radius = 2.5;
    let head_radius = 6.0;
    let head_length = 16.0;
    let axis_length = hud_size;

    for (r, color, show) in sorted_axes {
        if !*show {
            continue;
        }

        let dx = r[0] * axis_length;
        let dy = -r[1] * axis_length;

        let len_sq = dx * dx + dy * dy;
        if len_sq < 1.0 {
            continue;
        }
        let len = len_sq.sqrt();

        let nx = -dy / len;
        let ny = dx / len;

        let start_x = hud_cx;
        let start_y = hud_cy;
        let end_x = hud_cx + dx;
        let end_y = hud_cy + dy;
        let shaft_end_x = end_x - (dx / len) * head_length;
        let shaft_end_y = end_y - (dy / len) * head_length;

        // Gradient for depth
        let grad_start_x = start_x - nx * head_radius;
        let grad_start_y = start_y - ny * head_radius;
        let grad_end_x = start_x + nx * head_radius;
        let grad_end_y = start_y + ny * head_radius;

        let gradient =
            cairo::LinearGradient::new(grad_start_x, grad_start_y, grad_end_x, grad_end_y);
        let (cr_r, cr_g, cr_b) = *color;

        gradient.add_color_stop_rgb(0.0, cr_r * 0.4, cr_g * 0.4, cr_b * 0.4);
        gradient.add_color_stop_rgb(0.35, cr_r, cr_g, cr_b);
        gradient.add_color_stop_rgb(0.5, cr_r * 1.3, cr_g * 1.3, cr_b * 1.3);
        gradient.add_color_stop_rgb(0.65, cr_r, cr_g, cr_b);
        gradient.add_color_stop_rgb(1.0, cr_r * 0.3, cr_g * 0.3, cr_b * 0.3);

        cr.set_source(&gradient)
            .expect("Failed to set gradient source for axis");

        // Draw shaft
        cr.move_to(start_x - nx * shaft_radius, start_y - ny * shaft_radius);
        cr.line_to(
            shaft_end_x - nx * shaft_radius,
            shaft_end_y - ny * shaft_radius,
        );
        cr.line_to(
            shaft_end_x + nx * shaft_radius,
            shaft_end_y + ny * shaft_radius,
        );
        cr.line_to(start_x + nx * shaft_radius, start_y + ny * shaft_radius);
        cr.close_path();
        cr.fill().expect("Failed to fill axis shaft");

        // Draw arrow head
        cr.move_to(end_x, end_y);
        cr.line_to(
            shaft_end_x + nx * head_radius,
            shaft_end_y + ny * head_radius,
        );
        cr.line_to(
            shaft_end_x - nx * head_radius,
            shaft_end_y - ny * head_radius,
        );
        cr.close_path();
        cr.fill().expect("Failed to fill axis arrow head");
    }

    // Draw central hub
    let origin_grad =
        cairo::RadialGradient::new(hud_cx - 2.0, hud_cy - 2.0, 0.0, hud_cx, hud_cy, 6.0);
    origin_grad.add_color_stop_rgb(0.0, 1.0, 1.0, 1.0);
    origin_grad.add_color_stop_rgb(1.0, 0.2, 0.2, 0.2);
    cr.set_source(&origin_grad)
        .expect("Failed to set gradient source for axis hub");
    cr.arc(hud_cx, hud_cy, 5.0, 0.0, 2.0 * PI);
    cr.fill().expect("Failed to fill axis hub");
}

// ============================================================================
// MILLER PLANES DRAWING
// ============================================================================

pub fn draw_miller_planes(
    cr: &cairo::Context,
    tab: &TabState,
    lattice_corners: &[[f64; 2]],
    _scale: f64,
    _width: f64,
    _height: f64,
) {
    if lattice_corners.len() < 5 {
        return;
    }

    // Unit cell box vectors on screen
    let p_origin = lattice_corners[0];
    let p_x_vec = [
        lattice_corners[4][0] - p_origin[0],
        lattice_corners[4][1] - p_origin[1],
    ];
    let p_y_vec = [
        lattice_corners[2][0] - p_origin[0],
        lattice_corners[2][1] - p_origin[1],
    ];
    let p_z_vec = [
        lattice_corners[1][0] - p_origin[0],
        lattice_corners[1][1] - p_origin[1],
    ];

    for plane in &tab.miller_planes {
        // Calculate intersection polygon
        let math = MillerMath::new(plane.h, plane.k, plane.l);
        let poly_3d = math.get_intersection_polygon();

        if poly_3d.len() < 3 {
            continue;
        }

        // Map 3D fractional → 2D screen coordinates
        let poly_points: Vec<[f64; 2]> = poly_3d
            .iter()
            .map(|p| {
                let u = p[0];
                let v = p[1];
                let w = p[2];

                let sx = p_origin[0] + u * p_x_vec[0] + v * p_y_vec[0] + w * p_z_vec[0];
                let sy = p_origin[1] + u * p_x_vec[1] + v * p_y_vec[1] + w * p_z_vec[1];
                [sx, sy]
            })
            .collect();

        // Draw filled plane
        cr.set_source_rgba(0.0, 0.5, 1.0, 0.4);
        cr.move_to(poly_points[0][0], poly_points[0][1]);
        for p in poly_points.iter().skip(1) {
            cr.line_to(p[0], p[1]);
        }
        cr.close_path();
        cr.fill_preserve().expect("Failed to fill Miller plane");

        // Draw outline
        cr.set_source_rgba(0.0, 0.2, 0.8, 0.8);
        cr.set_line_width(2.0);
        cr.stroke().expect("Failed to stroke Miller plane outline");
    }
}

// ============================================================================
// SELECTION BOX DRAWING
// ============================================================================

pub fn draw_selection_box(cr: &cairo::Context, tab: &TabState) {
    if let Some(((start_x, start_y), (curr_x, curr_y))) = tab.interaction.selection_box {
        let width = curr_x - start_x;
        let height = curr_y - start_y;

        cr.rectangle(start_x, start_y, width, height);

        // Semi-transparent fill
        cr.set_source_rgba(0.0, 0.5, 1.0, 0.2);
        cr.fill_preserve().expect("Failed to fill selection box");

        // Dashed border
        cr.set_source_rgb(0.0, 0.5, 1.0);
        cr.set_line_width(1.0);
        cr.set_dash(&[4.0, 4.0], 0.0);
        cr.stroke().expect("Failed to stroke selection box border");

        // Reset dash
        cr.set_dash(&[], 0.0);
    }
}

// ============================================================================
// MEASUREMENT OVERLAY
// ============================================================================

const MEASURE_RGB: (f64, f64, f64) = (0.95, 0.45, 0.05);
const MIN_IMAGE_RGB: (f64, f64, f64) = (0.55, 0.30, 0.85);

/// Lines and values for the 2–4 atoms picked for measurement, in pick
/// order: distances on each segment, the angle at every interior vertex,
/// and the dihedral beside the central bond. For a pair in a periodic cell
/// whose nearest periodic separation is to a different image, a dashed line
/// runs to that image as well.
///
/// Drawn over the atoms rather than depth-sorted among them: a measurement
/// hidden behind the spheres it measures would be useless.
pub fn draw_measurements(
    cr: &cairo::Context,
    tab: &TabState,
    bounds: &crate::rendering::scene::SceneBounds,
) {
    use crate::utils::geometry;

    let picked = tab.interaction.selected_in_order();
    if !(2..=4).contains(&picked.len()) {
        return;
    }
    let cart: Vec<[f64; 3]> = picked.iter().map(|a| a.cart_pos).collect();
    let scr: Vec<[f64; 3]> = cart.iter().map(|&p| bounds.project(p)).collect();

    cr.save().ok();
    cr.set_line_cap(cairo::LineCap::Round);
    // Tags already placed, so later ones can step clear of them.
    let mut placed: Vec<[f64; 4]> = Vec::new();

    // Chain A–B(–C(–D)).
    cr.set_source_rgba(MEASURE_RGB.0, MEASURE_RGB.1, MEASURE_RGB.2, 0.95);
    cr.set_line_width(2.0);
    cr.move_to(scr[0][0], scr[0][1]);
    for p in &scr[1..] {
        cr.line_to(p[0], p[1]);
    }
    cr.stroke().ok();

    // Nearest periodic image of B, when a different one is closer to A.
    if cart.len() == 2 {
        let lattice = tab
            .structure
            .as_ref()
            .filter(|s| s.is_periodic)
            .map(|s| s.lattice);
        if let Some((image, _)) =
            lattice.and_then(|lat| geometry::closer_periodic_image(cart[0], cart[1], &lat))
        {
            let img = bounds.project(image);
            cr.set_source_rgba(MIN_IMAGE_RGB.0, MIN_IMAGE_RGB.1, MIN_IMAGE_RGB.2, 0.95);
            cr.set_line_width(2.0);
            cr.set_dash(&[6.0, 4.0], 0.0);
            cr.move_to(scr[0][0], scr[0][1]);
            cr.line_to(img[0], img[1]);
            cr.stroke().ok();
            cr.set_dash(&[], 0.0);
            // Hollow marker: the image may not be drawn as an atom.
            cr.arc(img[0], img[1], 6.0, 0.0, 2.0 * PI);
            cr.stroke().ok();

            let d = geometry::calculate_distance(cart[0], image);
            draw_measure_tag(
                cr,
                (scr[0][0] + img[0]) / 2.0,
                (scr[0][1] + img[1]) / 2.0,
                &format!("{d:.3} Å (periodic)"),
                MIN_IMAGE_RGB,
                &mut placed,
            );
        }
    }

    // Angle arcs at each interior vertex.
    for v in 1..scr.len() - 1 {
        let (a, b, c) = (scr[v - 1], scr[v], scr[v + 1]);
        let a1 = (a[1] - b[1]).atan2(a[0] - b[0]);
        let a2 = (c[1] - b[1]).atan2(c[0] - b[0]);
        // Shorter way round from BA to BC on screen.
        let mut sweep = a2 - a1;
        if sweep > PI {
            sweep -= 2.0 * PI;
        } else if sweep < -PI {
            sweep += 2.0 * PI;
        }
        let r = 18.0;
        cr.set_source_rgba(MEASURE_RGB.0, MEASURE_RGB.1, MEASURE_RGB.2, 0.95);
        cr.set_line_width(1.5);
        cr.new_sub_path();
        if sweep >= 0.0 {
            cr.arc(b[0], b[1], r, a1, a1 + sweep);
        } else {
            cr.arc_negative(b[0], b[1], r, a1, a1 + sweep);
        }
        cr.stroke().ok();

        let mid = a1 + sweep / 2.0;
        let angle = geometry::calculate_angle(cart[v - 1], cart[v], cart[v + 1]);
        draw_measure_tag(
            cr,
            b[0] + (r + 16.0) * mid.cos(),
            b[1] + (r + 16.0) * mid.sin(),
            &format!("{angle:.1}°"),
            MEASURE_RGB,
            &mut placed,
        );
    }

    // Segment lengths; the central bond of a dihedral also carries φ.
    for s in 0..scr.len() - 1 {
        let d = geometry::calculate_distance(cart[s], cart[s + 1]);
        let mut text = format!("{d:.3} Å");
        if scr.len() == 4 && s == 1 {
            let phi = geometry::calculate_dihedral(cart[0], cart[1], cart[2], cart[3]);
            text.push_str(&format!("  φ {phi:.1}°"));
        }
        draw_measure_tag(
            cr,
            (scr[s][0] + scr[s + 1][0]) / 2.0,
            (scr[s][1] + scr[s + 1][1]) / 2.0,
            &text,
            MEASURE_RGB,
            &mut placed,
        );
    }

    cr.restore().ok();
}

/// A value label centred on (x, y): dark text on a pale rounded box with an
/// accent border, legible on both light and dark backgrounds.
///
/// A segment that is short on screen (seen nearly end-on) puts its length
/// right on top of the angle beside it, so a tag that would overlap one in
/// `placed` is stepped down, then up, until it is clear.
fn draw_measure_tag(
    cr: &cairo::Context,
    x: f64,
    y: f64,
    text: &str,
    accent: (f64, f64, f64),
    placed: &mut Vec<[f64; 4]>,
) {
    cr.select_font_face("Sans", cairo::FontSlant::Normal, cairo::FontWeight::Bold);
    cr.set_font_size(12.0);
    let Ok(ext) = cr.text_extents(text) else {
        return;
    };
    let (pad_x, pad_y) = (5.0, 3.0);
    let w = ext.width() + 2.0 * pad_x;
    let h = ext.height() + 2.0 * pad_y;
    let left = x - w / 2.0;
    let overlaps = |top: f64| {
        placed
            .iter()
            .any(|p| left < p[0] + p[2] && p[0] < left + w && top < p[1] + p[3] && p[1] < top + h)
    };
    let step = h + 2.0;
    let top = (0..8)
        .map(|i| {
            // 0, +1, -1, +2, -2, ... steps from the requested position.
            let k = ((i + 1) / 2) as f64 * if i % 2 == 1 { 1.0 } else { -1.0 };
            y - h / 2.0 + k * step
        })
        .find(|&t| !overlaps(t))
        .unwrap_or(y - h / 2.0);
    placed.push([left, top, w, h]);
    let r = 4.0;

    cr.new_sub_path();
    cr.arc(left + w - r, top + r, r, -PI / 2.0, 0.0);
    cr.arc(left + w - r, top + h - r, r, 0.0, PI / 2.0);
    cr.arc(left + r, top + h - r, r, PI / 2.0, PI);
    cr.arc(left + r, top + r, r, PI, 1.5 * PI);
    cr.close_path();
    cr.set_source_rgba(1.0, 1.0, 1.0, 0.88);
    cr.fill_preserve().ok();
    cr.set_source_rgba(accent.0, accent.1, accent.2, 0.95);
    cr.set_line_width(1.0);
    cr.stroke().ok();

    cr.set_source_rgb(0.1, 0.1, 0.1);
    cr.move_to(left + pad_x - ext.x_bearing(), top + pad_y - ext.y_bearing());
    cr.show_text(text).ok();
}

// ============================================================================
// INTERSTITIAL SITE OVERLAY
// ============================================================================

/// Draw the interstitial sites found for the tab's candidate ion.
///
/// Sites are drawn at their true fitted radius, not a fixed marker size — the
/// whole point of the screen is how much room there is, so a site that barely
/// admits the ion must look tighter than one with space to spare. They are
/// laid over the atoms with alpha rather than depth-interleaved: an
/// interstitial is by definition surrounded by framework atoms, so
/// depth-sorting would bury most of them behind the very atoms that define
/// them.
///
/// Sites are periodic, so each is drawn once per cell corner it is near,
/// matching the ghost-atom convention for atoms on a boundary.
pub fn draw_interstitial_sites(
    cr: &cairo::Context,
    tab: &TabState,
    bounds: &crate::rendering::scene::SceneBounds,
) {
    let overlay = match &tab.interstitial {
        Some(o) if tab.view.show_interstitial_sites => o,
        _ => return,
    };

    let structure = match &tab.structure {
        Some(s) => s,
        None => return,
    };

    let lat = structure.lattice;
    let show_ghosts = tab.view.show_full_unit_cell;
    let tol = 0.05;

    // Sites nearest the viewer last, so overlapping markers stack the way the
    // atoms behind them do.
    let mut drawn: Vec<([f64; 3], f64)> = Vec::new();

    for site in &overlay.sites {
        let shifts: &[f64] = if show_ghosts {
            &[-1.0, 0.0, 1.0]
        } else {
            &[0.0]
        };

        for &sx in shifts {
            for &sy in shifts {
                for &sz in shifts {
                    let f = [
                        site.frac[0] + sx,
                        site.frac[1] + sy,
                        site.frac[2] + sz,
                    ];

                    if f.iter().any(|&v| v < -tol || v > 1.0 + tol) {
                        continue;
                    }

                    let cart = [
                        f[0] * lat[0][0] + f[1] * lat[1][0] + f[2] * lat[2][0],
                        f[0] * lat[0][1] + f[1] * lat[1][1] + f[2] * lat[2][1],
                        f[0] * lat[0][2] + f[1] * lat[1][2] + f[2] * lat[2][2],
                    ];

                    drawn.push((bounds.project(cart), site.radius));
                }
            }
        }
    }

    drawn.sort_by(|a, b| {
        a.0[2]
            .partial_cmp(&b.0[2])
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    for (screen, radius) in drawn {
        let r_px = (radius * bounds.scale).max(2.0);

        // Teal, distinct from both the element palette and the selection blue.
        cr.arc(screen[0], screen[1], r_px, 0.0, std::f64::consts::PI * 2.0);
        cr.set_source_rgba(0.0, 0.72, 0.66, 0.30);
        cr.fill_preserve().expect("Failed to fill interstitial site");

        cr.set_source_rgba(0.0, 0.55, 0.51, 0.85);
        cr.set_line_width(1.5);
        cr.stroke().expect("Failed to stroke interstitial site");

        // A dot at the centre keeps a tightly-fitting site visible when its
        // circle has shrunk to almost nothing.
        cr.arc(screen[0], screen[1], 1.5, 0.0, std::f64::consts::PI * 2.0);
        cr.set_source_rgba(0.0, 0.45, 0.42, 0.95);
        cr.fill().expect("Failed to fill interstitial centre");
    }
}
