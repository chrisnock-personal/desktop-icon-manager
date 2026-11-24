use eframe::egui;
use egui::text::LayoutJob;
use egui::{
    Align, Align2, Color32, CursorIcon, FontId, Pos2, Rect, RichText, Rounding, ScrollArea,
    Sense, Stroke, Vec2,
};
use std::collections::HashMap;
use std::fs;
use std::io::{self, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc, Arc};

mod icon_data;
mod icons;
mod install;
mod theme;

use theme::Palette;

// ──────────────────────────────────────────────────────────────────────────────
// Data structures
// ──────────────────────────────────────────────────────────────────────────────

/// Where a save ended up: in place, or in a local copy that shadows a
/// read-only system entry.
#[derive(Debug, Clone, PartialEq, Eq)]
enum SaveLocation {
    Original,
    LocalCopy(PathBuf),
}

#[derive(Debug, Clone)]
struct DesktopEntry {
    /// Absolute path to the .desktop file
    path: PathBuf,
    /// Human-readable application name (Name= field)
    name: String,
    /// Current value of Icon= field (may be a path or a named icon)
    icon_value: String,
    /// Whether this is a Chrome PWA
    is_chrome_pwa: bool,
    /// For Chrome PWAs: the app-id slug  (e.g. "chrome-<id>-Default")
    chrome_app_id: Option<String>,
    /// Current StartupWMClass= value (Chrome PWA only)
    startup_wm_class: Option<String>,
    /// Dirty flag -true if any field was modified but not yet saved
    modified: bool,
    /// Status message shown in the row
    status: Option<String>,
}

impl DesktopEntry {
    /// Parse a single .desktop file.
    fn load(path: PathBuf) -> Option<Self> {
        let content = fs::read_to_string(&path).ok()?;
        let mut name = path
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        let mut icon_value = String::new();
        let mut startup_wm_class: Option<String> = None;
        let mut in_desktop_section = false;

        for line in content.lines() {
            let line = line.trim();
            if line == "[Desktop Entry]" {
                in_desktop_section = true;
                continue;
            }
            if line.starts_with('[') {
                in_desktop_section = false;
                continue;
            }
            if !in_desktop_section {
                continue;
            }
            match key_value(line) {
                Some(("Name", v)) => name = v.to_string(),
                Some(("Icon", v)) => icon_value = v.to_string(),
                Some(("StartupWMClass", v)) => startup_wm_class = Some(v.to_string()),
                _ => {}
            }
        }

        // Detect Chrome PWA pattern:
        // File name like: com.google.Chrome.flextop.chrome-<appid>-Default.desktop
        let filename = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();

        let (is_chrome_pwa, chrome_app_id) = detect_chrome_pwa(&filename, &icon_value);

        Some(DesktopEntry {
            path,
            name,
            icon_value,
            is_chrome_pwa,
            chrome_app_id,
            startup_wm_class,
            modified: false,
            status: None,
        })
    }

    /// Write changes back to the .desktop file.
    fn save(&mut self) -> Result<SaveLocation, String> {
        self.save_to_home(&dirs_next())
    }

    /// The save implementation, with the home directory passed in so tests
    /// can exercise the read-only-system-entry fallback in a temp folder.
    fn save_to_home(&mut self, home: &Path) -> Result<SaveLocation, String> {
        let content =
            fs::read_to_string(&self.path).map_err(|e| format!("Read error: {e}"))?;

        let mut new_lines: Vec<String> = Vec::new();
        let mut in_desktop_section = false;
        let mut icon_written = false;
        let mut wm_written = false;

        for line in content.lines() {
            let trimmed = line.trim();

            if trimmed == "[Desktop Entry]" {
                in_desktop_section = true;
                new_lines.push(line.to_string());
                continue;
            }
            if trimmed.starts_with('[') && trimmed != "[Desktop Entry]" {
                // Before leaving the section, flush any missing keys
                if in_desktop_section {
                    if !icon_written {
                        new_lines.push(format!("Icon={}", self.icon_value));
                    }
                    if self.is_chrome_pwa && !wm_written {
                        if let Some(wm) = self.wm_class_to_write() {
                            new_lines.push(format!("StartupWMClass={wm}"));
                        }
                    }
                }
                in_desktop_section = false;
                new_lines.push(line.to_string());
                continue;
            }

            if in_desktop_section {
                match key_value(trimmed).map(|(k, _)| k) {
                    // Only the first occurrence is rewritten; a duplicate
                    // key is dropped rather than left with the old value.
                    Some("Icon") => {
                        if !icon_written {
                            new_lines.push(format!("Icon={}", self.icon_value));
                            icon_written = true;
                        }
                        continue;
                    }
                    Some("StartupWMClass") if self.is_chrome_pwa => {
                        match &self.startup_wm_class {
                            // Untouched: keep the line as it was.
                            None => new_lines.push(line.to_string()),
                            // Cleared in the editor: remove the key.
                            Some(wm) if wm.is_empty() => {}
                            Some(wm) => {
                                if !wm_written {
                                    new_lines.push(format!("StartupWMClass={wm}"));
                                }
                            }
                        }
                        wm_written = true;
                        continue;
                    }
                    _ => {}
                }
            }

            new_lines.push(line.to_string());
        }

        // Handle case where [Desktop Entry] is the last section
        if in_desktop_section {
            if !icon_written {
                new_lines.push(format!("Icon={}", self.icon_value));
            }
            if self.is_chrome_pwa && !wm_written {
                if let Some(wm) = self.wm_class_to_write() {
                    new_lines.push(format!("StartupWMClass={wm}"));
                }
            }
        }

        let new_content = new_lines.join("\n") + "\n";
        let denied = match write_atomic(&self.path, &new_content) {
            Ok(()) => {
                self.modified = false;
                self.status = Some("✔ Saved".to_string());
                return Ok(SaveLocation::Original);
            }
            Err(e) if e.kind() == io::ErrorKind::PermissionDenied => e,
            Err(e) => return Err(format!("Write error: {e}")),
        };

        // A root-owned /usr/share/applications entry can't be written, but a
        // file of the same name in ~/.local/share/applications shadows it.
        let local = local_application_path(home, &self.path)
            .ok_or_else(|| format!("Write error: {denied}"))?;
        if let Some(parent) = local.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("Write error: {denied}; could not create {}: {e}", parent.display()))?;
        }
        write_atomic(&local, &new_content).map_err(|e| {
            format!("Write error: {denied}; local copy at {} also failed: {e}", local.display())
        })?;

        self.path = local.clone();
        self.modified = false;
        self.status = Some(format!("✔ Saved to {}", local.display()));
        Ok(SaveLocation::LocalCopy(local))
    }

    /// The StartupWMClass to add when the file has none: never an empty one.
    fn wm_class_to_write(&self) -> Option<&str> {
        self.startup_wm_class.as_deref().filter(|s| !s.is_empty())
    }
}

/// Split a `Key=Value` line. The spec allows spaces around `=`; localised
/// keys (`Name[de]=`) keep their brackets, so they never match a plain key.
fn key_value(line: &str) -> Option<(&str, &str)> {
    let (k, v) = line.split_once('=')?;
    Some((k.trim(), v.trim_start()))
}

/// The user-level launcher that shadows `path`, if `path` isn't already there.
fn local_application_path(home: &Path, path: &Path) -> Option<PathBuf> {
    let dir = home.join(".local/share/applications");
    if path.parent() == Some(dir.as_path()) {
        return None;
    }
    Some(dir.join(path.file_name()?))
}

