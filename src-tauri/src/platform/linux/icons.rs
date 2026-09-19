//! Freedesktop icon theme lookup.
//!
//! Bloom only needs to turn an `.desktop` `Icon=` value into something an
//! `<img>` can render. The lookup follows the freedesktop icon theme
//! specification loosely: explicit paths win, then the user's icon theme, then
//! the other installed themes, then the flat pixmap directories. Resolved icons
//! are memoized because theme lookup stats a lot of paths.

use std::{
    collections::{HashMap, HashSet},
    env, fs,
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
};

const CATEGORIES: [&str; 7] = [
    "apps",
    "applications",
    "categories",
    "devices",
    "mimetypes",
    "places",
    "status",
];

const EXTENSIONS: [&str; 2] = ["svg", "png"];

/// Scalable art wins over any raster size.
const SCALABLE_SCORE: u32 = 1_000_000;

/// The freedesktop specification's own fallback theme.
const FALLBACK_THEME: &str = "hicolor";

fn home() -> Option<PathBuf> {
    env::var_os("HOME").map(PathBuf::from)
}

fn icon_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(home) = home() {
        roots.push(home.join(".local/share/icons"));
        roots.push(home.join(".icons"));
    }
    roots.push(PathBuf::from("/usr/local/share/icons"));
    roots.push(PathBuf::from("/usr/share/icons"));
    roots
}

fn pixmap_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(home) = home() {
        dirs.push(home.join(".local/share/pixmaps"));
    }
    dirs.push(PathBuf::from("/usr/local/share/pixmaps"));
    dirs.push(PathBuf::from("/usr/share/pixmaps"));
    dirs
}

/// The desktop's configured icon theme.
///
/// Consulted in order of authority without shelling out to `gsettings`: the
/// XSETTINGS manager (what Cinnamon uses) and then the GTK settings files.
fn detect_preferred_theme() -> Option<String> {
    if let Ok(session) = super::x11::session() {
        if let Some(theme) = session.xsettings_string("Net/IconThemeName") {
            let theme = theme.trim();
            if !theme.is_empty() {
                return Some(theme.to_owned());
            }
        }
    }

    let home = home()?;
    for relative in ["gtk-3.0", "gtk-4.0"] {
        let path = home.join(".config").join(relative).join("settings.ini");
        let Ok(contents) = fs::read_to_string(path) else {
            continue;
        };
        for line in contents.lines() {
            if let Some(value) = line.trim().strip_prefix("gtk-icon-theme-name=") {
                let value = value.trim().trim_matches('"');
                if !value.is_empty() {
                    return Some(value.to_owned());
                }
            }
        }
    }
    None
}

fn preferred_theme() -> Option<String> {
    static DETECTED: OnceLock<Option<String>> = OnceLock::new();
    DETECTED.get_or_init(detect_preferred_theme).clone()
}

/// Accessibility-first themes are kept as a last resort so the dock keeps the
/// desktop's normal, colourful artwork whenever it exists.
fn is_low_colour_theme(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    name.contains("contrast") || name.contains("locolor")
}

/// A theme is usable for application icons only if it ships an icon category.
/// Cursor-only themes are skipped rather than scanned on every lookup.
fn has_icon_category(theme_dir: &Path) -> bool {
    subdirectories(theme_dir).into_iter().any(|(name, dir)| {
        is_category(&name)
            || subdirectories(&dir)
                .iter()
                .any(|(nested, _)| is_category(nested))
    })
}

/// Ordered, de-duplicated list of installed theme directories.
///
/// The desktop's configured theme is tried first. Other themes follow in a
/// stable order so an icon is still found when the configured theme does not
/// ship it: normal themes, then accessibility-first themes, then the
/// specification's deliberately sparse `hicolor` fallback. Cursor-only themes
/// are skipped entirely.
fn theme_dirs() -> Vec<PathBuf> {
    let preferred = preferred_theme();
    let mut ordered: Vec<PathBuf> = Vec::new();
    let mut ordinary: Vec<(String, PathBuf)> = Vec::new();
    let mut low_colour: Vec<(String, PathBuf)> = Vec::new();
    let mut seen: HashSet<PathBuf> = HashSet::new();

    for root in icon_roots() {
        let Ok(entries) = fs::read_dir(&root) else {
            continue;
        };
        for entry in entries.flatten() {
            let dir = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            if name == FALLBACK_THEME
                || !dir.is_dir()
                || !seen.insert(dir.clone())
                || !has_icon_category(&dir)
            {
                continue;
            }
            if preferred.as_deref() == Some(name.as_str()) {
                ordered.push(dir);
            } else if is_low_colour_theme(&name) {
                low_colour.push((name, dir));
            } else {
                ordinary.push((name, dir));
            }
        }
    }

    ordinary.sort_by(|left, right| left.0.cmp(&right.0));
    low_colour.sort_by(|left, right| left.0.cmp(&right.0));
    ordered.extend(ordinary.into_iter().map(|(_, dir)| dir));
    ordered.extend(low_colour.into_iter().map(|(_, dir)| dir));

    for root in icon_roots() {
        let dir = root.join(FALLBACK_THEME);
        if dir.is_dir() && seen.insert(dir.clone()) {
            ordered.push(dir);
        }
    }
    ordered
}

