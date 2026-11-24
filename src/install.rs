//! Installing an uploaded image into the user's icon theme.
//!
//! The original is copied to the icons folder, then installed into
//! `~/.local/share/icons/hicolor` at every standard size (or as a scalable
//! SVG), with a fallback in `~/.local/share/pixmaps`. `Icon=` is then set to
//! the bare theme name, which docks and launchers resolve; they ignore a
//! file path for pinned and running apps.

use image::{imageops::FilterType, DynamicImage, ImageFormat, RgbaImage};
use std::fs;
use std::path::{Path, PathBuf};

/// Pixel sizes installed under `hicolor/<size>x<size>/apps`.
pub const SIZES: [u32; 7] = [16, 32, 48, 64, 128, 256, 512];

/// Where an imported icon goes.
pub struct Targets {
    /// The user's icons folder (default ~/Pictures/Icons): the original is kept here.
    pub icons_dir: PathBuf,
    /// ~/.local/share/icons/hicolor
    pub hicolor: PathBuf,
    /// ~/.local/share/pixmaps
    pub pixmaps: PathBuf,
}

impl Targets {
    pub fn for_user(icons_dir: &Path) -> Self {
        let home = crate::dirs_next();
        Self {
            icons_dir: icons_dir.to_path_buf(),
            hicolor: home.join(".local/share/icons/hicolor"),
            pixmaps: home.join(".local/share/pixmaps"),
        }
    }
}

/// A theme name derived from a `.desktop` entry's filename: its stem,
/// lower-cased, with anything but letters, digits, `-` and `_` replaced by
/// `-`. Naming the icon after the entry (not the image) means two entries
/// picking images both called `icon.png` don't share a theme name.
pub fn theme_name_for(entry: &Path) -> String {
    let name: String = entry
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy()
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '-' })
        .collect::<String>()
        .to_lowercase();
    let name = name.trim_matches('-');
    if name.is_empty() { "icon".into() } else { name.into() }
}

/// Every path an import of `name` writes, in the user's icon folders as well
/// as the theme. Used both to clean up before reinstalling and to spot names
/// that are already in use -including icons another app installed locally.
pub fn installed_files(targets: &Targets, name: &str) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = SIZES
        .iter()
        .map(|s| targets.hicolor.join(format!("{s}x{s}/apps/{name}.png")))
        .collect();
    files.push(targets.hicolor.join(format!("scalable/apps/{name}.svg")));
    files.push(targets.pixmaps.join(format!("{name}.png")));
    files.push(targets.pixmaps.join(format!("{name}.svg")));
    // The kept originals are named after the theme, never after the image,
    // so they can't overwrite an unrelated file that happens to share the
    // image's filename.
    for ext in ["png", "jpg", "jpeg", "svg", "ico", "bmp", "webp"] {
        files.push(targets.icons_dir.join(format!("{name}.{ext}")));
    }
    files
}

/// Pick the first free name, adding `-custom` (then `-custom-2`, …) when a
/// name would collide with another app's icon.
fn choose_theme_name(base: &str, taken: impl Fn(&str) -> bool) -> String {
    let mut candidate = base.to_string();
    let mut attempt = 0;
    while taken(&candidate) {
        attempt += 1;
        candidate = if attempt == 1 {
            format!("{base}-custom")
        } else {
            format!("{base}-custom-{attempt}")
        };
    }
    candidate
}

