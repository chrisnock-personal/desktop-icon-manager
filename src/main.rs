use eframe::egui;
use egui::{Color32, RichText, ScrollArea, Sense, Stroke, Vec2};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

mod icon_data;

// ──────────────────────────────────────────────────────────────────────────────
// Data structures
// ──────────────────────────────────────────────────────────────────────────────

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
    /// Dirty flag — true if any field was modified but not yet saved
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
            if let Some(v) = line.strip_prefix("Name=") {
                name = v.to_string();
            } else if let Some(v) = line.strip_prefix("Icon=") {
                icon_value = v.to_string();
            } else if let Some(v) = line.strip_prefix("StartupWMClass=") {
                startup_wm_class = Some(v.to_string());
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
    fn save(&mut self) -> Result<(), String> {
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
                        if let Some(ref wm) = self.startup_wm_class {
                            new_lines.push(format!("StartupWMClass={wm}"));
                        }
                    }
                }
                in_desktop_section = false;
                new_lines.push(line.to_string());
                continue;
            }

            if in_desktop_section {
                if trimmed.starts_with("Icon=") {
                    new_lines.push(format!("Icon={}", self.icon_value));
                    icon_written = true;
                    continue;
                }
                if trimmed.starts_with("StartupWMClass=") && self.is_chrome_pwa {
                    if let Some(ref wm) = self.startup_wm_class {
                        new_lines.push(format!("StartupWMClass={wm}"));
                    } else {
                        new_lines.push(line.to_string());
                    }
                    wm_written = true;
                    continue;
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
                if let Some(ref wm) = self.startup_wm_class {
                    new_lines.push(format!("StartupWMClass={wm}"));
                }
            }
        }

        let new_content = new_lines.join("\n") + "\n";
        fs::write(&self.path, new_content).map_err(|e| format!("Write error: {e}"))?;
        self.modified = false;
        self.status = Some("✓ Saved".to_string());
        Ok(())
    }
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

#[derive(Default)]
struct IconManagerApp {
    entries: Vec<DesktopEntry>,
    /// Index of the currently selected entry (for the detail pane)
    selected: Option<usize>,
    /// Search / filter string
    filter: String,
    /// Global status bar message
    global_status: String,
    /// Whether to show only Chrome PWA entries
    show_only_pwa: bool,
    /// Cached icon textures: icon_path_string → TextureHandle
    icon_textures: HashMap<String, egui::TextureHandle>,
    /// Icons folder (absolute path): default ~/Pictures/Icons, user-editable
    icons_dir: PathBuf,
    /// Editable text buffer for the icons dir path field
    icons_dir_input: String,
    /// Applications folder: ~/.local/share/applications
    apps_dir: PathBuf,
}