/// Write a file by creating a temporary file beside it and renaming it over
/// the original. A crash or a full disk can then leave either the old file or
/// the new one, never a truncated launcher that disappears from the menu.
///
/// The original's permissions are copied to the temporary file: Chrome's PWA
/// launchers are executable, and `fs::write` on a new file would drop that.
fn write_atomic(path: &Path, content: &str) -> io::Result<()> {
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let dir = path.parent().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "path has no parent directory")
    })?;
    let name = path.file_name().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "path has no file name")
    })?;
    let permissions = fs::metadata(path).ok().map(|m| m.permissions());

    for offset in 0..100 {
        let seq = SEQ.fetch_add(1, Ordering::Relaxed).wrapping_add(offset);
        let tmp = dir.join(format!(".{}.tmp-{}-{}", name.to_string_lossy(), std::process::id(), seq));
        let mut file = match fs::OpenOptions::new().write(true).create_new(true).open(&tmp) {
            Ok(file) => file,
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        };

        let result = (|| {
            file.write_all(content.as_bytes())?;
            file.sync_all()?;
            if let Some(permissions) = permissions {
                fs::set_permissions(&tmp, permissions)?;
            }
            drop(file);
            fs::rename(&tmp, path)
        })();
        if result.is_err() {
            let _ = fs::remove_file(&tmp);
        }
        return result;
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "could not create a temporary file beside the launcher",
    ))
}

/// Returns (is_chrome_pwa, Option<chrome_app_id_slug>).
/// Chrome PWA .desktop filenames look like:
///   com.google.Chrome.flextop.chrome-<appid>-Default.desktop
///   chrome-<appid>-Default.desktop  (older format)
fn detect_chrome_pwa(filename: &str, icon_value: &str) -> (bool, Option<String>) {
    // Pattern 1: com.google.Chrome… prefix
    if filename.contains("com.google.Chrome") || filename.contains("google-chrome") {
        // Try to extract slug like "chrome-<id>-Default"
        let stem = filename.strip_suffix(".desktop").unwrap_or(filename);
        // Find "chrome-" within the stem
        if let Some(idx) = stem.find("chrome-") {
            let slug = &stem[idx..];
            return (true, Some(slug.to_string()));
        }
        return (true, None);
    }

    // Pattern 2: icon value itself looks like chrome-<id>-Default
    if icon_value.starts_with("chrome-") && icon_value.ends_with("-Default") {
        return (true, Some(icon_value.to_string()));
    }

    (false, None)
}

// ──────────────────────────────────────────────────────────────────────────────
// Application state
// ──────────────────────────────────────────────────────────────────────────────

/// Which entries the list shows.
#[derive(Clone, Copy, PartialEq, Eq, Default)]
enum Filter {
    #[default]
    All,
    Pwa,
    Unsaved,
}

/// How the entries are laid out.
#[derive(Clone, Copy, PartialEq, Eq, Default)]
enum ViewMode {
    #[default]
    Grid,
    List,
}

/// An icon preview uploaded to the GPU.
#[derive(Clone)]
struct IconTexture {
    tex: egui::TextureHandle,
    /// A single-colour symbolic icon, stored as a white mask to be tinted
    symbolic: bool,
}

#[derive(Default)]
struct IconManagerApp {
    entries: Vec<DesktopEntry>,
    /// Index of the currently selected entry (for the detail pane)
    selected: Option<usize>,
    /// Search / filter string
    filter: String,
    /// Global status message (shown in the toolbar)
    global_status: String,
    /// Which subset of entries to show
    filter_mode: Filter,
    /// Grid of cards or a compact list
    view: ViewMode,
    /// Dark theme active
    dark: bool,
    /// Colours for the active theme
    palette: Palette,
    /// The app's own icon, shown in the header
    logo: Option<egui::TextureHandle>,
    /// Decoded icon previews by Icon= value; None caches "no preview"
    icon_textures: HashMap<String, Option<IconTexture>>,
    /// Every icon in the desktop's icon themes, for resolving Icon= names
    icon_index: icons::IconIndex,
    /// Icons folder (absolute path): default ~/Pictures/Icons, user-editable
    icons_dir: PathBuf,
    /// Editable text buffer for the icons dir path field
    icons_dir_input: String,
    /// Ordered list of directories to scan for .desktop files.
    /// Local (~/.local/share/applications) is first so it takes priority
    /// over system entries with the same filename.
    apps_dirs: Vec<PathBuf>,
    /// An open file picker, running on its own thread. A picker run on the
    /// UI thread stops the window answering the compositor; on Wayland that
    /// can get the app disconnected (it just vanishes) while the picker is up.
    picker: Option<mpsc::Receiver<Option<Picked>>>,
}

/// What a file picker returned, and what it was for.
enum Picked {
    /// An image for the entry whose .desktop file is `entry`.
    Icon { entry: PathBuf, file: PathBuf },
    /// A new icons folder.
    Folder(PathBuf),
}

impl Default for Palette {
    fn default() -> Self {
        Palette::light()
    }
}

impl IconManagerApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        theme::install_fonts(&cc.egui_ctx);
        let dark = theme::system_prefers_dark(cc);
        let palette = Palette::for_mode(dark);
        theme::apply(&cc.egui_ctx, &palette);

        let logo = Some(cc.egui_ctx.load_texture(
            "app-logo",
            egui::ColorImage::from_rgba_unmultiplied(
                [icon_data::ICON_WIDTH as usize, icon_data::ICON_HEIGHT as usize],
                icon_data::ICON_RGBA,
            ),
            egui::TextureOptions::LINEAR,
        ));

        let home = dirs_next();
        // Scan local first (higher priority), then system-wide
        let apps_dirs = vec![
            home.join(".local/share/applications"),
            PathBuf::from("/usr/share/applications"),
            PathBuf::from("/usr/local/share/applications"),
        ];
        let icons_dir = home.join("Pictures/Icons");

        // Ensure icons directory exists
        if !icons_dir.exists() {
            let _ = fs::create_dir_all(&icons_dir);
        }

        let icons_dir_input = icons_dir.display().to_string();
        let mut app = Self {
            apps_dirs,
            icons_dir,
            icons_dir_input,
            dark,
            palette,
            logo,
            ..Default::default()
        };
        app.refresh_entries();
        app
    }

    fn refresh_entries(&mut self) {
        self.icon_index = icons::IconIndex::for_desktop();
        self.icon_textures.clear();
        self.entries.clear();
        self.selected = None;
        self.global_status = String::new();

        // Track filenames already loaded (local entries shadow system ones)
        let mut seen_filenames = std::collections::HashSet::new();
        let mut total_dirs_scanned = 0;

        for apps_dir in &self.apps_dirs.clone() {
            if !apps_dir.exists() {
                continue;
            }
            let dir = match fs::read_dir(apps_dir) {
                Ok(d) => d,
                Err(_) => continue,
            };
            total_dirs_scanned += 1;

            let mut paths: Vec<PathBuf> = dir
                .filter_map(|e| e.ok())
                .map(|e| e.path())
                .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("desktop"))
                .collect();
            paths.sort();

            for path in paths {
                // Use just the filename as the dedup key so local entries
                // shadow system entries with the same name
                let fname = path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string();
                if seen_filenames.contains(&fname) {
                    continue;
                }
                seen_filenames.insert(fname);

                if let Some(entry) = DesktopEntry::load(path) {
                    self.entries.push(entry);
                }
            }
        }

        // Sort all entries alphabetically by name
        self.entries.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));

        self.global_status = format!(
            "Loaded {} entries from {} director{}",
            self.entries.len(),
            total_dirs_scanned,
            if total_dirs_scanned == 1 { "y" } else { "ies" }
        );
    }

    /// Install an image as `entry`'s icon and return its theme name.
    fn import_icon(&self, source: &Path, entry: &Path) -> Result<String, String> {
        let targets = install::Targets::for_user(&self.icons_dir);
        let ours = |p: &Path| {
            [&targets.hicolor, &targets.pixmaps, &targets.icons_dir]
                .iter()
                .any(|dir| p.starts_with(dir))
        };
        let current_is_ours = self
            .entries
            .iter()
            .find(|e| e.path == entry)
            .and_then(|e| icons::resolve_icon_path(&e.icon_value, &self.icon_index))
            .is_some_and(|p| ours(&p));

        // Names are taken by icons in system themes *and* by icons another
        // app put in the user's folders. Re-importing the entry's own current
        // name is the one case where overwriting is safe.
        let taken = |name: &str| {
            let files = install::installed_files(&targets, name);
            if current_is_ours && files.iter().any(|p| ours(p) && p.exists()) {
                return false;
            }
            files.iter().any(|p| p.exists())
                || icons::resolve_icon_path(name, &self.icon_index)
                    .is_some_and(|p| !ours(&p))
        };
        install::install_icon(source, entry, &targets, taken)
    }

    fn load_icon_texture(
        &mut self,
        ctx: &egui::Context,
        icon_value: &str,
    ) -> Option<IconTexture> {
        if icon_value.is_empty() {
            return None;
        }
        if let Some(cached) = self.icon_textures.get(icon_value) {
            return cached.clone();
        }
        let loaded = icons::resolve_icon_path(icon_value, &self.icon_index)
            .and_then(|path| icons::decode(&path))
            .map(|icon| IconTexture {
                tex: ctx.load_texture(icon_value, icon.image, egui::TextureOptions::LINEAR),
                symbolic: icon.symbolic,
            });
        self.icon_textures.insert(icon_value.to_string(), loaded.clone());
        loaded
    }
}

