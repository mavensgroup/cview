use gtk4::prelude::*;
// FIX 1: Added FileChooserNative and FileFilter
use gtk4::{
    Orientation, Button, Label, ResponseType, FileChooserNative,
    FileChooserAction, Grid, Frame, SpinButton, Align, FileFilter
};
use gtk4::glib;
use std::rc::Rc;
use std::cell::RefCell;
use crate::state::AppState;

use crate::physics::analysis::xrd::{calculate_pattern, XRDSettings, XRDPattern};
use crate::io::xrd_exp::{self, ExperimentalData};

use plotters::prelude::*;
use plotters::style::full_palette::GREY;
use plotters::backend::DrawingBackend;
use plotters_cairo::CairoBackend;
use cairo::{PdfSurface, Context};

struct PlotState {
    peaks: Option<Vec<XRDPattern>>,
    exp_data: Option<ExperimentalData>,
    settings: XRDSettings,
}

fn draw_xrd_chart<DB: DrawingBackend>(
    root: &plotters::drawing::DrawingArea<DB, plotters::coord::Shift>,
    peaks: &Vec<XRDPattern>,
    exp_data: &Option<ExperimentalData>,
    settings: &XRDSettings
) -> Result<(), std::boxed::Box<dyn std::error::Error>>
where DB::ErrorType: 'static {

    // 1. Generate Raw Simulated Curve
    let sigma = settings.smoothing / 2.355;
    let step = 0.05;
    let mut raw_curve = Vec::new();
    let mut t = settings.min_2theta;

    while t <= settings.max_2theta {
        let mut i_sum = 0.0;
        for p in peaks {
            let x = t - p.two_theta;
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

    let sim_curve: Vec<(f64, f64)> = raw_curve.into_iter()
        .map(|(t, i)| (t, i * scale))
        .collect();

    // 3. Draw Chart
    root.fill(&WHITE)?;

    let mut chart = ChartBuilder::on(root)
        .caption("XRD Pattern Comparison", ("sans-serif", 20).into_font())
        .margin(10)
        .x_label_area_size(40)
        .y_label_area_size(40)
        .build_cartesian_2d(settings.min_2theta..settings.max_2theta, 0.0..110.0)?;

    chart.configure_mesh()
        .x_desc("2Theta (deg)")
        .y_desc("Intensity (%)")
        .draw()?;

    // 4. Draw Experimental Data (Background, Grey)
    if let Some(data) = exp_data {
        let label_name = format!("Exp: {}", data.name);

        chart.draw_series(LineSeries::new(
            data.points.iter().map(|(x, y)| (*x, *y)).filter(|(x, _)| *x >= settings.min_2theta && *x <= settings.max_2theta),
            &GREY
        ))?
        .label(label_name)
        .legend(|(x, y)| PathElement::new(vec![(x, y), (x + 20, y)], &GREY));
    }

    // 5. Draw Simulation (Foreground, Red)
    chart.draw_series(LineSeries::new(
        sim_curve,
        RED.stroke_width(2)
    ))?
    .label("Simulation")
    .legend(|(x, y)| PathElement::new(vec![(x, y), (x + 20, y)], &RED));

    chart.configure_series_labels()
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

    grid.attach(&Label::new(Some("Min 2θ:")), 0, 0, 1, 1); grid.attach(&spin_min, 1, 0, 1, 1);
    grid.attach(&Label::new(Some("Max 2θ:")), 0, 1, 1, 1); grid.attach(&spin_max, 1, 1, 1, 1);
    grid.attach(&Label::new(Some("FWHM:")), 0, 2, 1, 1);   grid.attach(&spin_smooth, 1, 2, 1, 1);
    grid.attach(&Label::new(Some("λ (Å):")), 0, 3, 1, 1);  grid.attach(&spin_wave, 1, 3, 1, 1);

    frame_settings.set_child(Some(&grid));
    right_pane.append(&frame_settings);

    let btn_calc = Button::with_label("Recalculate");
    btn_calc.add_css_class("suggested-action");
    right_pane.append(&btn_calc);

    let btn_load_exp = Button::with_label("Load Experiment (.asc)");
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
             root.draw_text("Click 'Recalculate' to Simulate", &style, (w as i32/2 - 140, h as i32/2)).unwrap();
        }
    });

    // Recalculate
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
        if let Some(structure) = &app_st.structure {
            let peaks = calculate_pattern(structure, &ps.settings);
            ps.peaks = Some(peaks);
            da_calc.queue_draw();
        }
    });

    let refresh = refresh_plot.clone();
    btn_calc.connect_clicked(move |_| refresh());

    // --- FIX: Load Experiment Logic using FileChooserNative and Filters ---
    let ps_exp = plot_state.clone();
    let da_exp = drawing_area.clone();
    let parent_window = root.root().and_then(|r| r.downcast::<gtk4::Window>().ok());

    btn_load_exp.connect_clicked(move |_| {
        // Use FileChooserNative for system-native look
        let native = FileChooserNative::new(
            Some("Open Experimental Data"),
            parent_window.as_ref(),
            FileChooserAction::Open,
            Some("Open"),
            Some("Cancel"),
        );

        // Create Filter
        let filter = FileFilter::new();
        filter.set_name(Some("XRD Data (*.asc, *.xy)"));
        filter.add_pattern("*.asc");
        filter.add_pattern("*.ASC");
        filter.add_pattern("*.xy");
        filter.add_pattern("*.XY");

        native.add_filter(&filter);

        native.connect_response(glib::clone!(@strong ps_exp, @strong da_exp => move |d, response| {
            if response == ResponseType::Accept {
                if let Some(file) = d.file() {
                    if let Some(path) = file.path() {
                        if let Some(path_str) = path.to_str() {
                            match xrd_exp::parse(path_str) {
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
            }
        }));

        native.show();
    });

    // Export PDF (Using Native here too for consistency)
    let ps_export = plot_state.clone();

    btn_export.connect_clicked(move |_| {
        let ps = ps_export.borrow();
        if let Some(peaks) = &ps.peaks {
             // Native Save Dialog
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
                             let w = 800.0; let h = 600.0;
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
