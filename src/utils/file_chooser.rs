// src/utils/file_chooser.rs
//
// GtkNativeDialog is not a widget and GTK does not keep it alive: the caller
// must hold a reference until it responds. Our choosers are built inside a
// click/action handler and the local goes out of scope as soon as show()
// returns, so on GTK versions that don't hold a ref themselves (4.12 on EL9,
// using the in-process chooser) the dialog is disposed before it ever maps.
// Always show a FileChooserNative through `show`.

use gtk4::prelude::*;
use gtk4::FileChooserNative;
use std::cell::RefCell;

/// Show `dialog`, keeping it alive until it emits `response` (accept, cancel
/// or close). Connect the caller's own response handler before calling this.
pub fn show(dialog: &FileChooserNative) {
    // The handler owns a strong ref to its own dialog — a deliberate cycle,
    // broken on the first response.
    let keep = RefCell::new(Some(dialog.clone()));
    dialog.connect_response(move |_, _| {
        keep.borrow_mut().take();
    });
    dialog.show();
}
