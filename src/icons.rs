//! Finding and decoding the image behind an `Icon=` value, for previews.
//!
//! An `Icon=` value is either a file path or a name looked up in the
//! desktop's icon theme, following the freedesktop icon theme spec: the
//! active theme first (e.g. Adwaita), then the themes it inherits from
//! (e.g. AdwaitaLegacy), then hicolor, then the pixmaps folders.

use eframe::egui;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use crate::dirs_next;

/// Size icons are decoded at: large enough for the detail panel's hero icon.
pub const ICON_TEXTURE_SIZE: u32 = 128;

/// Image formats the preview can decode.
const EXTENSIONS: [&str; 2] = ["png", "svg"];

/// Every icon name available in the icon themes, mapped to its best file.
/// Built once (and again after a refresh or an icon import), because
/// searching the theme folders for each icon on every frame is far too slow.
#[derive(Default)]
pub struct IconIndex {
    icons: HashMap<String, PathBuf>,
}

/// Rank of a candidate file: lower is better. Your own icon folders come
/// first, then theme order, then the base folder, then size.
///
/// Putting your own folders ahead of the system themes departs from strict
/// theme order, but matches what GNOME shows: an icon installed by this tool
/// as ~/.local/share/icons/hicolor/…/phone.png appears in the dock and app
/// grid even though the active theme (Adwaita) has its own `phone`.
type Rank = (bool, usize, usize, i64);

impl IconIndex {
    /// Index the icon themes of the running desktop.
    pub fn for_desktop() -> Self {
        let data_dirs = data_dirs();
        let mut icon_bases = vec![dirs_next().join(".icons")];
        icon_bases.extend(data_dirs.iter().map(|d| d.join("icons")));
        // Keep ~/.local/share/icons (data_dirs[0]) ahead of ~/.icons.
        icon_bases.swap(0, 1);

        let themes = theme_chain(&icon_bases, active_theme());
        let pixmaps: Vec<PathBuf> = data_dirs.iter().map(|d| d.join("pixmaps")).collect();
        // The first two bases (~/.local/share/icons, ~/.icons) are the user's.
        Self::build(&icon_bases, 2, &themes, &pixmaps)
    }

    /// Index `themes` (in priority order) under each of `bases` (in priority
    /// order, the first `user_bases` of them being the user's own), falling
    /// back to the flat `pixmaps` folders.
    pub fn build(
        bases: &[PathBuf],
        user_bases: usize,
        themes: &[String],
        pixmaps: &[PathBuf],
    ) -> Self {
        let mut best: HashMap<String, (Rank, PathBuf)> = HashMap::new();
        let mut offer = |name: String, rank: Rank, path: PathBuf| {
            if best.get(&name).map_or(true, |(r, _)| rank < *r) {
                best.insert(name, (rank, path));
            }
        };

        for (ti, theme) in themes.iter().enumerate() {
            for (bi, base) in bases.iter().enumerate() {
                let root = base.join(theme);
                let mut files = Vec::new();
                collect_icon_files(&root, 3, &mut files);
                for file in files {
                    let rel = file.strip_prefix(&root).unwrap_or(&file);
                    let score = size_score(rel.parent().unwrap_or(Path::new("")));
                    if let Some(name) = icon_name(&file) {
                        offer(name, (bi >= user_bases, ti, bi, -score), file);
                    }
                }
            }
        }

        for (bi, dir) in pixmaps.iter().enumerate() {
            let mut files = Vec::new();
            collect_icon_files(dir, 0, &mut files);
            for file in files {
                if let Some(name) = icon_name(&file) {
                    offer(name, (true, themes.len(), bi, 0), file);
                }
            }
        }

        Self {
            icons: best.into_iter().map(|(k, (_, p))| (k, p)).collect(),
        }
    }

    fn lookup(&self, name: &str) -> Option<&Path> {
        self.icons.get(name).map(PathBuf::as_path)
    }
}

