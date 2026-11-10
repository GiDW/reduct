//! The UI language and every user-facing string, in US English, British
//! English and Dutch.
//!
//! Reduct follows the Windows display language: the first language in the
//! user's preferred UI languages that Reduct has (`en-*` or `nl-*`) wins, and
//! anything else gets US English, the package's default language.
//!
//! - US English (`Optimize`, `colors`, `Licenses`) for `en-US`, Canada, the
//!   Philippines and the US territories, and for a bare `en`.
//! - British English (`Optimise`, `colours`, `Licences`) for every other
//!   English region: the UK, Ireland, Australia, New Zealand, India, ...
//! - Dutch for `nl-*`: Belgian and Netherlands Dutch share one text. It
//!   follows Microsoft's Dutch style: informal `je`, `Verkenner` for File
//!   Explorer.
//!
//! The Store listing texts are in `store/` at the repository root.

use std::sync::OnceLock;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Lang {
    #[default]
    EnUs,
    EnGb,
    Nl,
}

impl Lang {
    /// The language for this process, detected once.
    pub fn current() -> Lang {
        static CURRENT: OnceLock<Lang> = OnceLock::new();
        *CURRENT.get_or_init(|| Lang::pick(preferred_ui_languages().as_slice()))
    }

    /// The first of `languages` (BCP 47 names such as `nl-BE`, most preferred
    /// first) that Reduct has; US English when none is.
    pub fn pick<S: AsRef<str>>(languages: &[S]) -> Lang {
        languages.iter().find_map(|tag| Lang::from_tag(tag.as_ref())).unwrap_or_default()
    }

    fn from_tag(tag: &str) -> Option<Lang> {
        let mut subtags = tag.split(['-', '_']);
        let primary = subtags.next().unwrap_or_default().to_ascii_lowercase();
        // The region is the first subtag after the language that is two
        // letters or three digits (`en-GB`, `en-Latn-GB`, `en-150`).
        let region = subtags
            .find(|s| s.len() == 2 || (s.len() == 3 && s.bytes().all(|b| b.is_ascii_digit())))
            .map(str::to_ascii_uppercase);
        match primary.as_str() {
            "en" => Some(match region.as_deref() {
                None | Some("US" | "CA" | "PH" | "PR" | "GU" | "AS" | "VI" | "MP" | "UM") => Lang::EnUs,
                Some(_) => Lang::EnGb,
            }),
            "nl" => Some(Lang::Nl),
            _ => None,
        }
    }

