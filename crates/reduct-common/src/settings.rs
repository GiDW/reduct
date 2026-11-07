//! The settings model and its INI-like file format.
//!
//! The format is `key=value` lines with `#` comments (whole-line or trailing)
//! and no sections. Unknown keys are ignored and an invalid value falls back
//! to that key's default, so parsing never fails.

use crate::naming::{OutputLocation, SuffixStyle};

/// Colour that transparent pixels are flattened onto.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Background {
    White,
    Black,
}

impl Background {
    pub fn rgb(self) -> [u8; 3] {
        match self {
            Background::White => [255, 255, 255],
            Background::Black => [0, 0, 0],
        }
    }
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Settings {
    /// JPEG quality, `1..=100`.
    pub quality: u8,
    pub output: OutputLocation,
    pub suffix: SuffixStyle,
    /// Longest edge in pixels to downscale to; `None` keeps the original size.
    pub max_long_edge: Option<u32>,
    pub background: Background,
    pub keep_file_date: bool,
    pub skip_if_larger: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            quality: 75,
            output: OutputLocation::NextToOriginal,
            suffix: SuffixStyle::Underscore,
            max_long_edge: None,
            background: Background::White,
            keep_file_date: false,
            skip_if_larger: true,
        }
    }
}

/// Free-function form of [`Settings::parse`].
pub fn parse(text: &str) -> Settings {
    Settings::parse(text)
}

fn parse_bool(v: &str) -> Option<bool> {
    match v {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    }
}

impl Settings {
    /// Parses settings text. Never fails: unknown keys are ignored and an
    /// invalid value keeps that key's default.
    pub fn parse(text: &str) -> Settings {
        let mut s = Settings::default();
        for line in text.lines() {
            let line = line.split('#').next().unwrap_or("").trim();
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            let (key, value) = (key.trim(), value.trim());
            match key {
                "quality" => {
                    if let Some(q) = value.parse::<u8>().ok().filter(|q| (1..=100).contains(q)) {
                        s.quality = q;
                    }
                }
                "output" => match value {
                    "next-to-original" => s.output = OutputLocation::NextToOriginal,
                    "subfolder" => s.output = OutputLocation::Subfolder,
                    _ => {}
                },
                "suffix" => match value {
                    "underscore" => s.suffix = SuffixStyle::Underscore,
                    "optimised" => s.suffix = SuffixStyle::Optimised,
                    _ => {}
                },
                "max_long_edge" => {
                    if value == "off" {
                        s.max_long_edge = None;
                    } else if let Some(n) = value.parse::<u32>().ok().filter(|n| *n > 0) {
                        s.max_long_edge = Some(n);
                    }
                }
                "background" => match value {
                    "white" => s.background = Background::White,
                    "black" => s.background = Background::Black,
                    _ => {}
                },
                "keep_file_date" => {
                    if let Some(b) = parse_bool(value) {
                        s.keep_file_date = b;
                    }
                }
                "skip_if_larger" => {
                    if let Some(b) = parse_bool(value) {
                        s.skip_if_larger = b;
                    }
                }
                _ => {}
            }
        }
        s
    }

    /// Serialises to the file format, one `key=value` line per key, in the
    /// documented order, each ending in `\n`.
    pub fn serialize(&self) -> String {
        let output = match self.output {
            OutputLocation::NextToOriginal => "next-to-original",
            OutputLocation::Subfolder => "subfolder",
        };
        let suffix = match self.suffix {
            SuffixStyle::Underscore => "underscore",
            SuffixStyle::Optimised => "optimised",
        };
        let max_long_edge = match self.max_long_edge {
            None => "off".to_string(),
            Some(n) => n.to_string(),
        };
        let background = match self.background {
            Background::White => "white",
            Background::Black => "black",
        };
        format!(
            "quality={}\noutput={}\nsuffix={}\nmax_long_edge={}\nbackground={}\nkeep_file_date={}\nskip_if_larger={}\n",
            self.quality, output, suffix, max_long_edge, background, self.keep_file_date, self.skip_if_larger
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_round_trips() {
        let d = Settings::default();
        assert_eq!(Settings::parse(&d.serialize()), d);
    }

    #[test]
    fn non_default_round_trips() {
        let s = Settings {
            quality: 90,
            output: OutputLocation::Subfolder,
            suffix: SuffixStyle::Optimised,
            max_long_edge: Some(2048),
            background: Background::Black,
            keep_file_date: true,
            skip_if_larger: false,
        };
        assert_eq!(Settings::parse(&s.serialize()), s);
    }

    #[test]
    fn serialize_uses_documented_order() {
        assert_eq!(
            Settings::default().serialize(),
            "quality=75\noutput=next-to-original\nsuffix=underscore\nmax_long_edge=off\nbackground=white\nkeep_file_date=false\nskip_if_larger=true\n"
        );
    }

    #[test]
    fn invalid_values_fall_back_per_key() {
        let s = Settings::parse("quality=500\noutput=moon\nsuffix=optimised\nmax_long_edge=0\nflavour=mint\n");
        assert_eq!(s.quality, 75);
        assert_eq!(s.output, OutputLocation::NextToOriginal);
        assert_eq!(s.suffix, SuffixStyle::Optimised);
        assert_eq!(s.max_long_edge, None);
    }

    #[test]
    fn comments_crlf_and_spaces_are_tolerated() {
        let s = Settings::parse("# Reduct settings\r\nquality = 80   # 1..=100\r\nmax_long_edge=3840\r\n");
        assert_eq!(s.quality, 80);
        assert_eq!(s.max_long_edge, Some(3840));
    }

    #[test]
    fn empty_text_is_default() {
        assert_eq!(parse(""), Settings::default());
    }

    #[test]
    fn background_rgb() {
        assert_eq!(Background::White.rgb(), [255, 255, 255]);
        assert_eq!(Background::Black.rgb(), [0, 0, 0]);
    }
}
