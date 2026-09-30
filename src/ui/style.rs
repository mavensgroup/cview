// src/ui/style.rs
// Loads the single app-wide stylesheet. All colors derive from the system
// theme's named colors, so light/dark follows the OS with no per-theme values.

use gtk4::gdk;
use gtk4::CssProvider;

const STYLE: &str = include_str!("style.css");

pub fn load() {
    let Some(display) = gdk::Display::default() else {
        return;
    };
    let provider = CssProvider::new();
    provider.load_from_data(STYLE);
    gtk4::style_context_add_provider_for_display(
        &display,
        &provider,
        gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
}

use gtk4::prelude::*;
use gtk4::{Align, Box as GtkBox, Label, Orientation, SpinButton};

/// A titled tonal card (see `.cview-group` in style.css). The child supplies
/// its own inner margins; use [`card_body`] for a ready-made padded child.
pub fn group(title: &str, child: &impl IsA<gtk4::Widget>) -> GtkBox {
    let outer = GtkBox::new(Orientation::Vertical, 0);
    outer.add_css_class("cview-group");
    let label = Label::builder()
        .label(title)
        .halign(Align::Start)
        .margin_top(12)
        .margin_start(12)
        .margin_end(12)
        .build();
    label.add_css_class("cview-group-title");
    outer.append(&label);
    outer.append(child);
    outer
}

/// A tonal card with no title, for when the dialog or tab title already names
/// the content. The child supplies its own inner margins.
pub fn surface(child: &impl IsA<gtk4::Widget>) -> GtkBox {
    let outer = GtkBox::new(Orientation::Vertical, 0);
    outer.add_css_class("cview-group");
    outer.append(child);
    outer
}

/// Vertical box with the standard card padding, for use as a [`group`] child.
pub fn card_body(spacing: i32) -> GtkBox {
    let b = GtkBox::new(Orientation::Vertical, spacing);
    b.set_margin_top(8);
    b.set_margin_bottom(12);
    b.set_margin_start(12);
    b.set_margin_end(12);
    b
}

/// A compact spin button for grids of numbers such as a transformation
/// matrix. Keeps the theme's native spin-button look.
pub fn plain_field(spin: &SpinButton) -> SpinButton {
    spin.add_css_class("cview-compact");
    spin.set_width_chars(2);
    spin.set_hexpand(true);
    spin.clone()
}

/// A compact spin button with a permanent dimmed prefix drawn inside it
/// (`h`, `k`, `l`), for short symbols. The prefix is an overlay, so it does
/// not depend on any theme styling the spin button; the number is inset with
/// a widget margin (not CSS) to leave room for it. Display only: the spin
/// button still holds a plain number, so nobody types the prefix.
pub fn prefixed_field(prefix: &str, spin: &SpinButton) -> gtk4::Overlay {
    plain_field(spin);
    if let Some(text) = spin.first_child() {
        text.set_margin_start(12 + 9 * prefix.chars().count() as i32);
    }
    let lbl = Label::new(Some(prefix));
    lbl.add_css_class("cview-field-prefix");
    lbl.set_halign(Align::Start);
    lbl.set_valign(Align::Center);
    lbl.set_margin_start(8);
    lbl.set_can_target(false);
    let overlay = gtk4::Overlay::new();
    overlay.set_hexpand(true);
    overlay.set_child(Some(spin));
    overlay.add_overlay(&lbl);
    overlay
}

/// A small dimmed caption above a widget, for labels that carry words or
/// units and would not fit inside the field.
pub fn captioned(caption: &str, child: &impl IsA<gtk4::Widget>) -> GtkBox {
    let b = GtkBox::new(Orientation::Vertical, 4);
    b.set_hexpand(true);
    let l = Label::builder().label(caption).halign(Align::Start).build();
    l.add_css_class("cview-caption");
    b.append(&l);
    b.append(child);
    b
}