/// Resolve an `Icon=` value to a file to preview: a path (absolute, or with a
/// legacy `~` prefix) if it exists, otherwise a name in the icon themes.
pub fn resolve_icon_path(value: &str, index: &IconIndex) -> Option<PathBuf> {
    if value.is_empty() {
        return None;
    }
    let p = if let Some(rest) = value.strip_prefix('~') {
        // strip_prefix (not a fixed &value[2..] byte slice) handles a bare
        // "~", a "~" not followed by '/', and a non-ASCII byte immediately
        // after '~' without panicking on an out-of-bounds or non-char-boundary
        // index -all of which a malformed Icon= value in a .desktop file can
        // produce.
        dirs_next().join(rest.trim_start_matches('/'))
    } else {
        PathBuf::from(value)
    };
    if p.is_file() {
        return Some(p);
    }
    if value.contains('/') {
        return None;
    }
    // Some entries wrongly include an extension in a theme name.
    let bare = EXTENSIONS
        .iter()
        .chain(["xpm"].iter())
        .find_map(|ext| value.strip_suffix(&format!(".{ext}")))
        .unwrap_or(value);
    index
        .lookup(value)
        .or_else(|| index.lookup(bare))
        .map(Path::to_path_buf)
}

/// A decoded icon. Symbolic icons are single-colour shapes, kept as a white
/// mask so the UI can tint them to suit the theme.
pub struct DecodedIcon {
    pub image: egui::ColorImage,
    pub symbolic: bool,
}

/// Decode a PNG, JPEG, SVG (and so on) to fit in ICON_TEXTURE_SIZE.
pub fn decode(path: &Path) -> Option<DecodedIcon> {
    let bytes = fs::read(path).ok()?;
    // Decide by content, not extension: AppImage integrations often write
    // icons with no extension at all.
    let head = String::from_utf8_lossy(&bytes[..bytes.len().min(512)]).to_ascii_lowercase();
    let is_svg = head.contains("<svg") || (head.contains("<?xml") && head.contains("svg"));
    let mut image = if is_svg {
        egui_extras::image::load_svg_bytes_with_size(
            &bytes,
            Some(egui::SizeHint::Size(ICON_TEXTURE_SIZE, ICON_TEXTURE_SIZE)),
        )
        .ok()?
    } else {
        let img = image::load_from_memory(&bytes).ok()?.resize(
            ICON_TEXTURE_SIZE,
            ICON_TEXTURE_SIZE,
            image::imageops::FilterType::Lanczos3,
        );
        let rgba = img.to_rgba8();
        let size = [rgba.width() as usize, rgba.height() as usize];
        egui::ColorImage::from_rgba_unmultiplied(size, rgba.as_raw())
    };

    let symbolic = path
        .file_stem()
        .and_then(|s| s.to_str())
        .is_some_and(|s| s.ends_with("-symbolic"));
    if symbolic {
        for px in &mut image.pixels {
            *px = egui::Color32::from_white_alpha(px.a());
        }
    }
    Some(DecodedIcon { image, symbolic })
}

/// $XDG_DATA_HOME then $XDG_DATA_DIRS, plus Flatpak's exports in case the
/// session didn't add them.
fn data_dirs() -> Vec<PathBuf> {
    let home = dirs_next();
    let mut dirs = vec![std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".local/share"))];
    let system = std::env::var("XDG_DATA_DIRS")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "/usr/local/share:/usr/share".into());
    dirs.extend(system.split(':').filter(|s| !s.is_empty()).map(PathBuf::from));
    for extra in [
        home.join(".local/share/flatpak/exports/share"),
        PathBuf::from("/var/lib/flatpak/exports/share"),
    ] {
        if !dirs.contains(&extra) {
            dirs.push(extra);
        }
    }
    let mut seen = HashSet::new();
    dirs.retain(|d| seen.insert(d.clone()));
    dirs
}