impl IconManagerApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        // ── Light theme ──────────────────────────────────────────────────
        let mut visuals = egui::Visuals::light();
        visuals.window_rounding        = egui::Rounding::same(10.0);
        // Panel backgrounds
        visuals.panel_fill             = Color32::from_rgb(245, 247, 252); // near-white blue tint
        visuals.window_fill            = Color32::from_rgb(255, 255, 255);
        // Widget states
        visuals.widgets.noninteractive.bg_fill = Color32::from_rgb(235, 239, 248);
        visuals.widgets.noninteractive.bg_stroke =
            egui::Stroke::new(1.0, Color32::from_rgb(210, 216, 232));
        visuals.widgets.inactive.bg_fill   = Color32::from_rgb(225, 231, 246);
        visuals.widgets.inactive.bg_stroke =
            egui::Stroke::new(1.0, Color32::from_rgb(195, 205, 228));
        visuals.widgets.hovered.bg_fill    = Color32::from_rgb(210, 222, 248);
        visuals.widgets.hovered.bg_stroke  =
            egui::Stroke::new(1.5, Color32::from_rgb(100, 149, 237));
        visuals.widgets.active.bg_fill     = Color32::from_rgb(100, 149, 237);
        visuals.widgets.active.fg_stroke   =
            egui::Stroke::new(2.0, Color32::WHITE);
        // Text
        visuals.override_text_color = Some(Color32::from_rgb(30, 35, 55));
        // Selection
        visuals.selection.bg_fill = Color32::from_rgb(180, 205, 250);
        visuals.selection.stroke  = egui::Stroke::new(1.0, Color32::from_rgb(90, 140, 230));
        // Separators / faint lines
        visuals.widgets.noninteractive.fg_stroke =
            egui::Stroke::new(1.0, Color32::from_rgb(200, 208, 228));
        cc.egui_ctx.set_visuals(visuals);

        let home = dirs_next();
        let apps_dir = home.join(".local/share/applications");
        let icons_dir = home.join("Pictures/Icons");

        // Ensure icons directory exists
        if !icons_dir.exists() {
            let _ = fs::create_dir_all(&icons_dir);
        }

        let icons_dir_input = icons_dir.display().to_string();
        let mut app = Self {
            apps_dir,
            icons_dir,
            icons_dir_input,
            ..Default::default()
        };
        app.refresh_entries();
        app
    }

    fn refresh_entries(&mut self) {
        self.entries.clear();
        self.selected = None;
        self.global_status = String::new();

        if !self.apps_dir.exists() {
            self.global_status = format!(
                "Applications directory not found: {}",
                self.apps_dir.display()
            );
            return;
        }

        match fs::read_dir(&self.apps_dir) {
            Ok(dir) => {
                let mut paths: Vec<PathBuf> = dir
                    .filter_map(|e| e.ok())
                    .map(|e| e.path())
                    .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("desktop"))
                    .collect();
                paths.sort();

                for path in paths {
                    if let Some(entry) = DesktopEntry::load(path) {
                        self.entries.push(entry);
                    }
                }
                self.global_status =
                    format!("Loaded {} .desktop entries", self.entries.len());
            }
            Err(e) => {
                self.global_status = format!("Error reading applications dir: {e}");
            }
        }
    }

    /// Copy icon file to the configured icons directory and return the absolute path string.
    /// Always uses an absolute path — ~ is not expanded by desktop file parsers.
    fn import_icon(&self, source: &Path) -> Result<String, String> {
        let filename = source
            .file_name()
            .ok_or("Invalid source filename")?
            .to_string_lossy()
            .to_string();
        let dest = self.icons_dir.join(&filename);
        fs::copy(source, &dest).map_err(|e| format!("Copy failed: {e}"))?;
        // Always return the absolute, canonical path — ~ is shell syntax and
        // is NOT expanded by desktop file parsers (GNOME, KDE, etc.)
        dest.canonicalize()
            .map(|p| p.display().to_string())
            .map_err(|e| format!("Could not resolve path: {e}"))
    }

    fn load_icon_texture(
        &mut self,
        ctx: &egui::Context,
        icon_value: &str,
    ) -> Option<egui::TextureHandle> {
        if icon_value.is_empty() {
            return None;
        }
        if let Some(tex) = self.icon_textures.get(icon_value) {
            return Some(tex.clone());
        }

        // Resolve path
        let path = resolve_icon_path(icon_value)?;
        let img = image::open(&path).ok()?;
        let img = img.resize(48, 48, image::imageops::FilterType::Lanczos3);
        let rgba = img.to_rgba8();
        let size = [rgba.width() as usize, rgba.height() as usize];
        let pixels = rgba.into_raw();
        let color_image =
            egui::ColorImage::from_rgba_unmultiplied(size, &pixels);
        let tex = ctx.load_texture(icon_value, color_image, egui::TextureOptions::LINEAR);
        self.icon_textures.insert(icon_value.to_string(), tex.clone());
        Some(tex)
    }
}

/// Resolve an icon value to an actual file path (absolute or ~-prefixed).
fn resolve_icon_path(value: &str) -> Option<PathBuf> {
    if value.is_empty() {
        return None;
    }
    let p = if value.starts_with('~') {
        dirs_next().join(&value[2..])
    } else {
        PathBuf::from(value)
    };
    if p.exists() {
        return Some(p);
    }
    // Try common icon theme locations for named icons
    for dir in &[
        "/usr/share/icons/hicolor/48x48/apps",
        "/usr/share/icons/hicolor/256x256/apps",
        "/usr/share/pixmaps",
    ] {
        for ext in &["png", "svg", "xpm"] {
            let candidate = PathBuf::from(dir).join(format!("{value}.{ext}"));
            if candidate.exists() {
                return Some(candidate);
            }
        }
    }
    None
}

