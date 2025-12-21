use gtk4::gio;

pub fn build_menu_model() -> gio::Menu {
    let menu_bar = gio::Menu::new();

    // --- FILE MENU ---
    let menu_file = gio::Menu::new();
    menu_file.append(Some("Open..."), Some("win.open"));
    menu_file.append(Some("Export Image..."), Some("win.export"));
    menu_file.append(Some("Preferences"), Some("win.preferences"));
    menu_file.append(Some("Close"), Some("win.close"));
    menu_bar.append_submenu(Some("File"), &menu_file);

    // --- VIEW MENU ---
    let menu_view = gio::Menu::new();
    menu_view.append(Some("Reset View"), Some("win.reset_view"));
    menu_view.append(Some("Toggle Rotation Center"), Some("win.toggle_center"));
    menu_view.append(Some("Toggle Export Format"), Some("win.toggle_format"));

    // Submenu: Alignment
    let menu_align = gio::Menu::new();
    menu_align.append(Some("Along X-Axis"), Some("win.view_x"));
    menu_align.append(Some("Along Y-Axis"), Some("win.view_y"));
    menu_align.append(Some("Along Z-Axis"), Some("win.view_z"));
    menu_view.append_submenu(Some("Align View"), &menu_align);

    menu_bar.append_submenu(Some("View"), &menu_view);

    menu_bar
}
