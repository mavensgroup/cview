// src/ui/analysis/charge_density_3d/scene_file.rs
//
// Reproducibility and interchange for the 3D charge-density page:
//
// - Scene files (`*.cview.json`): camera, isovalue (and how it was chosen),
//   lobes, colours, occlusion, display toggles, section plane and the figure
//   settings, plus a hash of the density they were made from. Every figure
//   export writes one next to the figure, so a referee-requested change is
//   "load scene, adjust, export".
// - Mesh export: the current isosurfaces as OBJ (+ MTL with the lobe
//   colours) or PLY with vertex colours, in Å, for Blender, ParaView or
//   POV-Ray.

use super::figure_dialog::FigureSettings;
use super::{IsoMode, Lobe, Lobes, SectionState, View};
use crate::utils::console;
use gtk4::prelude::*;
use gtk4::glib;
use nalgebra::{Quaternion, UnitQuaternion};
use serde::{Deserialize, Serialize};
use std::cell::{Cell, RefCell};
use std::io::Write;
use std::path::Path;
use std::rc::Rc;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(super) struct Scene {
    pub version: u32,
    /// Hash of the density values (hex); a mismatch on load means the scene
    /// was made from different data.
    pub data_hash: String,
    pub label: String,
    /// Camera orientation as a quaternion (i, j, k, w), main-view convention.
    pub rotation: [f64; 4],
    pub zoom: f64,
    pub iso: f64,
    pub iso_mode: IsoMode,
    pub lobes: Lobes,
    pub opacity: f32,
    pub color_pos: [f32; 3],
    pub color_neg: [f32; 3],
    pub ao: bool,
    pub ao_strength: f32,
    pub show_atoms: bool,
    pub show_cell: bool,
    pub section: SectionState,
    #[serde(default)]
    pub figure: Option<FigureSettings>,
}

pub(super) fn from_view(v: &View, figure: Option<FigureSettings>) -> Option<Scene> {
    let f = v.field.as_ref()?;
    let q = v.camera.rotation.into_inner().coords;
    Some(Scene {
        version: 1,
        data_hash: format!("{:016x}", f.hash),
        label: f.label.clone(),
        rotation: [q[0], q[1], q[2], q[3]],
        zoom: v.camera.zoom,
        iso: v.iso,
        iso_mode: v.iso_mode,
        lobes: v.lobes,
        opacity: v.opacity,
        color_pos: v.color_pos,
        color_neg: v.color_neg,
        ao: v.ao,
        ao_strength: v.ao_strength,
        show_atoms: v.show_atoms,
        show_cell: v.show_cell,
        section: v.section,
        figure,
    })
}

pub(super) fn save(path: &Path, scene: &Scene) -> Result<(), String> {
    let json = serde_json::to_string_pretty(scene).map_err(|e| e.to_string())?;
    std::fs::write(path, json).map_err(|e| e.to_string())
}

pub(super) fn load(path: &Path) -> Result<Scene, String> {
    let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    serde_json::from_str(&text).map_err(|e| format!("not a CView scene file: {e}"))
}

/// The controls a scene sets.
#[derive(Clone)]
pub(super) struct SceneWidgets {
    pub lobes: glib::WeakRef<gtk4::DropDown>,
    pub mode: glib::WeakRef<gtk4::DropDown>,
    pub percent: glib::WeakRef<gtk4::SpinButton>,
    pub opacity: glib::WeakRef<gtk4::Scale>,
    pub col_pos: glib::WeakRef<gtk4::ColorButton>,
    pub col_neg: glib::WeakRef<gtk4::ColorButton>,
    pub ao: glib::WeakRef<gtk4::CheckButton>,
    pub ao_strength: glib::WeakRef<gtk4::Scale>,
    pub atoms: glib::WeakRef<gtk4::CheckButton>,
    pub cell: glib::WeakRef<gtk4::CheckButton>,
    pub sec_show: glib::WeakRef<gtk4::CheckButton>,
    pub sec_cut: glib::WeakRef<gtk4::CheckButton>,
    pub sec_flip: glib::WeakRef<gtk4::CheckButton>,
    pub h: glib::WeakRef<gtk4::SpinButton>,
    pub k: glib::WeakRef<gtk4::SpinButton>,
    pub l: glib::WeakRef<gtk4::SpinButton>,
    pub sec_pos: glib::WeakRef<gtk4::Scale>,
}