/// Score a size directory so larger raster icons win. Handles both `48x48` and
/// `256@2x` spellings; anything unparseable scores lowest.
fn size_score(directory: &str) -> u32 {
    let (base, scale) = match directory.split_once('@') {
        Some((base, scale)) => (base, scale.trim_end_matches(['x', 'X'])),
        None => (directory, "1"),
    };
    let base = base
        .split(['x', 'X'])
        .next()
        .and_then(|value| value.trim().parse::<u32>().ok())
        .unwrap_or(1);
    let scale = scale.trim().parse::<u32>().unwrap_or(1);
    base.saturating_mul(scale)
}

fn is_category(name: &str) -> bool {
    CATEGORIES.contains(&name)
}

fn is_symbolic(dir_name: &str) -> bool {
    dir_name.contains("symbolic")
}

fn icon_in(dir: &Path, name: &str, size_hint: &str) -> Option<(u32, PathBuf)> {
    for extension in EXTENSIONS {
        let candidate = dir.join(format!("{name}.{extension}"));
        if candidate.is_file() {
            let score = if extension == "svg" {
                SCALABLE_SCORE
            } else if size_hint.is_empty() {
                1
            } else {
                size_score(size_hint)
            };
            return Some((score, candidate));
        }
    }
    None
}

fn subdirectories(dir: &Path) -> Vec<(String, PathBuf)> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            path.is_dir()
                .then(|| (entry.file_name().to_string_lossy().into_owned(), path))
        })
        .collect()
}

/// Collect icon candidates from a theme, supporting both layouts used in
/// practice:
///
/// * `<theme>/<size>/<category>/<name>` — hicolor, Adwaita, Yaru
/// * `<theme>/<category>/<size>/<name>` — Mint-X/Y/L, Papirus, ePapirus
fn candidates_in_theme(theme_dir: &Path, name: &str) -> Vec<(u32, PathBuf)> {
    let mut candidates = Vec::new();
    for (first_name, first_dir) in subdirectories(theme_dir) {
        if is_symbolic(&first_name) {
            continue;
        }
        if is_category(&first_name) {
            for (size_name, size_dir) in subdirectories(&first_dir) {
                if is_symbolic(&size_name) {
                    continue;
                }
                candidates.extend(icon_in(&size_dir, name, &size_name));
            }
            // `<theme>/<category>/<name>` also occurs.
            candidates.extend(icon_in(&first_dir, name, ""));
        } else {
            for (second_name, second_dir) in subdirectories(&first_dir) {
                if is_symbolic(&second_name) || !is_category(&second_name) {
                    continue;
                }
                candidates.extend(icon_in(&second_dir, name, &first_name));
            }
        }
    }
    // A few themes keep the artwork directly in the theme root.
    candidates.extend(icon_in(theme_dir, name, ""));
    candidates
}

fn best_candidate(theme_dir: &Path, name: &str) -> Option<PathBuf> {
    candidates_in_theme(theme_dir, name)
        .into_iter()
        .max_by_key(|(score, _)| *score)
        .map(|(_, path)| path)
}

/// Resolve a freedesktop icon value to a file on disk.
pub fn resolve(icon: &str) -> Option<PathBuf> {
    let icon = icon.trim();
    if icon.is_empty() {
        return None;
    }

    let direct = Path::new(icon);
    if direct.is_absolute() && direct.is_file() {
        return image_extension(direct).map(|_| direct.to_path_buf());
    }

    for theme_dir in theme_dirs() {
        if let Some(path) = best_candidate(&theme_dir, icon) {
            return Some(path);
        }
    }

    // Flat collections, including values that already carry an extension.
    for dir in pixmap_dirs() {
        if let Some(path) = icon_in(&dir, icon, "") {
            return Some(path.1);
        }
        let direct = dir.join(icon);
        if direct.is_file() && image_extension(&direct).is_some() {
            return Some(direct);
        }
    }

    None
}

