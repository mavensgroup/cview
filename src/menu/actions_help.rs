use gtk4::prelude::*;
use gtk4::{Application, ApplicationWindow, AboutDialog, License};

pub fn setup(app: &Application, window: &ApplicationWindow) {

    // --- ABOUT ACTION ---
    let about_action = gtk4::gio::SimpleAction::new("about", None);
    let win_weak = window.downgrade();

    about_action.connect_activate(move |_, _| {
        if let Some(win) = win_weak.upgrade() {
            let dialog = AboutDialog::builder()
                .transient_for(&win)
                .modal(true)
                .program_name("cview")
                .version("0.1.0")
                .comments("A high-performance crystal structure viewer written in Rust and GTK4.")
                .authors(vec!["Rudra".to_string()])
                .website("https://github.com/mavensgroup/cview")
                .license_type(License::MitX11)
                .logo_icon_name("applications-science")
                .build();

            dialog.present();
        }
    });
    app.add_action(&about_action);


    // --- HELP / DOCS ACTION ---
    let help_action = gtk4::gio::SimpleAction::new("help", None);
    let win_weak_h = window.downgrade();

    help_action.connect_activate(move |_, _| {
        if let Some(win) = win_weak_h.upgrade() {
             let url = "https://github.com/yourusername/cview";

             // FIX:
             // 1. Wrap window in Some()
             // 2. Do not check for Result (it returns void)
             gtk4::show_uri(Some(&win), url, gtk4::gdk::CURRENT_TIME);
        }
    });
    app.add_action(&help_action);
}