/// Apply `scene` to the view and its controls (without triggering the
/// controls' handlers; the caller remeshes once afterwards).
pub(super) fn apply(scene: &Scene, view: &Rc<RefCell<View>>, w: &SceneWidgets, syncing: &Cell<bool>) {
    syncing.set(true);
    let rgba = |c: [f32; 3]| gtk4::gdk::RGBA::new(c[0], c[1], c[2], 1.0);
    if let Some(x) = w.lobes.upgrade() {
        x.set_selected(match scene.lobes {
            Lobes::Positive => 0,
            Lobes::Negative => 1,
            Lobes::Both => 2,
        });
    }
    if let Some(x) = w.mode.upgrade() {
        x.set_selected(matches!(scene.iso_mode, IsoMode::Fraction(_)) as u32);
    }
    if let Some(x) = w.percent.upgrade() {
        if let IsoMode::Fraction(fr) = scene.iso_mode {
            x.set_value(fr * 100.0);
        }
        x.set_sensitive(matches!(scene.iso_mode, IsoMode::Fraction(_)));
    }
    if let Some(x) = w.opacity.upgrade() {
        x.set_value(scene.opacity as f64);
    }
    if let Some(x) = w.col_pos.upgrade() {
        x.set_rgba(&rgba(scene.color_pos));
    }
    if let Some(x) = w.col_neg.upgrade() {
        x.set_rgba(&rgba(scene.color_neg));
    }
    if let Some(x) = w.ao.upgrade() {
        x.set_active(scene.ao);
    }
    if let Some(x) = w.ao_strength.upgrade() {
        x.set_value(scene.ao_strength as f64);
    }
    if let Some(x) = w.atoms.upgrade() {
        x.set_active(scene.show_atoms);
    }
    if let Some(x) = w.cell.upgrade() {
        x.set_active(scene.show_cell);
    }
    let s = scene.section;
    for (c, v) in [(&w.sec_show, s.show), (&w.sec_cut, s.cut), (&w.sec_flip, s.flip)] {
        if let Some(x) = c.upgrade() {
            x.set_active(v);
        }
    }
    for (c, v) in [(&w.h, s.hkl[0]), (&w.k, s.hkl[1]), (&w.l, s.hkl[2])] {
        if let Some(x) = c.upgrade() {
            x.set_value(v as f64);
        }
    }
    if let Some(x) = w.sec_pos.upgrade() {
        x.set_value(s.pos);
    }
    syncing.set(false);

    let mut v = view.borrow_mut();
    if let Some(f) = &v.field {
        if format!("{:016x}", f.hash) != scene.data_hash {
            console::log_warn(&format!(
                "Scene was saved for different data ('{}'); its settings are applied to the current density",
                scene.label
            ));
        }
    }
    let r = scene.rotation;
    v.camera.rotation = UnitQuaternion::from_quaternion(Quaternion::new(r[3], r[0], r[1], r[2]));
    v.camera.zoom = scene.zoom;
    v.iso = scene.iso;
    v.iso_mode = scene.iso_mode;
    v.lobes = scene.lobes;
    v.opacity = scene.opacity;
    v.color_pos = scene.color_pos;
    v.color_neg = scene.color_neg;
    v.ao = scene.ao;
    v.ao_strength = scene.ao_strength;
    v.show_atoms = scene.show_atoms;
    v.show_cell = scene.show_cell;
    v.section = scene.section;
    super::apply_iso_mode(&mut v);
    drop(v);
    view.borrow_mut().mark_scene_dirty();
}

fn chooser(button: &gtk4::Button, title: &str, action: gtk4::FileChooserAction, patterns: &[(&str, &[&str])]) -> gtk4::FileChooserNative {
    let parent = button.root().and_then(|r| r.downcast::<gtk4::Window>().ok());
    let accept = if action == gtk4::FileChooserAction::Save { "Save" } else { "Open" };
    let d = gtk4::FileChooserNative::new(Some(title), parent.as_ref(), action, Some(accept), Some("Cancel"));
    for (name, pats) in patterns {
        let f = gtk4::FileFilter::new();
        f.set_name(Some(name));
        for p in *pats {
            f.add_pattern(p);
        }
        d.add_filter(&f);
    }
    d
}

