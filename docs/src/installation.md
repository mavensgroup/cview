# Installation

CView is available as **ready-to-install packages** for all major operating systems. No compilation needed — just download and install like any other application.

>[!TIP]
>**New to software installation?** Just pick your operating system below and follow the pictures-and-clicks instructions. It's as easy as installing any app!

---

## <i class="fab fa-windows"></i> Windows 10 / 11

### Step-by-Step Installation

1. **Download the installer**
   - Go to: [**CView Releases**](https://github.com/mavensgroup/cview/releases/latest)
   - Click on: **`cview-installer.exe`** (19.8 MB)
   - The file will download to your Downloads folder

2. **Run the installer**
   - Find `cview-installer.exe` in your Downloads folder
   - Double-click it to start installation

3. **Handle the security warning** (if it appears)
   - Windows might show: *"Windows protected your PC"*
   - Click **"More info"**
   - Then click **"Run anyway"**

   >[!NOTE]
   >This warning appears for new software. CView is safe — it's open-source and built from verified code.

4. **Follow the installation wizard**
   - Click "Next" through the steps
   - Choose install location (default is fine)
   - Wait for installation to complete (takes ~30 seconds)

5. **Launch CView**
   - Find "CView" in your Start Menu
   - Or double-click the desktop shortcut (if you chose to create one)

**That's it!** CView is now installed and ready to use.

---

## <i class="fab fa-apple"></i> macOS (Intel & Apple Silicon)

### Step-by-Step Installation

1. **Download the disk image**
   - Go to: [**CView Releases**](https://github.com/mavensgroup/cview/releases/latest)
   - Click on: **`cview-macos.dmg`** (15.9 MB)
   - The file will download to your Downloads folder

2. **Open the DMG file**
   - Find `cview-macos.dmg` in Downloads
   - Double-click to open it
   - A new window will appear showing the CView icon

3. **Install CView**
   - **Drag the CView icon** into the **Applications folder** icon
   - Wait for the copy to complete (a few seconds)

4. **First launch security step**

   When you first open CView from Applications, macOS will block it because it's not from the App Store.

   **Option A: Right-click method** (easiest)
   - Find CView in your Applications folder
   - **Right-click** (or Control-click) on CView
   - Select **"Open"** from the menu
   - Click **"Open"** again in the warning dialog

   **Option B: Terminal command** (if right-click doesn't work)
   - Open Terminal (Applications → Utilities → Terminal)
   - Type: `xattr -cr /Applications/CView.app`
   - Press Enter
   - Now open CView normally

5. **Launch CView**
   - Find CView in Applications or use Spotlight (⌘ + Space, type "CView")
   - After the first launch, it will open normally like any Mac app

>[!IMPORTANT]
>The security step is **only needed once**. After that, CView opens normally.

---

## <i class="fab fa-linux"></i> Linux

Choose your distribution below:

### Ubuntu / Debian / Linux Mint / Pop!_OS

**File to download**: `cview.deb` (1.1 MB)

#### Method 1: Graphical Install (Easiest)

1. Go to [**CView Releases**](https://github.com/mavensgroup/cview/releases/latest)
2. Click on **`cview.deb`** to download
3. Double-click the downloaded file
4. Your Software Center will open
5. Click **"Install"**
6. Enter your password when prompted
7. Launch CView from your applications menu (search for "CView")

#### Method 2: Command Line

```bash
# Download
wget https://github.com/mavensgroup/cview/releases/download/v0.8.4/cview.deb

# Install
sudo dpkg -i cview.deb

# If you see dependency errors, fix them with:
sudo apt-get install -f

# Run CView
cview
```

---

### Fedora / RHEL / CentOS / AlmaLinux

**File to download**: `cview.rpm` (1.19 MB)

#### Method 1: Graphical Install

1. Go to [**CView Releases**](https://github.com/mavensgroup/cview/releases/latest)
2. Click on **`cview.rpm`** to download
3. Double-click the downloaded file
4. Click **"Install"** when prompted
5. Enter your password
6. Launch CView from your applications menu

#### Method 2: Command Line

```bash
# Fedora
sudo dnf install https://github.com/mavensgroup/cview/releases/download/v0.8.4/cview.rpm

# RHEL/CentOS/AlmaLinux
sudo yum install https://github.com/mavensgroup/cview/releases/download/v0.8.4/cview.rpm

# Run CView
cview
```

---

### Any Linux Distribution (Flatpak)

**File to download**: `cview.flatpak` (1.11 MB)

Flatpak works on **all Linux distributions** — use this if the `.deb` or `.rpm` doesn't work for you.

**One-time setup** (if you haven't used Flatpak before):

```bash
# Ubuntu/Debian
sudo apt install flatpak

# Fedora (already included)
# Nothing needed!

# Arch
sudo pacman -S flatpak

# openSUSE
sudo zypper install flatpak
```

**Install CView**:

```bash
# Download
wget https://github.com/mavensgroup/cview/releases/download/v0.8.4/cview.flatpak

# Install (no sudo needed with --user)
flatpak install --user cview.flatpak

# Run CView
flatpak run org.mavensgroup.cview
```

CView will also appear in your applications menu.

>[!NOTE]
>Flatpak runs apps in a secure sandbox. Your files are accessible, but CView is isolated from the rest of your system.

---

## Quick Reference Table

| Operating System | File to Download | Size | Installation Method |
|------------------|------------------|------|---------------------|
| **Windows 10/11** | `cview-installer.exe` | 19.8 MB | Run installer, click "Next" |
| **macOS** (Intel/M1/M2/M3) | `cview-macos.dmg` | 15.9 MB | Drag to Applications |
| **Ubuntu/Debian** | `cview.deb` | 1.1 MB | Double-click or `dpkg -i` |
| **Fedora/RHEL** | `cview.rpm` | 1.19 MB | Double-click or `dnf install` |
| **Any Linux** | `cview.flatpak` | 1.11 MB | `flatpak install` |

**Download from**: [**https://github.com/mavensgroup/cview/releases/latest**](https://github.com/mavensgroup/cview/releases/latest)

---

## Build from Source (Advanced Users)

If you want the latest development version or prefer to compile yourself, you can build CView from source.

### Prerequisites

You need:
- **Rust** (1.70 or newer)
- **GTK4 development libraries**
- **Git**

### Install Dependencies

<details>
<summary><b>Ubuntu / Debian</b></summary>

```bash
# Install GTK4 development headers and build tools
sudo apt update
sudo apt install build-essential libgtk-4-dev libadwaita-1-dev git

# Install Rust (if not already installed)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source $HOME/.cargo/env
```
</details>

<details>
<summary><b>Fedora / RHEL</b></summary>

```bash
# Install GTK4 and build tools
sudo dnf install gcc gtk4-devel libadwaita-devel git

# Install Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source $HOME/.cargo/env
```
</details>

<details>
<summary><b>Arch Linux</b></summary>

```bash
# Install dependencies
sudo pacman -S base-devel gtk4 libadwaita git rust

# Rust is now installed
```
</details>

<details>
<summary><b>macOS</b></summary>

```bash
# Install Homebrew if you don't have it
/bin/bash -c "$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh)"

# Install dependencies
brew install rust gtk4 libadwaita git
```
</details>

<details>
<summary><b>Windows (MSYS2)</b></summary>

1. Install MSYS2 from [msys2.org](https://www.msys2.org/)
2. Open "MSYS2 MinGW 64-bit" terminal
3. Run:

```bash
pacman -S mingw-w64-x86_64-gtk4 mingw-w64-x86_64-rust mingw-w64-x86_64-gcc git
```

4. Add `C:\msys64\mingw64\bin` to your Windows PATH
</details>

### Compile and Run

```bash
# Clone the repository
git clone https://github.com/mavensgroup/cview.git
cd cview

# Build and run (release mode for best performance)
cargo run --release
```

**First compilation takes 5-10 minutes** as Rust downloads and compiles all dependencies. Subsequent builds are much faster.

### Install System-Wide (Optional)

```bash
# This installs the binary to ~/.cargo/bin/cview
cargo install --path .

# Make sure ~/.cargo/bin is in your PATH
# Add this to your ~/.bashrc or ~/.zshrc:
export PATH="$HOME/.cargo/bin:$PATH"

# Now you can run CView from anywhere
cview
```

---

## Troubleshooting

### "Package not found" or dependency errors

**Linux (.deb/.rpm)**:
```bash
# Ubuntu/Debian
sudo apt-get update
sudo apt-get install -f

# Fedora
sudo dnf install gtk4 libadwaita
```

The packages require GTK4 runtime libraries. Most modern distributions include these, but you might need to install them.

### macOS: "CView is damaged and can't be opened"

This means the quarantine flag is still set. Remove it:

```bash
xattr -cr /Applications/CView.app
```

Then try opening CView again.

### Windows: "VCRUNTIME140.dll is missing"

Install the Visual C++ Redistributable:
- Download from: [Microsoft's official site](https://learn.microsoft.com/en-us/cpp/windows/latest-supported-vc-redist)
- Install and restart

### Build from source fails: "gtk4 not found"

Make sure you installed the **development** packages (with `-dev` or `-devel` suffix), not just the runtime libraries.

```bash
# Ubuntu/Debian - WRONG
sudo apt install gtk4

# Ubuntu/Debian - CORRECT
sudo apt install libgtk-4-dev libadwaita-1-dev
```

### Flatpak: "No remote refs found"

Make sure Flathub is added:
```bash
flatpak remote-add --if-not-exists flathub https://flathub.org/repo/flathub.flatpakrepo
```

---

## Still Having Issues?

1. Check the [GitHub Issues](https://github.com/mavensgroup/cview/issues) page
2. Open a new issue with:
   - Your operating system and version
   - The exact error message
   - What you've already tried
3. We'll help you get CView running!

---

## Verifying Your Installation

After installation, launch CView and try:
1. Click **File → Open**
2. Load a sample structure (we include examples in the repository)
3. If you see atoms and can rotate the structure → **Success!**

Welcome to CView! Check out the [User Guide](guide/visualization.md) to get started.
