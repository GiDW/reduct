//! The HEIC support probe.

use reduct_engine::heic_supported;

/// The probe returns without panicking, with or without COM initialised by
/// the caller. GitHub's runners have no HEVC Video Extensions, so on CI it
/// must report HEIC as unsupported.
#[test]
fn heic_probe_returns_a_bool() {
    let first = heic_supported();
    let again = heic_supported();
    println!("heic_supported() = {first}");
    assert_eq!(first, again, "the probe gives the same answer twice");
    if std::env::var("CI").is_ok_and(|v| v == "true") {
        assert!(!first, "CI runners have no HEVC decoder, yet the probe decoded a HEIC");
    }
}
