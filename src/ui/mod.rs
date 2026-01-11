pub mod preferences;
pub mod interactions;
pub mod analysis;
pub mod dialogs; // Added this

// Re-exports
pub use preferences::show_preferences_window;
pub use interactions::setup_interactions;

// (Keep your log_to_console function below...)
use gtk4::TextView;
use gtk4::prelude::*;
pub fn log_to_console(console_view: &TextView, message: &str) {
    let buffer = console_view.buffer();
    let mut end_iter = buffer.end_iter();
    if buffer.char_count() > 0 { buffer.insert(&mut end_iter, "\n\n--------------------------------\n\n"); }
    buffer.insert(&mut end_iter, message);
    let mark = buffer.create_mark(None, &end_iter, false);
    console_view.scroll_to_mark(&mark, 0.0, true, 0.0, 1.0);
    buffer.delete_mark(&mark);
}
