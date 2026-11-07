//! Supported input formats and the context-menu selection filter.

use std::path::{Path, PathBuf};

/// Input extensions Reduct accepts, lowercase, without the dot.
pub const SUPPORTED_EXTENSIONS: &[&str] =
    &["jpg", "jpeg", "jfif", "png", "webp", "heic", "heif", "avif"];

/// Lowercased extension of `path`, if it has one.
fn ext_lower(path: &Path) -> Option<String> {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
}

fn ext_in(path: &Path, set: &[&str]) -> bool {
    ext_lower(path).is_some_and(|e| set.contains(&e.as_str()))
}

/// True when the extension is one of [`SUPPORTED_EXTENSIONS`], ignoring case.
pub fn is_supported(path: &Path) -> bool {
    ext_in(path, SUPPORTED_EXTENSIONS)
}

/// True for `jpg`, `jpeg` or `jfif`, ignoring case.
pub fn is_jpeg(path: &Path) -> bool {
    ext_in(path, &["jpg", "jpeg", "jfif"])
}

/// True for `heic` or `heif`, ignoring case.
pub fn is_heif(path: &Path) -> bool {
    ext_in(path, &["heic", "heif"])
}

/// Whether the context-menu entry should show for a selection.
///
/// Each item is the file-system path of a selected shell item, or `None` when
/// the item has no file-system path. True only if the selection is non-empty,
/// every item has a path and every path is supported.
pub fn should_show<I: IntoIterator<Item = Option<PathBuf>>>(items: I) -> bool {
    let mut any = false;
    for item in items {
        match item {
            Some(p) if is_supported(&p) => any = true,
            _ => return false,
        }
    }
    any
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &str) -> PathBuf {
        PathBuf::from(s)
    }

    #[test]
    fn is_supported_is_case_insensitive() {
        assert!(is_supported(&p("IMG_1.HEIC")));
        assert!(is_supported(&p("a.Jpg")));
        assert!(is_supported(&p("b.jfif")));
        assert!(!is_supported(&p("c.jp2")));
        assert!(!is_supported(&p("d.gif")));
        assert!(!is_supported(&p("noext")));
        assert!(!is_supported(&p(".heic")));
    }

    #[test]
    fn jpeg_and_heif_groups() {
        assert!(is_jpeg(&p("a.JPEG")));
        assert!(is_jpeg(&p("a.jfif")));
        assert!(!is_jpeg(&p("a.png")));
        assert!(is_heif(&p("a.HEIF")));
        assert!(is_heif(&p("a.heic")));
        assert!(!is_heif(&p("a.avif")));
    }

    #[test]
    fn should_show_hides_non_filesystem_and_unsupported() {
        assert!(!should_show(Vec::<Option<PathBuf>>::new()));
        assert!(!should_show(vec![Some(p("a.jpg")), None]));
        assert!(!should_show(vec![Some(p("a.jpg")), Some(p("b.txt"))]));
        assert!(should_show(vec![Some(p("a.jpg")), Some(p("B.PNG"))]));
    }
}
