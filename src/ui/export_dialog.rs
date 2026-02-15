// src/ui/export_dialog.rs
// Advanced Export Dialog - Add this new file to your project

use gtk4::prelude::*;
use gtk4::{
    ApplicationWindow, Box as GtkBox, CheckButton, ComboBoxText, Dialog, FileChooserAction,
    FileChooserNative, Label, Orientation, ResponseType, SpinButton,
};
use std::cell::RefCell;
use std::rc::Rc;

use crate::rendering::export::{
    export_for_journal, export_for_presentation, export_for_web, export_pdf_advanced,
    export_png_advanced, ExportFormat, ExportSettings,
};
use crate::state::AppState;

/// Show the advanced export dialog
pub fn show_export_dialog(window: &ApplicationWindow, state: Rc<RefCell<AppState>>) {
    let dialog = Dialog::builder()
        .title("Export Image")
        .transient_for(window)
        .modal(true)
        .default_width(450)
        .default_height(400)
        .build();

    let content = dialog.content_area();
    let vbox = GtkBox::new(Orientation::Vertical, 15);
    vbox.set_margin_top(15);
    vbox.set_margin_bottom(15);
    vbox.set_margin_start(20);
    vbox.set_margin_end(20);

    // ========================================================================
    // FORMAT SELECTION
    // ========================================================================
    let format_box = GtkBox::new(Orientation::Horizontal, 10);
    let format_label = Label::new(Some("Format:"));
    format_label.set_width_chars(12);
    format_label.set_xalign(0.0);

    let format_combo = ComboBoxText::new();
    format_combo.append_text("PNG (Raster Image)");
    format_combo.append_text("PDF (Vector Document)");
    // #[cfg(feature = "svg")]
    format_combo.append_text("SVG (Editable Vector)");
    format_combo.set_active(Some(0));
    format_combo.set_hexpand(true);

    format_box.append(&format_label);
    format_box.append(&format_combo);
    vbox.append(&format_box);

    // ========================================================================
    // PRESET SELECTION
    // ========================================================================
    let preset_box = GtkBox::new(Orientation::Horizontal, 10);
    let preset_label = Label::new(Some("Preset:"));
    preset_label.set_width_chars(12);
    preset_label.set_xalign(0.0);

    let preset_combo = ComboBoxText::new();
    preset_combo.append_text("Custom");
    preset_combo.append_text("📄 Journal (600 DPI, White BG)");
    preset_combo.append_text("📊 Presentation (150 DPI, Transparent)");
    preset_combo.append_text("🌐 Web (800px, 72 DPI)");
    preset_combo.set_active(Some(0));
    preset_combo.set_hexpand(true);

    preset_box.append(&preset_label);
    preset_box.append(&preset_combo);
    vbox.append(&preset_box);

    // ========================================================================
    // QUALITY SETTINGS (Custom only)
    // ========================================================================
    let settings_box = GtkBox::new(Orientation::Vertical, 10);
    settings_box.set_margin_start(10);

    // DPI
    let dpi_box = GtkBox::new(Orientation::Horizontal, 10);
    let dpi_label = Label::new(Some("DPI:"));
    dpi_label.set_width_chars(12);
    dpi_label.set_xalign(0.0);

    let dpi_spin = SpinButton::with_range(72.0, 600.0, 10.0);
    dpi_spin.set_value(300.0);
    dpi_spin.set_digits(0);
    dpi_spin.set_hexpand(true);

    dpi_box.append(&dpi_label);
    dpi_box.append(&dpi_spin);
    settings_box.append(&dpi_box);

    // Scale
    let scale_box = GtkBox::new(Orientation::Horizontal, 10);
    let scale_label = Label::new(Some("Zoom:"));
    scale_label.set_width_chars(12);
    scale_label.set_xalign(0.0);

    let scale_spin = SpinButton::with_range(10.0, 200.0, 5.0);
    scale_spin.set_value(100.0);
    scale_spin.set_digits(0);
    scale_spin.set_hexpand(true);

    scale_box.append(&scale_label);
    scale_box.append(&scale_spin);
    settings_box.append(&scale_box);

    vbox.append(&settings_box);

    // ========================================================================
    // OPTIONS
    // ========================================================================
    let options_label = Label::new(Some("Options:"));
    options_label.set_xalign(0.0);
    options_label.set_margin_top(5);
    vbox.append(&options_label);

    let transparent_check = CheckButton::with_label("Transparent Background (PNG only)");
    transparent_check.set_margin_start(10);
    vbox.append(&transparent_check);

    let axes_check = CheckButton::with_label("Include Coordinate Axes");
    axes_check.set_active(true);
    axes_check.set_margin_start(10);
    vbox.append(&axes_check);

    let unit_cell_check = CheckButton::with_label("Include Unit Cell Box");
    unit_cell_check.set_active(true);
    unit_cell_check.set_margin_start(10);
    vbox.append(&unit_cell_check);

    // ========================================================================
    // PRESET HANDLER - Update settings when preset changes
    // ========================================================================
    let dpi_spin_preset = dpi_spin.clone();
    let transparent_preset = transparent_check.clone();
    let settings_box_preset = settings_box.clone();

    preset_combo.connect_changed(move |combo| {
        let idx = combo.active().unwrap_or(0);

        // Show/hide custom settings
        settings_box_preset.set_visible(idx == 0);

        // Update values based on preset
        match idx {
            1 => {
                // Journal
                dpi_spin_preset.set_value(600.0);
                transparent_preset.set_active(false);
            }
            2 => {
                // Presentation
                dpi_spin_preset.set_value(150.0);
                transparent_preset.set_active(true);
            }
            3 => {
                // Web
                dpi_spin_preset.set_value(72.0);
                transparent_preset.set_active(true);
            }
            _ => {} // Custom - leave as is
        }
    });

    content.append(&vbox);

    // ========================================================================
    // BUTTONS
    // ========================================================================
    dialog.add_button("Cancel", ResponseType::Cancel);
    dialog.add_button("Export", ResponseType::Ok);

    let state_dialog = state.clone();
    let window_weak = window.downgrade();

    dialog.connect_response(move |dialog_ref, response| {
        if response == ResponseType::Ok {
            // Get format
            let format_idx = format_combo.active().unwrap_or(0);
            let extension = match format_idx {
                0 => "png",
                1 => "pdf",
                2 => "svg",
                _ => "png",
            };

            // Show file chooser
            if let Some(win) = window_weak.upgrade() {
                let file_dialog = FileChooserNative::new(
                    Some("Save Export"),
                    Some(&win),
                    FileChooserAction::Save,
                    Some("Save"),
                    Some("Cancel"),
                );

                // Add filter based on format
                let filter = gtk4::FileFilter::new();
                match format_idx {
                    0 => {
                        filter.set_name(Some("PNG Image"));
                        filter.add_pattern("*.png");
                    }
                    1 => {
                        filter.set_name(Some("PDF Document"));
                        filter.add_pattern("*.pdf");
                    }
                    2 => {
                        filter.set_name(Some("SVG Vector"));
                        filter.add_pattern("*.svg");
                    }
                    _ => {}
                }
                file_dialog.add_filter(&filter);

                // Set suggested filename
                file_dialog.set_current_name(&format!("structure.{}", extension));

                let state_save = state_dialog.clone();
                let preset_idx = preset_combo.active().unwrap_or(0);
                let dpi = dpi_spin.value() as u32;
                let scale = scale_spin.value();
                let transparent = transparent_check.is_active();
                let axes = axes_check.is_active();
                let unit_cell = unit_cell_check.is_active();

                file_dialog.connect_response(move |chooser, resp| {
                    if resp == ResponseType::Accept {
                        if let Some(file) = chooser.file() {
                            if let Some(path) = file.path() {
                                let path_str = path.to_string_lossy().to_string();

                                // Export using appropriate method
                                let result = if preset_idx > 0 {
                                    // Use preset
                                    match preset_idx {
                                        1 => {
                                            // Journal
                                            let format = match format_idx {
                                                0 => ExportFormat::PNG,
                                                1 => ExportFormat::PDF,
                                                2 => ExportFormat::SVG,
                                                _ => ExportFormat::PNG,
                                            };
                                            export_for_journal(
                                                state_save.clone(),
                                                &path_str,
                                                format,
                                            )
                                        }
                                        2 => {
                                            // Presentation
                                            export_for_presentation(state_save.clone(), &path_str)
                                        }
                                        3 => {
                                            // Web
                                            export_for_web(state_save.clone(), &path_str)
                                        }
                                        _ => Err("Unknown preset".to_string()),
                                    }
                                } else {
                                    // Custom settings
                                    let settings = ExportSettings {
                                        dpi,
                                        scale,
                                        transparent,
                                        include_axes: axes,
                                        include_unit_cell: unit_cell,
                                        ..ExportSettings::default()
                                    };

                                    match format_idx {
                                        0 => export_png_advanced(
                                            state_save.clone(),
                                            &path_str,
                                            settings,
                                        ),
                                        1 => export_pdf_advanced(
                                            state_save.clone(),
                                            &path_str,
                                            settings,
                                        ),
                                        2 => {
                                            use crate::rendering::export::export_svg_advanced;
                                            export_svg_advanced(
                                                state_save.clone(),
                                                &path_str,
                                                settings,
                                            )
                                        }
                                        _ => Err("Unknown format".to_string()),
                                    }
                                };

                                // Show result - print to console instead of dialog
                                match result {
                                    Ok(msg) => {
                                        println!("✓ {}", msg);
                                    }
                                    Err(e) => {
                                        eprintln!("✗ Export failed: {}", e);
                                    }
                                }
                            }
                        }
                    }
                    chooser.destroy();
                });

                file_dialog.show();
            }
        }
        dialog_ref.close();
    });

    dialog.show();
}