fn dirs_next() -> PathBuf {
    std::env::var("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/root"))
}

// ──────────────────────────────────────────────────────────────────────────────
// eframe App implementation
// ──────────────────────────────────────────────────────────────────────────────

impl eframe::App for IconManagerApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // ── Top panel: toolbar ───────────────────────────────────────────────
        egui::TopBottomPanel::top("toolbar")
            .frame(
                egui::Frame::none()
                    .fill(Color32::from_rgb(255, 255, 255))
                    .inner_margin(egui::Margin::symmetric(12.0, 8.0)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("🖥  Desktop Icon Manager")
                            .size(18.0)
                            .color(Color32::from_rgb(60, 100, 210))
                            .strong(),
                    );
                    ui.add_space(16.0);

                    if ui
                        .button(RichText::new("⟳  Refresh").size(13.0))
                        .on_hover_text("Reload all .desktop files")
                        .clicked()
                    {
                        self.refresh_entries();
                    }

                    ui.add_space(8.0);
                    ui.separator();
                    ui.add_space(8.0);

                    ui.label("🔍");
                    ui.add(
                        egui::TextEdit::singleline(&mut self.filter)
                            .hint_text("Filter apps…")
                            .desired_width(200.0),
                    );
                    if !self.filter.is_empty() && ui.small_button("✕").clicked() {
                        self.filter.clear();
                    }

                    ui.add_space(8.0);
                    ui.checkbox(&mut self.show_only_pwa, "Chrome PWAs only");

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            RichText::new(&self.global_status)
                                .size(11.0)
                                .color(Color32::from_rgb(30, 130, 80)),
                        );
                    });
                });
            });

        // ── Bottom status bar ────────────────────────────────────────────────
        egui::TopBottomPanel::bottom("statusbar")
            .frame(
                egui::Frame::none()
                    .fill(Color32::from_rgb(232, 237, 250))
                    .inner_margin(egui::Margin::symmetric(10.0, 6.0)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    // Apps dir (read-only label)
                    ui.label(
                        RichText::new(format!("Apps: {}", self.apps_dir.display()))
                            .size(10.0)
                            .color(Color32::from_rgb(90, 100, 140)),
                    );
                    ui.separator();

                    // Icons dir — editable text field + browse button
                    ui.label(
                        RichText::new("Icons dir:")
                            .size(10.0)
                            .color(Color32::from_rgb(90, 100, 140)),
                    );

                    let input_resp = ui.add(
                        egui::TextEdit::singleline(&mut self.icons_dir_input)
                            .desired_width(300.0)
                            .font(egui::FontId::monospace(11.0))
                            .hint_text("/absolute/path/to/icons"),
                    );

                    // Commit on Enter or focus-loss
                    if input_resp.lost_focus() || ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        let new_path = PathBuf::from(self.icons_dir_input.trim());
                        if new_path.is_absolute() {
                            if !new_path.exists() {
                                let _ = fs::create_dir_all(&new_path);
                            }
                            self.icons_dir = new_path;
                            self.icons_dir_input = self.icons_dir.display().to_string();
                            self.global_status = format!(
                                "Icons dir set to {}",
                                self.icons_dir.display()
                            );
                        } else {
                            // Reset the text field to current valid value
                            self.icons_dir_input = self.icons_dir.display().to_string();
                            self.global_status =
                                "Icons dir must be an absolute path (starting with /)".to_string();
                        }
                    }

                    // Browse button — native folder picker
                    if ui
                        .button(RichText::new("📂").size(13.0))
                        .on_hover_text("Browse for icons folder")
                        .clicked()
                    {
                        if let Some(picked) = rfd::FileDialog::new()
                            .set_title("Select Icons Folder")
                            .set_directory(&self.icons_dir)
                            .pick_folder()
                        {
                            if !picked.exists() {
                                let _ = fs::create_dir_all(&picked);
                            }
                            self.icons_dir = picked;
                            self.icons_dir_input = self.icons_dir.display().to_string();
                            self.global_status = format!(
                                "Icons dir set to {}",
                                self.icons_dir.display()
                            );
                        }
                    }
                });
            });

        // ── Right detail panel ───────────────────────────────────────────────
        if let Some(sel) = self.selected {
            if sel < self.entries.len() {
                egui::SidePanel::right("detail_panel")
                    .default_width(360.0)
                    .min_width(300.0)
                    .frame(
                        egui::Frame::none()
                            .fill(Color32::from_rgb(255, 255, 255))
                            .inner_margin(egui::Margin::same(16.0)),
                    )
                    .show(ctx, |ui| {
                        self.draw_detail_panel(ui, ctx, sel);
                    });
            }
        }

        // ── Central panel: app list ──────────────────────────────────────────
        egui::CentralPanel::default()
            .frame(
                egui::Frame::none()
                    .fill(Color32::from_rgb(245, 247, 252))
                    .inner_margin(egui::Margin::same(8.0)),
            )
            .show(ctx, |ui| {
                self.draw_entry_list(ui, ctx);
            });
    }
}

