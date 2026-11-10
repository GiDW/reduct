//! Collision-safe output naming.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::lang::Lang;

/// How a clashing output name gets a suffix.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SuffixStyle {
    /// `stem_2.jpg`, `stem_3.jpg`, ...
    Underscore,
    /// `stem (optimised).jpg`, `stem (optimised 2).jpg`, ..., with the word in
    /// the UI language (`geoptimaliseerd` in Dutch).
    Optimised,
}

/// Where outputs are written.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OutputLocation {
    /// In the source's folder.
    NextToOriginal,
    /// In a subfolder of the source's folder, named in the UI language
    /// (`Optimised`, `Geoptimaliseerd`: [`Text::subfolder`](crate::lang::Text::subfolder)).
    Subfolder,
}

/// The output file name for attempt `k`, including `.jpg`.
pub fn candidate(stem: &str, style: SuffixStyle, lang: Lang, k: u32) -> String {
    let word = lang.text().suffix_word;
    match (style, k) {
        (_, 0) => format!("{stem}.jpg"),
        (SuffixStyle::Underscore, k) => format!("{stem}_{}.jpg", k + 1),
        (SuffixStyle::Optimised, 1) => format!("{stem} ({word}).jpg"),
        (SuffixStyle::Optimised, k) => format!("{stem} ({word} {k}).jpg"),
    }
}

/// The folder the output for `source` goes into.
pub fn target_dir(source: &Path, loc: OutputLocation, lang: Lang) -> PathBuf {
    let parent = source.parent().map(Path::to_path_buf).unwrap_or_default();
    match loc {
        OutputLocation::NextToOriginal => parent,
        OutputLocation::Subfolder => parent.join(lang.text().subfolder),
    }
}

/// One planned output.
///
/// `style` is not in the brief's field list, but `target()` cannot build the
/// file name for `k > 0` without it, so it is carried here as well.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Planned {
    pub source: PathBuf,
    pub dir: PathBuf,
    pub stem: String,
    pub k: u32,
    pub style: SuffixStyle,
    /// The language of the `(optimised)` suffix.
    pub lang: Lang,
}

impl Planned {
    /// Full path of the planned output file: `dir` joined with `candidate(stem, style, lang, k)`.
    pub fn target(&self) -> PathBuf {
        self.dir.join(self.name(self.k))
    }

    /// The file name for attempt `k`: `candidate(stem, style, lang, k)`.
    pub fn name(&self, k: u32) -> String {
        candidate(&self.stem, self.style, self.lang, k)
    }
}

/// Lowercased full path string, used to compare paths case-insensitively.
fn key(path: &Path) -> String {
    path.to_string_lossy().to_lowercase()
}

