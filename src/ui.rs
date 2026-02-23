// src/ui/mod.rs

pub mod analysis;
pub mod dialogs;
pub mod export_dialog;
pub mod interactions;
pub mod preferences;

// Re-exports
pub use interactions::setup_interactions;
pub use preferences::show_preferences_window;

use crate::config::ColorMode;
use crate::rendering;
use crate::state::AppState;
use gtk4::prelude::*;
use gtk4::TextView;
use gtk4::{Box as GtkBox, Button, DrawingArea, Label, Notebook, Orientation, Widget};
use std::cell::RefCell;
use std::rc::Rc;

// Helper to log to console TextView
pub fn log_to_console(console_view: &TextView, message: &str) {
    let buffer = console_view.buffer();
    let mut end_iter = buffer.end_iter();
    if buffer.char_count() > 0 {
        buffer.insert(&mut end_iter, "\n\n--------------------------------\n\n");
    }
    buffer.insert(&mut end_iter, message);
    let mark = buffer.create_mark(None, &end_iter, false);
    console_view.scroll_to_mark(&mark, 0.0, true, 0.0, 1.0);
    buffer.delete_mark(&mark);
}

/// Helper: Creates the DrawingArea and wrapping Box for a specific Tab ID.
pub fn create_tab_content(state: Rc<RefCell<AppState>>, tab_id: usize) -> (DrawingArea, GtkBox) {
    let drawing_area = DrawingArea::new();
    drawing_area.set_vexpand(true);
    drawing_area.set_hexpand(true);

    let s = state.clone();
    let tid = tab_id;

    drawing_area.set_draw_func(move |_, cr, w, h| {
        // === CRITICAL FIX: Pre-calculate BVS before immutable borrow ===
        // We need to calculate BVS values BEFORE we create the immutable borrow
        // for rendering, because get_bvs_values() requires a mutable borrow.
        {
            let mut st = s.borrow_mut();
            if tid < st.tabs.len() {
                let tab = &mut st.tabs[tid];
                // Only calculate if we're in BVS mode
                if matches!(tab.style.color_mode, ColorMode::BondValence) {
                    let _ = tab.get_bvs_values(); // This populates the cache
                }
            }
        } // Drop mutable borrow here

        // Now proceed with immutable borrow for rendering
        let st = s.borrow();

        // Safety check to prevent crash if tab doesn't exist yet
        if tid >= st.tabs.len() {
            return;
        }

        let tab = &st.tabs[tid];

        // 1. Background
        let (bg_r, bg_g, bg_b) = tab.style.background_color;
        cr.set_source_rgb(bg_r, bg_g, bg_b);
        cr.paint().unwrap();

        // 2. Calculate Scene
        let (atoms, lattice_corners, bounds) = rendering::scene::calculate_scene(
            tab, &st.config, w as f64, h as f64, false, None, None,
        );

        // 3. Draw Elements
        rendering::painter::draw_unit_cell(cr, &lattice_corners, false);
        rendering::painter::draw_structure(cr, &atoms, tab, bounds.scale, false);
        rendering::painter::draw_miller_planes(
            cr,
            tab,
            &lattice_corners,
            bounds.scale,
            w as f64,
            h as f64,
        );
        rendering::painter::draw_axes(cr, tab, w as f64, h as f64);
        rendering::painter::draw_selection_box(cr, tab);
    });

    let container = GtkBox::new(Orientation::Vertical, 0);
    container.append(&drawing_area);

    (drawing_area, container)
}

/// HELPER: closing tab
pub fn add_closable_tab(
    notebook: &Notebook,
    content: &impl IsA<Widget>, // Changed from &Widget to &impl IsA<Widget>
    title: &str,
    state: Rc<RefCell<AppState>>,
) {
    // 1. Create the Custom Tab Label (Text + X Button)
    let label_box = GtkBox::new(Orientation::Horizontal, 8); // 8px spacing
    let label_text = Label::new(Some(title));

    // STANDARD ICON: window-close-symbolic
    let close_btn = Button::from_icon_name("window-close-symbolic");

    // MAKE IT LOOK LIKE A TAB BUTTON (Flat, Small)
    close_btn.set_has_frame(false);
    close_btn.set_tooltip_text(Some("Close Tab"));
    close_btn.set_valign(gtk4::Align::Center);

    // Layout: [Label] ... [Close Button]
    label_box.append(&label_text);
    label_box.append(&close_btn);
    label_box.show();

    // 2. Append the page with this custom label widget
    notebook.append_page(content, Some(&label_box));

    // 3. Connect the Close Signal
    let nb_weak = notebook.downgrade();
    let content_weak = content.downgrade();
    let state_weak = Rc::downgrade(&state);

    close_btn.connect_clicked(move |_| {
        if let (Some(nb), Some(content_widget), Some(st)) = (
            nb_weak.upgrade(),
            content_weak.upgrade(),
            state_weak.upgrade(),
        ) {
            // Find the *current* index of this tab (it shifts as others close)
            if let Some(page_num) = nb.page_num(&content_widget) {
                // A. Remove from State
                st.borrow_mut().remove_tab(page_num as usize);

                // B. Remove from UI
                nb.remove_page(Some(page_num));

                // (Optional) If all tabs closed, you could open a new untitled one here
            }
        }
    });
}

/// Finds the DrawingArea inside the currently active Notebook tab.
/// This fixes the glitch where dialogs update the wrong tab.
pub fn get_active_drawing_area(notebook: &Notebook) -> Option<DrawingArea> {
    // 1. Get the page widget currently visible (the active tab)
    if let Some(page) = notebook.nth_page(notebook.current_page()) {
        // 2. We need to find the DrawingArea inside this page.
        // We look at the page itself, then its children (in case it's inside a Box).
        if let Some(da) = page.downcast_ref::<DrawingArea>() {
            return Some(da.clone());
        }

        // Iterate through children (e.g., if page is a Box containing the DrawingArea)
        let mut child = page.first_child();
        while let Some(widget) = child {
            if let Some(da) = widget.downcast_ref::<DrawingArea>() {
                return Some(da.clone());
            }

            // Check one level deeper (e.g. Box -> Overlay -> DrawingArea)
            let mut sub_child = widget.first_child();
            while let Some(sub) = sub_child {
                if let Some(da) = sub.downcast_ref::<DrawingArea>() {
                    return Some(da.clone());
                }
                sub_child = sub.next_sibling();
            }

            child = widget.next_sibling();
        }
    }
    None
}
