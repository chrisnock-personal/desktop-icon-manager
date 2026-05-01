# Desktop Icon Manager

A native Linux GUI application for managing icons in `.desktop` application
entries — including full support for Chrome PWA apps.

Built with **Rust** + **[egui](https://github.com/emilk/egui)** (pure-Rust
immediate mode GUI, no GTK/Qt dependency at runtime).

---

## Features

| Feature | Details |
|---|---|
| **Discover .desktop files** | Automatically scans `~/.local/share/applications` |
| **Live icon preview** | Thumbnails rendered inline for PNG/JPG/ICO/BMP/WebP/SVG icons |
| **Upload icons** | Native file picker → copied to `~/Pictures/Icons/` (created if absent) |
| **Edit Icon= path** | Replace old icon value with the uploaded file path |
| **Chrome PWA support** | Auto-detects PWA entries; one-click `StartupWMClass` fix |
| **Search / filter** | Filter by app name or icon path; toggle "PWAs only" |
| **Non-destructive** | Changes only written when you click **Save** |
| **Dark theme** | Polished dark UI, no compositor required |

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

## Requirements

### Runtime

- Linux x86\_64 (tested on Ubuntu 22.04/24.04, Fedora 39+, Arch)
- X11 **or** Wayland (via XWayland)
- OpenGL 2.1+ (virtually all hardware since 2008)

### Build

- Rust 1.76+ (`rustup` recommended)
- A handful of `libxcb` and `libxkbcommon` headers (the build script installs
  them automatically on Debian/Ubuntu)

---

## Build & Install

```bash
# Clone / download the project, then:
chmod +x build.sh
./build.sh
```

The script will:
1. Check for Rust — prints install instructions if missing
2. Install any missing `libxcb*` / `libxkbcommon-dev` packages via `apt`
3. Run `cargo build --release`
4. Optionally copy the binary to `~/.local/bin/`
5. Optionally create a `.desktop` launcher so it appears in your app menu

### Manual build

```bash
cargo build --release
./target/release/desktop-icon-manager
```

---

## Usage

### Main window

```
┌────────────────────────────────────────────────────────┐
│ 🖥 Desktop Icon Manager   ⟳ Refresh  🔍 filter…  [PWA] │
├──────┬──────────────────┬──────────────────────┬───────┤
│ Icon │ Application      │ Icon Value           │ Flags │
├──────┼──────────────────┼──────────────────────┼───────┤
│  🖼  │ Notion           │ ~/Pictures/Icons/... │       │
│  🌐  │ Gmail PWA        │ chrome-abc123-Default│ [PWA] │
│  …   │ …                │ …                    │ …     │
└──────┴──────────────────┴──────────────────────┴───────┘
```

Click any row to open the **Edit panel** on the right.

### Edit panel

1. **Icon preview** — shows a large preview of the current icon (if resolvable)
2. **Icon= value** — editable text field; change manually or use the picker
3. **📁 Upload Icon…** — opens a native file chooser:
   - Supports PNG, JPG, SVG, ICO, BMP, WebP
   - File is **copied** to `~/Pictures/Icons/`
   - `Icon=` field is updated to the new path automatically
4. **Chrome PWA section** (visible for PWA entries only):
   - Shows the detected App ID slug
   - Editable `StartupWMClass=` field
   - **⚡ Auto-fill** sets `StartupWMClass` from the original icon value
5. **💾 Save** — writes changes back to the `.desktop` file
6. **↩ Discard** — reloads the entry from disk

---

## File structure

```
desktop-icon-manager/
├── Cargo.toml          # Rust package manifest
├── build.sh            # Build + install helper
├── README.md           # This file
└── src/
    └── main.rs         # All application code (~500 lines)
```

---

## How icon paths work

| Icon= value | Meaning |
|---|---|
| `myapp` | Named icon, looked up in `/usr/share/icons/…` |
| `/usr/share/pixmaps/myapp.png` | Absolute path |
| `~/Pictures/Icons/MyApp.png` | Home-relative path (this tool uses this format) |
| `chrome-abc123-Default` | Chrome PWA named icon |

After uploading an icon the tool writes a `~/Pictures/Icons/…` style path,
which is supported by all major desktops (GNOME, KDE, XFCE, etc.).

---

## Troubleshooting

**Window doesn't open / crashes on Wayland**

```bash
WAYLAND_DISPLAY="" ./desktop-icon-manager   # force X11
```

Or set `GDK_BACKEND=x11` / `QT_QPA_PLATFORM=xcb`.

**Icon previews show "?" for all entries**

The icon is a *named* icon (e.g. `firefox`) that lives in
`/usr/share/icons/`. The app still shows the name correctly and you can
replace it; preview only works for file-path icons.

**Permission denied when saving**

`.desktop` files in `~/.local/share/applications` are owned by your user;
saving should always work. If a file was created by another process with
restrictive permissions run:

```bash
chmod 644 ~/.local/share/applications/<file>.desktop
```

---

## License

MIT — do whatever you like with it.