pub(super) fn save_dialog(button: &gtk4::Button, view: Rc<RefCell<View>>) {
    let d = chooser(button, "Save Scene", gtk4::FileChooserAction::Save, &[("CView scene (*.cview.json)", &["*.json"])]);
    d.set_current_name("charge_density.cview.json");
    d.connect_response(move |d, r| {
        if r == gtk4::ResponseType::Accept {
            if let Some(path) = d.file().and_then(|f| f.path()) {
                match from_view(&view.borrow(), None).ok_or("no density loaded".to_string()).and_then(|s| save(&path, &s)) {
                    Ok(()) => console::log_info(&format!("Scene saved to {}", path.display())),
                    Err(e) => console::log_error(&format!("Could not save scene: {e}")),
                }
            }
        }
        d.destroy();
    });
    d.show();
}

pub(super) fn load_dialog(button: &gtk4::Button, on_loaded: impl Fn(Scene) + 'static) {
    let d = chooser(button, "Load Scene", gtk4::FileChooserAction::Open, &[("CView scene (*.cview.json)", &["*.json"])]);
    d.connect_response(move |d, r| {
        if r == gtk4::ResponseType::Accept {
            if let Some(path) = d.file().and_then(|f| f.path()) {
                match load(&path) {
                    Ok(s) => {
                        on_loaded(s);
                        console::log_info(&format!("Scene loaded from {}", path.display()));
                    }
                    Err(e) => console::log_error(&format!("Could not load scene: {e}")),
                }
            }
        }
        d.destroy();
    });
    d.show();
}

pub(super) fn export_mesh_dialog(button: &gtk4::Button, view: Rc<RefCell<View>>) {
    let d = chooser(
        button,
        "Export Isosurface Mesh",
        gtk4::FileChooserAction::Save,
        &[("Wavefront OBJ (*.obj)", &["*.obj"]), ("Stanford PLY (*.ply)", &["*.ply"])],
    );
    d.set_current_name("isosurface.obj");
    d.connect_response(move |d, r| {
        if r == gtk4::ResponseType::Accept {
            if let Some(path) = d.file().and_then(|f| f.path()) {
                let v = view.borrow();
                let lobes = [
                    ("positive_lobe", v.lobes_now[0].clone(), v.color_pos),
                    ("negative_lobe", v.lobes_now[1].clone(), v.color_neg),
                ];
                let parts: Vec<(&str, &Lobe, [f32; 3])> =
                    lobes.iter().filter_map(|(n, l, c)| l.as_deref().map(|l| (*n, l, *c))).collect();
                let result = if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("ply")) {
                    write_ply(&path, &parts)
                } else {
                    write_obj(&path, &parts, v.opacity)
                };
                match result {
                    Ok(()) => console::log_info(&format!("Isosurface mesh written to {}", path.display())),
                    Err(e) => console::log_error(&format!("Could not write mesh: {e}")),
                }
            }
        }
        d.destroy();
    });
    d.show();
}

/// OBJ with normals, one object per lobe, and an MTL with their colours.
pub(super) fn write_obj(path: &Path, parts: &[(&str, &Lobe, [f32; 3])], opacity: f32) -> Result<(), String> {
    let mtl = path.with_extension("mtl");
    let mtl_name = mtl.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let mut f = std::io::BufWriter::new(std::fs::File::create(path).map_err(|e| e.to_string())?);
    let mut m = std::io::BufWriter::new(std::fs::File::create(&mtl).map_err(|e| e.to_string())?);
    let io = |e: std::io::Error| e.to_string();
    writeln!(f, "# CView isosurface export; coordinates in angstrom").map_err(io)?;
    writeln!(f, "mtllib {mtl_name}").map_err(io)?;
    let mut base = 1usize;
    for (name, lobe, color) in parts {
        writeln!(m, "newmtl {name}\nKd {} {} {}\nd {opacity}\n", color[0], color[1], color[2]).map_err(io)?;
        writeln!(f, "o {name}\nusemtl {name}").map_err(io)?;
        for p in &lobe.mesh.positions {
            writeln!(f, "v {} {} {}", p[0], p[1], p[2]).map_err(io)?;
        }
        for n in &lobe.mesh.normals {
            writeln!(f, "vn {} {} {}", n[0], n[1], n[2]).map_err(io)?;
        }
        for t in lobe.mesh.indices.chunks_exact(3) {
            let (a, b, c) = (t[0] as usize + base, t[1] as usize + base, t[2] as usize + base);
            writeln!(f, "f {a}//{a} {b}//{b} {c}//{c}").map_err(io)?;
        }
        base += lobe.mesh.positions.len();
    }
    f.flush().map_err(io)?;
    m.flush().map_err(io)
}

