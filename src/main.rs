// src/main.rs
//
// Thin binary shim. All application code lives in the `cview` library crate
// (src/lib.rs) so that it can also be linked by integration tests and by the
// `corpus_audit` tool.

fn main() {
    cview::run();
}