/// Install `source` as the icon for `entry` and return its theme name.
///
/// `taken(name)` says whether `name` already belongs to another icon: a
/// system theme's icon, or one in the user's own icon folders. Installing
/// over it would change that app's icon too, so a free suffix is chosen.
pub fn install_icon(
    source: &Path,
    entry: &Path,
    targets: &Targets,
    taken: impl Fn(&str) -> bool,
) -> Result<String, String> {
    let theme_name = choose_theme_name(&theme_name_for(entry), taken);
    let bytes = fs::read(source).map_err(|e| format!("Could not read {}: {e}", source.display()))?;
    let is_svg = looks_like_svg(&bytes);
    // Decode before touching anything, so a bad file leaves no half-install.
    let raster = if is_svg {
        None
    } else {
        // Trust the content, not the extension: a ".png" is often a JPEG.
        let img = image::load_from_memory(&bytes)
            .map_err(|e| format!("Could not read the image: {e}"))?;
        Some(square(img))
    };

    // 1. Keep the original in the icons folder.
    fs::create_dir_all(&targets.icons_dir)
        .map_err(|e| format!("Could not create {}: {e}", targets.icons_dir.display()))?;
    let ext = source
        .extension()
        .and_then(|e| e.to_str())
        .filter(|e| !e.is_empty())
        .unwrap_or("png");
    let dest = targets.icons_dir.join(format!("{theme_name}.{ext}"));
    // fs::copy truncates the destination before reading the source, so
    // copying a file onto itself (re-picking an icon from icons_dir) would
    // empty it.
    let same_file = source
        .canonicalize()
        .ok()
        .zip(dest.canonicalize().ok())
        .is_some_and(|(s, d)| s == d);
    if !same_file {
        fs::write(&dest, &bytes).map_err(|e| format!("Copy failed: {e}"))?;
    }

    // 2. Drop what an earlier import of this name left behind: a stale
    //    scalable SVG would outrank the new PNGs, and vice versa.
    remove_installed(targets, &theme_name);

    // 3. Install into the hicolor theme and the pixmaps fallback.
    let write_err = |p: &Path, e: &dyn std::fmt::Display| format!("Could not write {}: {e}", p.display());
    fs::create_dir_all(&targets.pixmaps).map_err(|e| write_err(&targets.pixmaps, &e))?;
    if let Some(img) = raster {
        for size in SIZES {
            let dir = targets.hicolor.join(format!("{size}x{size}/apps"));
            fs::create_dir_all(&dir).map_err(|e| write_err(&dir, &e))?;
            let out = dir.join(format!("{theme_name}.png"));
            resize(&img, size)
                .save_with_format(&out, ImageFormat::Png)
                .map_err(|e| write_err(&out, &e))?;
        }
        let out = targets.pixmaps.join(format!("{theme_name}.png"));
        resize(&img, 256)
            .save_with_format(&out, ImageFormat::Png)
            .map_err(|e| write_err(&out, &e))?;
    } else {
        let dir = targets.hicolor.join("scalable/apps");
        fs::create_dir_all(&dir).map_err(|e| write_err(&dir, &e))?;
        for out in [dir.join(format!("{theme_name}.svg")), targets.pixmaps.join(format!("{theme_name}.svg"))] {
            fs::write(&out, &bytes).map_err(|e| write_err(&out, &e))?;
        }
    }

    // 4. Refresh the icon cache so the change shows at once. An old cache
    //    left in place would hide the new icon from GTK.
    let _ = std::process::Command::new("gtk-update-icon-cache")
        .args(["-f", "-t", "-q"])
        .arg(&targets.hicolor)
        .output();

    Ok(theme_name)
}

/// Remove every theme/pixmap file an import of `name` installs. The kept
/// original is deliberately left in the icons folder: it is the user's copy.
fn remove_installed(targets: &Targets, name: &str) {
    for p in installed_files(targets, name) {
        if !p.starts_with(&targets.icons_dir) {
            let _ = fs::remove_file(p);
        }
    }
}

fn looks_like_svg(bytes: &[u8]) -> bool {
    let head = String::from_utf8_lossy(&bytes[..bytes.len().min(512)]).to_ascii_lowercase();
    head.contains("<svg") || (head.contains("<?xml") && head.contains("svg"))
}

/// Centre the image on a transparent square, so icons in `NxN` folders are
/// really N×N.
fn square(img: DynamicImage) -> RgbaImage {
    let img = img.to_rgba8();
    let (w, h) = img.dimensions();
    if w == h {
        return img;
    }
    let side = w.max(h);
    let mut canvas = RgbaImage::new(side, side);
    image::imageops::overlay(&mut canvas, &img, i64::from((side - w) / 2), i64::from((side - h) / 2));
    canvas
}