/// ASCII PLY with normals and per-vertex colours (all lobes in one mesh).
pub(super) fn write_ply(path: &Path, parts: &[(&str, &Lobe, [f32; 3])]) -> Result<(), String> {
    let nv: usize = parts.iter().map(|p| p.1.mesh.positions.len()).sum();
    let nf: usize = parts.iter().map(|p| p.1.mesh.triangle_count()).sum();
    let mut f = std::io::BufWriter::new(std::fs::File::create(path).map_err(|e| e.to_string())?);
    let io = |e: std::io::Error| e.to_string();
    write!(
        f,
        "ply\nformat ascii 1.0\ncomment CView isosurface export; coordinates in angstrom\n\
         element vertex {nv}\nproperty float x\nproperty float y\nproperty float z\n\
         property float nx\nproperty float ny\nproperty float nz\n\
         property uchar red\nproperty uchar green\nproperty uchar blue\n\
         element face {nf}\nproperty list uchar int vertex_indices\nend_header\n"
    )
    .map_err(io)?;
    for (_, lobe, c) in parts {
        let rgb = c.map(|x| (x.clamp(0.0, 1.0) * 255.0).round() as u8);
        for (p, n) in lobe.mesh.positions.iter().zip(&lobe.mesh.normals) {
            writeln!(f, "{} {} {} {} {} {} {} {} {}", p[0], p[1], p[2], n[0], n[1], n[2], rgb[0], rgb[1], rgb[2]).map_err(io)?;
        }
    }
    let mut base = 0u32;
    for (_, lobe, _) in parts {
        for t in lobe.mesh.indices.chunks_exact(3) {
            writeln!(f, "3 {} {} {}", t[0] + base, t[1] + base, t[2] + base).map_err(io)?;
        }
        base += lobe.mesh.positions.len() as u32;
    }
    f.flush().map_err(io)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::physics::analysis::isosurface::Mesh;
    use crate::physics::analysis::volumetric::Enclosed;

    fn lobe() -> Lobe {
        Lobe {
            mesh: Mesh {
                positions: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
                normals: vec![[0.0, 0.0, 1.0]; 3],
                indices: vec![0, 1, 2],
            },
            ao: None,
            enclosed: Enclosed::default(),
        }
    }

    #[test]
    fn obj_and_ply_index_both_lobes() {
        let dir = std::env::temp_dir().join(format!("cview_mesh_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (a, b) = (lobe(), lobe());
        let parts = [("positive_lobe", &a, [1.0, 1.0, 0.0]), ("negative_lobe", &b, [0.0, 1.0, 1.0])];
        let obj = dir.join("m.obj");
        write_obj(&obj, &parts, 0.7).unwrap();
        let text = std::fs::read_to_string(&obj).unwrap();
        assert!(text.contains("f 1//1 2//2 3//3") && text.contains("f 4//4 5//5 6//6"));
        assert!(std::fs::read_to_string(dir.join("m.mtl")).unwrap().contains("Kd 0 1 1"));
        let ply = dir.join("m.ply");
        write_ply(&ply, &parts).unwrap();
        let text = std::fs::read_to_string(&ply).unwrap();
        assert!(text.contains("element vertex 6") && text.contains("3 3 4 5"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn scene_round_trips_through_json() {
        let s = Scene {
            version: 1,
            data_hash: "00ff".into(),
            label: "Total density".into(),
            rotation: [0.1, 0.2, 0.3, 0.927],
            zoom: 1.5,
            iso: 0.05,
            iso_mode: IsoMode::Fraction(0.5),
            lobes: Lobes::Both,
            opacity: 0.7,
            color_pos: [1.0, 0.8, 0.2],
            color_neg: [0.2, 0.7, 0.9],
            ao: true,
            ao_strength: 0.6,
            show_atoms: true,
            show_cell: false,
            section: SectionState::default(),
            figure: None,
        };
        let back: Scene = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
        assert_eq!(back.iso_mode, IsoMode::Fraction(0.5));
        assert_eq!(back.lobes, Lobes::Both);
        assert_eq!(back.section, SectionState::default());
    }
}
