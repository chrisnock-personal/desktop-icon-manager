# Desktop Icon Manager

A native Linux GUI application for managing icons in `.desktop` application
entries — including full support for Chrome PWA apps.

Built with **Rust** + **[egui](https://github.com/emilk/egui)** (pure-Rust
immediate mode GUI, no GTK/Qt dependency at runtime).

---

## Features

| Feature | Details |
|---|---|
| **Discover .desktop files** | Scans `~/.local/share/applications`, `/usr/share/applications`, and `/usr/local/share/applications` |
| **Live icon preview** | Thumbnails rendered inline for PNG/JPG/ICO/BMP/WebP/SVG icons |
| **Upload icons** | Native file picker → copied to `~/Pictures/Icons/` and installed into the XDG icon theme |
| **Dock & launcher compatible** | Icons installed at all standard sizes (16–512px) into `~/.local/share/icons/hicolor/`; `Icon=` is set to a theme name, not a file path |
| **Edit Icon= value** | Replace the icon value manually or via the file picker |
| **Chrome PWA support** | Auto-detects PWA entries; one-click `StartupWMClass` fix |
| **Search / filter** | Filter by app name or icon value; toggle "PWAs only" |
| **Configurable icons dir** | Icons directory is editable in the status bar (defaults to `~/Pictures/Icons`) |
| **Non-destructive** | Changes only written when you click **Save** |
| **Light theme** | Clean light UI with blue accents |
| **Multi-distro build** | Build script detects and uses apt, dnf/yum, zypper, pacman, or xbps |

---

## Screenshots

<img width="1107" height="740" alt="Image" src="https://github.com/user-attachments/assets/0dfbc44d-912b-401e-8e3c-229ef1ca0b0e" /> <br>
<img width="1107" height="740" alt="Image" src="https://github.com/user-attachments/assets/b4501d16-168f-434b-86a2-e4847630e827" /><br>
<img width="1107" height="740" alt="Image" src="https://github.com/user-attachments/assets/db66bb59-c6b3-4970-8407-19b6a519ba7e" /><br>
<img width="1107" height="740" alt="Image" src="https://github.com/user-attachments/assets/5898b5fd-f8da-4c91-988e-b00672713131" /><br>
<img width="493" height="205" alt="Image" src="https://github.com/user-attachments/assets/a43bddf7-d705-4ff2-9be2-e3f78d934dab" /><br>
<img width="256" height="83" alt="Image" src="https://github.com/user-attachments/assets/5885c1c4-ebad-4527-81f3-c259c2882f6b" /> <br>

---

## Chrome PWA Details

Chrome Progressive Web App entries are detected by their filename pattern:

```
com.google.Chrome.flextop.chrome-<appid>-Default.desktop
```

or by an `Icon=` field that starts with `chrome-` and ends with `-Default`.

### The `StartupWMClass` problem

Without a correct `StartupWMClass`, your taskbar/dock may not group PWA
windows properly, or the custom icon won't show. The fix is to set:

```ini
StartupWMClass=chrome-<appid>-Default
```

(the *original* value of `Icon=` before you changed it).

The **"⚡ Auto-fill from Icon value"** button does this automatically:
it copies the detected app-ID slug into `StartupWMClass` for you.

---

## How icon installation works

When you upload an icon, the tool does the following automatically:

1. Copies the original file to your icons directory (`~/Pictures/Icons/` by default)
2. Rasterises it to every standard size and installs into `~/.local/share/icons/hicolor/<size>x<size>/apps/`
3. Copies a fallback to `~/.local/share/pixmaps/`
4. Runs `gtk-update-icon-cache` so the change takes effect immediately
5. Sets `Icon=` to the **bare theme name** (e.g. `myapp`) rather than a file path

This ensures icons appear correctly everywhere — in the file manager, dock,
app launcher, alt-tab switcher, and taskbar.

### Why not a file path?

| Icon= value | File manager | Dock / launcher |
|---|---|---|
| `/home/user/Pictures/Icons/myapp.png` | ✅ Works | ❌ Blank |
| `myapp` (theme name) | ✅ Works | ✅ Works |

Docks and panels (GNOME Shell, KDE Plasma, XFCE Panel) resolve `Icon=` by
theme name lookup — a raw file path is silently ignored for pinned and running
app entries.

---

## Requirements

### Runtime

- Linux x86\_64 (tested on Ubuntu 22.04/24.04, Fedora 39+, Arch)
- X11 **or** Wayland (via XWayland)
- OpenGL 2.1+ (virtually all hardware since 2008)

### Build

- Rust 1.76+ (`rustup` recommended)
- A handful of `libxcb` and `libxkbcommon` headers — the build script installs
  these automatically for supported distros

---

## Build & Install

```bash
# Extract the tarball, then:
chmod +x build.sh
./build.sh
```

