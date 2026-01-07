// src/ui/analysis/xrd_tab.rs

// 1. Explicitly import only what doesn't conflict, or use fully qualified names
use gtk4::prelude::*;
use gtk4::{Orientation, Button, Label, Entry, ResponseType, FileChooserDialog, FileChooserAction};

// We DO NOT import 'Box' or 'DrawingArea' from gtk4 here to avoid confusion.
// We will refer to them as 'gtk4::Box' and 'gtk4::DrawingArea' in the code.

use std::rc::Rc;
use std::cell::RefCell;
use crate::state::AppState;
use crate::physics::xrd::{calculate_pattern, XRDSettings, XRDPattern};

// Plotters imports
use plotters::prelude::*;
use plotters::style::TextStyle;
use plotters::drawing::DrawingArea; // This is the Plotters canvas
use plotters::coord::Shift;
use plotters::backend::DrawingBackend;
use plotters_cairo::CairoBackend;

// Import cairo for PDF Surface creation
// Note: If this fails, ensure 'cairo-rs' is in your Cargo.toml,
// or try 'use gtk4::cairo;' if it's re-exported.
use cairo::{PdfSurface, Context};

// --- Helper Function: Draws the Chart to ANY Backend (Screen or PDF) ---
// We use 'std::boxed::Box' explicitly for the error return type
fn draw_xrd_chart<DB: DrawingBackend>(
    root: &DrawingArea<DB, Shift>, // Pass by reference
    peaks: &Vec<XRDPattern>
) -> Result<(), std::boxed::Box<dyn std::error::Error>>
where DB::ErrorType: 'static {

    // 1. Generate Raw Continuous Profile
    let min_theta = 10.0;
    let max_theta = 90.0;
    let step = 0.05;
    let sigma = 0.2;

    let mut raw_curve = Vec::new();
    let mut t = min_theta;

    while t <= max_theta {
        let mut intensity_sum = 0.0;
        for peak in peaks {
            let diff = t - peak.two_theta;
            if diff.abs() < 5.0 * sigma {
                intensity_sum += peak.intensity * (-0.5 * (diff / sigma).powi(2)).exp();
            }
        }
        raw_curve.push((t, intensity_sum));
        t += step;
    }

    // 2. Normalize
    let max_raw = raw_curve.iter().map(|(_, y)| *y).fold(0.0f64, f64::max);
    let scale_factor = if max_raw > 1e-9 { 100.0 / max_raw } else { 1.0 };

    let final_curve: Vec<(f64, f64)> = raw_curve.into_iter()
        .map(|(t, y)| (t, y * scale_factor))
        .collect();

    // 3. Build Chart
    let chart_max_y = 115.0;

    // Note: We use 'root' directly here (it's already a reference to a DrawingArea)
    let mut chart = ChartBuilder::on(root)
        .caption("Simulated XRD Pattern", ("sans-serif", 20))
        .margin(20)
        .x_label_area_size(50)
        .y_label_area_size(60)
        .build_cartesian_2d(min_theta..max_theta, 0.0..chart_max_y)?;

    chart.configure_mesh()
        .x_desc("2θ°")
        .y_desc("Relative Intensity (%)")
        .axis_desc_style(("sans-serif", 20))
        .draw()?;

    chart.draw_series(LineSeries::new(
        final_curve,
        &BLUE,
    ))?;

    // 4. Smart Labeling (Grouped/Clustered)
    let mut i = 0;
    while i < peaks.len() {
        let current_peak = &peaks[i];

        let mut cluster_intensity = current_peak.intensity;
        let mut best_hkl = current_peak.hkl;
        let mut max_in_cluster = current_peak.intensity;

        let mut j = i + 1;
        while j < peaks.len() {
            let next_peak = &peaks[j];
            if (next_peak.two_theta - current_peak.two_theta).abs() > 0.5 {
                break;
            }
            cluster_intensity += next_peak.intensity;
            if next_peak.intensity > max_in_cluster {
                max_in_cluster = next_peak.intensity;
                best_hkl = next_peak.hkl;
            }
            j += 1;
        }

        let visual_height = cluster_intensity * scale_factor;

        if visual_height > 2.0 {
            chart.draw_series(std::iter::once(Text::new(
                format!("({} {} {})", best_hkl.0, best_hkl.1, best_hkl.2),
                (current_peak.two_theta, visual_height + 3.0),
                ("sans-serif", 12).into_font(),
            )))?;
        }
        i = j;
    }

    Ok(())
}