fn resize(img: &RgbaImage, size: u32) -> RgbaImage {
    image::imageops::resize(img, size, size, FilterType::Lanczos3)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Tmp(PathBuf);
    impl Tmp {
        fn new(tag: &str) -> Self {
            let dir = std::env::temp_dir().join(format!("dim-install-{tag}-{}", std::process::id()));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(&dir).unwrap();
            Tmp(dir)
        }
        fn targets(&self) -> Targets {
            Targets {
                icons_dir: self.0.join("Icons"),
                hicolor: self.0.join("hicolor"),
                pixmaps: self.0.join("pixmaps"),
            }
        }
    }
    impl Drop for Tmp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn write_png(path: &Path, w: u32, h: u32, format: ImageFormat) {
        let img = image::RgbImage::from_pixel(w, h, image::Rgb([200, 30, 30]));
        img.save_with_format(path, format).unwrap();
    }

    /// Names in use from the user's folders, as the app checks them.
    fn local_taken(targets: &Targets) -> impl Fn(&str) -> bool + '_ {
        |name| installed_files(targets, name).iter().any(|p| p.exists())
    }

    const SVG: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10"><rect width="10" height="10"/></svg>"#;

    #[test]
    fn raster_is_installed_as_square_pngs_at_every_size() {
        let t = Tmp::new("raster");
        let src = t.0.join("My App.png");
        write_png(&src, 300, 100, ImageFormat::Png);
        let name = install_icon(&src, &t.0.join("My App.desktop"), &t.targets(), |_| false).unwrap();
        assert_eq!(name, "my-app");
        for s in SIZES {
            let p = t.0.join(format!("hicolor/{s}x{s}/apps/my-app.png"));
            assert_eq!(image::image_dimensions(&p).unwrap(), (s, s), "{}", p.display());
        }
        assert!(t.0.join("Icons/my-app.png").is_file());
        // The pixmaps fallback is a real PNG, not a renamed copy.
        let pix = t.0.join("pixmaps/my-app.png");
        assert_eq!(image::ImageFormat::from_path(&pix).unwrap(), ImageFormat::Png);
        assert_eq!(image::image_dimensions(&pix).unwrap(), (256, 256));
    }

    #[test]
    fn content_decides_the_format_not_the_extension() {
        let t = Tmp::new("mislabelled");
        let src = t.0.join("photo.png");
        write_png(&src, 64, 64, ImageFormat::Jpeg);
        install_icon(&src, &t.0.join("photo.desktop"), &t.targets(), |_| false).unwrap();
        assert!(t.0.join("hicolor/48x48/apps/photo.png").is_file());
    }

    #[test]
    fn an_undecodable_file_is_an_error_and_installs_nothing() {
        let t = Tmp::new("broken");
        let src = t.0.join("broken.png");
        fs::write(&src, b"not an image").unwrap();
        assert!(install_icon(&src, &t.0.join("broken.desktop"), &t.targets(), |_| false).is_err());
        assert!(!t.0.join("hicolor").exists());
        assert!(!t.0.join("Icons/broken.png").exists());
    }

    #[test]
    fn svg_goes_to_scalable_and_pixmaps_as_svg() {
        let t = Tmp::new("svg");
        let src = t.0.join("logo.svg");
        fs::write(&src, SVG).unwrap();
        install_icon(&src, &t.0.join("logo.desktop"), &t.targets(), |_| false).unwrap();
        assert!(t.0.join("hicolor/scalable/apps/logo.svg").is_file());
        assert!(t.0.join("pixmaps/logo.svg").is_file());
        assert!(!t.0.join("pixmaps/logo.png").exists());
    }

    #[test]
    fn reimporting_a_name_removes_the_old_format() {
        let t = Tmp::new("reimport");
        let svg = t.0.join("logo.svg");
        fs::write(&svg, SVG).unwrap();
        install_icon(&svg, &t.0.join("logo.desktop"), &t.targets(), |_| false).unwrap();
        let png = t.0.join("logo.png");
        write_png(&png, 32, 32, ImageFormat::Png);
        install_icon(&png, &t.0.join("logo.desktop"), &t.targets(), |_| false).unwrap();
        assert!(!t.0.join("hicolor/scalable/apps/logo.svg").exists());
        assert!(!t.0.join("pixmaps/logo.svg").exists());
        assert!(t.0.join("hicolor/256x256/apps/logo.png").is_file());
    }

    #[test]
    fn a_name_used_by_another_app_gets_a_suffix() {
        let t = Tmp::new("taken");
        let src = t.0.join("firefox.png");
        write_png(&src, 32, 32, ImageFormat::Png);
        let name = install_icon(
            &src,
            &t.0.join("firefox.desktop"),
            &t.targets(),
            |n| n == "firefox",
        )
        .unwrap();
        assert_eq!(name, "firefox-custom");
    }

    #[test]
    fn two_entries_with_the_same_image_name_get_different_theme_names() {
        let t = Tmp::new("same-image");
        let src = t.0.join("icon.png");
        write_png(&src, 32, 32, ImageFormat::Png);
        let targets = t.targets();
        let music = install_icon(&src, &t.0.join("Music.desktop"), &targets, local_taken(&targets)).unwrap();
        let mail = install_icon(&src, &t.0.join("Mail.desktop"), &targets, local_taken(&targets)).unwrap();
        assert_eq!((music.as_str(), mail.as_str()), ("music", "mail"));
        assert!(targets.icons_dir.join("music.png").is_file());
        assert!(targets.icons_dir.join("mail.png").is_file());
    }

    #[test]
    fn an_icon_in_the_users_theme_is_taken_and_left_alone() {
        let t = Tmp::new("user-taken");
        let targets = t.targets();
        let other = targets.hicolor.join("48x48/apps/notes.png");
        fs::create_dir_all(other.parent().unwrap()).unwrap();
        write_png(&other, 48, 48, ImageFormat::Png);
        let before = fs::read(&other).unwrap();

        let src = t.0.join("icon.png");
        write_png(&src, 32, 32, ImageFormat::Png);
        let name = install_icon(
            &src,
            &t.0.join("notes.desktop"),
            &targets,
            local_taken(&targets),
        )
        .unwrap();
        assert_eq!(name, "notes-custom");
        assert_eq!(fs::read(&other).unwrap(), before);
    }

    #[test]
    fn an_unrelated_file_with_the_image_name_is_not_overwritten() {
        let t = Tmp::new("keep-original");
        let targets = t.targets();
        fs::create_dir_all(&targets.icons_dir).unwrap();
        let unrelated = targets.icons_dir.join("icon.png");
        fs::write(&unrelated, b"someone else's picture").unwrap();

        let src = t.0.join("icon.png");
        write_png(&src, 32, 32, ImageFormat::Png);
        let name = install_icon(
            &src,
            &t.0.join("notes.desktop"),
            &targets,
            local_taken(&targets),
        )
        .unwrap();
        assert_eq!(name, "notes");
        assert_eq!(fs::read(&unrelated).unwrap(), b"someone else's picture");
        assert!(image::image_dimensions(&targets.icons_dir.join("notes.png")).is_ok());
    }

    #[test]
    fn suffixes_escalate_until_the_name_is_free() {
        let t = Tmp::new("escalate");
        let src = t.0.join("icon.png");
        write_png(&src, 32, 32, ImageFormat::Png);
        let name = install_icon(
            &src,
            &t.0.join("notes.desktop"),
            &t.targets(),
            |n| n == "notes" || n == "notes-custom",
        )
        .unwrap();
        assert_eq!(name, "notes-custom-2");
    }

    #[test]
    fn repicking_the_kept_original_does_not_empty_it() {
        let t = Tmp::new("same");
        let targets = t.targets();
        fs::create_dir_all(&targets.icons_dir).unwrap();
        let src = targets.icons_dir.join("a.png");
        write_png(&src, 16, 16, ImageFormat::Png);
        install_icon(&src, &t.0.join("a.desktop"), &targets, |_| false).unwrap();
        assert!(fs::metadata(&src).unwrap().len() > 0);
    }
}
