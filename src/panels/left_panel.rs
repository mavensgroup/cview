use gtk4::prelude::*;
use gtk4::{Box, Label, Orientation, Align, Frame, ListBox, SelectionMode};
use crate::state::AppState;

pub struct LeftPanel {
    pub container: Box,
    info_label: Label,
    selection_list: ListBox,
}

impl LeftPanel {
    pub fn new() -> Self {
        let container = Box::new(Orientation::Vertical, 10);
        container.set_width_request(200);
        container.set_margin_start(10);
        container.set_margin_end(10);
        container.set_margin_top(10);
        container.set_margin_bottom(10);

        let title = Label::new(Some("<b>Geometry</b>"));
        title.set_use_markup(true);
        title.set_halign(Align::Start);
        container.append(&title);

        let frame = Frame::new(Some("Selection"));
        let info_box = Box::new(Orientation::Vertical, 10);
        info_box.set_margin_top(10);
        info_box.set_margin_bottom(10);
        info_box.set_margin_start(10);
        info_box.set_margin_end(10);

        let info_label = Label::new(Some("Select atoms to see info."));
        info_label.set_wrap(true);
        info_label.set_xalign(0.0);

        info_box.append(&info_label);
        frame.set_child(Some(&info_box));
        container.append(&frame);

        let list_label = Label::new(Some("Selected Atoms:"));
        list_label.set_halign(Align::Start);
        list_label.set_margin_top(10);
        container.append(&list_label);

        let selection_list = ListBox::new();
        selection_list.set_selection_mode(SelectionMode::None);
        selection_list.set_height_request(150);
        selection_list.add_css_class("frame");

        container.append(&selection_list);

        Self {
            container,
            info_label,
            selection_list,
        }
    }

    pub fn update(&self, state: &AppState) {
        while let Some(child) = self.selection_list.first_child() {
            self.selection_list.remove(&child);
        }

        if let Some(structure) = &state.structure {
            for &idx in &state.selected_atoms {
                if let Some(atom) = structure.atoms.get(idx) {
                    let label = Label::new(Some(&format!("{}: {}", idx + 1, atom.element)));
                    self.selection_list.append(&label);
                }
            }

            match state.selected_atoms.len() {
                2 => {
                    let a1 = &structure.atoms[state.selected_atoms[0]];
                    let a2 = &structure.atoms[state.selected_atoms[1]];
                    // FIXED: position[]
                    let dx = a1.position[0] - a2.position[0];
                    let dy = a1.position[1] - a2.position[1];
                    let dz = a1.position[2] - a2.position[2];

                    let dist = (dx.powi(2) + dy.powi(2) + dz.powi(2)).sqrt();
                    self.info_label.set_markup(&format!("<b>Distance:</b>\n{:.4} Å", dist));
                },
                3 => {
                    let a1 = &structure.atoms[state.selected_atoms[0]];
                    let a2 = &structure.atoms[state.selected_atoms[1]]; // Center
                    let a3 = &structure.atoms[state.selected_atoms[2]];

                    // FIXED: position[]
                    let v1 = [
                        a1.position[0] - a2.position[0],
                        a1.position[1] - a2.position[1],
                        a1.position[2] - a2.position[2]
                    ];
                    let v2 = [
                        a3.position[0] - a2.position[0],
                        a3.position[1] - a2.position[1],
                        a3.position[2] - a2.position[2]
                    ];

                    let dot = v1[0]*v2[0] + v1[1]*v2[1] + v1[2]*v2[2];
                    let mag1 = (v1[0].powi(2) + v1[1].powi(2) + v1[2].powi(2)).sqrt();
                    let mag2 = (v2[0].powi(2) + v2[1].powi(2) + v2[2].powi(2)).sqrt();

                    let angle_rad = (dot / (mag1 * mag2)).clamp(-1.0, 1.0).acos();
                    let angle_deg = angle_rad * 180.0 / std::f64::consts::PI;

                    self.info_label.set_markup(&format!("<b>Angle:</b>\n{:.2}°", angle_deg));
                },
                _ => {
                    self.info_label.set_label("Select 2 atoms for distance,\n3 for angle.");
                }
            }
        } else {
             self.info_label.set_label("No structure loaded.");
        }
    }
}