/// The desktop's icon theme: GNOME's setting, else KDE's, if either is set.
fn active_theme() -> Option<String> {
    let gnome = std::process::Command::new("gsettings")
        .args(["get", "org.gnome.desktop.interface", "icon-theme"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().trim_matches('\'').to_string())
        .filter(|s| !s.is_empty());
    gnome.or_else(|| {
        let kde = fs::read_to_string(dirs_next().join(".config/kdeglobals")).ok()?;
        let mut in_icons = false;
        for line in kde.lines() {
            let line = line.trim();
            if line.starts_with('[') {
                in_icons = line == "[Icons]";
            } else if in_icons {
                if let Some(v) = line.strip_prefix("Theme=") {
                    return Some(v.trim().to_string());
                }
            }
        }
        None
    })
}

/// The theme, everything it inherits from (depth first, as the spec says),
/// and hicolor last.
pub fn theme_chain(bases: &[PathBuf], theme: Option<String>) -> Vec<String> {
    fn visit(name: &str, bases: &[PathBuf], out: &mut Vec<String>) {
        if out.iter().any(|t| t == name) || name == "hicolor" {
            return;
        }
        out.push(name.to_string());
        let inherits = bases
            .iter()
            .find_map(|b| fs::read_to_string(b.join(name).join("index.theme")).ok())
            .and_then(|text| {
                text.lines()
                    .find_map(|l| l.trim().strip_prefix("Inherits=").map(str::to_string))
            })
            .unwrap_or_default();
        for parent in inherits.split(',').map(str::trim).filter(|s| !s.is_empty()) {
            visit(parent, bases, out);
        }
    }

    let mut chain = Vec::new();
    if let Some(theme) = theme {
        visit(&theme, bases, &mut chain);
    }
    chain.push("hicolor".to_string());
    chain
}

/// Collect image files in `dir` and its subfolders, `depth` levels down.
fn collect_icon_files(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        // Follow symlinks: themes often link one size folder to another.
        if path.is_dir() {
            if depth > 0 {
                collect_icon_files(&path, depth - 1, out);
            }
        } else if icon_name(&path).is_some() {
            out.push(path);
        }
    }
}

/// The icon name of a decodable file: its name without the extension.
fn icon_name(path: &Path) -> Option<String> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    if !EXTENSIONS.contains(&ext.as_str()) {
        return None;
    }
    Some(path.file_stem()?.to_str()?.to_string())
}

