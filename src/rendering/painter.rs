// src/rendering/painter.rs
// Publication-quality vector exports + optimized screen rendering
// Draw order: Polyhedra (background) → Bonds → Atoms (foreground)
// All unwraps eliminated, NaN-safe

use super::primitives::*;
use super::scene::RenderAtom;
use crate::config::ColorMode;
use crate::model::elements::{ColorScheme, get_element_color};
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
    // Which pairs bond: the tab's per-element-pair rules.
    let bonds = tab.bond_table();

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
    // the longest bond any rule allows, so each query visits a 3×3×3 block.
    // Atoms filtered out at grid build time are never returned as neighbors,
    // so the inner loop doesn't need to re-check is_coord_only / is_ghost.
    // ========================================================================
    if tab.view.show_bonds && bonds.max_length() > 0.0 {
        let max_bond_dist = bonds.max_length();

        let grid = SpatialGrid::build(atoms, max_bond_dist, |a| {
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
            neighbors.clear();
            grid.query(r1.cart_pos, max_bond_dist, &mut neighbors);

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

                if bonds.bonded(&r1.element, &r2.element, dist) {
                    let raw_r1 = tab.base_radius(&r1.element);
                    let raw_r2 = tab.base_radius(&r2.element);

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

    // Magnetic moments: an arrow through each drawn atom that has one, by
    // index into `render_atoms`.
    let rotation = tab.view.rotation_matrix();
    let moment_axis = tab.structure.as_ref().and_then(|s| s.magnetic_axis());
    let collinear = tab.structure.as_ref().map_or(true, |s| s.moments_collinear());
    let arrows_only = tab.style.moment_arrows_only;
    // An OVF mesh: arrows thinned to `moment_stride`, and optionally a
    // colour map of cell tiles under them. With the map on, only the layer
    // facing the viewer is drawn (tiles and arrows), as the depth test does
    // in the Trajectory Player.
    let ovf_grid = tab.mesh_grid();
    let is_ovf = ovf_grid.is_some();
    let ovf_cmap = tab.style.moment_color_map && is_ovf && tab.style.show_moments;
    let ovf_stride = if is_ovf { tab.style.moment_stride.max(1) } else { 1 };
    let surface = ovf_grid.as_ref().filter(|_| ovf_cmap).map(|g| {
        let mut n = nalgebra::Vector3::zeros();
        n[g.normal] = 1.0;
        // Larger view z is farther: +normal pointing away puts the viewer
        // on the − side.
        g.surface_layer((rotation * n).z <= 0.0)
    });
    let on_surface = |k: usize| match (&ovf_grid, surface) {
        (Some(g), Some(layer)) => g.idx.get(k).is_some_and(|i| i[g.normal] == layer),
        _ => true,
    };
    let arrows: Vec<Option<MomentArrow>> = render_atoms
        .iter()
        .map(|(_, atom)| {
            if !tab.style.show_moments {
                return None;
            }
            if let Some(g) = &ovf_grid {
                if !g.on_pitch(atom.original_index, ovf_stride) || !on_surface(atom.original_index) {
                    return None;
                }
            }
            let m = tab.structure.as_ref()?.atoms.get(atom.original_index)?.magmom?;
            // Arrows only: no sphere to stick out of, so the arrow is its
            // full length.
            let radius = if arrows_only {
                0.0
            } else {
                tab.base_radius(&atom.element)
                    * tab.style.atom_scale
                    * tab.override_radius_scale(atom.original_index)
                    * scale
            };
            let color = moment_color(m, moment_axis, collinear, &tab.style);
            let grow = (ovf_stride as f64).powf(0.75);
            MomentArrow::new(m, &rotation, atom.screen_pos, radius, scale, color, tab, is_ovf, grow)
        })
        .collect();

    let mut draw_atom = |atom: &RenderAtom, arrow: Option<&MomentArrow>| {
        let raw_r = tab.base_radius(&atom.element);
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

        // The half of the moment arrow pointing away from the viewer goes
        // under the sphere; the near half is its own item in the depth sort.
        if let Some(a) = arrow {
            a.draw(cr, false);
        }

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

        // Arrows only: the arrow (or colour-map tile) stands for the atom.
        if arrows_only && (arrow.is_some() || ovf_cmap) {
            return;
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
        Arrow(usize),
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
    // The near half of a moment arrow sorts at its tip, so a bond running
    // the same way (a moment along a bond, as in rock salt) does not cover it.
    for (i, a) in arrows.iter().enumerate() {
        if let Some(depth) = a.as_ref().and_then(|a| a.front_depth(render_atoms[i].1.screen_pos[2], scale)) {
            order.push((depth, 3, Item::Arrow(i)));
        }
    }
    order.sort_by(|a, b| {
        b.0.partial_cmp(&a.0)
            .unwrap_or(Ordering::Equal)
            .then(a.1.cmp(&b.1))
    });

    // Colour-map tiles first, under everything: drawn among the atoms, a
    // nearer cell's tile would cover the part of a farther arrow standing
    // out of the film in a tilted view. One layer, so they never overlap.
    if let (true, Some(g)) = (ovf_cmap, &ovf_grid) {
        let [e1, e2] = g.tile_half_edges().map(|e| {
            let r = rotation * nalgebra::Vector3::new(e[0], e[1], e[2]) * scale;
            [r.x, r.y]
        });
        let bg = tab.style.background_color;
        let atoms = tab.structure.as_ref().map(|s| s.atoms.as_slice()).unwrap_or(&[]);
        for (_, atom) in render_atoms.iter() {
            let Some(m) = atoms.get(atom.original_index).and_then(|a| a.magmom) else { continue };
            if (m[0] * m[0] + m[1] * m[1] + m[2] * m[2]).sqrt() < 0.01 || !on_surface(atom.original_index) {
                continue;
            }
            let c = moment_color(m, moment_axis, collinear, &tab.style);
            let fade = MOMENT_MAP_FADE;
            cr.set_source_rgb(c.0 + (bg.0 - c.0) * fade, c.1 + (bg.1 - c.1) * fade, c.2 + (bg.2 - c.2) * fade);
            let (cx, cy) = (atom.screen_pos[0], atom.screen_pos[1]);
            cr.move_to(cx - e1[0] - e2[0], cy - e1[1] - e2[1]);
            cr.line_to(cx + e1[0] - e2[0], cy + e1[1] - e2[1]);
            cr.line_to(cx + e1[0] + e2[0], cy + e1[1] + e2[1]);
            cr.line_to(cx - e1[0] + e2[0], cy - e1[1] + e2[1]);
            cr.close_path();
            cr.fill().ok();
        }
    }

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
            Item::Atom(i) => draw_atom(render_atoms[*i].1, arrows[*i].as_ref()),
            Item::Arrow(i) => {
                if let Some(a) = &arrows[*i] {
                    a.draw(cr, true);
                }
            }
        }
    }

    // Colour scale for property-coloured polyhedra, over everything else.
    if let Some(legend) = plan.as_ref().and_then(|p| p.legend.as_ref()) {
        draw_poly_legend(cr, legend, tab.style.background_color);
    } else if tab.style.show_moments
        && tab.structure.as_ref().is_some_and(|s| s.atoms.iter().any(|a| a.magmom.is_some()))
    {
        draw_moment_legend(cr, tab, collinear);
    }
}

/// Colour legend for the main view's moment arrows (bottom-right), when
/// they are coloured by direction or m_z.
pub fn draw_moment_legend(cr: &cairo::Context, tab: &TabState, collinear: bool) {
    use crate::config::MomentColorMode;
    let mode = match tab.style.moment_color_mode {
        MomentColorMode::Auto if collinear => MomentColorMode::UpDown,
        MomentColorMode::Auto => MomentColorMode::Direction,
        m => m,
    };
    let Ok((x0, y0, x1, y1)) = cr.clip_extents() else { return };
    let bg = tab.style.background_color;
    let lum = 0.299 * bg.0 + 0.587 * bg.1 + 0.114 * bg.2;
    let ink = if lum > 0.5 { (0.12, 0.12, 0.14) } else { (0.92, 0.92, 0.94) };
    draw_moment_color_legend(cr, x0, y0, x1, y1, mode, ink);
}

/// The legend of a moment colour `mode` in the bottom-right corner of the
/// box (x0, y0)–(x1, y1): a bar for m_z scales; for Direction, a wheel for
/// the in-plane angle (centre +z, rim in-plane) and a bar for m_z. Every
/// swatch comes from `moment_color`, so the legend is the arrows' scale by
/// construction. Shared by the main view, its exports and the Trajectory
/// Player. Nothing for Up / Down and Single, or in a box too small for it.
pub fn draw_moment_color_legend(
    cr: &cairo::Context,
    x0: f64,
    y0: f64,
    x1: f64,
    y1: f64,
    mode: crate::config::MomentColorMode,
    ink: (f64, f64, f64),
) {
    use crate::config::MomentColorMode;
    if !matches!(mode, MomentColorMode::Direction | MomentColorMode::MzRainbow | MomentColorMode::Mz)
        || x1 - x0 < 180.0
        || y1 - y0 < 120.0
    {
        return;
    }
    let style = crate::config::RenderStyle { moment_color_mode: mode, ..Default::default() };
    let color = |m: [f64; 3]| moment_color(m, None, false, &style);
    let margin = 16.0;
    cr.save().ok();
    cr.select_font_face("Sans", cairo::FontSlant::Normal, cairo::FontWeight::Normal);
    cr.set_font_size(11.0);

    if mode != MomentColorMode::Direction {
        // Horizontal m_z bar, −1 … +1.
        let (bar_w, bar_h) = (130.0, 10.0);
        let bx = x1 - margin - bar_w;
        let by = y1 - margin - 16.0 - bar_h;
        cr.set_source_rgb(ink.0, ink.1, ink.2);
        cr.move_to(bx, by - 6.0);
        let _ = cr.show_text("m_z");
        let steps = 64;
        for i in 0..steps {
            let mz = -1.0 + 2.0 * (i as f64 / (steps - 1) as f64);
            let (r, g, b) = color([(1.0 - mz * mz).max(0.0).sqrt(), 0.0, mz]);
            cr.set_source_rgb(r, g, b);
            cr.rectangle(bx + bar_w * i as f64 / steps as f64, by, bar_w / steps as f64 + 0.5, bar_h);
            let _ = cr.fill();
        }
        cr.set_source_rgba(ink.0, ink.1, ink.2, 0.6);
        cr.set_line_width(0.8);
        cr.rectangle(bx, by, bar_w, bar_h);
        let _ = cr.stroke();
        cr.set_source_rgb(ink.0, ink.1, ink.2);
        for (frac, lbl) in [(0.0, "−1"), (0.5, "0"), (1.0, "+1")] {
            let tw = cr.text_extents(lbl).map(|e| e.width()).unwrap_or(0.0);
            cr.move_to(bx + bar_w * frac - tw * frac, by + bar_h + 12.0);
            let _ = cr.show_text(lbl);
        }
        cr.restore().ok();
        return;
    }

    let r_wheel = 24.0;
    let bar_w = 9.0;
    let bar_h = 2.0 * r_wheel;
    let bx = x1 - margin - 18.0 - bar_w;
    let by = y1 - margin - bar_h - 6.0;
    let cx = bx - 18.0 - r_wheel;
    let cy = by + r_wheel;

    // Wheel: the upper hemisphere seen from +z (centre +z, rim in-plane).
    // Screen y points down, so world +y is up: m_y = −sin of the screen angle.
    let (n_ang, n_rad) = (48, 6);
    for ir in 0..n_rad {
        let r0 = r_wheel * (ir as f64 / n_rad as f64);
        let r1 = r_wheel * ((ir + 1) as f64 / n_rad as f64) + 0.4;
        let rho = (ir as f64 + 0.5) / n_rad as f64;
        let mz = (1.0 - rho * rho).max(0.0).sqrt();
        for ia in 0..n_ang {
            let a0 = (ia as f64 / n_ang as f64) * 2.0 * PI;
            let a1 = ((ia + 1) as f64 / n_ang as f64) * 2.0 * PI + 0.03;
            let am = 0.5 * (a0 + a1);
            let (r, g, b) = color([rho * am.cos(), -rho * am.sin(), mz]);
            cr.set_source_rgb(r, g, b);
            cr.new_sub_path();
            cr.arc(cx, cy, r1, a0, a1);
            cr.arc_negative(cx, cy, r0, a1, a0);
            cr.close_path();
            let _ = cr.fill();
        }
    }
    cr.set_source_rgba(ink.0, ink.1, ink.2, 0.65);
    cr.set_line_width(0.9);
    cr.arc(cx, cy, r_wheel, 0.0, 2.0 * PI);
    let _ = cr.stroke();

    cr.set_font_size(10.0);
    cr.set_source_rgb(ink.0, ink.1, ink.2);
    let tw = cr.text_extents("m_xy").map(|e| e.width()).unwrap_or(20.0);
    cr.move_to(cx - 0.5 * tw, by - 5.0);
    let _ = cr.show_text("m_xy");

    // m_z bar, +1 at the top.
    let steps = 32;
    for i in 0..steps {
        let mz = 1.0 - 2.0 * (i as f64 / (steps - 1) as f64);
        let (r, g, b) = color([0.0, 0.0, mz]);
        cr.set_source_rgb(r, g, b);
        cr.rectangle(bx, by + bar_h * (i as f64 / steps as f64), bar_w, bar_h / steps as f64 + 0.5);
        let _ = cr.fill();
    }
    cr.set_source_rgba(ink.0, ink.1, ink.2, 0.65);
    cr.set_line_width(0.8);
    cr.rectangle(bx, by, bar_w, bar_h);
    let _ = cr.stroke();
    cr.set_source_rgb(ink.0, ink.1, ink.2);
    cr.move_to(bx - 2.0, by - 5.0);
    let _ = cr.show_text("m_z");
    cr.move_to(bx + bar_w + 3.0, by + 8.0);
    let _ = cr.show_text("+1");
    cr.move_to(bx + bar_w + 3.0, by + bar_h);
    let _ = cr.show_text("−1");
    cr.restore().ok();
}

// ============================================================================
// MAGNETIC MOMENT ARROWS

/// How far colour-map tiles under mesh arrows are faded towards the
/// background, so the arrows on top, in the same colours, stand out.
/// Shared with the Trajectory Player.
pub(crate) const MOMENT_MAP_FADE: f64 = 0.3;
// ============================================================================

/// Diverging Blue–White–Red scale matching the CHGCAR 2D plotter
/// (`Colormap::BlueWhiteRed`): pure blue at `mz = -1`, crisp white at `0`,
/// pure red at `+1`.
pub(crate) fn mz_color(mz: f64) -> (f64, f64, f64) {
    let t = 0.5 * (mz.clamp(-1.0, 1.0) + 1.0);
    crate::rendering::colormap::colormap_rgb(crate::rendering::colormap::Colormap::BlueWhiteRed, t)
}

/// Rainbow (Blue → Cyan → Green → Yellow → Orange → Red) scale for `mz` in
/// `[-1, 1]`, matching Nature / npj Computational Materials skyrmion figures.
pub(crate) fn mz_rainbow_color(mz: f64) -> (f64, f64, f64) {
    let t = 0.5 * (mz.clamp(-1.0, 1.0) + 1.0);
    const STOPS: &[(f64, (f64, f64, f64))] = &[
        (0.00, (0.10, 0.20, 0.88)), // -1.0: Deep royal blue
        (0.20, (0.12, 0.68, 0.96)), // -0.6: Vivid cyan-blue
        (0.40, (0.18, 0.82, 0.45)), // -0.2: Emerald green
        (0.60, (0.78, 0.86, 0.14)), // +0.2: Chartreuse / yellow-green
        (0.80, (0.96, 0.58, 0.10)), // +0.6: Warm orange
        (1.00, (0.84, 0.12, 0.10)), // +1.0: Deep crimson red
    ];
    for i in 1..STOPS.len() {
        let (t0, c0) = STOPS[i - 1];
        let (t1, c1) = STOPS[i];
        if t <= t1 {
            let u = ((t - t0) / (t1 - t0)).clamp(0.0, 1.0);
            return (
                c0.0 + u * (c1.0 - c0.0),
                c0.1 + u * (c1.1 - c0.1),
                c0.2 + u * (c1.2 - c0.2),
            );
        }
    }
    STOPS[STOPS.len() - 1].1
}

/// Arrow colour for moment `m` under the tab's colour mode. `axis` and
/// `collinear` describe the whole structure (`Structure::magnetic_axis`,
/// `moments_collinear`); Auto uses them to choose Up/Down or Direction.
pub(crate) fn moment_color(
    m: [f64; 3],
    axis: Option<[f64; 3]>,
    collinear: bool,
    style: &crate::config::RenderStyle,
) -> (f64, f64, f64) {
    use crate::config::MomentColorMode;
    let mode = match style.moment_color_mode {
        MomentColorMode::Auto if collinear => MomentColorMode::UpDown,
        MomentColorMode::Auto => MomentColorMode::Direction,
        m => m,
    };
    match mode {
        MomentColorMode::Single | MomentColorMode::Auto => style.moment_color,
        MomentColorMode::UpDown => {
            let a = axis.unwrap_or([0.0, 0.0, 1.0]);
            // A tiny tolerance so a moment exactly perpendicular to the
            // axis counts as up rather than flickering with rounding.
            if m[0] * a[0] + m[1] * a[1] + m[2] * a[2] >= -1e-6 {
                style.moment_color
            } else {
                style.moment_color_down
            }
        }
        MomentColorMode::MzRainbow => {
            let n = (m[0] * m[0] + m[1] * m[1] + m[2] * m[2]).sqrt().max(1e-12);
            mz_rainbow_color(m[2] / n)
        }
        MomentColorMode::Mz => {
            let n = (m[0] * m[0] + m[1] * m[1] + m[2] * m[2]).sqrt().max(1e-12);
            mz_color(m[2] / n)
        }
        MomentColorMode::Direction => {
            let n = (m[0] * m[0] + m[1] * m[1] + m[2] * m[2]).sqrt().max(1e-12);
            let mz = (m[2] / n).clamp(-1.0, 1.0);
            let xy = (m[0] * m[0] + m[1] * m[1]).sqrt() / n;
            let hue = m[1].atan2(m[0]).to_degrees().rem_euclid(360.0);
            // White at +z, charcoal at −z; saturation scales with in-plane
            // projection `xy` so collinear ±z backgrounds stay achromatic
            // instead of picking up a random hue from atan2(0, 0) floating-point noise.
            let lightness = 0.54 + 0.41 * mz;
            let saturation = 0.88 * xy.clamp(0.0, 1.0).powf(0.35);
            hsl_to_rgb(hue, saturation, lightness)
        }
    }
}

fn hsl_to_rgb(h: f64, s: f64, l: f64) -> (f64, f64, f64) {
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let hp = h / 60.0;
    let x = c * (1.0 - (hp.rem_euclid(2.0) - 1.0).abs());
    let (r, g, b) = match hp as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = l - c / 2.0;
    (r + m, g + m, b + m)
}

/// One half of a moment arrow: from where it leaves the sphere to its end,
/// in screen pixels along a direction `dir` (unit, rotated frame).
struct ArrowHalf {
    dir: [f64; 3],
    /// Distance from the atom centre to the end, px (3D, before projection).
    length: f64,
    is_tip: bool,
}

/// An arrow through an atom's centre along its magnetic moment. Each end
/// sticks out of the sphere by ½·|m|·`moment_scale` Å, so the visible length
/// is |m|·`moment_scale` whatever the atom size, and a small induced moment
/// still shows.
struct MomentArrow {
    centre: [f64; 2],
    radius: f64,
    halves: [ArrowHalf; 2],
    shaft_r: f64,
    head_r: f64,
    head_len: f64,
    color: (f64, f64, f64),
    metallic: f64,
    roughness: f64,
}

impl MomentArrow {
    #[allow(clippy::too_many_arguments)]
    fn new(
        m: [f64; 3],
        rotation: &nalgebra::Rotation3<f64>,
        screen_pos: [f64; 3],
        radius: f64,
        scale: f64,
        color: (f64, f64, f64),
        tab: &TabState,
        is_ovf: bool,
        grow: f64,
    ) -> Option<Self> {
        let len = (m[0] * m[0] + m[1] * m[1] + m[2] * m[2]).sqrt();
        if len < 0.01 {
            return None;
        }
        let u = rotation * nalgebra::Vector3::new(m[0] / len, m[1] / len, m[2] / len);
        let out = 0.5 * len * tab.style.moment_scale * scale * grow;
        let half = radius + out;
        // On an OVF mesh, cell spacing is in nm (tens of Å), so scale the shaft
        // with the arrow length (`0.08 * 2 * out`), matching the GPU player.
        let shaft_r = if is_ovf {
            (0.14 * out).max(1.2)
        } else {
            (0.5 * tab.style.bond_radius * scale).max(1.2)
        };
        let head_r = if is_ovf { shaft_r * 1.95 } else { shaft_r * 2.4 };
        let head_len = if is_ovf {
            (0.9 * out).min(head_r * 3.2)
        } else {
            (0.6 * out).min(0.5 * scale).max(shaft_r * 3.0)
        };
        Some(Self {
            centre: [screen_pos[0], screen_pos[1]],
            radius,
            halves: [
                ArrowHalf { dir: [u.x, u.y, u.z], length: half, is_tip: true },
                ArrowHalf { dir: [-u.x, -u.y, -u.z], length: half, is_tip: false },
            ],
            shaft_r,
            head_r,
            head_len,
            color,
            metallic: tab.style.metallic,
            roughness: tab.style.roughness,
        })
    }

    /// Depth (as `RenderAtom::screen_pos[2]`, Å) of the nearest end of the
    /// half in front of the atom, or `None` when no half shows in front.
    fn front_depth(&self, atom_depth: f64, scale: f64) -> Option<f64> {
        self.halves
            .iter()
            .filter(|h| h.dir[2] <= 0.0 && h.length > self.radius)
            .map(|h| atom_depth + h.dir[2] * h.length / scale)
            .reduce(f64::min)
    }

    /// Draw the halves in front of the atom centre (`front`) or behind it.
    /// Larger depth is farther, as in `RenderAtom::screen_pos`.
    fn draw(&self, cr: &cairo::Context, front: bool) {
        for h in &self.halves {
            if (h.dir[2] <= 0.0) != front || h.length <= self.radius {
                continue; // other pass, or wholly inside the sphere
            }
            let at = |t: f64| [self.centre[0] + t * h.dir[0], self.centre[1] + t * h.dir[1], 0.0];
            let head_from = if h.is_tip { (h.length - self.head_len).max(self.radius) } else { h.length };
            draw_cylinder_impostor(
                cr,
                at(self.radius),
                at(head_from),
                self.shaft_r,
                self.color,
                self.metallic,
                self.roughness,
                0.0,
            );
            if h.is_tip {
                self.draw_head(cr, at(head_from), at(h.length));
            }
        }
    }

    /// A cone from `base` to `apex`, shaded across like the bonds. Seen
    /// end-on it is a disc.
    fn draw_head(&self, cr: &cairo::Context, base: [f64; 3], apex: [f64; 3]) {
        let (r, g, b) = self.color;
        let (dx, dy) = (apex[0] - base[0], apex[1] - base[1]);
        let l = (dx * dx + dy * dy).sqrt();
        cr.save().ok();
        if l < 1.0 {
            let grad = cairo::RadialGradient::new(
                apex[0] - self.head_r * 0.3,
                apex[1] - self.head_r * 0.3,
                0.0,
                apex[0],
                apex[1],
                self.head_r,
            );
            grad.add_color_stop_rgb(0.0, (r * 1.4).min(1.0), (g * 1.4).min(1.0), (b * 1.4).min(1.0));
            grad.add_color_stop_rgb(1.0, r * 0.4, g * 0.4, b * 0.4);
            cr.set_source(&grad).ok();
            cr.arc(apex[0], apex[1], self.head_r, 0.0, 2.0 * PI);
        } else {
            let (nx, ny) = (-dy / l * self.head_r, dx / l * self.head_r);
            let grad = cairo::LinearGradient::new(base[0] + nx, base[1] + ny, base[0] - nx, base[1] - ny);
            grad.add_color_stop_rgb(0.0, r * 0.35, g * 0.35, b * 0.35);
            grad.add_color_stop_rgb(0.4, r, g, b);
            grad.add_color_stop_rgb(0.55, (r * 1.3).min(1.0), (g * 1.3).min(1.0), (b * 1.3).min(1.0));
            grad.add_color_stop_rgb(1.0, r * 0.3, g * 0.3, b * 0.3);
            cr.set_source(&grad).ok();
            cr.move_to(apex[0], apex[1]);
            cr.line_to(base[0] + nx, base[1] + ny);
            cr.line_to(base[0] - nx, base[1] - ny);
            cr.close_path();
        }
        cr.fill().ok();
        cr.restore().ok();
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

#[cfg(test)]
mod moment_color_tests {
    use super::*;
    use crate::config::{MomentColorMode, RenderStyle};
    use crate::model::{Atom, Structure};

    fn with_moments(ms: &[[f64; 3]]) -> Structure {
        Structure {
            lattice: [[4.0, 0.0, 0.0], [0.0, 4.0, 0.0], [0.0, 0.0, 4.0]],
            atoms: ms
                .iter()
                .enumerate()
                .map(|(i, m)| Atom {
                    element: "Fe".into(),
                    position: [i as f64, 0.0, 0.0],
                    original_index: i,
                    oxidation: None,
                    occupancy: 1.0,
                    magmom: Some(*m),
                })
                .collect(),
            formula: String::new(),
            is_periodic: true,
        }
    }

    #[test]
    fn up_is_along_plus_z_whichever_equal_moment_comes_first() {
        let style = RenderStyle::default();
        assert_eq!(style.moment_color_mode, MomentColorMode::Auto);
        for order in [[[0.0, 0.0, -4.6], [0.0, 0.0, 4.6]], [[0.0, 0.0, 4.6], [0.0, 0.0, -4.6]]] {
            let axis = with_moments(&order).magnetic_axis();
            assert_eq!(moment_color([0.0, 0.0, 4.6], axis, true, &style), style.moment_color);
            assert_eq!(moment_color([0.0, 0.0, -4.6], axis, true, &style), style.moment_color_down);
        }
    }

    #[test]
    fn ferrimagnet_splits_by_sign_along_an_in_plane_axis() {
        let style = RenderStyle::default();
        let s = with_moments(&[[-4.0, 0.0, 0.0], [2.5, 0.0, 0.0]]);
        let axis = s.magnetic_axis();
        assert_eq!(axis, Some([1.0, 0.0, 0.0]));
        assert_eq!(moment_color([2.5, 0.0, 0.0], axis, true, &style), style.moment_color);
        assert_eq!(moment_color([-4.0, 0.0, 0.0], axis, true, &style), style.moment_color_down);
    }

    #[test]
    fn auto_switches_to_direction_for_a_noncollinear_texture() {
        let style = RenderStyle::default();
        let s = with_moments(&[[0.0, 0.0, 2.0], [2.0, 0.0, 0.0], [0.0, 0.0, -2.0]]);
        assert!(!s.moments_collinear());
        let axis = s.magnetic_axis();
        let up = moment_color([0.0, 0.0, 2.0], axis, false, &style);
        let down = moment_color([0.0, 0.0, -2.0], axis, false, &style);
        // Direction: +z near white, −z near black.
        assert!(up.0 > 0.9 && up.1 > 0.9 && up.2 > 0.9, "{:?}", up);
        assert!(down.0 < 0.2 && down.1 < 0.2 && down.2 < 0.2, "{:?}", down);
        assert_ne!(up, style.moment_color);
    }
}