/// Plans one output per source, in order.
///
/// For each source it picks the lowest `k` whose target neither `exists` nor
/// matches a target already claimed in this batch (case-insensitive). The
/// sources themselves also count as claimed, so an original is never chosen
/// as a target even if `exists` says it is absent. No directories are created.
pub fn plan_outputs(
    sources: &[PathBuf],
    style: SuffixStyle,
    loc: OutputLocation,
    lang: Lang,
    exists: &dyn Fn(&Path) -> bool,
) -> Vec<Planned> {
    let mut claimed: HashSet<String> = sources.iter().map(|s| key(s)).collect();
    sources
        .iter()
        .map(|source| {
            let dir = target_dir(source, loc, lang);
            let stem = source.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
            let mut planned = Planned { source: source.clone(), dir, stem, k: 0, style, lang };
            loop {
                let target = planned.target();
                if !exists(&target) && claimed.insert(key(&target)) {
                    break;
                }
                planned.k += 1;
            }
            planned
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn none(_: &Path) -> bool {
        false
    }

    fn targets(planned: &[Planned]) -> Vec<PathBuf> {
        planned.iter().map(Planned::target).collect()
    }

    #[test]
    fn candidate_table() {
        use SuffixStyle::*;
        assert_eq!(candidate("s", Underscore, Lang::EnGb, 0), "s.jpg");
        assert_eq!(candidate("s", Underscore, Lang::EnGb, 1), "s_2.jpg");
        assert_eq!(candidate("s", Underscore, Lang::EnGb, 2), "s_3.jpg");
        assert_eq!(candidate("s", Optimised, Lang::EnGb, 0), "s.jpg");
        assert_eq!(candidate("s", Optimised, Lang::EnGb, 1), "s (optimised).jpg");
        assert_eq!(candidate("s", Optimised, Lang::EnGb, 2), "s (optimised 2).jpg");
        assert_eq!(candidate("s", Optimised, Lang::EnGb, 7), "s (optimised 7).jpg");
        assert_eq!(candidate("s", Optimised, Lang::EnUs, 1), "s (optimized).jpg");
        assert_eq!(candidate("s", Optimised, Lang::EnUs, 2), "s (optimized 2).jpg");
        assert_eq!(candidate("s", Optimised, Lang::Nl, 1), "s (geoptimaliseerd).jpg");
        assert_eq!(candidate("s", Optimised, Lang::Nl, 2), "s (geoptimaliseerd 2).jpg");
        assert_eq!(candidate("s", Underscore, Lang::Nl, 1), "s_2.jpg");
    }

    #[test]
    fn heic_gets_plain_jpg_name() {
        let p = plan_outputs(
            &[PathBuf::from(r"C:\p\IMG_0042.HEIC")],
            SuffixStyle::Underscore,
            OutputLocation::NextToOriginal,
            Lang::EnGb,
            &none,
        );
        assert_eq!(targets(&p), vec![PathBuf::from(r"C:\p\IMG_0042.jpg")]);
        assert_eq!(p[0].k, 0);
    }

    #[test]
    fn jpg_source_always_gets_suffix() {
        let src = PathBuf::from(r"C:\p\photo.jpg");
        let exists = |p: &Path| key(p) == key(Path::new(r"C:\p\photo.jpg"));
        let u =
            plan_outputs(&[src.clone()], SuffixStyle::Underscore, OutputLocation::NextToOriginal, Lang::EnGb, &exists);
        assert_eq!(targets(&u), vec![PathBuf::from(r"C:\p\photo_2.jpg")]);
        let o =
            plan_outputs(&[src.clone()], SuffixStyle::Optimised, OutputLocation::NextToOriginal, Lang::EnGb, &exists);
        assert_eq!(targets(&o), vec![PathBuf::from(r"C:\p\photo (optimised).jpg")]);
        // Even if `exists` misses it, the source is never its own target.
        let n = plan_outputs(
            &[PathBuf::from(r"C:\p\PHOTO.JPG")],
            SuffixStyle::Underscore,
            OutputLocation::NextToOriginal,
            Lang::EnGb,
            &none,
        );
        assert_eq!(targets(&n), vec![PathBuf::from(r"C:\p\PHOTO_2.jpg")]);
    }

    #[test]
    fn batch_clash_is_case_insensitive() {
        let p = plan_outputs(
            &[PathBuf::from(r"C:\p\IMG_1.HEIC"), PathBuf::from(r"C:\p\IMG_1.PNG")],
            SuffixStyle::Underscore,
            OutputLocation::NextToOriginal,
            Lang::EnGb,
            &none,
        );
        assert_eq!(targets(&p), vec![PathBuf::from(r"C:\p\IMG_1.jpg"), PathBuf::from(r"C:\p\IMG_1_2.jpg")]);
    }

    #[test]
    fn existing_outputs_are_skipped() {
        let exists = |p: &Path| {
            let k = key(p);
            k == key(Path::new(r"C:\p\a.jpg")) || k == key(Path::new(r"C:\p\a_2.jpg"))
        };
        let p = plan_outputs(
            &[PathBuf::from(r"C:\p\a.png")],
            SuffixStyle::Underscore,
            OutputLocation::NextToOriginal,
            Lang::EnGb,
            &exists,
        );
        assert_eq!(targets(&p), vec![PathBuf::from(r"C:\p\a_3.jpg")]);
        assert_eq!(p[0].k, 2);
    }

    #[test]
    fn subfolder_location() {
        let exists = |p: &Path| key(p) == key(Path::new(r"C:\p\photo.jpg"));
        let p = plan_outputs(
            &[PathBuf::from(r"C:\p\a.png"), PathBuf::from(r"C:\p\photo.jpg")],
            SuffixStyle::Underscore,
            OutputLocation::Subfolder,
            Lang::EnGb,
            &exists,
        );
        assert_eq!(
            targets(&p),
            vec![PathBuf::from(r"C:\p\Optimised\a.jpg"), PathBuf::from(r"C:\p\Optimised\photo.jpg")]
        );
        assert_eq!(p[0].dir, PathBuf::from(r"C:\p\Optimised"));

        let nl = plan_outputs(
            &[PathBuf::from(r"C:\p\a.png")],
            SuffixStyle::Optimised,
            OutputLocation::Subfolder,
            Lang::Nl,
            &none,
        );
        assert_eq!(targets(&nl), vec![PathBuf::from(r"C:\p\Geoptimaliseerd\a.jpg")]);
        let us = plan_outputs(
            &[PathBuf::from(r"C:\p\a.png")],
            SuffixStyle::Underscore,
            OutputLocation::Subfolder,
            Lang::EnUs,
            &none,
        );
        assert_eq!(targets(&us), vec![PathBuf::from(r"C:\p\Optimized\a.jpg")]);
    }

    #[test]
    fn plan_keeps_unicode_stem() {
        let p = plan_outputs(
            &[PathBuf::from(r"C:\p\Été à Gent.HEIC")],
            SuffixStyle::Underscore,
            OutputLocation::NextToOriginal,
            Lang::EnGb,
            &none,
        );
        assert_eq!(p[0].stem, "Été à Gent");
        assert_eq!(targets(&p), vec![PathBuf::from(r"C:\p\Été à Gent.jpg")]);
    }
}
