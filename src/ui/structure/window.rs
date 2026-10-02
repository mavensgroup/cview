// src/ui/structure/window.rs
//
// The Structure window: the tools that change the structure (Supercell,
// Basis, Atom Instances, Slab) as tabs of one notebook, laid out like the
// Analysis window. Supercell, Basis and Atom Instances share one preview
// pane, which moves into whichever of those tabs is showing; Slab keeps its
// own cutting-plane canvas.

use super::preview::Preview;
use super::{basis_tab, instances_tab, slab_tab, supercell_tab, TabParts, CONTROL_PANE_WIDTH};
use crate::state::AppState;
use gtk4::prelude::*;
use gtk4::{
    ApplicationWindow, Box as GtkBox, Label, Notebook, Orientation, PolicyType, ScrolledWindow,
    Window,
};
use std::cell::RefCell;
use std::rc::Rc;

/// Tabs of the Structure window, in notebook order.
#[derive(Clone, Copy)]
pub enum StructureTab {
    Supercell = 0,
    Basis = 1,
    Instances = 2,
    Slab = 3,
}

/// A tab page with a slot on the left for the shared preview and the tab's
/// controls on the right (scrolled, so a tall control column never sets the
/// window's minimum height).
fn page_with_preview_slot(controls: &GtkBox) -> (GtkBox, GtkBox) {
    let page = GtkBox::new(Orientation::Horizontal, 15);
    page.set_margin_top(15);
    page.set_margin_bottom(15);
    page.set_margin_start(15);
    page.set_margin_end(15);

    let slot = GtkBox::new(Orientation::Vertical, 0);
    slot.set_hexpand(true);
    page.append(&slot);

    let scroll = ScrolledWindow::builder()
        .child(controls)
        .hscrollbar_policy(PolicyType::Never)
        .vscrollbar_policy(PolicyType::Automatic)
        .build();
    scroll.set_width_request(CONTROL_PANE_WIDTH);
    page.append(&scroll);
    (page, slot)
}

pub fn show_structure_window(
    parent: &ApplicationWindow,
    state: Rc<RefCell<AppState>>,
    main_notebook: &Notebook,
    start: StructureTab,
) {
    let window = Window::builder()
        .title("Structure")
        .transient_for(parent)
        .default_width(1060)
        .default_height(660)
        .modal(false)
        .build();

    let preview = Preview::new(state.clone());
    let notebook = Notebook::new();
    notebook.set_scrollable(true);

    // (preview slot, on_enter) per tab; Slab has no slot.
    let mut slots: Vec<Option<GtkBox>> = Vec::new();
    let mut enters: Vec<Box<dyn Fn()>> = Vec::new();

    let mut add_preview_tab = |title: &str, parts: TabParts| {
        let (page, slot) = page_with_preview_slot(&parts.controls);
        notebook.append_page(&page, Some(&Label::new(Some(title))));
        slots.push(Some(slot));
        enters.push(parts.on_enter);
    };
    add_preview_tab(
        "Supercell",
        supercell_tab::build(state.clone(), main_notebook, preview.clone()),
    );
    add_preview_tab(
        "Basis",
        basis_tab::build(state.clone(), main_notebook, preview.clone()),
    );
    add_preview_tab(
        "Atom Instances",
        instances_tab::build(state.clone(), main_notebook, preview.clone()),
    );

    let (slab_page, slab_enter) = slab_tab::build(state.clone());
    notebook.append_page(&slab_page, Some(&Label::new(Some("Slab"))));
    slots.push(None);
    enters.push(slab_enter);

    // The preview lives in the slot of the tab on screen.
    let slots = Rc::new(slots);
    let enters = Rc::new(enters);
    let move_preview = {
        let preview = preview.clone();
        let slots = slots.clone();
        let enters = enters.clone();
        move |idx: usize| {
            if let Some(Some(slot)) = slots.get(idx) {
                let w = preview.widget();
                if let Some(old) = w.parent().and_then(|p| p.downcast::<GtkBox>().ok()) {
                    if &old != slot {
                        old.remove(w);
                    }
                }
                if w.parent().is_none() {
                    slot.append(w);
                }
            }
            if let Some(enter) = enters.get(idx) {
                enter();
            }
        }
    };
    {
        let mv = move_preview.clone();
        notebook.connect_switch_page(move |_, _, n| mv(n as usize));
    }

    window.set_child(Some(&notebook));
    // switch-page does not fire for the page that is already current, so
    // place the preview and run the first refresh by hand.
    move_preview(0);
    notebook.set_current_page(Some(start as u32));
    window.present();
}