pub(crate) fn dirs_next() -> PathBuf {
    std::env::var("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/root"))
}

// ──────────────────────────────────────────────────────────────────────────────
// Drawing helpers
// ──────────────────────────────────────────────────────────────────────────────

fn text(s: impl Into<String>, size: f32, color: Color32) -> RichText {
    RichText::new(s).size(size).color(color)
}

fn bold_text(s: impl Into<String>, size: f32, color: Color32) -> RichText {
    RichText::new(s).size(size).color(color).family(theme::bold())
}

/// Small upper-case label above a group of controls.
fn section_label(ui: &mut egui::Ui, p: &Palette, s: &str) {
    ui.label(bold_text(s.to_uppercase(), 10.5, p.text_faint));
}

/// Filled accent button for the main action.
fn primary_button(p: &Palette, label: &str) -> egui::Button<'static> {
    egui::Button::new(bold_text(label, 13.0, p.on_accent))
        .fill(p.accent)
        .stroke(Stroke::NONE)
        .rounding(Rounding::same(8.0))
}

/// Outlined button for secondary actions.
fn secondary_button(p: &Palette, label: &str) -> egui::Button<'static> {
    egui::Button::new(text(label, 13.0, p.text))
        .fill(p.surface)
        .stroke(Stroke::new(1.0_f32, p.border_strong))
        .rounding(Rounding::same(8.0))
}

/// Square, borderless button holding a single glyph.
fn icon_button(ui: &mut egui::Ui, p: &Palette, glyph: &str, tip: &str) -> egui::Response {
    ui.add(
        egui::Button::new(text(glyph, 15.0, p.text_muted))
            .fill(p.surface_alt)
            .stroke(Stroke::NONE)
            .rounding(Rounding::same(8.0))
            .min_size(Vec2::splat(32.0)),
    )
    .on_hover_text(tip)
    .on_hover_cursor(CursorIcon::PointingHand)
}

/// Lay out a single- or multi-line piece of text, ending in "…" when it
/// doesn't fit in `max_rows` lines of `width`.
fn layout_text(
    ui: &egui::Ui,
    s: &str,
    font: FontId,
    color: Color32,
    width: f32,
    max_rows: usize,
    halign: Align,
) -> Arc<egui::Galley> {
    let mut job = LayoutJob::simple(s.to_owned(), font, color, width);
    job.wrap.max_rows = max_rows;
    job.wrap.break_anywhere = max_rows == 1;
    job.wrap.overflow_character = Some('…');
    job.halign = halign;
    ui.fonts(|f| f.layout_job(job))
}

/// Paint a rounded "pill" badge with its top-left corner at `pos` and
/// return its rect.
fn paint_pill(ui: &egui::Ui, pos: Pos2, label: &str, fg: Color32, bg: Color32) -> Rect {
    let galley = layout_text(
        ui,
        label,
        FontId::new(10.0, theme::bold()),
        fg,
        f32::INFINITY,
        1,
        Align::LEFT,
    );
    let pad = egui::vec2(7.0, 2.5);
    let rect = Rect::from_min_size(pos, galley.size() + pad * 2.0);
    let painter = ui.painter();
    painter.rect_filled(rect, Rounding::same(rect.height() / 2.0), bg);
    painter.galley(rect.min + pad, galley, fg);
    rect
}

/// Width a pill with this label will take, for right-aligning a row of them.
fn pill_width(ui: &egui::Ui, label: &str) -> f32 {
    layout_text(ui, label, FontId::new(10.0, theme::bold()), Color32::WHITE, f32::INFINITY, 1, Align::LEFT)
        .size()
        .x
        + 14.0
}

/// Paint an icon texture fitted (aspect preserved) into `rect`, or a coloured
/// tile with the app's initial when there is no preview.
fn paint_icon(ui: &egui::Ui, p: &Palette, rect: Rect, icon: Option<&IconTexture>, name: &str) {
    let painter = ui.painter();
    let fit = |tex: &egui::TextureHandle, rect: Rect| {
        let size = tex.size_vec2();
        let scale = (rect.width() / size.x).min(rect.height() / size.y);
        Rect::from_center_size(rect.center(), size * scale)
    };
    let uv = Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0));
    match icon {
        // Symbolic icons are plain shapes: draw them on a tinted tile, in
        // the tile's colour, so they read like app icons in both themes.
        Some(icon) if icon.symbolic => {
            let (fg, bg) = p.tint_for(name);
            painter.rect_filled(rect, Rounding::same(rect.width() * 0.24), bg);
            let inner = rect.shrink(rect.width() * 0.24);
            painter.image(icon.tex.id(), fit(&icon.tex, inner), uv, fg);
        }
        Some(icon) => {
            painter.image(icon.tex.id(), fit(&icon.tex, rect), uv, Color32::WHITE);
        }
        None => {
            let (fg, bg) = p.tint_for(name);
            painter.rect_filled(rect, Rounding::same(rect.width() * 0.24), bg);
            let initial = name
                .chars()
                .find(|c| c.is_alphanumeric())
                .map(|c| c.to_uppercase().to_string())
                .unwrap_or_else(|| "?".into());
            painter.text(
                rect.center(),
                Align2::CENTER_CENTER,
                initial,
                FontId::new(rect.height() * 0.46, theme::bold()),
                fg,
            );
        }
    }
}

