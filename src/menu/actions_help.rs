// src/menu/actions_help.rs

use gtk4::prelude::*;
use gtk4::{AboutDialog, Application, ApplicationWindow, ButtonsType, MessageDialog, MessageType};

pub fn setup(app: &Application, window: &ApplicationWindow) {
    // --- 1. CONTROLS ACTION ---
    let controls_action = gtk4::gio::SimpleAction::new("help_controls", None);
    let win_weak_c = window.downgrade();

    controls_action.connect_activate(move |_, _| {
        let win = match win_weak_c.upgrade() {
            Some(w) => w,
            None => return,
        };

        let info_text = r#"<b>Mouse Controls:</b>
• <b>Left Click + Drag:</b> Rotate View
• <b>Right Click + Drag:</b> Pan View
• <b>Scroll Wheel:</b> Zoom In/Out

<b>Keyboard Shortcuts:</b>
• <b>Ctrl + O:</b> Open File
• <b>Ctrl + Shift + S:</b> Save As
• <b>Ctrl + E:</b> Export Image
• <b>Ctrl + P:</b> Preferences
• <b>Ctrl + R:</b> Reset View
• <b>Ctrl + B:</b> Toggle Bonds
• <b>Ctrl + Shift + C:</b> Supercell Tool
• <b>Ctrl + M:</b> Miller Indices Tool
"#;

        let dialog = MessageDialog::new(
            Some(&win),
            gtk4::DialogFlags::MODAL,
            MessageType::Info,
            ButtonsType::Ok,
            "Controls & Shortcuts",
        );
        dialog.set_markup(info_text);
        dialog.connect_response(|d, _| d.destroy());
        dialog.show();
    });
    app.add_action(&controls_action);

    // --- 2. MANUAL ACTION ---
    let manual_action = gtk4::gio::SimpleAction::new("help_manual", None);
    let win_weak_m = window.downgrade();

    manual_action.connect_activate(move |_, _| {
        let win = match win_weak_m.upgrade() {
            Some(w) => w,
            None => return,
        };

        gtk4::show_uri(
            Some(&win),
            "https://mavensgroup.github.io/cview/",
            gtk4::gdk::CURRENT_TIME,
        );
    });
    app.add_action(&manual_action);

    // --- 3. ABOUT ACTION (Safe & Portable) ---
    let about_action = gtk4::gio::SimpleAction::new("help_about", None);
    let win_weak_a = window.downgrade();

    about_action.connect_activate(move |_, _| {
        let win = match win_weak_a.upgrade() {
            Some(w) => w,
            None => return,
        };

        let dialog = AboutDialog::builder()
            .transient_for(&win)
            .modal(true)
            .program_name("CView")
            .version(env!("CARGO_PKG_VERSION"))
            .copyright("© 2026 Rudra")
            .authors(vec!["Rudra".to_string()])
            .comments("Crystal Structure Visualization and Analysis")
            .logo_icon_name("org.mavensgroup.cview")
            .website("https://mavensgroup.github.io/cview/")
            .license_type(gtk4::License::Gpl30)
            .build();

        dialog.show();
    });
    app.add_action(&about_action);
}
