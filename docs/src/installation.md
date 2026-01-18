# Installation

```admonish tip title="Quick Install"
If you have `cargo` installed, then jump to [Build from source](#3-build-from-source).
```


CView is a cross-platform application built on **Rust** and **GTK4**. Because it uses the native GTK4 libraries for rendering, the installation process involves two steps:
1.  Installing the system dependencies (GTK4).
2.  Compiling the application using Cargo.

---

## 1. Install Dependencies

Select your operating system below to set up the required libraries.

### <i class="fab fa-linux"></i> Linux

You need the GTK4 development headers and the standard build tools (gcc/clang).

**Fedora / RHEL**

```bash
sudo dnf install gcc gtk4-devel libadwaita-devel

```

**Ubuntu / Debian / Mint**
```bash
sudo apt update
sudo apt install build-essential libgtk-4-dev libadwaita-1-dev

```

**Arch Linux / Manjaro**

```bash
sudo pacman -S base-devel gtk4 libadwaita

```

### <i class="fab fa-apple"></i> macOS

The easiest way to install GTK4 on macOS is via **Homebrew**.

```bash
# 1. Install Homebrew if you haven't already
/bin/bash -c "$(curl -fsSL [https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh](https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh))"

# 2. Install Rust and GTK4
brew install rust gtk4 libadwaita adwaita-icon-theme

```

### <i class="fab fa-windows"></i> Windows

For Windows, we recommend using the **MSYS2** environment to manage the native GTK4 libraries.

1. **Install MSYS2:** Download the installer from [msys2.org](https://www.msys2.org/).
2. **Open the Terminal:** Launch `MSYS2 MinGW 64-bit`.
3. **Install Toolchain:**
    Run this command *inside the MSYS2 terminal*:
    ```bash
    pacman -S mingw-w64-x86_64-gtk4 mingw-w64-x86_64-rust mingw-w64-x86_64-gcc
    ```
4.  **Add to Path:** Add `C:\msys64\mingw64\bin` to your Windows System PATH environment variable.
    * *Why?* This allows you to run `cargo run` from VS Code or PowerShell later.

```admonish warning title="Do not use PowerShell yet"
The `pacman` command above works **only** inside the MSYS2 terminal. It will fail if you try to run it in PowerShell or Command Prompt.
```

---

## 2. Install Rust

If you do not have the Rust compiler installed, the recommended way is via `rustup`.

```admonish note title="Check your version"
CView requires **Rust 1.70** or higher.
Check your version with: `cargo --version`

```

**Command (Linux / macOS / Windows PowerShell):**

```bash
curl --proto '=https' --tlsv1.2 -sSf [https://sh.rustup.rs](https://sh.rustup.rs) | sh

```

---

## 3. Build from Source

Once the dependencies are ready, you can compile CView directly from the repository.

```bash
# 1. Clone the repository
git clone [https://github.com/mavensgroup/cview.git](https://github.com/mavensgroup/cview.git)
cd cview

# 2. Build and Run (Release mode recommended for performance)
cargo run --release

```

This will install `cview` in `~/bin` by default. For the subsequent run, just type `cview`

The first compilation may take a few minutes as it compiles the dependencies. Subsequent runs will be instant.

---


## Troubleshooting

```admonish failure title="Error: pkg-config not found"
If the build fails claiming it cannot find `gtk4` or `pkg-config`, it means the development headers are missing.
* **Linux:** Ensure you installed the `-dev` or `-devel` packages (e.g., `libgtk-4-dev`), not just the runtime library.
* **macOS:** Try running `brew link gtk4`.

```

```admonish failure title="Error: Linker failed"
If you see errors related to `cc` or `ld`:
* Ensure you have `build-essential` (Linux) or Xcode Command Line Tools (macOS) installed.
```
