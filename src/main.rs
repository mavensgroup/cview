// src/main.rs
//
// Thin binary shim. All application code lives in the `cview` library crate
// (src/lib.rs) so that it can also be linked by integration tests and by the
// `corpus_audit` tool.

// Release builds on Windows are GUI-subsystem so no console window opens.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

fn main() {
    cview::run();
}
