// src/rendering/primitives.rs

use super::scene::RenderAtom;
use gtk4::cairo::{self, Context, Format, ImageSurface, RadialGradient};
use std::f64::consts::PI;

// Make fields public so painter.rs can access them
#[derive(Clone)]
pub struct RenderBond {
  pub start: [f64; 3],
  pub end: [f64; 3],
  pub radius: f64,
  /// Indices (into the scene atom list) of the two atoms joined.
  pub atoms: (usize, usize),
}

pub enum RenderPrimitive<'a> {
  Atom(&'a RenderAtom),
  Bond(RenderBond),
}

impl<'a> RenderPrimitive<'a> {
  pub fn z_depth(&self) -> f64 {
    match self {
      RenderPrimitive::Atom(atom) => atom.screen_pos[2],
      RenderPrimitive::Bond(bond) => (bond.start[2] + bond.end[2]) / 2.0,
    }
  }
}

pub fn draw_atom_vector(cr: &cairo::Context, x: f64, y: f64, radius: f64, color: (f64, f64, f64)) {
  let (r, g, b) = color;

  // 1. Create a Radial Gradient to simulate 3D lighting
  // Inner circle (highlight): offset to top-left (-radius/3)
  // Outer circle (shadow): centered
  let gradient = RadialGradient::new(
    x - radius * 0.3,
    y - radius * 0.3,
    radius * 0.1, // Highlight position/size
    x,
    y,
    radius, // Base sphere position/size
  );

  // "Shininess" (Highlight) -> Base Color -> Shadow
  gradient.add_color_stop_rgb(0.0, 1.0, 1.0, 1.0); // White highlight
  gradient.add_color_stop_rgb(0.2, r + 0.2, g + 0.2, b + 0.2); // Lighter base
  gradient.add_color_stop_rgb(1.0, r * 0.6, g * 0.6, b * 0.6); // Darker shadow

  // 2. Draw the Circle
  cr.set_source(&gradient).unwrap();
  cr.arc(x, y, radius, 0.0, 2.0 * std::f64::consts::PI);
  cr.fill().unwrap();

  // Optional: Thin outline for extra crispness in PDF
  cr.set_source_rgba(0.0, 0.0, 0.0, 0.3);
  cr.set_line_width(radius * 0.05);
  cr.arc(x, y, radius, 0.0, 2.0 * std::f64::consts::PI);
  cr.stroke().unwrap();
}

/// Colour of the unoccupied share of a partially occupied site.
const VACANCY_RGB: (f64, f64, f64) = (0.93, 0.93, 0.93);

/// A partially occupied site: one sphere split into pie sectors, each sized
/// by a species' occupancy, with any unoccupied remainder drawn as a pale
/// vacancy sector (the VESTA convention). Sectors start at 12 o'clock and
/// run clockwise in the order given. Each sector carries the same lighting
/// gradient as `draw_atom_vector`, so the pie still reads as a sphere.
pub fn draw_atom_pie(
  cr: &cairo::Context,
  x: f64,
  y: f64,
  radius: f64,
  slices: &[((f64, f64, f64), f64)],
) {
  use std::f64::consts::FRAC_PI_2;

  let occupied: f64 = slices.iter().map(|(_, occ)| occ.max(0.0)).sum();
  // Over-full sites (bad input) are scaled down to a whole sphere.
  let total = occupied.max(1.0);
  let mut sectors: Vec<((f64, f64, f64), f64)> = slices
    .iter()
    .map(|&(rgb, occ)| (rgb, occ.max(0.0) / total))
    .collect();
  if occupied < 0.999 {
    sectors.push((VACANCY_RGB, 1.0 - occupied));
  }

  let mut start = -FRAC_PI_2;
  for &((r, g, b), share) in &sectors {
    if share <= 0.0 {
      continue;
    }
    let end = start + share * 2.0 * PI;
    let gradient = RadialGradient::new(x - radius * 0.3, y - radius * 0.3, radius * 0.1, x, y, radius);
    gradient.add_color_stop_rgb(0.0, 1.0, 1.0, 1.0);
    gradient.add_color_stop_rgb(0.2, r + 0.2, g + 0.2, b + 0.2);
    gradient.add_color_stop_rgb(1.0, r * 0.6, g * 0.6, b * 0.6);

    cr.move_to(x, y);
    cr.arc(x, y, radius, start, end);
    cr.close_path();
    cr.set_source(&gradient).ok();
    cr.fill().ok();
    start = end;
  }

  // Sector boundaries, so two similar colours still read as separate species.
  if sectors.iter().filter(|(_, s)| *s > 0.0).count() > 1 {
    cr.set_source_rgba(0.0, 0.0, 0.0, 0.45);
    cr.set_line_width((radius * 0.04).max(0.5));
    let mut angle = -FRAC_PI_2;
    for &(_, share) in &sectors {
      if share <= 0.0 {
        continue;
      }
      cr.move_to(x, y);
      cr.line_to(x + radius * angle.cos(), y + radius * angle.sin());
      angle += share * 2.0 * PI;
    }
    cr.stroke().ok();
  }

  cr.set_source_rgba(0.0, 0.0, 0.0, 0.3);
  cr.set_line_width(radius * 0.05);
  cr.arc(x, y, radius, 0.0, 2.0 * PI);
  cr.stroke().ok();
}

