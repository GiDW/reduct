//! Per-file outcomes, the batch summary and the result copy.

use std::path::PathBuf;

use reduct_common::lang::Lang;

/// What happened to one file of a batch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Converted {
        source_bytes: u64,
        output_bytes: u64,
    },
    /// The source is a JPEG and the result was not smaller; nothing was written.
    SkippedLarger,
    Failed {
        source: PathBuf,
        reason: String,
        codec_missing: bool,
    },
}

/// Counts over a whole batch.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Summary {
    pub converted: u32,
    pub skipped: u32,
    pub failed: u32,
    /// Failures caused by a missing HEIC codec (also counted in `failed`).
    pub codec_missing: u32,
    /// Files left out because the batch was cancelled before they started.
    pub cancelled: u32,
    /// Source bytes minus output bytes over the converted files; negative
    /// when the outputs are bigger in total.
    pub bytes_saved: i64,
}

pub fn summarize(outcomes: &[Outcome]) -> Summary {
    let mut s = Summary::default();
    for outcome in outcomes {
        match outcome {
            Outcome::Converted { source_bytes, output_bytes } => {
                s.converted += 1;
                s.bytes_saved += *source_bytes as i64 - *output_bytes as i64;
            }
            Outcome::SkippedLarger => s.skipped += 1,
            Outcome::Failed { codec_missing, .. } => {
                s.failed += 1;
                if *codec_missing {
                    s.codec_missing += 1;
                }
            }
        }
    }
    s
}

/// The progress window's headline and detail line for a finished batch. A
/// saving under 1 KB counts as no saving.
pub fn result_text(s: &Summary, lang: Lang) -> (String, String) {
    let saved = (s.converted > 0 && s.bytes_saved >= 1000).then(|| format_size(s.bytes_saved as u64, lang));
    let title = lang.optimised(s.converted, saved.as_deref());

    let mut extras = Vec::new();
    if s.skipped > 0 {
        extras.push(lang.skipped(s.skipped));
    }
    if s.failed > 0 {
        extras.push(lang.failed(s.failed));
    }
    if s.codec_missing > 0 {
        extras.push(lang.text().heic_needs_hevc.to_owned());
    }
    if s.cancelled > 0 {
        extras.push(lang.cancelled(s.cancelled));
    }
    (title, extras.join(", "))
}