/// Drop shadow under a card.
fn paint_shadow(ui: &egui::Ui, p: &Palette, rect: Rect, rounding: f32, lift: f32) {
    let shadow = egui::epaint::Shadow {
        offset: egui::vec2(0.0, lift),
        blur: lift * 3.0,
        spread: 0.0,
        color: p.shadow,
    };
    ui.painter().add(shadow.tessellate(rect, Rounding::same(rounding)));
}

// ──────────────────────────────────────────────────────────────────────────────
// eframe App implementation
// ──────────────────────────────────────────────────────────────────────────────

impl eframe::App for IconManagerApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_picker();
        self.handle_shortcuts(ctx);
        self.draw_header(ctx);
        self.draw_status_bar(ctx);

        if let Some(sel) = self.selected.filter(|&s| s < self.entries.len()) {
            let p = self.palette;
            egui::SidePanel::right("detail_panel")
                .default_width(380.0)
                .min_width(320.0)
                .max_width(560.0)
                .frame(
                    egui::Frame::none()
                        .fill(p.surface)
                        .stroke(Stroke::new(1.0_f32, p.border)),
                )
                .show(ctx, |ui| {
                    self.draw_detail_panel(ui, ctx, sel);
                });
        }

        egui::CentralPanel::default()
            .frame(
                egui::Frame::none()
                    .fill(self.palette.bg)
                    .inner_margin(egui::Margin {
                        left: 20.0,
                        right: 12.0,
                        top: 16.0,
                        bottom: 0.0,
                    }),
            )
            .show(ctx, |ui| {
                self.draw_entries(ui, ctx);
            });
    }
}

impl IconManagerApp {
    fn set_dark(&mut self, ctx: &egui::Context, dark: bool) {
        self.dark = dark;
        self.palette = Palette::for_mode(dark);
        theme::apply(ctx, &self.palette);
    }