/// Render a shaded sphere impostor into a square ARGB sprite of `size` pixels.
///
/// `size` is chosen by the caller to match the atom's on-screen diameter (see
/// `SpriteCache::size_bucket`). Cairo's default `Filter::Good` falls back to a
/// full resampling pass whenever a source is scaled below 0.5, which costs
/// ~58 us per blit for a 128 px sprite drawn at 16 px — two orders of
/// magnitude more than the near-1:1 blit a size-matched sprite gets.
pub fn create_atom_sprite(
  r: f64,
  g: f64,
  b: f64,
  metallic: f64,
  roughness: f64,
  transmission: f64,
  size: i32,
) -> ImageSurface {
  let size = size.max(2);
  let surface =
    ImageSurface::create(Format::ARgb32, size, size).expect("Failed to create sprite surface");
  let cr = Context::new(&surface).expect("Failed to create sprite context");

  let center = size as f64 / 2.0;
  let radius = size as f64 / 2.0;

  let (red, green, blue) = (r, g, b);
  let alpha = 1.0 - transmission;

  let spec_r = 1.0 + (red - 1.0) * metallic;
  let spec_g = 1.0 + (green - 1.0) * metallic;
  let spec_b = 1.0 + (blue - 1.0) * metallic;

  let highlight_size = 0.05 + roughness * 0.35;
  let light_offset = 0.25;

  let pat = cairo::RadialGradient::new(
    center - radius * light_offset,
    center - radius * light_offset,
    radius * highlight_size,
    center,
    center,
    radius,
  );

  let shine_alpha = (1.0 - roughness * 0.5) * alpha;
  pat.add_color_stop_rgba(0.0, spec_r, spec_g, spec_b, shine_alpha);

  let lit_pos = 0.1 + roughness * 0.2;
  pat.add_color_stop_rgba(lit_pos, red, green, blue, alpha);

  let ambient_level = 0.4 - (metallic * 0.3);
  pat.add_color_stop_rgba(
    0.85,
    red * ambient_level,
    green * ambient_level,
    blue * ambient_level,
    alpha,
  );

  let rim_darkness = 0.1 * (1.0 - transmission);
  pat.add_color_stop_rgba(
    1.0,
    red * rim_darkness,
    green * rim_darkness,
    blue * rim_darkness,
    alpha,
  );

  cr.set_source(&pat).unwrap();
  cr.arc(center, center, radius, 0.0, 2.0 * PI);
  cr.fill().unwrap();

  surface
}

pub fn draw_cylinder_impostor(
  cr: &cairo::Context,
  p1: [f64; 3],
  p2: [f64; 3],
  radius: f64,
  color: (f64, f64, f64),
  metallic: f64,
  roughness: f64,
  transmission: f64,
) {
  let dx = p2[0] - p1[0];
  let dy = p2[1] - p1[1];
  let len_sq = dx * dx + dy * dy;
  if len_sq < 0.0001 {
    return;
  }

  let nx = -dy / len_sq.sqrt();
  let ny = dx / len_sq.sqrt();

  let c1x = p1[0] + nx * radius;
  let c1y = p1[1] + ny * radius;
  let c2x = p2[0] + nx * radius;
  let c2y = p2[1] + ny * radius;
  let c3x = p2[0] - nx * radius;
  let c3y = p2[1] - ny * radius;
  let c4x = p1[0] - nx * radius;
  let c4y = p1[1] - ny * radius;

  let gradient = cairo::LinearGradient::new(c1x, c1y, c4x, c4y);
  let (r, g, b) = color;
  let alpha = 1.0 - transmission;

  let sr = 1.0 + (r - 1.0) * metallic;
  let sg = 1.0 + (g - 1.0) * metallic;
  let sb = 1.0 + (b - 1.0) * metallic;

  let shadow = 0.3 - (metallic * 0.2);

  gradient.add_color_stop_rgba(0.0, r * shadow, g * shadow, b * shadow, alpha);
  gradient.add_color_stop_rgba(0.3, r, g, b, alpha);

  let h_width = 0.05 + roughness * 0.2;
  gradient.add_color_stop_rgba(0.5 - h_width, r, g, b, alpha);
  gradient.add_color_stop_rgba(0.5, sr, sg, sb, alpha * (1.0 - roughness * 0.3));
  gradient.add_color_stop_rgba(0.5 + h_width, r, g, b, alpha);

  gradient.add_color_stop_rgba(0.7, r, g, b, alpha);
  gradient.add_color_stop_rgba(1.0, r * shadow, g * shadow, b * shadow, alpha);

  cr.set_source(&gradient).unwrap();
  cr.move_to(c1x, c1y);
  cr.line_to(c2x, c2y);
  cr.line_to(c3x, c3y);
  cr.line_to(c4x, c4y);
  cr.close_path();
  cr.fill().unwrap();
}