/// Formats a byte count in decimal units: whole `KB` under 1 MB, one decimal
/// from 1 MB to under 10 MB, whole `MB` from 10 MB (including values that
/// round up to 10.0 MB). The decimal separator is the language's (`4,2 MB`
/// in Dutch).
pub fn format_size(bytes: u64, lang: Lang) -> String {
    const MB: u64 = 1_000_000;
    if bytes < MB {
        format!("{} KB", (bytes + 500) / 1000)
    } else if bytes < 10 * MB {
        let tenths = (bytes + 50_000) / 100_000;
        if tenths >= 100 {
            format!("{} MB", tenths / 10)
        } else {
            format!("{}{}{} MB", tenths / 10, lang.decimal_separator(), tenths % 10)
        }
    } else {
        format!("{} MB", (bytes + MB / 2) / MB)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn summary(converted: u32, skipped: u32, failed: u32, codec_missing: u32, bytes_saved: i64) -> Summary {
        Summary { converted, skipped, failed, codec_missing, cancelled: 0, bytes_saved }
    }

    fn text(title: &str, body: &str) -> (String, String) {
        (title.to_owned(), body.to_owned())
    }

    #[test]
    fn result_text_cases() {
        // Something converted, with a saving.
        assert_eq!(
            result_text(&summary(42, 0, 0, 0, 61_000_000), Lang::EnGb),
            text("42 images optimised, saved 61 MB", "")
        );
        assert_eq!(
            result_text(&summary(42, 2, 3, 0, 61_000_000), Lang::EnGb),
            text("42 images optimised, saved 61 MB", "2 skipped (already smaller), 3 failed")
        );
        // Something converted, no saving.
        assert_eq!(result_text(&summary(3, 0, 0, 0, 0), Lang::EnGb), text("3 images optimised", ""));
        assert_eq!(
            result_text(&summary(3, 1, 0, 0, -5_000), Lang::EnGb),
            text("3 images optimised", "1 skipped (already smaller)")
        );
        // Nothing converted.
        assert_eq!(
            result_text(&summary(0, 2, 3, 3, 0), Lang::EnGb),
            text("No images optimised", "2 skipped (already smaller), 3 failed, HEIC needs Microsoft's HEVC extension")
        );
        // Singular.
        assert_eq!(
            result_text(&summary(1, 0, 0, 0, 4_200_000), Lang::EnGb),
            text("1 image optimised, saved 4.2 MB", "")
        );
        assert_eq!(result_text(&summary(1, 0, 1, 0, 0), Lang::EnGb), text("1 image optimised", "1 failed"));
    }

    #[test]
    fn cancelled_files_are_counted_last() {
        let s = Summary { cancelled: 6, ..summary(4, 0, 1, 0, 2_000_000) };
        assert_eq!(result_text(&s, Lang::EnGb), text("4 images optimised, saved 2.0 MB", "1 failed, 6 cancelled"));
    }

    #[test]
    fn savings_under_one_kb_count_as_none() {
        assert_eq!(result_text(&summary(2, 0, 0, 0, 999), Lang::EnGb), text("2 images optimised", ""));
        assert_eq!(result_text(&summary(2, 0, 0, 0, 1), Lang::EnGb), text("2 images optimised", ""));
        assert_eq!(result_text(&summary(2, 0, 0, 0, 1000), Lang::EnGb), text("2 images optimised, saved 1 KB", ""));
    }

    #[test]
    fn format_sizes() {
        assert_eq!(format_size(999_999, Lang::EnGb), "1000 KB");
        assert_eq!(format_size(4_200_000, Lang::EnGb), "4.2 MB");
        assert_eq!(format_size(61_000_000, Lang::EnGb), "61 MB");
        assert_eq!(format_size(1_000_000, Lang::EnGb), "1.0 MB");
        assert_eq!(format_size(10_000_000, Lang::EnGb), "10 MB");
        assert_eq!(format_size(12_345, Lang::EnGb), "12 KB");
        assert_eq!(format_size(9_949_999, Lang::EnGb), "9.9 MB");
        assert_eq!(format_size(9_950_000, Lang::EnGb), "10 MB");
        assert_eq!(format_size(4_200_000, Lang::Nl), "4,2 MB");
        assert_eq!(format_size(61_000_000, Lang::Nl), "61 MB");
    }

    #[test]
    fn us_result_text() {
        assert_eq!(
            result_text(&Summary { cancelled: 6, ..summary(4, 0, 1, 0, 2_000_000) }, Lang::EnUs),
            text("4 images optimized, saved 2.0 MB", "1 failed, 6 canceled")
        );
        assert_eq!(result_text(&summary(0, 0, 1, 0, 0), Lang::EnUs), text("No images optimized", "1 failed"));
    }

    #[test]
    fn dutch_result_text() {
        assert_eq!(
            result_text(&summary(13, 0, 0, 0, 38_000_000), Lang::Nl),
            text("13 afbeeldingen geoptimaliseerd, 38 MB bespaard", "")
        );
        assert_eq!(
            result_text(&Summary { cancelled: 6, ..summary(1, 2, 3, 1, 4_200_000) }, Lang::Nl),
            text(
                "1 afbeelding geoptimaliseerd, 4,2 MB bespaard",
                "2 overgeslagen (al kleiner), 3 mislukt, HEIC vereist de HEVC-extensie van Microsoft, 6 geannuleerd"
            )
        );
        assert_eq!(
            result_text(&summary(0, 1, 0, 0, 0), Lang::Nl),
            text("Geen afbeeldingen geoptimaliseerd", "1 overgeslagen (al kleiner)")
        );
    }

    #[test]
    fn summarize_counts_and_savings() {
        let outcomes = vec![
            Outcome::Converted { source_bytes: 1000, output_bytes: 400 },
            Outcome::Converted { source_bytes: 500, output_bytes: 700 },
            Outcome::SkippedLarger,
            Outcome::Failed { source: PathBuf::from("a.heic"), reason: "x".into(), codec_missing: true },
            Outcome::Failed { source: PathBuf::from("b.jpg"), reason: "y".into(), codec_missing: false },
        ];
        assert_eq!(summarize(&outcomes), summary(2, 1, 2, 1, 400));
    }
}
