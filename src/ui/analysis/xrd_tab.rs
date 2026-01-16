// src/ui/analysis/xrd_tab.rs

use crate::state::AppState;
use gtk4::glib;
use gtk4::prelude::*;
use gtk4::{
    Align, Button, FileChooserAction, FileChooserNative, FileFilter, Frame, Grid, Label,
    Orientation, ResponseType, SpinButton,
};
use std::cell::RefCell;
use std::rc::Rc;

use crate::io::xrd_exp::{self, ExperimentalData};
use crate::physics::analysis::xrd::{calculate_pattern, XRDPattern, XRDSettings};

use cairo::{Context, PdfSurface};
use plotters::backend::DrawingBackend;
use plotters::prelude::*;
use plotters::style::full_palette::GREY;
use plotters_cairo::CairoBackend;

// Calamine imports
use calamine::{open_workbook_auto, Data, Reader};

struct PlotState {
    peaks: Option<Vec<XRDPattern>>,
    exp_data: Option<ExperimentalData>,
    settings: XRDSettings,
}

fn draw_xrd_chart<DB: DrawingBackend>(
    root: &plotters::drawing::DrawingArea<DB, plotters::coord::Shift>,
    peaks: &Vec<XRDPattern>,
    exp_data: &Option<ExperimentalData>,
    settings: &XRDSettings,
) -> Result<(), std::boxed::Box<dyn std::error::Error>>
where
    DB::ErrorType: 'static,
{
    // 1. Generate Raw Simulated Curve (Gaussian convolution)
    let sigma = settings.smoothing / 2.355;
    let step = 0.05;
    let mut raw_curve = Vec::new();
    let mut t = settings.min_2theta;

    while t <= settings.max_2theta {
        let mut i_sum = 0.0;
        for p in peaks {
            let x = t - p.two_theta;
            // Optimization: Only calc Gaussian if close to peak
            if x.abs() < 5.0 * sigma {
                i_sum += p.intensity * f64::exp(-0.5 * (x / sigma).powi(2));
            }
        }
        raw_curve.push((t, i_sum));
        t += step;
    }

    // 2. NORMALIZE Simulation to 0-100%
    let max_sim = raw_curve.iter().map(|(_, y)| *y).fold(0.0f64, f64::max);
    let scale = if max_sim > 1e-6 { 100.0 / max_sim } else { 1.0 };

    let sim_curve: Vec<(f64, f64)> = raw_curve.into_iter().map(|(t, i)| (t, i * scale)).collect();

    // 3. Draw Chart Frame
    root.fill(&WHITE)?;

    let mut chart = ChartBuilder::on(root)
        .caption("XRD Pattern Comparison", ("sans-serif", 20).into_font())
        .margin(10)
        .x_label_area_size(40)
        .y_label_area_size(40)
        .build_cartesian_2d(settings.min_2theta..settings.max_2theta, 0.0..115.0)?; // Extra Y space for labels

    chart
        .configure_mesh()
        .x_desc("2Theta (deg)")
        .y_desc("Intensity (%)")
        .draw()?;

    // 4. Draw Experimental Data (Background, Grey)
    if let Some(data) = exp_data {
        let label_name = format!("Exp: {}", data.name);

        chart
            .draw_series(LineSeries::new(
                data.points
                    .iter()
                    .map(|(x, y)| (*x, *y))
                    .filter(|(x, _)| *x >= settings.min_2theta && *x <= settings.max_2theta),
                &GREY,
            ))?
            .label(label_name)
            .legend(|(x, y)| PathElement::new(vec![(x, y), (x + 20, y)], &GREY));
    }

    // 5. Draw Simulation (Foreground, Red)
    chart
        .draw_series(LineSeries::new(sim_curve, RED.stroke_width(2)))?
        .label("Simulation")
        .legend(|(x, y)| PathElement::new(vec![(x, y), (x + 20, y)], &RED));

    // 6. DRAW LABELS (h k l) on top of peaks
    let visible_peaks: Vec<&XRDPattern> = peaks
        .iter()
        .filter(|p| {
            p.two_theta >= settings.min_2theta
                && p.two_theta <= settings.max_2theta
                && p.intensity > 1.5
        })
        .collect();

    chart.draw_series(visible_peaks.iter().enumerate().map(|(i, p)| {
        let (h, k, l) = p.hkl[0];
        let label = format!("({} {} {})", h, k, l);

        let y_offset = if i % 2 == 0 { -15 } else { -28 };

        EmptyElement::at((p.two_theta, p.intensity))
            + Text::new(label, (0, y_offset), ("sans-serif", 10).into_font())
    }))?;

    chart
        .configure_series_labels()
        .background_style(&WHITE.mix(0.8))
        .border_style(&BLACK)
        .draw()?;

    Ok(())
}

