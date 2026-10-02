// src/rendering/colormap.rs
//
// Sequential/diverging colormaps shared by the charge-density plots and the
// polyhedra property colouring. `t` is the normalised value in [0, 1].

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Colormap {
    #[default]
    Viridis,
    Plasma,
    BlueWhiteRed,
    Grayscale,
}

pub fn colormap_rgb(choice: Colormap, t: f64) -> (f64, f64, f64) {
    let t = t.clamp(0.0, 1.0);
    match choice {
        Colormap::Viridis => {
            let stops: &[(f64, (f64, f64, f64))] = &[
                (0.00, (0.267, 0.005, 0.329)),
                (0.13, (0.283, 0.141, 0.457)),
                (0.25, (0.254, 0.265, 0.530)),
                (0.38, (0.207, 0.372, 0.553)),
                (0.50, (0.164, 0.471, 0.558)),
                (0.63, (0.127, 0.566, 0.551)),
                (0.75, (0.190, 0.660, 0.498)),
                (0.88, (0.432, 0.761, 0.380)),
                (1.00, (0.993, 0.906, 0.144)),
            ];
            lerp_stops(t, stops)
        }
        Colormap::Plasma => {
            let stops: &[(f64, (f64, f64, f64))] = &[
                (0.00, (0.050, 0.030, 0.528)),
                (0.13, (0.299, 0.006, 0.627)),
                (0.25, (0.494, 0.011, 0.657)),
                (0.38, (0.659, 0.126, 0.600)),
                (0.50, (0.796, 0.236, 0.494)),
                (0.63, (0.904, 0.369, 0.373)),
                (0.75, (0.973, 0.528, 0.259)),
                (0.88, (0.994, 0.710, 0.161)),
                (1.00, (0.940, 0.975, 0.131)),
            ];
            lerp_stops(t, stops)
        }
        Colormap::BlueWhiteRed => {
            if t < 0.5 {
                let u = t * 2.0;
                (u, u, 1.0)
            } else {
                let u = (t - 0.5) * 2.0;
                (1.0, 1.0 - u, 1.0 - u)
            }
        }
        Colormap::Grayscale => (t, t, t),
    }
}

fn lerp_stops(t: f64, stops: &[(f64, (f64, f64, f64))]) -> (f64, f64, f64) {
    if stops.is_empty() {
        return (0.0, 0.0, 0.0);
    }
    if t <= stops[0].0 {
        return stops[0].1;
    }
    if t >= stops[stops.len() - 1].0 {
        return stops[stops.len() - 1].1;
    }
    for i in 1..stops.len() {
        let (t0, c0) = stops[i - 1];
        let (t1, c1) = stops[i];
        if t <= t1 {
            let u = (t - t0) / (t1 - t0);
            return (
                c0.0 + u * (c1.0 - c0.0),
                c0.1 + u * (c1.1 - c0.1),
                c0.2 + u * (c1.2 - c0.2),
            );
        }
    }
    stops[stops.len() - 1].1
}