    /// Ctrl+S saves the open entry; Escape closes the detail panel.
    fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        let Some(sel) = self.selected.filter(|&s| s < self.entries.len()) else {
            return;
        };
        let (save, close) = ctx.input(|i| {
            (
                i.modifiers.command && i.key_pressed(egui::Key::S),
                i.key_pressed(egui::Key::Escape),
            )
        });
        if save && self.entries[sel].modified {
            self.save_entry(sel);
        }
        if close && ctx.memory(|m| m.focused().is_none()) {
            self.selected = None;
        }
    }

    fn save_entry(&mut self, sel: usize) {
        let name = self.entries[sel].name.clone();
        match self.entries[sel].save() {
            Ok(SaveLocation::Original) => self.global_status = format!("Saved {name}"),
            Ok(SaveLocation::LocalCopy(path)) => {
                self.global_status = format!("Saved a local copy of {name}: {}", path.display());
            }
            Err(e) => self.entries[sel].status = Some(format!("✖ {e}")),
        }
    }

    fn discard_entry(&mut self, sel: usize) {
        let path = self.entries[sel].path.clone();
        if let Some(fresh) = DesktopEntry::load(path) {
            self.entries[sel] = fresh;
        }
    }

    fn matches(&self, e: &DesktopEntry, needle: &str) -> bool {
        let in_subset = match self.filter_mode {
            Filter::All => true,
            Filter::Pwa => e.is_chrome_pwa,
            Filter::Unsaved => e.modified,
        };
        in_subset
            && (needle.is_empty()
                || e.name.to_lowercase().contains(needle)
                || e.icon_value.to_lowercase().contains(needle))
    }

    // ── Header: title, search, filters ───────────────────────────────────────

    fn draw_header(&mut self, ctx: &egui::Context) {
        let p = self.palette;
        egui::TopBottomPanel::top("header")
            .frame(
                egui::Frame::none()
                    .fill(p.surface)
                    .inner_margin(egui::Margin::symmetric(20.0, 14.0)),
            )
            .show(ctx, |ui| {
                // Row 1: logo, title, view & theme controls
                ui.horizontal(|ui| {
                    if let Some(logo) = &self.logo {
                        let (rect, _) = ui.allocate_exact_size(Vec2::splat(36.0), Sense::hover());
                        ui.painter().image(
                            logo.id(),
                            rect,
                            Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                            Color32::WHITE,
                        );
                    }
                    ui.add_space(4.0);
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 0.0;
                        ui.label(bold_text("Desktop Icon Manager", 17.0, p.text));
                        let pwas = self.entries.iter().filter(|e| e.is_chrome_pwa).count();
                        ui.label(text(
                            format!("{} applications  ·  {} web apps", self.entries.len(), pwas),
                            12.0,
                            p.text_muted,
                        ));
                    });

                    ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                        let (glyph, tip) = if self.dark {
                            ("☀", "Switch to light theme")
                        } else {
                            ("🌙", "Switch to dark theme")
                        };
                        if icon_button(ui, &p, glyph, tip).clicked() {
                            self.set_dark(ctx, !self.dark);
                        }
                        if icon_button(ui, &p, "⟳", "Reload all .desktop files").clicked() {
                            self.refresh_entries();
                        }
                        ui.add_space(4.0);
                        // Right-to-left layout: List is added first so it
                        // ends up on the right, reading "Grid | List".
                        segmented(ui, &p, &mut self.view, &[(ViewMode::List, "List"), (ViewMode::Grid, "Grid")]);
                    });
                });

                ui.add_space(12.0);

                // Row 2: search box and filter chips
                ui.horizontal(|ui| {
                    egui::Frame::none()
                        .fill(p.surface_alt)
                        .stroke(Stroke::new(1.0_f32, p.border))
                        .rounding(Rounding::same(10.0))
                        .inner_margin(egui::Margin::symmetric(10.0, 3.0))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.label(text("🔍", 13.0, p.text_faint));
                                ui.add(
                                    egui::TextEdit::singleline(&mut self.filter)
                                        .frame(false)
                                        .hint_text(text("Search apps or icon names…", 13.0, p.text_faint))
                                        .font(FontId::proportional(13.5))
                                        .desired_width(260.0),
                                );
                                let clear = egui::Button::new(text("✖", 11.0, p.text_faint)).frame(false);
                                if ui
                                    .add_visible(!self.filter.is_empty(), clear)
                                    .on_hover_text("Clear search")
                                    .clicked()
                                {
                                    self.filter.clear();
                                }
                            });
                        });

                    ui.add_space(8.0);

                    let pwas = self.entries.iter().filter(|e| e.is_chrome_pwa).count();
                    let unsaved = self.entries.iter().filter(|e| e.modified).count();
                    for (mode, label, count) in [
                        (Filter::All, "All", self.entries.len()),
                        (Filter::Pwa, "Web apps", pwas),
                        (Filter::Unsaved, "Unsaved", unsaved),
                    ] {
                        let on = self.filter_mode == mode;
                        let (fg, fill, stroke) = if on {
                            (p.accent, p.accent_soft, p.accent)
                        } else {
                            (p.text_muted, p.surface, p.border)
                        };
                        let label = if on {
                            bold_text(format!("{label}  {count}"), 12.5, fg)
                        } else {
                            text(format!("{label}  {count}"), 12.5, fg)
                        };
                        let chip = egui::Button::new(label)
                            .fill(fill)
                            .stroke(Stroke::new(1.0_f32, stroke))
                            .rounding(Rounding::same(15.0))
                            .min_size(Vec2::new(0.0, 30.0));
                        if ui.add(chip).on_hover_cursor(CursorIcon::PointingHand).clicked() {
                            self.filter_mode = mode;
                        }
                    }

                    ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                        ui.add(
                            egui::Label::new(text(&self.global_status, 12.0, p.text_muted))
                                .truncate(true),
                        );
                    });
                });
            });
    }

    // ── Status bar: icons folder & scanned dirs ──────────────────────────────

    fn draw_status_bar(&mut self, ctx: &egui::Context) {
        let p = self.palette;
        egui::TopBottomPanel::bottom("statusbar")
            .frame(
                egui::Frame::none()
                    .fill(p.surface)
                    .inner_margin(egui::Margin::symmetric(20.0, 8.0)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(bold_text("Icon folder", 12.0, p.text_muted));

                    let input_resp = ui.add(
                        egui::TextEdit::singleline(&mut self.icons_dir_input)
                            .desired_width(320.0)
                            .font(FontId::monospace(12.0))
                            .margin(egui::vec2(8.0, 4.0))
                            .hint_text("/absolute/path/to/icons"),
                    );

                    // Commit on Enter or focus-loss
                    if input_resp.lost_focus() {
                        let new_path = PathBuf::from(self.icons_dir_input.trim());
                        if new_path.is_absolute() {
                            if !new_path.exists() {
                                let _ = fs::create_dir_all(&new_path);
                            }
                            self.icons_dir = new_path;
                            self.icons_dir_input = self.icons_dir.display().to_string();
                            self.global_status =
                                format!("Icon folder set to {}", self.icons_dir.display());
                        } else {
                            // Reset the text field to current valid value
                            self.icons_dir_input = self.icons_dir.display().to_string();
                            self.global_status =
                                "Icon folder must be an absolute path (starting with /)".to_string();
                        }
                    }

                    // Browse button: native folder picker
                    let idle = self.picker.is_none();
                    if ui
                        .add_enabled(idle, secondary_button(&p, "Browse…").min_size(Vec2::new(0.0, 28.0)))
                        .on_hover_text("Browse for icon folder")
                        .clicked()
                    {
                        let start = self.icons_dir.clone();
                        self.open_picker(ctx, move || {
                            rfd::FileDialog::new()
                                .set_title("Select Icons Folder")
                                .set_directory(start)
                                .pick_folder()
                                .map(Picked::Folder)
                        });
                    }

                    ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                        let dirs_str = self
                            .apps_dirs
                            .iter()
                            .filter(|d| d.exists())
                            .map(|d| d.display().to_string())
                            .collect::<Vec<_>>()
                            .join("  ·  ");
                        ui.add(
                            egui::Label::new(text(format!("Scanning  {dirs_str}"), 11.0, p.text_faint))
                                .truncate(true),
                        );
                    });
                });
            });
    }

    // ── Entry grid / list ────────────────────────────────────────────────────

    fn draw_entries(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let p = self.palette;
        let needle = self.filter.to_lowercase();
        let filtered: Vec<usize> = (0..self.entries.len())
            .filter(|&i| self.matches(&self.entries[i], &needle))
            .collect();

        ui.horizontal(|ui| {
            let title = match self.filter_mode {
                Filter::All => "All applications",
                Filter::Pwa => "Chrome web apps",
                Filter::Unsaved => "Unsaved changes",
            };
            ui.label(bold_text(title, 15.0, p.text));
            ui.label(text(
                if filtered.len() == self.entries.len() {
                    format!("{}", filtered.len())
                } else {
                    format!("{} of {}", filtered.len(), self.entries.len())
                },
                13.0,
                p.text_faint,
            ));
        });
        ui.add_space(10.0);

        if filtered.is_empty() {
            ui.add_space(60.0);
            ui.vertical_centered(|ui| {
                ui.label(text("🔍", 32.0, p.text_faint));
                ui.add_space(6.0);
                ui.label(bold_text("Nothing here", 15.0, p.text));
                ui.label(text(
                    match (self.filter_mode, needle.is_empty()) {
                        (Filter::Unsaved, true) => "Every change has been saved.",
                        (Filter::Pwa, true) => "No Chrome web apps were found.",
                        _ => "No applications match your search.",
                    },
                    13.0,
                    p.text_muted,
                ));
            });
            return;
        }

        ScrollArea::vertical()
            .id_source("entry_list")
            .auto_shrink([false; 2])
            .show(ui, |ui| {
                ui.add_space(2.0);
                match self.view {
                    ViewMode::Grid => self.draw_grid(ui, ctx, &filtered),
                    ViewMode::List => self.draw_list(ui, ctx, &filtered),
                }
                ui.add_space(16.0);
            });
    }

    fn draw_grid(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, filtered: &[usize]) {
        const GAP: f32 = 12.0;
        const MIN_W: f32 = 136.0;
        const CARD_H: f32 = 150.0;

        // Leave room for the floating scroll bar on the right.
        let avail = ui.available_width() - 8.0;
        let cols = (((avail + GAP) / (MIN_W + GAP)).floor() as usize).max(1);
        let card_w = ((avail - GAP * (cols - 1) as f32) / cols as f32).min(200.0);
        let size = Vec2::new(card_w, CARD_H);

        for row in filtered.chunks(cols) {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = GAP;
                for &idx in row {
                    self.grid_card(ui, ctx, idx, size);
                }
            });
            ui.add_space(GAP - ui.spacing().item_spacing.y);
        }
    }

    fn grid_card(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, idx: usize, size: Vec2) {
        let p = self.palette;
        let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
        if resp.clicked() {
            self.selected = Some(idx);
        }
        if !ui.is_rect_visible(rect) {
            return;
        }
        let icon_value = self.entries[idx].icon_value.clone();
        let tex = self.load_icon_texture(ctx, &icon_value);
        let e = &self.entries[idx];
        let selected = self.selected == Some(idx);
        let hovered = resp.hovered();
        if hovered {
            ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
        }

        let (fill, stroke) = if selected {
            (p.accent_soft, Stroke::new(1.5_f32, p.accent))
        } else if hovered {
            (p.surface, Stroke::new(1.0_f32, p.border_strong))
        } else {
            (p.surface, Stroke::new(1.0_f32, p.border))
        };
        paint_shadow(ui, &p, rect, 12.0, if hovered || selected { 4.0 } else { 1.5 });
        ui.painter().rect(rect, Rounding::same(12.0), fill, stroke);

        let icon_rect = Rect::from_center_size(
            Pos2::new(rect.center().x, rect.top() + 22.0 + 30.0),
            Vec2::splat(60.0),
        );
        paint_icon(ui, &p, icon_rect, tex.as_ref(), &e.name);

        let galley = layout_text(
            ui,
            &e.name,
            FontId::proportional(13.0),
            p.text,
            rect.width() - 20.0,
            2,
            Align::Center,
        );
        ui.painter()
            .galley(Pos2::new(rect.center().x, icon_rect.bottom() + 12.0), galley, p.text);

        if e.is_chrome_pwa {
            let bg = if selected { p.surface } else { p.accent_soft };
            paint_pill(ui, rect.left_top() + egui::vec2(8.0, 8.0), "PWA", p.accent, bg);
        }
        if e.modified {
            ui.painter()
                .circle_filled(rect.right_top() + egui::vec2(-13.0, 13.0), 4.5, p.warning);
        } else if e.status.as_deref().is_some_and(|s| s.starts_with('✔')) {
            ui.painter().text(
                rect.right_top() + egui::vec2(-13.0, 13.0),
                Align2::CENTER_CENTER,
                "✔",
                FontId::proportional(12.0),
                p.success,
            );
        }

        resp.on_hover_text(&e.name);
    }

    fn draw_list(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, filtered: &[usize]) {
        const ROW_H: f32 = 56.0;
        let width = ui.available_width() - 8.0;
        let p = self.palette;

        // One card holding all rows, separated by hairlines.
        let total = Rect::from_min_size(
            ui.cursor().min,
            Vec2::new(width, ROW_H * filtered.len() as f32),
        );
        paint_shadow(ui, &p, total, 12.0, 1.5);
        ui.painter()
            .rect(total, Rounding::same(12.0), p.surface, Stroke::new(1.0_f32, p.border));

        ui.spacing_mut().item_spacing.y = 0.0;
        for (n, &idx) in filtered.iter().enumerate() {
            let (rect, resp) = ui.allocate_exact_size(Vec2::new(width, ROW_H), Sense::click());
            if resp.clicked() {
                self.selected = Some(idx);
            }
            if !ui.is_rect_visible(rect) {
                continue;
            }
            let icon_value = self.entries[idx].icon_value.clone();
            let tex = self.load_icon_texture(ctx, &icon_value);
            let e = &self.entries[idx];
            let selected = self.selected == Some(idx);
            let hovered = resp.hovered();
            if hovered {
                ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
            }

            // Round only the outer corners of the first and last rows.
            let r = 12.0;
            let rounding = Rounding {
                nw: if n == 0 { r } else { 0.0 },
                ne: if n == 0 { r } else { 0.0 },
                sw: if n + 1 == filtered.len() { r } else { 0.0 },
                se: if n + 1 == filtered.len() { r } else { 0.0 },
            };
            let inner = rect.shrink(1.0);
            if selected {
                ui.painter().rect_filled(inner, rounding, p.accent_soft);
                ui.painter().rect_filled(
                    Rect::from_min_size(inner.min, Vec2::new(3.0, inner.height())),
                    Rounding { nw: rounding.nw, sw: rounding.sw, ..Default::default() },
                    p.accent,
                );
            } else if hovered {
                ui.painter().rect_filled(inner, rounding, p.surface_alt);
            }
            if n > 0 {
                ui.painter().hline(
                    rect.x_range().shrink(16.0),
                    rect.top(),
                    Stroke::new(1.0_f32, p.border),
                );
            }

            let icon_rect = Rect::from_min_size(
                Pos2::new(rect.left() + 16.0, rect.center().y - 18.0),
                Vec2::splat(36.0),
            );
            paint_icon(ui, &p, icon_rect, tex.as_ref(), &e.name);

            // Badges, right-aligned
            let mut badges: Vec<(&str, Color32, Color32)> = Vec::new();
            if e.is_chrome_pwa {
                badges.push(("PWA", p.accent, if selected { p.surface } else { p.accent_soft }));
            }
            if e.modified {
                badges.push(("Unsaved", p.warning, p.warning_soft));
            } else if e.status.as_deref().is_some_and(|s| s.starts_with('✔')) {
                badges.push(("Saved", p.success, p.success_soft));
            }
            let mut x = rect.right() - 16.0;
            for (label, fg, bg) in badges.iter().rev() {
                x -= pill_width(ui, label);
                let pill = paint_pill(ui, Pos2::new(x, rect.center().y - 9.0), label, *fg, *bg);
                x = pill.left() - 6.0;
            }

            let text_left = icon_rect.right() + 14.0;
            let text_w = (x - text_left - 12.0).max(40.0);
            let name = layout_text(
                ui,
                &e.name,
                FontId::new(14.0, theme::bold()),
                p.text,
                text_w,
                1,
                Align::LEFT,
            );
            let value = if e.icon_value.is_empty() { "(no icon set)" } else { &e.icon_value };
            let sub = layout_text(
                ui,
                value,
                FontId::monospace(11.5),
                p.text_muted,
                text_w,
                1,
                Align::LEFT,
            );
            let block_h = name.size().y + 2.0 + sub.size().y;
            let top = rect.center().y - block_h / 2.0;
            let name_h = name.size().y;
            ui.painter().galley(Pos2::new(text_left, top), name, p.text);
            ui.painter()
                .galley(Pos2::new(text_left, top + name_h + 2.0), sub, p.text_muted);
        }
    }

    // ── Detail / edit panel ──────────────────────────────────────────────────

    fn draw_detail_panel(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, sel: usize) {
        let p = self.palette;

        // Sticky footer with the status message and Save / Discard.
        egui::TopBottomPanel::bottom("detail_footer")
            .frame(
                egui::Frame::none()
                    .fill(p.surface)
                    .inner_margin(egui::Margin::symmetric(20.0, 14.0)),
            )
            .show_inside(ui, |ui| {
                if let Some(s) = self.entries[sel].status.clone() {
                    let (fg, bg) = if s.starts_with('✔') {
                        (p.success, p.success_soft)
                    } else if s.starts_with('✖') {
                        (p.danger, p.danger_soft)
                    } else {
                        (p.warning, p.warning_soft)
                    };
                    egui::Frame::none()
                        .fill(bg)
                        .rounding(Rounding::same(8.0))
                        .inner_margin(egui::Margin::symmetric(10.0, 7.0))
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.label(text(s, 12.5, fg));
                        });
                    ui.add_space(10.0);
                }

                let modified = self.entries[sel].modified;
                ui.horizontal(|ui| {
                    let w = (ui.available_width() - 8.0) / 2.0;
                    let size = Vec2::new(w, 36.0);
                    if ui
                        .add_enabled(modified, secondary_button(&p, "Discard").min_size(size))
                        .on_hover_text("Reload this entry from disk")
                        .clicked()
                    {
                        self.discard_entry(sel);
                    }
                    if ui
                        .add_enabled(modified, primary_button(&p, "Save changes").min_size(size))
                        .on_hover_text("Write changes to the .desktop file  (Ctrl+S)")
                        .clicked()
                    {
                        self.save_entry(sel);
                    }
                });
            });

        egui::CentralPanel::default()
            .frame(
                egui::Frame::none()
                    .fill(p.surface)
                    .inner_margin(egui::Margin { left: 20.0, right: 20.0, top: 16.0, bottom: 8.0 }),
            )
            .show_inside(ui, |ui| {
                ScrollArea::vertical()
                    .id_source("detail_scroll")
                    .auto_shrink([false; 2])
                    .show(ui, |ui| {
                        self.draw_detail_body(ui, ctx, sel);
                    });
            });
    }

    fn draw_detail_body(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, sel: usize) {
        let p = self.palette;

        ui.horizontal(|ui| {
            section_label(ui, &p, "Edit entry");
            ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                let close = egui::Button::new(text("✖", 12.0, p.text_muted))
                    .fill(Color32::TRANSPARENT)
                    .stroke(Stroke::NONE)
                    .min_size(Vec2::splat(26.0));
                if ui.add(close).on_hover_text("Close  (Esc)").clicked() {
                    self.selected = None;
                }
            });
        });
        ui.add_space(8.0);

        // ── Hero: big icon, name, file name ─────────────────────────────────
        let icon_value = self.entries[sel].icon_value.clone();
        let tex = self.load_icon_texture(ctx, &icon_value);
        let e = &self.entries[sel];
        let name = e.name.clone();
        let file_name = e
            .path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        let is_pwa = e.is_chrome_pwa;
        let is_user = e.path.starts_with(dirs_next());

        egui::Frame::none()
            .fill(p.surface_alt)
            .rounding(Rounding::same(14.0))
            .inner_margin(egui::Margin::symmetric(16.0, 20.0))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.vertical_centered(|ui| {
                    let (well, _) = ui.allocate_exact_size(Vec2::splat(112.0), Sense::hover());
                    ui.painter().rect(
                        well,
                        Rounding::same(24.0),
                        p.surface,
                        Stroke::new(1.0_f32, p.border),
                    );
                    paint_icon(ui, &p, well.shrink(16.0), tex.as_ref(), &name);
                    ui.add_space(10.0);
                    ui.add(egui::Label::new(bold_text(&name, 18.0, p.text)).wrap(true));
                    ui.add(egui::Label::new(text(&file_name, 11.5, p.text_muted)).truncate(true));
                    ui.add_space(4.0);
                    let tags = format!(
                        "{}{}",
                        if is_user { "User entry" } else { "System entry" },
                        if is_pwa { "  ·  Chrome web app" } else { "" }
                    );
                    ui.label(text(tags, 11.5, p.text_faint));
                });
            });

        ui.add_space(18.0);

        // ── Icon ────────────────────────────────────────────────────────────
        section_label(ui, &p, "Icon");
        ui.add_space(2.0);
        let icon_changed = {
            let entry = &mut self.entries[sel];
            let before = entry.icon_value.clone();
            ui.add(
                egui::TextEdit::singleline(&mut entry.icon_value)
                    .desired_width(f32::INFINITY)
                    .font(FontId::monospace(13.0))
                    .margin(egui::vec2(10.0, 8.0))
                    .hint_text("Theme name (e.g. firefox) or absolute path"),
            );
            entry.icon_value != before
        };
        if icon_changed {
            self.entries[sel].modified = true;
            self.entries[sel].status = None;
        }

        ui.add_space(6.0);
        let choose = ui
            .add_enabled(
                self.picker.is_none(),
                secondary_button(&p, "Choose image…")
                    .min_size(Vec2::new(ui.available_width(), 34.0)),
            )
            .on_hover_cursor(CursorIcon::PointingHand);
        ui.add_space(2.0);
        ui.add(
            egui::Label::new(text(
                format!(
                    "Copied to {} and installed into your icon theme at every size, so docks and launchers pick it up.",
                    self.icons_dir.display()
                ),
                11.5,
                p.text_faint,
            ))
            .wrap(true),
        );

        if choose.clicked() {
            let entry = self.entries[sel].path.clone();
            self.open_picker(ctx, move || {
                rfd::FileDialog::new()
                    .add_filter("Images", &["png", "jpg", "jpeg", "svg", "ico", "bmp", "webp"])
                    .set_title("Select Icon Image")
                    .pick_file()
                    .map(|file| Picked::Icon { entry, file })
            });
        }

        // ── Chrome PWA ──────────────────────────────────────────────────────
        if is_pwa {
            ui.add_space(18.0);
            section_label(ui, &p, "Chrome web app");
            ui.add_space(2.0);
            egui::Frame::none()
                .fill(p.surface_alt)
                .stroke(Stroke::new(1.0_f32, p.border))
                .rounding(Rounding::same(10.0))
                .inner_margin(egui::Margin::same(14.0))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.label(text("App ID", 11.5, p.text_muted));
                    let chrome_id = self.entries[sel]
                        .chrome_app_id
                        .clone()
                        .unwrap_or_else(|| "(not detected)".to_string());
                    ui.add(
                        egui::Label::new(
                            RichText::new(chrome_id).monospace().size(12.0).color(p.text),
                        )
                        .wrap(true),
                    );
                    ui.add_space(8.0);

                    ui.label(text("StartupWMClass", 11.5, p.text_muted));
                    let wm_changed = {
                        let entry = &mut self.entries[sel];
                        let mut wm_val = entry.startup_wm_class.clone().unwrap_or_default();
                        let before = wm_val.clone();
                        ui.add(
                            egui::TextEdit::singleline(&mut wm_val)
                                .desired_width(f32::INFINITY)
                                .font(FontId::monospace(12.5))
                                .margin(egui::vec2(10.0, 7.0))
                                .hint_text("chrome-<appid>-Default"),
                        );
                        // Only store an edit: storing the empty field of an
                        // entry without the key would make Save add one.
                        let changed = wm_val != before;
                        if changed {
                            entry.startup_wm_class = Some(wm_val);
                        }
                        changed
                    };
                    if wm_changed {
                        self.entries[sel].modified = true;
                    }

                    ui.add_space(6.0);
                    if ui
                        .add(secondary_button(&p, "⚡  Fill from app ID"))
                        .on_hover_text(
                            "Sets StartupWMClass to the app's original icon name\n\
                             (chrome-<appid>-Default), so the dock groups its\n\
                             windows under your custom icon.",
                        )
                        .clicked()
                    {
                        let slug = self.entries[sel]
                            .chrome_app_id
                            .clone()
                            .unwrap_or_else(|| self.entries[sel].icon_value.clone());
                        self.entries[sel].startup_wm_class = Some(slug);
                        self.entries[sel].modified = true;
                        self.entries[sel].status = Some("WMClass set, save to apply".into());
                    }
                    ui.add_space(2.0);
                    ui.add(
                        egui::Label::new(text(
                            "Without it, the dock may show Chrome's icon for this app's windows.",
                            11.5,
                            p.text_faint,
                        ))
                        .wrap(true),
                    );
                });
        }

        // ── File ────────────────────────────────────────────────────────────
        ui.add_space(18.0);
        section_label(ui, &p, "File");
        ui.add_space(2.0);
        let path = self.entries[sel].path.display().to_string();
        ui.add(
            egui::Label::new(RichText::new(path).monospace().size(11.5).color(p.text_muted))
                .wrap(true),
        );
        ui.add_space(8.0);
    }

    /// Run a file picker on its own thread; `poll_picker` collects the result.
    fn open_picker(
        &mut self,
        ctx: &egui::Context,
        pick: impl FnOnce() -> Option<Picked> + Send + 'static,
    ) {
        if self.picker.is_some() {
            return;
        }
        let (tx, rx) = mpsc::channel();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(pick());
            ctx.request_repaint();
        });
        self.picker = Some(rx);
    }

    fn poll_picker(&mut self) {
        let Some(rx) = &self.picker else { return };
        let picked = match rx.try_recv() {
            Ok(picked) => picked,
            Err(mpsc::TryRecvError::Empty) => return,
            Err(mpsc::TryRecvError::Disconnected) => None,
        };
        self.picker = None;
        match picked {
            Some(Picked::Icon { entry, file }) => self.apply_image(&entry, &file),
            Some(Picked::Folder(dir)) => {
                if let Err(e) = fs::create_dir_all(&dir) {
                    self.global_status = format!("Could not create {}: {e}", dir.display());
                    return;
                }
                self.icons_dir = dir;
                self.icons_dir_input = self.icons_dir.display().to_string();
                self.global_status = format!("Icon folder set to {}", self.icons_dir.display());
            }
            None => {}
        }
    }

    /// Install a picked image into the icon theme and point the entry's
    /// Icon= at it. The entry is found by path: the list may have been
    /// reloaded while the picker was open.
    fn apply_image(&mut self, entry: &Path, file: &Path) {
        let Some(sel) = self.entries.iter().position(|e| e.path == entry) else {
            self.global_status = "The entry was reloaded while choosing; pick the image again".into();
            return;
        };
        match self.import_icon(file, entry) {
            Ok(theme_name) => {
                // The new icon is now in the theme: re-index and drop
                // cached previews so it (and anything it replaced) reloads.
                self.icon_index = icons::IconIndex::for_desktop();
                self.icon_textures.clear();

                self.entries[sel].icon_value = theme_name.clone();
                self.entries[sel].modified = true;
                self.entries[sel].status =
                    Some(format!("✔ Icon set to \"{theme_name}\", save to apply"));
                self.global_status = format!(
                    "Installed icon \"{theme_name}\"; the dock and launcher update on save"
                );
            }
            Err(e) => {
                self.entries[sel].status = Some(format!("✖ {e}"));
            }
        }
    }
}