pub fn build(state: Rc<RefCell<AppState>>) -> gtk4::Box {
    let root = gtk4::Box::new(Orientation::Horizontal, 15);
    root.set_margin_top(15);
    root.set_margin_bottom(15);
    root.set_margin_start(15);
    root.set_margin_end(15);

    let plot_state = Rc::new(RefCell::new(PlotState {
        peaks: None,
        exp_data: None,
        settings: XRDSettings::default(),
    }));

    // LEFT PANE (Plot)
    let left_pane = gtk4::Box::new(Orientation::Vertical, 5);
    left_pane.set_hexpand(true);

    let frame_plot = Frame::new(Some(" Diffractogram "));
    let drawing_area = gtk4::DrawingArea::new();
    drawing_area.set_content_width(600);
    drawing_area.set_content_height(400);
    drawing_area.set_hexpand(true);
    drawing_area.set_vexpand(true);

    frame_plot.set_child(Some(&drawing_area));
    left_pane.append(&frame_plot);
    root.append(&left_pane);

    // RIGHT PANE (Controls)
    let right_pane = gtk4::Box::new(Orientation::Vertical, 10);
    right_pane.set_width_request(260);

    let title = Label::new(Some("Settings"));
    title.add_css_class("title-3");
    title.set_halign(Align::Start);
    right_pane.append(&title);

    let frame_settings = Frame::new(None);
    let grid = Grid::new();
    grid.set_row_spacing(10);
    grid.set_column_spacing(10);
    grid.set_margin_top(10);
    grid.set_margin_bottom(10);
    grid.set_margin_start(10);
    grid.set_margin_end(10);

    let adj_min = gtk4::Adjustment::new(10.0, 0.0, 180.0, 1.0, 5.0, 0.0);
    let spin_min = SpinButton::new(Some(&adj_min), 1.0, 1);

    let adj_max = gtk4::Adjustment::new(90.0, 0.0, 180.0, 1.0, 5.0, 0.0);
    let spin_max = SpinButton::new(Some(&adj_max), 1.0, 1);

    let adj_smooth = gtk4::Adjustment::new(0.2, 0.01, 2.0, 0.05, 0.1, 0.0);
    let spin_smooth = SpinButton::new(Some(&adj_smooth), 0.05, 2);

    let adj_wave = gtk4::Adjustment::new(1.5406, 0.1, 5.0, 0.0001, 0.01, 0.0);
    let spin_wave = SpinButton::new(Some(&adj_wave), 0.0001, 4);

    grid.attach(&Label::new(Some("Min 2θ:")), 0, 0, 1, 1);
    grid.attach(&spin_min, 1, 0, 1, 1);
    grid.attach(&Label::new(Some("Max 2θ:")), 0, 1, 1, 1);
    grid.attach(&spin_max, 1, 1, 1, 1);
    grid.attach(&Label::new(Some("FWHM:")), 0, 2, 1, 1);
    grid.attach(&spin_smooth, 1, 2, 1, 1);
    grid.attach(&Label::new(Some("λ (Å):")), 0, 3, 1, 1);
    grid.attach(&spin_wave, 1, 3, 1, 1);

    frame_settings.set_child(Some(&grid));
    right_pane.append(&frame_settings);

    let btn_calc = Button::with_label("Recalculate");
    btn_calc.add_css_class("suggested-action");
    right_pane.append(&btn_calc);

    let btn_load_exp = Button::with_label("Load Experiment");
    right_pane.append(&btn_load_exp);

    let btn_export = Button::with_label("Export PDF");
    right_pane.append(&btn_export);

    root.append(&right_pane);

    // LOGIC
    let ps = plot_state.clone();

    drawing_area.set_draw_func(move |_, ctx, w, h| {
        let state = ps.borrow();
        if let Some(peaks) = &state.peaks {
            let backend = CairoBackend::new(ctx, (w as u32, h as u32)).unwrap();
            let root = backend.into_drawing_area();
            draw_xrd_chart(&root, peaks, &state.exp_data, &state.settings).unwrap();
        } else {
            let backend = CairoBackend::new(ctx, (w as u32, h as u32)).unwrap();
            let root = backend.into_drawing_area();
            root.fill(&WHITE).unwrap();
            let style = TextStyle::from(("sans-serif", 20).into_font()).color(&BLACK);
            root.draw_text(
                "Click 'Recalculate' to Simulate",
                &style,
                (w as i32 / 2 - 140, h as i32 / 2),
            )
            .unwrap();
        }
    });

    // Recalculate Logic
    let ps_calc = plot_state.clone();
    let st_calc = state.clone();
    let da_calc = drawing_area.clone();

    let refresh_plot = Rc::new(move || {
        let mut ps = ps_calc.borrow_mut();
        ps.settings.min_2theta = spin_min.value();
        ps.settings.max_2theta = spin_max.value();
        ps.settings.smoothing = spin_smooth.value();
        ps.settings.wavelength = spin_wave.value();

        let app_st = st_calc.borrow();
        // FIX: Access the active tab
        let tab = app_st.active_tab();
        if let Some(structure) = &tab.structure {
            let peaks = calculate_pattern(structure, &ps.settings);
            ps.peaks = Some(peaks);
            da_calc.queue_draw();
        }
    });

    let refresh = refresh_plot.clone();
    btn_calc.connect_clicked(move |_| refresh());

    // --- LOAD EXPERIMENT LOGIC (ASC/XY/EXCEL) ---
    let ps_exp = plot_state.clone();
    let da_exp = drawing_area.clone();
    let parent_window = root.root().and_then(|r| r.downcast::<gtk4::Window>().ok());

    btn_load_exp.connect_clicked(move |_| {
        let native = FileChooserNative::new(
            Some("Open Experimental Data"),
            parent_window.as_ref(),
            FileChooserAction::Open,
            Some("Open"),
            Some("Cancel"),
        );

        let filter = FileFilter::new();
        filter.set_name(Some("XRD Data"));
        filter.add_pattern("*.asc");
        filter.add_pattern("*.ASC");
        filter.add_pattern("*.xy");
        filter.add_pattern("*.XY");
        filter.add_pattern("*.xlsx");
        filter.add_pattern("*.xls");

        native.add_filter(&filter);

        native.connect_response(glib::clone!(@strong ps_exp, @strong da_exp => move |d, response| {
            if response == ResponseType::Accept {
                if let Some(file) = d.file() {
                    if let Some(path) = file.path() {
                        let path_str = path.to_str().unwrap_or_default();
                        let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("").to_lowercase();

                        let result = if ext == "xlsx" || ext == "xls" {
                             parse_excel_local(&path)
                        } else {
                             xrd_exp::parse(path_str).map_err(|e| e.to_string())
                        };

                        match result {
                            Ok(data) => {
                                println!("Loaded: {} with {} points", data.name, data.points.len());
                                ps_exp.borrow_mut().exp_data = Some(data);
                                da_exp.queue_draw();
                            },
                            Err(e) => println!("Error loading experiment: {}", e),
                        }
                    }
                }
            }
        }));

        native.show();
    });

    // Export PDF
    let ps_export = plot_state.clone();

    btn_export.connect_clicked(move |_| {
        let ps = ps_export.borrow();
        if let Some(peaks) = &ps.peaks {
            let native = FileChooserNative::new(
                Some("Export PDF"),
                None::<&gtk4::Window>,
                FileChooserAction::Save,
                Some("Save"),
                Some("Cancel"),
            );
            native.set_current_name("xrd_comparison.pdf");

            let peaks_ex: Vec<XRDPattern> = peaks.clone();
            let exp_ex: Option<ExperimentalData> = ps.exp_data.clone();
            let settings_ex: XRDSettings = ps.settings.clone();

            native.connect_response(move |d, resp| {
                if resp == ResponseType::Accept {
                    if let Some(f) = d.file() {
                        if let Some(p) = f.path() {
                            let w = 800.0;
                            let h = 600.0;
                            let surf = PdfSurface::new(w, h, &p).expect("PDF Error");
                            let ctx = Context::new(&surf).expect("Context Error");
                            let backend = CairoBackend::new(&ctx, (w as u32, h as u32)).unwrap();
                            let root = backend.into_drawing_area();
                            draw_xrd_chart(&root, &peaks_ex, &exp_ex, &settings_ex).unwrap();
                            surf.finish();
                            println!("PDF Saved.");
                        }
                    }
                }
            });
            native.show();
        }
    });

    root
}