impl IconManagerApp {
    // ── Entry list ───────────────────────────────────────────────────────────

    fn draw_entry_list(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let filter = self.filter.to_lowercase();

        // Build filtered index list first (to avoid borrow issues)
        let filtered: Vec<usize> = (0..self.entries.len())
            .filter(|&i| {
                let e = &self.entries[i];
                if self.show_only_pwa && !e.is_chrome_pwa {
                    return false;
                }
                if !filter.is_empty() {
                    let name_lower = e.name.to_lowercase();
                    let icon_lower = e.icon_value.to_lowercase();
                    if !name_lower.contains(&filter) && !icon_lower.contains(&filter) {
                        return false;
                    }
                }
                true
            })
            .collect();

        ui.label(
            RichText::new(format!("Showing {} of {} entries", filtered.len(), self.entries.len()))
                .size(11.0)
                .color(Color32::from_rgb(140, 155, 185)),
        );
        ui.add_space(4.0);

        // Column headers
        egui::Grid::new("header_grid")
            .num_columns(4)
            .min_col_width(10.0)
            .show(ui, |ui| {
                ui.label(RichText::new("Icon").size(11.0).color(Color32::from_rgb(80, 90, 140)));
                ui.label(RichText::new("Application").size(11.0).color(Color32::from_rgb(80, 90, 140)));
                ui.label(RichText::new("Icon Value").size(11.0).color(Color32::from_rgb(80, 90, 140)));
                ui.label(RichText::new("Flags").size(11.0).color(Color32::from_rgb(80, 90, 140)));
                ui.end_row();
            });

        ui.separator();

        ScrollArea::vertical()
            .id_source("entry_list")
            .auto_shrink([false; 2])
            .show(ui, |ui| {
                for &idx in &filtered {
                    let is_selected = self.selected == Some(idx);

                    // Clone what we need for display
                    let entry_name = self.entries[idx].name.clone();
                    let entry_icon = self.entries[idx].icon_value.clone();
                    let is_pwa = self.entries[idx].is_chrome_pwa;
                    let is_modified = self.entries[idx].modified;
                    let entry_status = self.entries[idx].status.clone();

                    // Try to load texture
                    let tex = self.load_icon_texture(ctx, &entry_icon);

                    let bg_color = if is_selected {
                        Color32::from_rgb(210, 225, 252)
                    } else {
                        Color32::TRANSPARENT
                    };

                    let row_frame = egui::Frame::none()
                        .fill(bg_color)
                        .rounding(egui::Rounding::same(6.0))
                        .inner_margin(egui::Margin::symmetric(6.0, 4.0));

                    let response = row_frame.show(ui, |ui| {
                        ui.horizontal(|ui| {
                            // Icon thumbnail
                            if let Some(ref tex) = tex {
                                ui.image(egui::load::SizedTexture::new(
                                    tex.id(),
                                    Vec2::new(32.0, 32.0),
                                ));
                            } else {
                                // Placeholder
                                let (rect, _) = ui.allocate_exact_size(
                                    Vec2::new(32.0, 32.0),
                                    Sense::hover(),
                                );
                                ui.painter().rect_filled(
                                    rect,
                                    egui::Rounding::same(4.0),
                                    Color32::from_rgb(210, 220, 240),
                                );
                                ui.painter().text(
                                    rect.center(),
                                    egui::Align2::CENTER_CENTER,
                                    "?",
                                    egui::FontId::proportional(16.0),
                                    Color32::from_rgb(140, 155, 185),
                                );
                            }

                            ui.add_space(6.0);

                            // Name
                            let name_label = if is_modified {
                                RichText::new(format!("● {entry_name}"))
                                    .color(Color32::from_rgb(180, 100, 20))
                                    .size(13.0)
                            } else {
                                RichText::new(&entry_name).size(13.0)
                            };
                            ui.add_sized(
                                Vec2::new(180.0, 32.0),
                                egui::Label::new(name_label).truncate(true),
                            );

                            // Icon path (truncated)
                            ui.add_sized(
                                Vec2::new(180.0, 32.0),
                                egui::Label::new(
                                    RichText::new(&entry_icon)
                                        .size(11.0)
                                        .color(Color32::from_rgb(30, 120, 70)),
                                )
                                .truncate(true),
                            );

                            // Badges
                            if is_pwa {
                                let badge_frame = egui::Frame::none()
                                    .fill(Color32::from_rgb(210, 225, 252))
                                    .rounding(egui::Rounding::same(4.0))
                                    .inner_margin(egui::Margin::symmetric(4.0, 2.0));
                                badge_frame.show(ui, |ui| {
                                    ui.label(
                                        RichText::new("PWA")
                                            .size(10.0)
                                            .color(Color32::from_rgb(50, 90, 200)),
                                    );
                                });
                            }

                            // Status
                            if let Some(ref s) = entry_status {
                                ui.label(
                                    RichText::new(s)
                                        .size(11.0)
                                        .color(Color32::from_rgb(20, 140, 70)),
                                );
                            }
                        });
                    });

                    // Make the whole row clickable
                    let row_resp = ui.interact(
                        response.response.rect,
                        ui.id().with(("row", idx)),
                        Sense::click(),
                    );
                    if row_resp.clicked() {
                        self.selected = Some(idx);
                    }
                    if row_resp.hovered() && !is_selected {
                        ui.painter().rect_stroke(
                            response.response.rect,
                            egui::Rounding::same(6.0),
                            Stroke::new(1.0, Color32::from_rgb(100, 149, 237)),
                        );
                    }

                    ui.add_space(2.0);
                }
            });
    }