The script will:
1. Detect your Linux distro and package manager
2. Install any missing system libraries (`libxcb`, `libxkbcommon`, `openssl`, `fontconfig`)
3. Check for Rust — prints install instructions if missing
4. Run `cargo build --release`
5. Optionally copy the binary to `~/.local/bin/`
6. Install the app icon into `~/.local/share/icons/hicolor/` at all standard sizes
7. Optionally create a `.desktop` launcher so it appears in your app menu

### Supported package managers

| Distro family | Manager |
|---|---|
| Debian, Ubuntu, Mint, Pop!\_OS | `apt` |
| Fedora, RHEL, CentOS, Rocky, AlmaLinux | `dnf` / `yum` |
| openSUSE Leap / Tumbleweed | `zypper` |
| Arch, Manjaro, EndeavourOS | `pacman` |
| Void Linux | `xbps` |

### Manual build

```bash
cargo build --release
./target/release/desktop-icon-manager
```

If you compiled manually (without `build.sh`), run the install script
separately to set up the icon and `.desktop` launcher:

```bash
chmod +x install-icons.sh
./install-icons.sh
```

To skip the dependency install step on an unsupported distro:

```bash
SKIP_DEPS=1 ./build.sh
```

---

## Usage

### Main window

The app lists all discovered `.desktop` entries with their icon thumbnail,
application name, current `Icon=` value, and flags (e.g. `[PWA]`).

- **Refresh** — re-scans all application directories
- **Filter** — search by app name or icon value
- **Chrome PWAs only** — toggle to show only detected PWA entries
- Click any row to open the **Edit panel**

### Edit panel

1. **Icon preview** — large preview of the current icon
2. **Icon= value** — editable text field; edit manually or use the picker
3. **📁 Upload Icon…** — native file picker (PNG, JPG, SVG, ICO, BMP, WebP):
   - Installs into the XDG icon theme automatically
   - Sets `Icon=` to the theme name so docks and launchers work correctly
4. **Chrome PWA section** (PWA entries only):
   - Shows the detected App ID slug
   - Editable `StartupWMClass=` field
   - **⚡ Auto-fill** sets `StartupWMClass` from the original icon value
5. **💾 Save** — writes changes back to the `.desktop` file
6. **↩ Discard** — reloads the entry from disk

### Configuring the icons directory

The icons directory is shown and editable in the bottom status bar. Type an
absolute path and press **Enter**, or click **📂** to browse. The directory
is created automatically if it doesn't exist.

---

## File structure

```
desktop-icon-manager/
├── Cargo.toml           # Rust package manifest
├── build.sh             # Build + install helper (multi-distro)
├── install-icons.sh     # Standalone icon/launcher installer (run after manual build)
├── README.md            # This file
├── assets/
│   ├── icon.svg         # App icon (SVG, scalable)
│   ├── icon_16.png      # App icon at standard sizes
│   ├── icon_32.png
│   ├── icon_48.png
│   ├── icon_64.png
│   ├── icon_128.png
│   ├── icon_256.png
│   └── icon_512.png
└── src/
    ├── icon_data.rs     # Embedded app icon pixel data (auto-generated)
    └── main.rs          # All application code
```

---

## Scanned directories

The app reads `.desktop` files from these locations, in priority order.
If the same filename exists in more than one directory, the first match wins
(so local user entries take precedence over system entries).

| Directory | Typical contents |
|---|---|
| `~/.local/share/applications` | User-installed apps, Chrome PWAs |
| `/usr/share/applications` | System-wide apps |
| `/usr/local/share/applications` | Locally compiled/installed apps |

> **Note:** Entries from `/usr/share/applications` are owned by root. The app
> can display and edit their values in the UI, but saving will fail without
> root privileges. To safely edit a system entry, copy it to
> `~/.local/share/applications/` first — your local copy will automatically
> shadow the system one.

---

## Troubleshooting

**Window doesn't open / crashes on Wayland**

```bash
WAYLAND_DISPLAY="" ./desktop-icon-manager   # force X11
```

Or set `GDK_BACKEND=x11` / `QT_QPA_PLATFORM=xcb`.

**Icon previews show "?" for an entry**

The icon is a named theme icon (e.g. `firefox`) that the app couldn't locate
on disk. The name is still displayed correctly and you can replace it with an
uploaded icon. Preview works for any icon the tool can find in the hicolor
theme or as a direct file path.

**Uploaded icon appears in file manager but not in dock**

Make sure you uploaded the icon through the app rather than editing `Icon=`
manually. The upload process installs into the hicolor theme, which is required
for docks and launchers. If you edited manually, re-upload via **📁 Upload Icon…**
and save again.

**App shows as "Unknown" in the dock with no icon**

Run `./install-icons.sh` (or `./build.sh`) from the project directory to
install the app's own icon and create the `.desktop` launcher. The dock reads
these rather than the binary itself.

**Permission denied when saving a system entry**

Copy the `.desktop` file to your local applications directory first:

```bash
cp /usr/share/applications/<file>.desktop ~/.local/share/applications/
```

Then edit it in the app — the local copy shadows the system one automatically.

---

## License

MIT — do whatever you like with it.