/// A row of mutually exclusive pill buttons inside a rounded track.
fn segmented<T: PartialEq + Copy>(ui: &mut egui::Ui, p: &Palette, value: &mut T, options: &[(T, &str)]) {
    egui::Frame::none()
        .fill(p.surface_alt)
        .rounding(Rounding::same(9.0))
        .inner_margin(egui::Margin::same(3.0))
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            ui.horizontal(|ui| {
                for (opt, label) in options {
                    let on = *value == *opt;
                    let btn = egui::Button::new(if on {
                        bold_text(*label, 12.5, p.text)
                    } else {
                        text(*label, 12.5, p.text_muted)
                    })
                    .fill(if on { p.surface } else { Color32::TRANSPARENT })
                    .stroke(if on { Stroke::new(1.0_f32, p.border) } else { Stroke::NONE })
                    .rounding(Rounding::same(7.0))
                    .min_size(Vec2::new(54.0, 26.0));
                    if ui.add(btn).on_hover_cursor(CursorIcon::PointingHand).clicked() {
                        *value = *opt;
                    }
                }
            });
        });
}

// ──────────────────────────────────────────────────────────────────────────────
// Entry point
// ──────────────────────────────────────────────────────────────────────────────

fn main() -> eframe::Result<()> {
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Desktop Icon Manager")
            .with_app_id("desktop-icon-manager")  // sets WM_CLASS -must match .desktop filename
            .with_inner_size([1100.0, 700.0])
            .with_min_inner_size([800.0, 500.0])
            .with_icon(load_app_icon()),
        ..Default::default()
    };

    eframe::run_native(
        "Desktop Icon Manager",
        native_options,
        Box::new(|cc| Box::new(IconManagerApp::new(cc))),
    )
}