    /// The strings in this language.
    pub fn text(self) -> &'static Text {
        match self {
            Lang::EnUs => &EN_US,
            Lang::EnGb => &EN_GB,
            Lang::Nl => &NL,
        }
    }

    /// The running window's title: `Optimising N images`.
    pub fn progress_caption(self, total: usize) -> String {
        match (self, total) {
            (_, 0) => "Reduct".to_owned(),
            (Lang::EnUs, 1) => "Optimizing 1 image".to_owned(),
            (Lang::EnUs, n) => format!("Optimizing {n} images"),
            (Lang::EnGb, 1) => "Optimising 1 image".to_owned(),
            (Lang::EnGb, n) => format!("Optimising {n} images"),
            (Lang::Nl, 1) => "1 afbeelding optimaliseren".to_owned(),
            (Lang::Nl, n) => format!("{n} afbeeldingen optimaliseren"),
        }
    }

    /// The running window's count: `4 of 10 done`.
    pub fn progress_status(self, done: usize, total: usize) -> String {
        match (self, total) {
            (_, 0) => self.text().preparing.to_owned(),
            (Lang::EnUs | Lang::EnGb, total) => format!("{done} of {total} done"),
            (Lang::Nl, total) => format!("{done} van {total} klaar"),
        }
    }

    /// The result headline: `3 images optimised`, with `, saved 4.2 MB` when
    /// `saved` is given.
    pub fn optimised(self, count: u32, saved: Option<&str>) -> String {
        let base = match (self, count) {
            (Lang::EnUs, 0) => return "No images optimized".to_owned(),
            (Lang::EnGb, 0) => return "No images optimised".to_owned(),
            (Lang::Nl, 0) => return "Geen afbeeldingen geoptimaliseerd".to_owned(),
            (Lang::EnUs, 1) => "1 image optimized".to_owned(),
            (Lang::EnUs, n) => format!("{n} images optimized"),
            (Lang::EnGb, 1) => "1 image optimised".to_owned(),
            (Lang::EnGb, n) => format!("{n} images optimised"),
            (Lang::Nl, 1) => "1 afbeelding geoptimaliseerd".to_owned(),
            (Lang::Nl, n) => format!("{n} afbeeldingen geoptimaliseerd"),
        };
        match (self, saved) {
            (_, None) => base,
            (Lang::EnUs | Lang::EnGb, Some(size)) => format!("{base}, saved {size}"),
            (Lang::Nl, Some(size)) => format!("{base}, {size} bespaard"),
        }
    }

    /// `2 skipped (already smaller)`.
    pub fn skipped(self, n: u32) -> String {
        match self {
            Lang::EnUs | Lang::EnGb => format!("{n} skipped (already smaller)"),
            Lang::Nl => format!("{n} overgeslagen (al kleiner)"),
        }
    }

    /// `3 failed`.
    pub fn failed(self, n: u32) -> String {
        match self {
            Lang::EnUs | Lang::EnGb => format!("{n} failed"),
            Lang::Nl => format!("{n} mislukt"),
        }
    }

    /// `6 cancelled` (`canceled` in US English).
    pub fn cancelled(self, n: u32) -> String {
        match self {
            Lang::EnUs => format!("{n} canceled"),
            Lang::EnGb => format!("{n} cancelled"),
            Lang::Nl => format!("{n} geannuleerd"),
        }
    }

    /// `Couldn't save the settings: <error>`.
    pub fn save_failed(self, error: &str) -> String {
        match self {
            Lang::EnUs | Lang::EnGb => format!("Couldn't save the settings: {error}"),
            Lang::Nl => format!("Kan de instellingen niet opslaan: {error}"),
        }
    }

    /// `Couldn't write <path>: <error>`.
    pub fn write_failed(self, path: &str, error: &str) -> String {
        match self {
            Lang::EnUs | Lang::EnGb => format!("Couldn't write {path}: {error}"),
            Lang::Nl => format!("Kan {path} niet schrijven: {error}"),
        }
    }

    /// `Couldn't open <target>.`
    pub fn open_failed(self, target: &str) -> String {
        match self {
            Lang::EnUs | Lang::EnGb => format!("Couldn't open {target}."),
            Lang::Nl => format!("Kan {target} niet openen."),
        }
    }

    /// The decimal separator in sizes such as `4.2 MB`.
    pub fn decimal_separator(self) -> char {
        match self {
            Lang::EnUs | Lang::EnGb => '.',
            Lang::Nl => ',',
        }
    }
}

/// The fixed strings of one language.
#[derive(Debug)]
pub struct Text {
    /// The File Explorer context menu command.
    pub menu_title: &'static str,
    /// The output subfolder's name.
    pub subfolder: &'static str,
    /// The word in `photo (optimised).jpg` and `photo (optimised 2).jpg`.
    pub suffix_word: &'static str,

