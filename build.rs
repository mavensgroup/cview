// Windows only: embed the application icon (assets/cview.ico) in cview.exe so
// Explorer, the taskbar and shortcuts show it. Uses windres from the MinGW
// toolchain (already present in the MSYS2 build environment).
use std::{env, path::PathBuf, process::Command};

fn main() {
    println!("cargo:rerun-if-changed=assets/cview.rc");
    println!("cargo:rerun-if-changed=assets/cview.ico");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let out = PathBuf::from(env::var("OUT_DIR").unwrap()).join("cview_icon.o");
    let status = Command::new("windres")
        .args(["--input", "assets/cview.rc", "--output-format=coff", "--output"])
        .arg(&out)
        .args(["--include-dir", "assets"])
        .status();
    match status {
        Ok(s) if s.success() => println!("cargo:rustc-link-arg-bins={}", out.display()),
        _ => println!("cargo:warning=windres failed; cview.exe will have no embedded icon"),
    }
}
