use gtk4::prelude::*;
use gtk4::{Orientation, Button, Label, ResponseType, FileChooserDialog, FileChooserAction, Align, Grid, Frame, SpinButton};
use std::rc::Rc;
use std::cell::RefCell;
use crate::state::AppState;
use crate::physics::xrd::{calculate_pattern, XRDSettings, XRDPattern};

// Plotters & Cairo Imports
use plotters::prelude::*;
use plotters::style::TextStyle;
use plotters::backend::DrawingBackend;
use plotters_cairo::CairoBackend;
use cairo::{PdfSurface, Context};

// State to hold data between draw calls
struct PlotState {
    peaks: Option<Vec<XRDPattern>>,
    settings: XRDSettings,
}

/// Helper: Draws the professional chart to any backend (Screen or PDF)
/// uses the styling and clustering logic you preferred.
fn draw_xrd_chart<DB: DrawingBackend>(
    root: &plotters::drawing::DrawingArea<DB, plotters::coord::Shift>,
    peaks: &Vec<XRDPattern>,
    settings: &XRDSettings
) -> Result<(), std::boxed::Box<dyn std::error::Error>>
where DB::ErrorType: 'static {

    // 1. Generate Raw Continuous Profile (Gaussian Broadening)
    // Convert FWHM to Sigma: sigma = FWHM / 2.355
    let sigma = settings.smoothing / 2.355;
    let step = 0.05;

    let mut curve_points = Vec::new();
    let mut t = settings.min_2theta;

    while t <= settings.max_2theta {
        let mut intensity = 0.0;
        for p in peaks {
            let diff = t - p.two_theta;
            // Optimization: only calc if within 5 sigma
            if diff.abs() < 5.0 * sigma {
                intensity += p.intensity * (-0.5 * (diff / sigma).powi(2)).exp();
            }
        }
        curve_points.push((t, intensity));
        t += step;
    }

    // Normalize to 100% relative intensity
    let max_y = curve_points.iter().map(|(_, y)| *y).fold(0.0f64, f64::max);
    let scale = if max_y > 1e-6 { 100.0 / max_y } else { 1.0 };

    let final_data: Vec<(f64, f64)> = curve_points.into_iter()
        .map(|(x, y)| (x, y * scale))
        .collect();

    // 2. Build the Chart (Using your preferred margins/fonts)
    root.fill(&WHITE)?;

    let mut chart = ChartBuilder::on(root)
        .caption("Simulated Powder Diffraction", ("sans-serif", 20).into_font())
        .margin(20)
        .x_label_area_size(50)
        .y_label_area_size(60)
        .build_cartesian_2d(settings.min_2theta..settings.max_2theta, 0.0..115.0)?;

    // 3. Configure Grid and Labels (Professional Style)
    chart.configure_mesh()
        .x_desc("2θ (Degrees)")
        .y_desc("Relative Intensity (%)")
        .axis_desc_style(("sans-serif", 20))
        .label_style(("sans-serif", 15))
        .draw()?;

    // 4. Draw the Blue Line
    chart.draw_series(LineSeries::new(
        final_data,
        BLUE.stroke_width(2), // Thicker line looks better
    ))?;

    // 5. Smart Labeling (Grouped/Clustered)
    // This logic prevents labels from overlapping by grouping nearby peaks
    let mut i = 0;
    while i < peaks.len() {
        let current_peak = &peaks[i];

        let mut cluster_intensity = current_peak.intensity;
        let mut best_hkl = current_peak.hkl;
        let mut max_in_cluster = current_peak.intensity;

        // Look ahead for nearby peaks to group
        let mut j = i + 1;
        while j < peaks.len() {
            let next_peak = &peaks[j];
            // If peaks are within 0.8 degrees, treat as one cluster
            if (next_peak.two_theta - current_peak.two_theta).abs() > 0.8 {
                break;
            }

            cluster_intensity += next_peak.intensity;
            if next_peak.intensity > max_in_cluster {
                max_in_cluster = next_peak.intensity;
                best_hkl = next_peak.hkl;
            }
            j += 1;
        }

        // Determine height of the label based on the curve intensity at this point
        // (Approximated by scaled intensity sum)
        let visual_height = cluster_intensity * scale;

        // Only draw label if it's significant (> 5%)
        if visual_height > 5.0 {
             chart.draw_series(std::iter::once(Text::new(
                format!("({} {} {})", best_hkl.0, best_hkl.1, best_hkl.2),
                (current_peak.two_theta, visual_height + 5.0),
                ("sans-serif", 12).into_font(),
            )))?;
        }

        // Advance outer loop
        i = j;
    }

    Ok(())
}

