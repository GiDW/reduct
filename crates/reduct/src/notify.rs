//! The batch toast notifications: one at the start of a big batch, one at
//! the end. Both carry the same tag and group, so the end toast replaces the
//! start toast instead of stacking under it.

use std::path::Path;

use windows::core::HSTRING;
use windows::Data::Xml::Dom::XmlDocument;
use windows::Win32::System::Diagnostics::Debug::OutputDebugStringW;
use windows::UI::Notifications::{ToastNotification, ToastNotificationManager};

use crate::summary::{toast_text, Summary};

/// The Store page of Microsoft's HEVC Video Extensions.
pub(crate) const HEVC_STORE_URI: &str = "ms-windows-store://pdp/?ProductId=9NMZLZ57R3T7";

/// The toast group shared by every Reduct toast.
const GROUP: &str = "reduct";

/// Tells the user how the batch went. Ignores every error: an unpackaged
/// run (no package identity, as in the CI smoke test) shows no toast.
pub fn notify(s: &Summary, log: Option<&Path>, codec_missing: bool) {
    let (title, body) = toast_text(s);
    notify_text(&title, &body, log, codec_missing);
}

/// Tells the user a batch of `count` images has started. Ignores every error.
pub(crate) fn notify_start(count: usize) {
    show_or_log(&toast_xml(&start_toast_title(count), "", None, false));
}

/// Shows a toast with the given copy. Ignores every error.
pub(crate) fn notify_text(title: &str, body: &str, log: Option<&Path>, codec_missing: bool) {
    show_or_log(&toast_xml(title, body, log, codec_missing));
}

/// The start toast's title: `Optimising N images…`.
pub(crate) fn start_toast_title(count: usize) -> String {
    format!("Optimising {count} images\u{2026}")
}

/// The tag of this batch's toasts: the same for the start and the end toast.
fn batch_tag() -> String {
    format!("batch-{}", std::process::id())
}

/// Shows the toast; a failure only goes to the debugger output.
fn show_or_log(xml: &str) {
    if let Err(err) = show(xml) {
        let line = format!(
            "Reduct: toast failed: 0x{:08X} {}\n",
            err.code().0 as u32,
            err.message().trim_end()
        );
        // SAFETY: the HSTRING is NUL-terminated and outlives the call.
        unsafe { OutputDebugStringW(&HSTRING::from(line)) };
    }
}

fn show(xml: &str) -> windows::core::Result<()> {
    let doc = XmlDocument::new()?;
    doc.LoadXml(&HSTRING::from(xml))?;
    let toast = ToastNotification::CreateToastNotification(&doc)?;
    toast.SetTag(&HSTRING::from(batch_tag()))?;
    toast.SetGroup(&HSTRING::from(GROUP))?;
    ToastNotificationManager::CreateToastNotifier()?.Show(&toast)
}

/// The toast XML: a `ToastGeneric` binding with the title and (when not
/// empty) the body, plus an `Open log` action when there is a log and a
/// `Get HEIC support` action when the HEIC codec is missing.
pub fn toast_xml(title: &str, body: &str, log: Option<&Path>, codec_missing: bool) -> String {
    let mut xml = String::from(r#"<toast><visual><binding template="ToastGeneric">"#);
    xml.push_str(&format!("<text>{}</text>", escape(title)));
    if !body.is_empty() {
        xml.push_str(&format!("<text>{}</text>", escape(body)));
    }
    xml.push_str("</binding></visual>");

    let mut actions = Vec::new();
    if let Some(log) = log {
        actions.push(("Open log", file_uri(log)));
    }
    if codec_missing {
        actions.push(("Get HEIC support", HEVC_STORE_URI.to_owned()));
    }
    if !actions.is_empty() {
        xml.push_str("<actions>");
        for (content, uri) in actions {
            xml.push_str(&format!(
                r#"<action content="{}" activationType="protocol" arguments="{}"/>"#,
                escape(content),
                escape(&uri)
            ));
        }
        xml.push_str("</actions>");
    }
    xml.push_str("</toast>");
    xml
}

/// Escapes text for XML element content and attribute values.
fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            _ => out.push(c),
        }
    }
    out
}

