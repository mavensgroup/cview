// src/utils/logger.rs

use gtk4::prelude::*;
use gtk4::{glib, TextView};
use log::{Level, Metadata, Record, SetLoggerError};
use std::sync::OnceLock;

static LOG_VIEW: OnceLock<glib::SendWeakRef<TextView>> = OnceLock::new();
static LOGGER: GtkLogger = GtkLogger;

struct GtkLogger;

pub fn init(view: &TextView) -> Result<(), SetLoggerError> {
  // 1. Define Colors (Tags) in the Buffer
  let buffer = view.buffer();
  let tag_table = buffer.tag_table();

  // Error: Red & Bold
  if tag_table.lookup("error").is_none() {
    let tag = gtk4::TextTag::new(Some("error"));
    tag.set_property("foreground", "#ff4444"); // Soft Red
    tag.set_property("weight", 700); // Bold
    tag_table.add(&tag);
  }

  // Warn: Orange
  if tag_table.lookup("warn").is_none() {
    let tag = gtk4::TextTag::new(Some("warn"));
    tag.set_property("foreground", "#ffbb33"); // Soft Orange
    tag_table.add(&tag);
  }

  // Info: Blue
  if tag_table.lookup("info").is_none() {
    let tag = gtk4::TextTag::new(Some("info"));
    tag.set_property("foreground", "#33b5e5"); // Soft Blue
    tag_table.add(&tag);
  }

  // Debug: Gray
  if tag_table.lookup("debug").is_none() {
    let tag = gtk4::TextTag::new(Some("debug"));
    tag.set_property("foreground", "#aaaaaa");
    tag_table.add(&tag);
  }

  let _ = LOG_VIEW.set(view.downgrade().into());
  log::set_logger(&LOGGER).map(|()| log::set_max_level(log::LevelFilter::Debug))
}

impl log::Log for GtkLogger {
  fn enabled(&self, metadata: &Metadata) -> bool {
    metadata.level() <= Level::Debug
  }

  fn log(&self, record: &Record) {
    if self.enabled(record.metadata()) {
      // --- UPDATED SYMBOLS ---
      let (icon, tag_name) = match record.level() {
        Level::Error => ("🔴", "error"), // Red Circle
        Level::Warn => ("🟠", "warn"),   // Orange Circle
        Level::Info => ("🔵", "info"),   // Blue Circle
        Level::Debug => ("⚪", "debug"), // White/Gray Circle
        Level::Trace => ("▫️", "debug"), // Small dot
      };

      // Format: "🔴  File not found"
      let msg = format!("{}  {}\n", icon, record.args());

      glib::MainContext::default().spawn_local(async move {
        if let Some(weak_ref) = LOG_VIEW.get() {
          if let Some(view) = weak_ref.upgrade() {
            let buffer = view.buffer();
            let mut end = buffer.end_iter();

            buffer.insert_with_tags_by_name(&mut end, &msg, &[tag_name]);

            // Auto-scroll
            let mark = buffer.create_mark(None, &buffer.end_iter(), false);
            view.scroll_to_mark(&mark, 0.0, true, 0.0, 1.0);
            buffer.delete_mark(&mark);
          }
        }
      });
    }
  }

  fn flush(&self) {}
}