    // The settings dialog.
    pub settings_caption: &'static str,
    pub settings_hint: &'static str,
    pub quality: &'static str,
    pub save_to: &'static str,
    pub name_taken: &'static str,
    pub image_size: &'static str,
    pub transparent_areas: &'static str,
    pub keep_date: &'static str,
    pub licences: &'static str,
    pub ok: &'static str,
    pub cancel: &'static str,
    pub get_from_store: &'static str,
    pub heic_installed: &'static str,
    pub heic_missing: &'static str,
    /// `Save to` entries: next to the original, the subfolder.
    pub save_to_entries: [&'static str; 2],
    /// `When the name is taken` entries: `_2, _3, ...`, `(optimised)`.
    pub name_taken_entries: [&'static str; 2],
    /// `Image size` entries: original, 3840, 2048, 1280 and 800 px.
    pub image_size_entries: [&'static str; 5],
    /// The line below `Image size` for each entry.
    pub image_size_hints: [&'static str; 5],
    /// `Transparent areas` entries: white, black.
    pub background_entries: [&'static str; 2],

    // The progress window.
    pub preparing: &'static str,
    pub cancelling: &'static str,
    pub close: &'static str,
    pub show_details: &'static str,
    pub get_hevc: &'static str,
    pub heic_needs_hevc: &'static str,
    pub something_went_wrong: &'static str,
    pub selection_unreadable: &'static str,
}

pub static EN_US: Text = Text {
    menu_title: "Optimize with Reduct",
    subfolder: "Optimized",
    suffix_word: "optimized",

    settings_caption: "Reduct settings",
    settings_hint: "To optimize images, select them in File Explorer, right-click and choose Optimize with Reduct.",
    quality: "Quality",
    save_to: "Save to",
    name_taken: "When the name is taken",
    image_size: "Image size",
    transparent_areas: "Transparent areas",
    keep_date: "Keep the original file date",
    licences: "Licenses",
    ok: "OK",
    cancel: "Cancel",
    get_from_store: "Get from Microsoft Store",
    heic_installed: "HEIC support: installed",
    heic_missing: "HEIC support: missing. HEIC photos need Microsoft's HEVC Video Extensions.",
    save_to_entries: ["Next to the original", "\"Optimized\" subfolder"],
    name_taken_entries: ["Add _2, _3, \u{2026}", "Add (optimized)"],
    image_size_entries: [
        "Original size",
        "Large: for big screens and TVs (3840 px)",
        "Medium: for sharing online (2048 px)",
        "Small: for email and chat apps (1280 px)",
        "Very small: for attachments and forms (800 px)",
    ],
    image_size_hints: [
        "Images keep their size. Only the compression makes the files smaller.",
        "Sharp on big screens and TVs, with somewhat smaller files. Smaller images stay as they are.",
        "Fine on any screen, with much smaller files. Smaller images stay as they are.",
        "Good for email and chat apps, with small files. Smaller images stay as they are.",
        "For attachments and upload forms with a size limit. Smaller images stay as they are.",
    ],
    background_entries: ["White", "Black"],

    preparing: "Preparing\u{2026}",
    cancelling: "Canceling\u{2026}",
    close: "Close",
    show_details: "Show details",
    get_hevc: "Get HEVC extension",
    heic_needs_hevc: "HEIC needs Microsoft's HEVC extension",
    something_went_wrong: "Something went wrong",
    selection_unreadable: "Couldn't read the selection",
};

pub static EN_GB: Text = Text {
    menu_title: "Optimise with Reduct",
    subfolder: "Optimised",
    suffix_word: "optimised",

    settings_caption: "Reduct settings",
    settings_hint: "To optimise images, select them in File Explorer, right-click and choose Optimise with Reduct.",
    quality: "Quality",
    save_to: "Save to",
    name_taken: "When the name is taken",
    image_size: "Image size",
    transparent_areas: "Transparent areas",
    keep_date: "Keep the original file date",
    licences: "Licences",
    ok: "OK",
    cancel: "Cancel",
    get_from_store: "Get from Microsoft Store",
    heic_installed: "HEIC support: installed",
    heic_missing: "HEIC support: missing. HEIC photos need Microsoft's HEVC Video Extensions.",
    save_to_entries: ["Next to the original", "\"Optimised\" subfolder"],
    name_taken_entries: ["Add _2, _3, \u{2026}", "Add (optimised)"],
    image_size_entries: [
        "Original size",
        "Large: for big screens and TVs (3840 px)",
        "Medium: for sharing online (2048 px)",
        "Small: for email and chat apps (1280 px)",
        "Very small: for attachments and forms (800 px)",
    ],
    image_size_hints: [
        "Images keep their size. Only the compression makes the files smaller.",
        "Sharp on big screens and TVs, with somewhat smaller files. Smaller images stay as they are.",
        "Fine on any screen, with much smaller files. Smaller images stay as they are.",
        "Good for email and chat apps, with small files. Smaller images stay as they are.",
        "For attachments and upload forms with a size limit. Smaller images stay as they are.",
    ],
    background_entries: ["White", "Black"],

    preparing: "Preparing\u{2026}",
    cancelling: "Cancelling\u{2026}",
    close: "Close",
    show_details: "Show details",
    get_hevc: "Get HEVC extension",
    heic_needs_hevc: "HEIC needs Microsoft's HEVC extension",
    something_went_wrong: "Something went wrong",
    selection_unreadable: "Couldn't read the selection",
};

pub static NL: Text = Text {
    menu_title: "Optimaliseren met Reduct",
    subfolder: "Geoptimaliseerd",
    suffix_word: "geoptimaliseerd",

    settings_caption: "Reduct-instellingen",
    settings_hint: "Om afbeeldingen te optimaliseren, selecteer je ze in Verkenner, klik je met de rechtermuisknop en kies je Optimaliseren met Reduct.",
    quality: "Kwaliteit",
    save_to: "Opslaan in",
    name_taken: "Als de naam al bestaat",
    image_size: "Afbeeldingsgrootte",
    transparent_areas: "Transparante delen",
    keep_date: "Oorspronkelijke bestandsdatum behouden",
    licences: "Licenties",
    ok: "OK",
    cancel: "Annuleren",
    get_from_store: "Naar de Microsoft Store",
    heic_installed: "HEIC-ondersteuning: geïnstalleerd",
    heic_missing: "HEIC-ondersteuning: ontbreekt. Voor HEIC-foto's zijn de HEVC-video-extensies van Microsoft nodig.",
    save_to_entries: ["Naast het origineel", "Submap \"Geoptimaliseerd\""],
    name_taken_entries: ["_2, _3, \u{2026} toevoegen", "(geoptimaliseerd) toevoegen"],
    image_size_entries: [
        "Oorspronkelijke grootte",
        "Groot: grote schermen en tv's (3840 px)",
        "Middel: online delen (2048 px)",
        "Klein: e-mail en chat-apps (1280 px)",
        "Heel klein: bijlagen en formulieren (800 px)",
    ],
    image_size_hints: [
        "Afbeeldingen behouden hun grootte. Alleen de compressie maakt de bestanden kleiner.",
        "Scherp op grote schermen en tv's, met iets kleinere bestanden. Kleinere afbeeldingen blijven ongewijzigd.",
        "Prima op elk scherm, met veel kleinere bestanden. Kleinere afbeeldingen blijven ongewijzigd.",
        "Goed voor e-mail en chat-apps, met kleine bestanden. Kleinere afbeeldingen blijven ongewijzigd.",
        "Voor bijlagen en uploadformulieren met een limiet. Kleinere afbeeldingen blijven ongewijzigd.",
    ],
    background_entries: ["Wit", "Zwart"],

    preparing: "Voorbereiden\u{2026}",
    cancelling: "Annuleren\u{2026}",
    close: "Sluiten",
    show_details: "Details tonen",
    get_hevc: "HEVC-extensie ophalen",
    heic_needs_hevc: "HEIC vereist de HEVC-extensie van Microsoft",
    something_went_wrong: "Er is iets misgegaan",
    selection_unreadable: "Kan de selectie niet lezen",
};

/// The user's preferred UI languages, most preferred first.
#[cfg(windows)]
fn preferred_ui_languages() -> Vec<String> {
    // kernel32 is always linked on Windows; declaring the one call here keeps
    // reduct-common free of the windows crate.
    #[link(name = "kernel32")]
    extern "system" {
        fn GetUserPreferredUILanguages(flags: u32, count: *mut u32, buffer: *mut u16, size: *mut u32) -> i32;
    }
    const MUI_LANGUAGE_NAME: u32 = 0x8;

    let (mut count, mut size) = (0u32, 0u32);
    // SAFETY: a NULL buffer asks for the size, in UTF-16 units, NULs included.
    if unsafe { GetUserPreferredUILanguages(MUI_LANGUAGE_NAME, &mut count, std::ptr::null_mut(), &mut size) } == 0 {
        return Vec::new();
    }
    let mut buffer = vec![0u16; size as usize];
    // SAFETY: `buffer` holds `size` units, as the first call asked for.
    if unsafe { GetUserPreferredUILanguages(MUI_LANGUAGE_NAME, &mut count, buffer.as_mut_ptr(), &mut size) } == 0 {
        return Vec::new();
    }
    // A NUL-separated list, ended by an empty string.
    buffer.split(|&c| c == 0).filter(|s| !s.is_empty()).map(String::from_utf16_lossy).collect()
}

#[cfg(not(windows))]
fn preferred_ui_languages() -> Vec<String> {
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_the_first_language_reduct_has() {
        assert_eq!(Lang::pick(&["nl-BE", "en-US"]), Lang::Nl);
        assert_eq!(Lang::pick(&["nl-NL"]), Lang::Nl);
        assert_eq!(Lang::pick(&["NL"]), Lang::Nl);
        assert_eq!(Lang::pick(&["fr-BE", "nl-BE", "en-GB"]), Lang::Nl);
        assert_eq!(Lang::pick(&["en-GB", "nl-BE"]), Lang::EnGb);
        assert_eq!(Lang::pick(&["fr-FR", "de-DE"]), Lang::EnUs);
        assert_eq!(Lang::pick::<&str>(&[]), Lang::EnUs);
    }

    #[test]
    fn english_regions_get_their_spelling() {
        for tag in ["en-US", "en-us", "en", "en-CA", "en-PH", "en-PR", "en_US"] {
            assert_eq!(Lang::pick(&[tag]), Lang::EnUs, "{tag}");
        }
        for tag in ["en-GB", "en-IE", "en-AU", "en-NZ", "en-IN", "en-ZA", "en-150", "en-Latn-GB"] {
            assert_eq!(Lang::pick(&[tag]), Lang::EnGb, "{tag}");
        }
    }

    #[test]
    fn formatted_copy() {
        assert_eq!(Lang::EnGb.progress_caption(0), "Reduct");
        assert_eq!(Lang::Nl.progress_caption(1), "1 afbeelding optimaliseren");
        assert_eq!(Lang::Nl.progress_caption(13), "13 afbeeldingen optimaliseren");
        assert_eq!(Lang::Nl.progress_status(0, 0), "Voorbereiden\u{2026}");
        assert_eq!(Lang::Nl.progress_status(10, 13), "10 van 13 klaar");
        assert_eq!(Lang::Nl.optimised(0, Some("1 MB")), "Geen afbeeldingen geoptimaliseerd");
        assert_eq!(Lang::Nl.optimised(1, None), "1 afbeelding geoptimaliseerd");
        assert_eq!(Lang::Nl.optimised(13, Some("38 MB")), "13 afbeeldingen geoptimaliseerd, 38 MB bespaard");
        assert_eq!(Lang::EnGb.optimised(13, Some("38 MB")), "13 images optimised, saved 38 MB");
        assert_eq!(Lang::EnUs.optimised(13, Some("38 MB")), "13 images optimized, saved 38 MB");
        assert_eq!(Lang::EnUs.optimised(0, None), "No images optimized");
        assert_eq!(Lang::EnUs.progress_caption(2), "Optimizing 2 images");
        assert_eq!(Lang::EnUs.cancelled(6), "6 canceled");
        assert_eq!(Lang::EnGb.cancelled(6), "6 cancelled");
        assert_eq!(EN_US.menu_title, "Optimize with Reduct");
    }
}