pub fn build(state: Rc<RefCell<AppState>>) -> gtk4::Box {
    // Root Layout (Horizontal)
    let root = gtk4::Box::new(Orientation::Horizontal, 15);
    root.set_margin_top(15);
    root.set_margin_bottom(15);
    root.set_margin_start(15);
    root.set_margin_end(15);

    // ================= LEFT PANE: Plot Area =================
    let left_pane = gtk4::Box::new(Orientation::Vertical, 5);
    left_pane.set_hexpand(true);

    let plot_frame = Frame::new(Some("Powder Diffraction Pattern"));

    // Use fully qualified name for GTK DrawingArea
    let drawing_area = gtk4::DrawingArea::new();
    drawing_area.set_content_height(400);
    drawing_area.set_content_width(500);
    drawing_area.set_hexpand(true);
    drawing_area.set_vexpand(true);

    plot_frame.set_child(Some(&drawing_area));
    left_pane.append(&plot_frame);
    root.append(&left_pane);

    // ================= RIGHT PANE: Controls =================
    let right_pane = gtk4::Box::new(Orientation::Vertical, 10);
    right_pane.set_width_request(250);

    let title = Label::new(Some("XRD Settings"));
    title.add_css_class("title-2");
    title.set_halign(Align::Start);
    right_pane.append(&title);

    let grid = Grid::new();
    grid.set_column_spacing(10);
    grid.set_row_spacing(10);

    // 1. Wavelength
    grid.attach(&Label::new(Some("Wavelength (Å):")), 0, 0, 1, 1);
    let spin_lambda = SpinButton::with_range(0.1, 5.0, 0.001);
    spin_lambda.set_value(1.5406); // Cu K-alpha
    grid.attach(&spin_lambda, 1, 0, 1, 1);

    // 2. Range (2-Theta)
    grid.attach(&Label::new(Some("Min 2θ:")), 0, 1, 1, 1);
    let spin_min = SpinButton::with_range(0.0, 180.0, 1.0);
    spin_min.set_value(10.0);
    grid.attach(&spin_min, 1, 1, 1, 1);

    grid.attach(&Label::new(Some("Max 2θ:")), 0, 2, 1, 1);
    let spin_max = SpinButton::with_range(0.0, 180.0, 1.0);
    spin_max.set_value(90.0);
    grid.attach(&spin_max, 1, 2, 1, 1);

    // 3. FWHM
    grid.attach(&Label::new(Some("FWHM (deg):")), 0, 3, 1, 1);
    let spin_fwhm = SpinButton::with_range(0.01, 2.0, 0.01);
    spin_fwhm.set_value(0.3);
    grid.attach(&spin_fwhm, 1, 3, 1, 1);

    right_pane.append(&grid);

    // Buttons
    let btn_calc = Button::with_label("Calculate Pattern");
    btn_calc.add_css_class("suggested-action");
    btn_calc.set_margin_top(20);
    right_pane.append(&btn_calc);

    let btn_export = Button::with_label("Export PDF");
    right_pane.append(&btn_export);

    // Status Label
    let lbl_status = Label::new(Some("Ready."));
    lbl_status.set_wrap(true);
    lbl_status.set_margin_top(10);
    right_pane.append(&lbl_status);

    root.append(&right_pane);

    // ================= LOGIC =================

    // Shared state for the Plotter
    let plot_state = Rc::new(RefCell::new(PlotState {
        peaks: None,
        settings: XRDSettings::default(),
    }));

    // --- Draw Function (Screen) ---
    let ps_draw = plot_state.clone();

    drawing_area.set_draw_func(move |_, context, w, h| {
        // Create Cairo backend
        let backend = CairoBackend::new(context, (w as u32, h as u32)).unwrap();
        let root_area = backend.into_drawing_area();

        let st = ps_draw.borrow();

        if let Some(peaks) = &st.peaks {
            // Draw chart using the shared helper
            if let Err(e) = draw_xrd_chart(&root_area, peaks, &st.settings) {
                eprintln!("Error drawing chart: {:?}", e);
            }
        } else {
            root_area.fill(&WHITE).unwrap();
            let style = TextStyle::from(("sans-serif", 20).into_font()).color(&BLACK);
            root_area.draw_text(
                "Load Structure & Click Calculate",
                &style,
                (w as i32 / 2 - 120, h as i32 / 2),
            ).unwrap();
        }
    });

    // --- Signal: Calculate ---
    let da_clone = drawing_area.clone();
    let ps_calc = plot_state.clone();
    let lbl_calc = lbl_status.clone();
    let state_calc = state.clone();

    btn_calc.connect_clicked(move |_| {
        let app_st = state_calc.borrow();
        if let Some(structure) = &app_st.structure {
            // 1. Gather Settings
            let settings = XRDSettings {
                wavelength: spin_lambda.value(),
                min_2theta: spin_min.value(),
                max_2theta: spin_max.value(),
                smoothing: spin_fwhm.value(), // Pass FWHM directly
            };

            // 2. Physics Calculation
            let result = calculate_pattern(structure, &settings);

            // 3. Update State
            {
                let mut ps = ps_calc.borrow_mut();
                ps.peaks = Some(result);
                ps.settings = settings;
            }

            lbl_calc.set_text("Calculation complete.");
            da_clone.queue_draw(); // Trigger redraw
        } else {
            lbl_calc.set_text("No structure loaded.");
        }
    });

    // --- Signal: Export PDF ---
    let ps_export = plot_state.clone();

    btn_export.connect_clicked(move |btn| {
        let ps = ps_export.borrow();
        if let Some(peaks) = &ps.peaks {
            // Open Save Dialog
            let window = btn.root().and_then(|root| root.downcast::<gtk4::Window>().ok());
            let dialog = FileChooserDialog::new(
                Some("Export XRD to PDF"),
                window.as_ref(),
                FileChooserAction::Save,
                &[("Cancel", ResponseType::Cancel), ("Save", ResponseType::Accept)],
            );
            dialog.set_current_name("xrd_pattern.pdf");

            // Clone data for the closure
            let peaks_ex = peaks.clone();
            let settings_ex = ps.settings.clone();

            dialog.connect_response(move |d, response| {
                if response == ResponseType::Accept {
                    if let Some(file) = d.file() {
                        if let Some(path) = file.path() {
                            // --- PDF Generation ---
                            let w = 800.0;
                            let h = 600.0;
                            let surf = PdfSurface::new(w, h, &path).expect("PDF Surface failed");
                            let ctx = Context::new(&surf).expect("Cairo Context failed");
                            let backend = CairoBackend::new(&ctx, (w as u32, h as u32)).unwrap();
                            let root = backend.into_drawing_area();

                            draw_xrd_chart(&root, &peaks_ex, &settings_ex).unwrap();

                            surf.finish();
                            println!("Exported to {:?}", path);
                        }
                    }
                }
                d.destroy();
            });
            dialog.present();
        } else {
            println!("No data to export");
        }
    });

    root
}
