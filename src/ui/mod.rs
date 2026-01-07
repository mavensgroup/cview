// src/ui/mod.rs
pub mod preferences;
pub mod interactions;
pub mod analysis;

// Re-exports
pub use preferences::show_preferences_window;
pub use interactions::setup_interactions;

// --- NEW HELPER FOR LOGGING ---
use gtk4::TextView;
use gtk4::prelude::*;

pub fn log_to_console(console_view: &TextView, message: &str) {
    let buffer = console_view.buffer();
    let mut end_iter = buffer.end_iter();

    // If the console is not empty, add a separator line first
    if buffer.char_count() > 0 {
        buffer.insert(&mut end_iter, "\n\n--------------------------------\n\n");
    }

    // Insert the new message
    buffer.insert(&mut end_iter, message);

    // Auto-scroll to the bottom
    // We create a temporary 'mark' at the end and tell the view to scroll to it
    let mark = buffer.create_mark(None, &end_iter, false);
    console_view.scroll_to_mark(&mark, 0.0, true, 0.0, 1.0);
    buffer.delete_mark(&mark);
}
