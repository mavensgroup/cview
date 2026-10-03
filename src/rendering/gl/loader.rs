// src/rendering/gl/loader.rs
//
// GL function pointers for glow, taken from libepoxy, which GTK 4 itself uses
// for OpenGL on every platform and so is always loaded. libepoxy exports each
// entry point as a pointer variable `epoxy_<name>` that dispatches to the
// current context; reading those is exactly what the old `epoxy` crate did.

use std::ffi::c_void;

fn library() -> Option<libloading::Library> {
    // The process itself first: libepoxy is already loaded as a GTK
    // dependency, and this avoids guessing its file name in app bundles.
    #[cfg(unix)]
    {
        let this: libloading::Library = libloading::os::unix::Library::this().into();
        if unsafe { this.get::<*const c_void>(b"epoxy_glClear\0") }.is_ok() {
            return Some(this);
        }
    }
    #[cfg(target_os = "macos")]
    const NAMES: &[&str] = &["libepoxy.0.dylib", "libepoxy.dylib"];
    #[cfg(all(unix, not(target_os = "macos")))]
    const NAMES: &[&str] = &["libepoxy.so.0", "libepoxy.so"];
    #[cfg(windows)]
    const NAMES: &[&str] = &["libepoxy-0.dll", "epoxy-0.dll"];
    NAMES
        .iter()
        .find_map(|n| unsafe { libloading::Library::new(n) }.ok())
}

/// Create a glow context for the GL context that is current right now
/// (call it from a `GLArea` realize handler, after `make_current`).
pub fn create_context() -> Result<glow::Context, String> {
    let lib = library().ok_or("libepoxy not found; cannot load OpenGL functions")?;
    let ctx = unsafe {
        glow::Context::from_loader_function(|name| {
            let sym = format!("epoxy_{name}\0");
            match lib.get::<*const *const c_void>(sym.as_bytes()) {
                // The symbol is a variable holding the function pointer.
                Ok(var) => **var,
                Err(_) => std::ptr::null(),
            }
        })
    };
    // Keep libepoxy mapped for the life of the process.
    std::mem::forget(lib);
    Ok(ctx)
}