    // ── Detail / edit panel ──────────────────────────────────────────────────

    fn draw_detail_panel(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, sel: usize) {
        let entry = &self.entries[sel];
        let entry_name = entry.name.clone();
        let entry_path = entry.path.display().to_string();
        let is_pwa = entry.is_chrome_pwa;

        ui.label(
            RichText::new("Edit Entry")
                .size(15.0)
                .color(Color32::from_rgb(50, 90, 200))
                .strong(),
        );
        ui.add_space(4.0);
        ui.separator();
        ui.add_space(8.0);

        // App name
        ui.label(RichText::new("Application").size(11.0).color(Color32::from_rgb(80, 95, 140)));
        ui.label(RichText::new(&entry_name).size(14.0).strong());
        ui.add_space(4.0);

        // File path
        ui.label(RichText::new("File").size(11.0).color(Color32::from_rgb(80, 95, 140)));
        ui.add(
            egui::Label::new(
                RichText::new(&entry_path)
                    .size(10.0)
                    .color(Color32::from_rgb(100, 115, 155)),
            )
            .truncate(true),
        );
        ui.add_space(10.0);

        // ── Icon preview ─────────────────────────────────────────────────────
        {
            let icon_value = self.entries[sel].icon_value.clone();
            let tex = self.load_icon_texture(ctx, &icon_value);
            ui.label(RichText::new("Current Icon").size(11.0).color(Color32::from_rgb(80, 95, 140)));
            ui.add_space(4.0);

            egui::Frame::none()
                .fill(Color32::from_rgb(235, 240, 252))
                .rounding(egui::Rounding::same(8.0))
                .inner_margin(egui::Margin::same(12.0))
                .show(ui, |ui| {
                    ui.vertical_centered(|ui| {
                        if let Some(ref t) = tex {
                            ui.image(egui::load::SizedTexture::new(
                                t.id(),
                                Vec2::new(80.0, 80.0),
                            ));
                        } else {
                            let (rect, _) =
                                ui.allocate_exact_size(Vec2::new(80.0, 80.0), Sense::hover());
                            ui.painter().rect_filled(
                                rect,
                                egui::Rounding::same(8.0),
                                Color32::from_rgb(210, 220, 240),
                            );
                            ui.painter().text(
                                rect.center(),
                                egui::Align2::CENTER_CENTER,
                                "No Preview",
                                egui::FontId::proportional(11.0),
                                Color32::from_rgb(130, 130, 160),
                            );
                        }
                    });
                });
        }

        ui.add_space(10.0);

        // ── Icon= field editor ───────────────────────────────────────────────
        ui.label(RichText::new("Icon= value").size(11.0).color(Color32::from_rgb(80, 95, 140)));
        ui.add_space(2.0);

        let icon_changed = {
            let entry = &mut self.entries[sel];
            let before = entry.icon_value.clone();
            ui.add(
                egui::TextEdit::singleline(&mut entry.icon_value)
                    .desired_width(f32::INFINITY)
                    .font(egui::FontId::monospace(12.0)),
            );
            entry.icon_value != before
        };
        if icon_changed {
            self.entries[sel].modified = true;
            self.entries[sel].status = None;
        }

        ui.add_space(8.0);

        // ── Upload icon button ────────────────────────────────────────────────
        ui.horizontal(|ui| {
            let upload_btn = ui
                .button(RichText::new("📁  Upload Icon…").size(13.0))
                .on_hover_text(format!(
                    "Select an image file — it will be copied to {}",
                    self.icons_dir.display()
                ));

            if upload_btn.clicked() {
                // rfd file dialog (native)
                let picked = rfd::FileDialog::new()
                    .add_filter("Images", &["png", "jpg", "jpeg", "svg", "ico", "bmp", "webp"])
                    .set_title("Select Icon Image")
                    .pick_file();

                if let Some(src_path) = picked {
                    match self.import_icon(&src_path) {
                        Ok(new_icon_path) => {
                            // Invalidate old texture cache entry
                            let old_key = self.entries[sel].icon_value.clone();
                            self.icon_textures.remove(&old_key);

                            self.entries[sel].icon_value = new_icon_path;
                            self.entries[sel].modified = true;
                            self.entries[sel].status = Some("Icon imported — save to apply".into());
                            self.global_status = format!(
                                "Icon copied to {}",
                                self.icons_dir.display()
                            );
                        }
                        Err(e) => {
                            self.entries[sel].status = Some(format!("✗ {e}"));
                        }
                    }
                }
            }
        });

        // ── Chrome PWA section ────────────────────────────────────────────────
        if is_pwa {
            ui.add_space(14.0);
            egui::Frame::none()
                .fill(Color32::from_rgb(235, 242, 255))
                .rounding(egui::Rounding::same(8.0))
                .inner_margin(egui::Margin::same(10.0))
                .stroke(Stroke::new(1.0, Color32::from_rgb(160, 190, 240)))
                .show(ui, |ui| {
                    ui.label(
                        RichText::new("🌐  Chrome PWA Configuration")
                            .size(13.0)
                            .color(Color32::from_rgb(50, 90, 200))
                            .strong(),
                    );
                    ui.add_space(6.0);

                    // Detected app id
                    let chrome_id = self.entries[sel]
                        .chrome_app_id
                        .clone()
                        .unwrap_or_else(|| "(not detected)".to_string());
                    ui.label(
                        RichText::new(format!("App ID slug: {chrome_id}"))
                            .size(11.0)
                            .color(Color32::from_rgb(20, 120, 60)),
                    );
                    ui.add_space(6.0);

                    // StartupWMClass editor
                    ui.label(
                        RichText::new("StartupWMClass=")
                            .size(11.0)
                            .color(Color32::from_rgb(160, 160, 190)),
                    );

                    let wm_changed = {
                        let entry = &mut self.entries[sel];
                        let mut wm_val = entry
                            .startup_wm_class
                            .clone()
                            .unwrap_or_default();
                        let before = wm_val.clone();
                        ui.add(
                            egui::TextEdit::singleline(&mut wm_val)
                                .desired_width(f32::INFINITY)
                                .font(egui::FontId::monospace(12.0))
                                .hint_text("e.g. chrome-<appid>-Default"),
                        );
                        let changed = wm_val != before;
                        entry.startup_wm_class = Some(wm_val);
                        changed
                    };
                    if wm_changed {
                        self.entries[sel].modified = true;
                    }

                    ui.add_space(6.0);

                    // Auto-fill StartupWMClass from original Icon value
                    if ui
                        .button(RichText::new("⚡ Auto-fill from Icon value").size(12.0))
                        .on_hover_text(
                            "Sets StartupWMClass to the original icon name\n\
                             (e.g. chrome-<appid>-Default), which fixes\n\
                             window matching for Chrome PWAs.",
                        )
                        .clicked()
                    {
                        let slug = self.entries[sel]
                            .chrome_app_id
                            .clone()
                            .unwrap_or_else(|| self.entries[sel].icon_value.clone());
                        self.entries[sel].startup_wm_class = Some(slug);
                        self.entries[sel].modified = true;
                        self.entries[sel].status = Some("WMClass set — save to apply".into());
                    }
                });
        }

        // ── Status message ────────────────────────────────────────────────────
        if let Some(ref s) = self.entries[sel].status.clone() {
            ui.add_space(8.0);
            let color = if s.starts_with('✓') {
                Color32::from_rgb(20, 140, 70)
            } else if s.starts_with('✗') {
                Color32::from_rgb(190, 40, 40)
            } else {
                Color32::from_rgb(150, 110, 20)
            };
            ui.label(RichText::new(s).size(12.0).color(color));
        }

        ui.add_space(12.0);
        ui.separator();
        ui.add_space(8.0);

        // ── Save / Discard buttons ─────────────────────────────────────────────
        ui.horizontal(|ui| {
            let modified = self.entries[sel].modified;

            let save_btn = ui.add_enabled(
                modified,
                egui::Button::new(RichText::new("💾  Save").size(13.0)).min_size(Vec2::new(100.0, 30.0)),
            )
            .on_hover_text("Write changes back to the .desktop file");

            if save_btn.clicked() {
                match self.entries[sel].save() {
                    Ok(()) => {
                        self.global_status = format!("Saved: {}", self.entries[sel].name);
                    }
                    Err(e) => {
                        self.entries[sel].status = Some(format!("✗ {e}"));
                    }
                }
            }

            ui.add_space(8.0);

            let discard_btn = ui
                .add_enabled(
                    modified,
                    egui::Button::new(RichText::new("↩  Discard").size(13.0)).min_size(Vec2::new(100.0, 30.0)),
                )
                .on_hover_text("Reload this entry from disk");

            if discard_btn.clicked() {
                let path = self.entries[sel].path.clone();
                if let Some(fresh) = DesktopEntry::load(path) {
                    self.entries[sel] = fresh;
                }
            }
        });

        ui.add_space(8.0);

        // Close detail panel
        if ui
            .button(RichText::new("✕  Close").size(12.0).color(Color32::from_rgb(160, 60, 60)))
            .clicked()
        {
            self.selected = None;
        }
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// Entry point
// ──────────────────────────────────────────────────────────────────────────────

fn main() -> eframe::Result<()> {
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Desktop Icon Manager")
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