/// How good a theme sub-folder is for a large preview: scalable (SVG) best,
/// then the largest pixel size named in the path (`256x256/apps`,
/// `apps/48`, `16x16@2/apps`).
fn size_score(rel_dir: &Path) -> i64 {
    let s = rel_dir.to_string_lossy();
    if s.contains("scalable") || s.contains("symbolic") {
        return 10_000;
    }
    let mut best = 0;
    for part in s.split(|c: char| !c.is_ascii_digit() && c != '@') {
        let (size, scale) = part.split_once('@').unwrap_or((part, "1"));
        let size: i64 = size.parse().unwrap_or(0);
        let scale: i64 = scale.parse().unwrap_or(1).max(1);
        best = best.max(size * scale);
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;

    // Regression tests for a panic previously triggered by a malformed
    // Icon= value in a .desktop file (a fixed &value[2..] byte slice paniced
    // on "byte index 2 out of bounds" / "not a char boundary"). None of
    // these paths exist on a test machine, so each call falls through to
    // the theme lookup and returns None -the only thing under test is that
    // none of them panic.

    fn empty() -> IconIndex {
        IconIndex::default()
    }

    #[test]
    fn bare_tilde_does_not_panic() {
        // "~" strips to an empty remainder, which joins to the home
        // directory itself. The property under test is "doesn't panic",
        // not any particular Some/None outcome.
        let _ = resolve_icon_path("~", &empty());
    }

    #[test]
    fn tilde_followed_by_multibyte_char_does_not_panic() {
        // 'é' is 2 UTF-8 bytes starting at byte offset 1 -byte index 2 (the
        // old &value[2..]) lands mid-character here, not on a char boundary.
        assert_eq!(resolve_icon_path("~é", &empty()), None);
    }

    #[test]
    fn tilde_with_no_separator_does_not_panic() {
        assert_eq!(resolve_icon_path("~foo.png", &empty()), None);
    }

    #[test]
    fn well_formed_tilde_path_still_resolves_without_panicking() {
        assert_eq!(resolve_icon_path("~/nonexistent-icon-xyz.png", &empty()), None);
    }

    #[test]
    fn empty_value_returns_none() {
        assert_eq!(resolve_icon_path("", &empty()), None);
    }

    // ── Theme lookup, against a throwaway icon tree ─────────────────────────

    struct Tree(PathBuf);

    impl Tree {
        fn new(tag: &str) -> Self {
            let dir = std::env::temp_dir().join(format!(
                "dim-icons-test-{tag}-{}",
                std::process::id()
            ));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(&dir).unwrap();
            Tree(dir)
        }
        fn file(&self, rel: &str) -> PathBuf {
            let p = self.0.join(rel);
            fs::create_dir_all(p.parent().unwrap()).unwrap();
            fs::write(&p, b"").unwrap();
            p
        }
    }

    impl Drop for Tree {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn active_theme_wins_over_hicolor() {
        let t = Tree::new("theme-order");
        t.file("sys/hicolor/256x256/apps/foo.png");
        let want = t.file("sys/Adwaita/16x16/apps/foo.png");
        let bases = vec![t.0.join("sys")];
        let themes = vec!["Adwaita".into(), "hicolor".into()];
        let index = IconIndex::build(&bases, 0, &themes, &[]);
        assert_eq!(resolve_icon_path("foo", &index), Some(want));
    }

    #[test]
    fn inherited_theme_is_searched_before_hicolor() {
        let t = Tree::new("inherits");
        let sys = t.0.join("sys");
        fs::create_dir_all(sys.join("Adwaita")).unwrap();
        fs::write(sys.join("Adwaita/index.theme"), "[Icon Theme]\nInherits=AdwaitaLegacy,hicolor\n")
            .unwrap();
        let chain = theme_chain(&[sys.clone()], Some("Adwaita".into()));
        assert_eq!(chain, ["Adwaita", "AdwaitaLegacy", "hicolor"]);

        let want = t.file("sys/AdwaitaLegacy/22x22/legacy/security-medium.png");
        let index = IconIndex::build(&[sys], 0, &chain, &[]);
        assert_eq!(resolve_icon_path("security-medium", &index), Some(want));
    }

    #[test]
    fn user_icons_override_system_and_larger_sizes_win() {
        let t = Tree::new("user-first");
        t.file("sys/hicolor/scalable/apps/app.svg");
        t.file("user/hicolor/16x16/apps/app.png");
        let want = t.file("user/hicolor/256x256/apps/app.png");
        let bases = vec![t.0.join("user"), t.0.join("sys")];
        let index = IconIndex::build(&bases, 1, &["hicolor".into()], &[]);
        assert_eq!(resolve_icon_path("app", &index), Some(want));
    }

    #[test]
    fn your_own_icons_override_the_active_theme() {
        // An icon this tool installed (user hicolor) beats the same name in
        // the system's active theme, as it does in GNOME's dock.
        let t = Tree::new("user-over-theme");
        t.file("sys/Adwaita/scalable/devices/phone.svg");
        let want = t.file("user/hicolor/256x256/apps/phone.png");
        let bases = vec![t.0.join("user"), t.0.join("sys")];
        let themes = vec!["Adwaita".into(), "hicolor".into()];
        let index = IconIndex::build(&bases, 1, &themes, &[]);
        assert_eq!(resolve_icon_path("phone", &index), Some(want));
    }

    #[test]
    fn scalable_beats_fixed_sizes_and_pixmaps_are_the_fallback() {
        let t = Tree::new("scalable");
        t.file("sys/hicolor/512x512/apps/a.png");
        let want_a = t.file("sys/hicolor/scalable/apps/a.svg");
        let want_b = t.file("pix/b.png");
        let index =
            IconIndex::build(&[t.0.join("sys")], 0, &["hicolor".into()], &[t.0.join("pix")]);
        assert_eq!(resolve_icon_path("a", &index), Some(want_a));
        assert_eq!(resolve_icon_path("b", &index), Some(want_b.clone()));
        // A theme name written with an extension still resolves.
        assert_eq!(resolve_icon_path("b.png", &index), Some(want_b));
    }

    #[test]
    fn size_score_reads_common_layouts() {
        assert_eq!(size_score(Path::new("256x256/apps")), 256);
        assert_eq!(size_score(Path::new("apps/48")), 48);
        assert_eq!(size_score(Path::new("16x16@2/apps")), 32);
        assert!(size_score(Path::new("scalable/apps")) > 512);
    }
}