/// A `file:` URI for a Windows path: forward slashes, and every byte other
/// than unreserved characters, `/` and `:` percent-encoded (UTF-8). UNC
/// paths become `file://server/share/...`.
fn file_uri(path: &Path) -> String {
    let raw = path.to_string_lossy().replace('\\', "/");
    let raw = raw.strip_prefix("//?/UNC/").map(|rest| format!("//{rest}")).unwrap_or(raw);
    let raw = raw.strip_prefix("//?/").map(str::to_owned).unwrap_or(raw);
    let (prefix, rest) = match raw.strip_prefix("//") {
        Some(unc) => ("file://", unc.to_owned()),
        None => ("file:///", raw.trim_start_matches('/').to_owned()),
    };
    let mut uri = String::from(prefix);
    for b in rest.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~' | b'/' | b':') {
            uri.push(b as char);
        } else {
            uri.push_str(&format!("%{b:02X}"));
        }
    }
    uri
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toast_xml_escapes_and_adds_actions() {
        let log = Path::new(r"C:\Users\me\AppData\Local\Reduct\logs\failures-1-2.txt");
        let xml = toast_xml("A & B <c>", "1 failed", Some(log), true);
        assert_eq!(
            xml,
            concat!(
                r#"<toast><visual><binding template="ToastGeneric">"#,
                "<text>A &amp; B &lt;c&gt;</text><text>1 failed</text>",
                "</binding></visual><actions>",
                r#"<action content="Open log" activationType="protocol" "#,
                r#"arguments="file:///C:/Users/me/AppData/Local/Reduct/logs/failures-1-2.txt"/>"#,
                r#"<action content="Get HEIC support" activationType="protocol" "#,
                r#"arguments="ms-windows-store://pdp/?ProductId=9NMZLZ57R3T7"/>"#,
                "</actions></toast>"
            )
        );

        let plain = toast_xml("Converted 3 images", "", None, false);
        assert_eq!(
            plain,
            r#"<toast><visual><binding template="ToastGeneric"><text>Converted 3 images</text></binding></visual></toast>"#
        );
        assert!(!plain.contains("<actions>"));
    }

    #[test]
    fn toast_xml_escapes_quotes_and_apostrophes() {
        let xml = toast_xml("No images converted", "Couldn't read the \"selection\"", None, false);
        assert!(xml.contains("<text>Couldn&apos;t read the &quot;selection&quot;</text>"), "{xml}");
    }

    #[test]
    fn start_toast_copy_and_xml() {
        assert_eq!(start_toast_title(500), "Optimising 500 images\u{2026}");
        assert_eq!(
            toast_xml(&start_toast_title(20), "", None, false),
            "<toast><visual><binding template=\"ToastGeneric\"><text>Optimising 20 images\u{2026}</text></binding></visual></toast>"
        );
        assert_eq!(batch_tag(), format!("batch-{}", std::process::id()));
    }

    #[test]
    fn toast_xml_heic_action_only() {
        let xml = toast_xml("No images converted", "1 failed", None, true);
        assert!(xml.contains("Get HEIC support"));
        assert!(!xml.contains("Open log"));
    }

    #[test]
    fn file_uri_encodes_spaces_and_non_ascii() {
        assert_eq!(
            file_uri(Path::new(r"C:\Users\Jürgen Ä\logs\f #1%?.txt")),
            "file:///C:/Users/J%C3%BCrgen%20%C3%84/logs/f%20%231%25%3F.txt"
        );
        assert_eq!(file_uri(Path::new(r"\\?\C:\x\y.txt")), "file:///C:/x/y.txt");
        assert_eq!(file_uri(Path::new(r"\\server\share\a b.txt")), "file://server/share/a%20b.txt");
    }
}