// --- HELPER: Parse Excel Files Locally ---
fn parse_excel_local(path: &std::path::Path) -> Result<ExperimentalData, String> {
    let mut workbook = open_workbook_auto(path).map_err(|e| e.to_string())?;

    let sheet_name = workbook
        .sheet_names()
        .first()
        .cloned()
        .ok_or("No sheets in Excel file")?;

    let range = workbook
        .worksheet_range(&sheet_name)
        .map_err(|e| e.to_string())?;

    let mut points = Vec::new();

    for row in range.rows() {
        if row.len() < 2 {
            continue;
        }

        let x_val = match row[0] {
            Data::Float(v) => Some(v),
            Data::Int(v) => Some(v as f64),
            _ => None,
        };

        let y_val = match row[1] {
            Data::Float(v) => Some(v),
            Data::Int(v) => Some(v as f64),
            _ => None,
        };

        if let (Some(x), Some(y)) = (x_val, y_val) {
            points.push((x, y));
        }
    }

    if points.is_empty() {
        return Err("No numeric data found in first two columns of the first sheet.".to_string());
    }

    let max_intensity = points.iter().map(|(_, y)| *y).fold(0.0f64, f64::max);

    if max_intensity > 1e-10 {
        for (_, y) in &mut points {
            *y = (*y / max_intensity) * 100.0;
        }
    }

    points.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));

    Ok(ExperimentalData {
        name: path
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string(),
        points,
    })
}