fn image_extension(path: &Path) -> Option<String> {
    let extension = path.extension()?.to_str()?.to_ascii_lowercase();
    matches!(extension.as_str(), "svg" | "png").then_some(extension)
}

/// Encode an icon as a data URI the frontend can render directly.
pub fn data_uri(path: &Path) -> Option<String> {
    let extension = image_extension(path)?;
    let metadata = fs::metadata(path).ok()?;
    // Guard against pathological files so a single icon cannot blow up memory.
    if metadata.len() > 4 * 1024 * 1024 {
        return None;
    }
    let mime = if extension == "svg" {
        "image/svg+xml"
    } else {
        "image/png"
    };
    use base64::{engine::general_purpose::STANDARD, Engine};
    let bytes = fs::read(path).ok()?;
    Some(format!("data:{mime};base64,{}", STANDARD.encode(bytes)))
}

/// Resolve an icon value straight to a data URI, memoized per icon name.
pub fn data_uri_for_name(name: &str) -> Option<String> {
    static CACHE: OnceLock<Mutex<HashMap<String, Option<String>>>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    if let Ok(cache) = cache.lock() {
        if let Some(hit) = cache.get(name) {
            return hit.clone();
        }
    }
    let resolved = resolve(name).as_deref().and_then(data_uri);
    if let Ok(mut cache) = cache.lock() {
        cache.insert(name.to_owned(), resolved.clone());
    }
    resolved
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn size_directories_are_ranked_by_actual_pixel_size() {
        assert_eq!(size_score("48x48"), 48);
        assert_eq!(size_score("256x256"), 256);
        assert_eq!(size_score("256@2x"), 512);
        assert_eq!(size_score("16@2"), 32);
        assert_eq!(size_score("96"), 96);
        assert_eq!(size_score("scalable"), 1);
    }

    #[test]
    fn both_theme_layouts_are_recognised() {
        let root = env::temp_dir().join("bloom-icon-layout-test");
        let _ = fs::remove_dir_all(&root);
        // Layout A: <theme>/<size>/<category>/<name>
        let a = root.join("theme-a").join("48x48").join("apps");
        fs::create_dir_all(&a).expect("create layout a");
        fs::write(a.join("sample.png"), b"png").expect("write layout a icon");
        // Layout B: <theme>/<category>/<size>/<name>
        let b = root
            .join("theme-b")
            .join("apps")
            .join("256@2x");
        fs::create_dir_all(&b).expect("create layout b");
        fs::write(b.join("sample.png"), b"png").expect("write layout b icon");

        let from_a = candidates_in_theme(&root.join("theme-a"), "sample");
        assert_eq!(from_a.len(), 1);
        assert_eq!(from_a[0].0, 48);

        let from_b = candidates_in_theme(&root.join("theme-b"), "sample");
        assert_eq!(from_b.len(), 1);
        assert_eq!(from_b[0].0, 512);

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn symbolic_directories_are_never_used() {
        let root = env::temp_dir().join("bloom-icon-symbolic-test");
        let _ = fs::remove_dir_all(&root);
        let dir = root.join("theme").join("symbolic").join("apps");
        fs::create_dir_all(&dir).expect("create symbolic dir");
        fs::write(dir.join("sample.svg"), b"<svg/>").expect("write symbolic icon");
        assert!(candidates_in_theme(&root.join("theme"), "sample").is_empty());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn missing_icons_resolve_to_nothing_instead_of_panicking() {
        assert!(data_uri_for_name("bloom-definitely-not-an-installed-icon").is_none());
    }

    #[test]
    fn only_renderable_extensions_become_data_uris() {
        let path = env::temp_dir().join("bloom-test-icon.svg");
        fs::write(&path, b"<svg xmlns='http://www.w3.org/2000/svg'/>").expect("write icon");
        let uri = data_uri(&path).expect("svg should encode");
        assert!(uri.starts_with("data:image/svg+xml;base64,"));
        let _ = fs::remove_file(path);

        let xpm = env::temp_dir().join("bloom-test-icon.xpm");
        fs::write(&xpm, b"/* XPM */").expect("write xpm");
        assert!(data_uri(&xpm).is_none());
        let _ = fs::remove_file(xpm);
    }
}
