// src/ui/geometry.rs
use gtk4::prelude::*;
use gtk4::{Window, Box, Label, Orientation};
use crate::state::AppState;
use crate::geometry;

pub struct MeasurementWindow {
    pub window: Window,
    label_info: Label,
    label_result: Label,
}

impl MeasurementWindow {
    pub fn new(app: &gtk4::Application) -> Self {
        let window = Window::builder()
            .application(app)
            .title("Geometry Calculations")
            .default_width(320)
            .default_height(220)
            .resizable(false)
            .build();

        let container = Box::new(Orientation::Vertical, 15);
        container.set_margin_top(20);
        container.set_margin_bottom(20);
        container.set_margin_start(20);
        container.set_margin_end(20);

        let label_info = Label::builder()
            .label("Select atoms in the viewer to measure.")
            .justify(gtk4::Justification::Center)
            .wrap(true)
            .build();

        let label_result = Label::new(None);
        // Initial placeholder
        label_result.set_markup("<span size='xx-large' weight='bold' foreground='#888888'>-</span>");

        // Helper text for the user
        let help_text = Label::builder()
            .label("2 Atoms: Distance\n3 Atoms: Angle\n4 Atoms: Torsion")
            .css_classes(["dim-label"]) // Assuming you might have some CSS, or just for clarity
            .build();
        help_text.set_opacity(0.6);

        container.append(&label_info);
        container.append(&label_result);
        container.append(&help_text);

        window.set_child(Some(&container));

        Self {
            window,
            label_info,
            label_result,
        }
    }

    /// Updates the window content based on the current selection in AppState
    pub fn update(&self, state: &AppState) {
        // 1. Safely get atoms from the structure
        let atoms = match &state.structure {
            Some(s) => &s.atoms,
            None => {
                self.label_info.set_text("No molecule loaded.");
                self.label_result.set_text("-");
                return;
            }
        };

        let indices = &state.selected_indices;

        // 2. Filter to ensure selected indices are still valid for the current structure
        let valid_indices: Vec<usize> = indices.iter()
            .filter(|&&i| i < atoms.len())
            .cloned()
            .collect();

        // 3. Match based on number of selected atoms
        match valid_indices.len() {
            2 => {
                let p1 = atoms[valid_indices[0]].position;
                let p2 = atoms[valid_indices[1]].position;
                let dist = geometry::calculate_distance(p1, p2);

                self.label_info.set_text(&format!(
                    "Distance: {} (#{}) to {} (#{})",
                    atoms[valid_indices[0]].element, valid_indices[0],
                    atoms[valid_indices[1]].element, valid_indices[1]
                ));
                self.label_result.set_markup(&format!(
                    "<span size='xx-large' weight='bold' foreground='#4CAF50'>{:.4} Å</span>",
                    dist
                ));
            },
            3 => {
                let p1 = atoms[valid_indices[0]].position;
                let p2 = atoms[valid_indices[1]].position; // Center/Vertex
                let p3 = atoms[valid_indices[2]].position;
                let angle = geometry::calculate_angle(p1, p2, p3);

                self.label_info.set_text(&format!("Angle: {}-{}-{}", valid_indices[0], valid_indices[1], valid_indices[2]));
                self.label_result.set_markup(&format!(
                    "<span size='xx-large' weight='bold' foreground='#2196F3'>{:.2}°</span>",
                    angle
                ));
            },
            4 => {
                let p1 = atoms[valid_indices[0]].position;
                let p2 = atoms[valid_indices[1]].position;
                let p3 = atoms[valid_indices[2]].position;
                let p4 = atoms[valid_indices[3]].position;
                let dihedral = geometry::calculate_dihedral(p1, p2, p3, p4);

                self.label_info.set_text(&format!("Dihedral: {}-{}-{}-{}", valid_indices[0], valid_indices[1], valid_indices[2], valid_indices[3]));
                self.label_result.set_markup(&format!(
                    "<span size='xx-large' weight='bold' foreground='#FF9800'>{:.2}°</span>",
                    dihedral
                ));
            },
            n if n > 4 => {
                self.label_info.set_text(&format!("{} atoms selected", n));
                self.label_result.set_markup("<span size='large'>Clear selection to reset</span>");
            },
            n => {
                self.label_info.set_text(&format!("Selected: {} atom(s)", n));
                self.label_result.set_markup("<span foreground='#888888'>-</span>");
            }
        }
    }
}