// --- Main Build Function ---
// Returns gtk4::Box explicitly
pub fn build(state: Rc<RefCell<AppState>>) -> gtk4::Box {
    let root = gtk4::Box::new(Orientation::Vertical, 10);
    root.set_margin_top(10);
    root.set_margin_bottom(10);
    root.set_margin_start(10);
    root.set_margin_end(10);

    // --- Controls ---
    let controls = gtk4::Box::new(Orientation::Horizontal, 10);
    controls.append(&Label::new(Some("λ (Å):")));

    let entry_lambda = Entry::new();
    entry_lambda.set_text("1.5406");
    entry_lambda.set_width_chars(6);
    controls.append(&entry_lambda);

    let btn_calc = Button::with_label("Calculate Pattern");
    controls.append(&btn_calc);

    let btn_export = Button::with_label("Export PDF");
    controls.append(&btn_export);

    root.append(&controls);

    // --- Plot Area (GTK Widget) ---
    // Use fully qualified name to distinguish from Plotters DrawingArea
    let drawing_area = gtk4::DrawingArea::new();
    drawing_area.set_hexpand(true);
    drawing_area.set_vexpand(true);
    drawing_area.set_content_height(400);

    let current_peaks: Rc<RefCell<Option<Vec<XRDPattern>>>> = Rc::new(RefCell::new(None));
    let peaks_clone = current_peaks.clone();

    // --- Draw Function (Screen) ---
    drawing_area.set_draw_func(move |_, context, width, height| {
        // Create Cairo Backend from the GTK context
        let backend = CairoBackend::new(context, (width as u32, height as u32)).unwrap();
        let root_area = backend.into_drawing_area();
        root_area.fill(&WHITE).unwrap();

        let peaks_ref = peaks_clone.borrow();

        if let Some(peaks) = &*peaks_ref {
            // Re-use the shared logic, passing the Plotters DrawingArea
            draw_xrd_chart(&root_area, peaks).unwrap();
        } else {
            let style = TextStyle::from(("sans-serif", 20).into_font()).color(&BLACK);
            root_area.draw_text(
                "No Data. Click Calculate.",
                &style,
                (width as i32 / 2 - 100, height as i32 / 2),
            ).unwrap();
        }
    });

    root.append(&drawing_area);

    // --- Interactions ---
    let da_clone = drawing_area.clone();
    let peaks_clone_2 = current_peaks.clone();

    // 1. Calculate Button
    btn_calc.connect_clicked(move |_| {
        let st = state.borrow();
        if let Some(structure) = &st.structure {
            let lambda: f64 = entry_lambda.text().parse().unwrap_or(1.5406);
            let settings = XRDSettings {
                wavelength: lambda,
                min_2theta: 10.0,
                max_2theta: 90.0,
                smoothing: 0.2,
            };
            let result = calculate_pattern(structure, &settings);
            *peaks_clone_2.borrow_mut() = Some(result);
            da_clone.queue_draw();
        }
    });

    // 2. Export PDF Button
    let peaks_clone_3 = current_peaks.clone();

    btn_export.connect_clicked(move |btn| {
        let peaks_ref = peaks_clone_3.borrow();

        if let Some(peaks) = &*peaks_ref {
            // Create File Chooser Dialog
            let window = btn.root().and_then(|root| root.downcast::<gtk4::Window>().ok());

            let dialog = FileChooserDialog::new(
                Some("Export XRD to PDF"),
                window.as_ref(),
                FileChooserAction::Save,
                &[("Cancel", ResponseType::Cancel), ("Save", ResponseType::Accept)],
            );
            dialog.set_current_name("xrd_pattern.pdf");

            let peaks_for_export = peaks.clone();

            dialog.connect_response(move |d, response| {
                if response == ResponseType::Accept {
                    if let Some(file) = d.file() {
                        if let Some(path) = file.path() {

                            // --- PDF EXPORT LOGIC ---
                            let width = 800.0;
                            let height = 600.0;

                            // 1. Create PDF Surface
                            let surface = PdfSurface::new(width, height, &path)
                                .expect("Failed to create PDF surface");

                            // 2. Create Context
                            let ctx = Context::new(&surface)
                                .expect("Failed to create Cairo context");

                            // 3. Create Plotters Backend
                            let backend = CairoBackend::new(&ctx, (width as u32, height as u32))
                                .unwrap();

                            let root_area = backend.into_drawing_area();
                            root_area.fill(&WHITE).unwrap();

                            // 4. Draw
                            if let Err(e) = draw_xrd_chart(&root_area, &peaks_for_export) {
                                eprintln!("Error exporting PDF: {:?}", e); // Use {:?} for box<dyn error>
                            } else {
                                println!("XRD exported successfully to {:?}", path);
                            }

                            // 5. Finish (ensure write)
                            surface.finish();
                        }
                    }
                }
                d.destroy();
            });

            dialog.present();
        } else {
            println!("No data to export!");
        }
    });

    root
}