fn load_app_icon() -> egui::IconData {
    egui::IconData {
        rgba:   icon_data::ICON_RGBA.to_vec(),
        width:  icon_data::ICON_WIDTH,
        height: icon_data::ICON_HEIGHT,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry_file(tag: &str, name: &str, content: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("dim-save-{tag}-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let p = dir.join(name);
        fs::write(&p, content).unwrap();
        p
    }

    const PWA: &str = "com.google.Chrome.flextop.chrome-abc-Default.desktop";

    #[test]
    fn saving_a_pwa_without_wm_class_adds_no_empty_key() {
        let p = entry_file("nowm", PWA, "[Desktop Entry]\nName=Chat\nIcon=chrome-abc-Default\n");
        let mut e = DesktopEntry::load(p.clone()).unwrap();
        e.icon_value = "chat".into();
        e.save().unwrap();
        let out = fs::read_to_string(&p).unwrap();
        assert!(out.contains("Icon=chat\n"));
        assert!(!out.contains("StartupWMClass"), "{out}");
        let _ = fs::remove_dir_all(p.parent().unwrap());
    }

    #[test]
    fn a_cleared_wm_class_is_removed() {
        let p = entry_file("clear", PWA, "[Desktop Entry]\nName=Chat\nStartupWMClass=old\nIcon=x\n");
        let mut e = DesktopEntry::load(p.clone()).unwrap();
        e.startup_wm_class = Some(String::new());
        e.save().unwrap();
        assert!(!fs::read_to_string(&p).unwrap().contains("StartupWMClass"));
        let _ = fs::remove_dir_all(p.parent().unwrap());
    }

    #[test]
    fn saving_keeps_an_executable_launcher_executable() {
        let p = entry_file(
            "executable",
            "app.desktop",
            "[Desktop Entry]\nName=App\nIcon=old\n",
        );
        fs::set_permissions(&p, fs::Permissions::from_mode(0o755)).unwrap();
        let mut e = DesktopEntry::load(p.clone()).unwrap();
        e.icon_value = "new".into();
        e.save().unwrap();
        assert_eq!(
            fs::metadata(&p).unwrap().permissions().mode() & 0o777,
            0o755
        );
        assert!(fs::read_to_string(&p).unwrap().contains("Icon=new"));
        let _ = fs::remove_dir_all(p.parent().unwrap());
    }

    #[test]
    fn a_read_only_system_entry_is_saved_as_a_local_copy() {
        let home = std::env::temp_dir().join(format!("dim-home-{}", std::process::id()));
        let system = home.join("system");
        fs::create_dir_all(&system).unwrap();
        let p = system.join("app.desktop");
        fs::write(&p, "[Desktop Entry]\nName=App\nIcon=old\n").unwrap();

        let mut e = DesktopEntry::load(p.clone()).unwrap();
        e.icon_value = "new".into();
        let perms = fs::metadata(&system).unwrap().permissions();
        fs::set_permissions(&system, fs::Permissions::from_mode(0o555)).unwrap();
        let result = e.save_to_home(&home);
        fs::set_permissions(&system, perms).unwrap();

        let local = home.join(".local/share/applications/app.desktop");
        assert_eq!(result.unwrap(), SaveLocation::LocalCopy(local.clone()));
        assert_eq!(e.path, local);
        assert!(fs::read_to_string(&local).unwrap().contains("Icon=new"));
        assert!(fs::read_to_string(&p).unwrap().contains("Icon=old"));
        let _ = fs::remove_dir_all(&home);
    }

    #[test]
    fn a_local_launcher_is_not_given_a_second_local_copy() {
        let home = PathBuf::from("/home/someone");
        let p = home.join(".local/share/applications/app.desktop");
        assert_eq!(local_application_path(&home, &p), None);
        assert_eq!(
            local_application_path(&home, Path::new("/usr/share/applications/app.desktop")),
            Some(p)
        );
    }

    #[test]
    fn saving_leaves_no_temporary_files_behind() {
        let p = entry_file("clean", "app.desktop", "[Desktop Entry]\nName=App\nIcon=old\n");
        let mut e = DesktopEntry::load(p.clone()).unwrap();
        e.icon_value = "new".into();
        e.save().unwrap();
        let leftovers: Vec<_> = fs::read_dir(p.parent().unwrap())
            .unwrap()
            .filter_map(Result::ok)
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .filter(|name| name != "app.desktop")
            .collect();
        assert!(leftovers.is_empty(), "{leftovers:?}");
        let _ = fs::remove_dir_all(p.parent().unwrap());
    }

    #[test]
    fn keys_with_spaces_are_read_and_replaced_not_duplicated() {
        let p = entry_file(
            "spaces",
            "app.desktop",
            "[Desktop Entry]\nName = App\nIcon = old\n\n[Desktop Action new]\nIcon=action\n",
        );
        let mut e = DesktopEntry::load(p.clone()).unwrap();
        assert_eq!((e.name.as_str(), e.icon_value.as_str()), ("App", "old"));
        e.icon_value = "new".into();
        e.save().unwrap();
        let out = fs::read_to_string(&p).unwrap();
        assert_eq!(out.matches("Icon=new").count(), 1, "{out}");
        assert!(out.contains("Icon=action"), "other sections are left alone: {out}");
        assert!(!out.contains("old"), "{out}");
        let _ = fs::remove_dir_all(p.parent().unwrap());
    }
}
