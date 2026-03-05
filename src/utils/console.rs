// src/utils/console.rs
//
// Centralized console output for the two bottom-panel tabs:
//   • "Structure Info" — scientific data, structure summaries, analysis results
//   • "System Log"     — file I/O events, errors, warnings, export status
//
// After calling `init()` once in main.rs, any module can write to either tab
// without needing a &TextView reference threaded through function signatures.

use gtk4::prelude::*;
use gtk4::{glib, TextTag, TextView};
use std::sync::OnceLock;

// ---------------------------------------------------------------------------
// Global weak references — set once during init, read from anywhere
// ---------------------------------------------------------------------------

static INFO_VIEW: OnceLock<glib::SendWeakRef<TextView>> = OnceLock::new();
static LOG_VIEW: OnceLock<glib::SendWeakRef<TextView>> = OnceLock::new();

// ---------------------------------------------------------------------------
// Initialization (call once from main.rs after creating TextViews)
// ---------------------------------------------------------------------------

/// Register the two console TextViews. Call exactly once at startup.
pub fn init(info_view: &TextView, log_view: &TextView) {
  let _ = INFO_VIEW.set(info_view.downgrade().into());
  let _ = LOG_VIEW.set(log_view.downgrade().into());

  // Set up text tags for the info view
  setup_info_tags(info_view);
  // Log view tags are already set up by utils::logger::init
}

fn setup_info_tags(view: &TextView) {
  let buffer = view.buffer();
  let tag_table = buffer.tag_table();

  if tag_table.lookup("heading").is_none() {
    let tag = TextTag::new(Some("heading"));
    tag.set_property("foreground", "#4fc3f7"); // Light blue
    tag.set_property("weight", 700);
    tag_table.add(&tag);
  }

  if tag_table.lookup("dim").is_none() {
    let tag = TextTag::new(Some("dim"));
    tag.set_property("foreground", "#888888");
    tag_table.add(&tag);
  }
}

// ---------------------------------------------------------------------------
// Structure Info tab — scientific data, reports, analysis results
// ---------------------------------------------------------------------------

/// Write a message to the Structure Info tab.
/// Use for: structure summaries, BVS reports, geometry measurements,
/// symmetry results, charge density stats, slab generation info.
pub fn info(message: &str) {
  do_append(&INFO_VIEW, message);
}

/// Write a message with a separator to the Structure Info tab.
/// Use for major reports that should be visually separated from prior content.
pub fn info_report(message: &str) {
  let formatted = format!("\n━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n{}", message);
  do_append(&INFO_VIEW, &formatted);
}

// ---------------------------------------------------------------------------
// System Log tab — operational events, errors, file I/O
// ---------------------------------------------------------------------------

/// Log an informational event to the System Log tab.
pub fn log_info(message: &str) {
  log::info!("{}", message);
}

/// Log a warning to the System Log tab.
pub fn log_warn(message: &str) {
  log::warn!("{}", message);
}

/// Log an error to the System Log tab.
pub fn log_error(message: &str) {
  log::error!("{}", message);
}

/// Log a debug message to the System Log tab.
pub fn log_debug(message: &str) {
  log::debug!("{}", message);
}

// ---------------------------------------------------------------------------
// Internal helper — clone weak ref out of OnceLock before moving into async
// ---------------------------------------------------------------------------

fn do_append(cell: &'static OnceLock<glib::SendWeakRef<TextView>>, message: &str) {
  // Clone the weak ref out of the OnceLock so we own it — avoids lifetime escape.
  let weak = match cell.get() {
    Some(w) => w.clone(),
    None => return,
  };
  let msg = message.to_string();

  glib::MainContext::default().spawn_local(async move {
    if let Some(view) = weak.upgrade() {
      let buffer = view.buffer();
      let mut end = buffer.end_iter();
      if buffer.char_count() > 0 {
        buffer.insert(&mut end, "\n");
      }
      buffer.insert(&mut end, &msg);

      // Auto-scroll to bottom
      let mark = buffer.create_mark(None, &buffer.end_iter(), false);
      view.scroll_to_mark(&mark, 0.0, true, 0.0, 1.0);
      buffer.delete_mark(&mark);
    }
  });
}
