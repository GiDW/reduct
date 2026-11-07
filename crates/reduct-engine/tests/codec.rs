//! The HEIC support probe.

use std::fs;
use std::path::PathBuf;

use reduct_engine::{decode, heic_supported, init_com_mta};

/// The probe returns without panicking and agrees with the real decode
/// pipeline: when it reports HEIC as supported, its own probe image decodes
/// from a `.heic` file through `decode`; when it does not, that decode fails.
///
/// Which answer is right depends on the runner: in practice the x64 runner
/// has no HEVC decoder and the Windows 11 arm64 runner has one.
#[test]
fn heic_probe_agrees_with_decode() {
    let supported = heic_supported();
    assert_eq!(supported, heic_supported(), "the probe gives the same answer twice");

    let _com = init_com_mta().expect("COM init");
    assert_eq!(supported, heic_supported(), "same answer with COM already initialised");

    let probe = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets/probe.heic");
    let copy = std::env::temp_dir().join(format!("reduct-probe-{}.heic", std::process::id()));
    fs::copy(&probe, &copy).expect("copy the probe image");
    let decoded = decode(&copy, None, [255, 255, 255]);
    let _ = fs::remove_file(&copy);

    match decoded {
        Ok(img) => {
            assert!(supported, "decode read the probe HEIC, yet heic_supported() is false");
            assert_eq!((img.width, img.height), (64, 64));
            let [r, g, b] = [img.data[0], img.data[1], img.data[2]];
            assert!(
                r.abs_diff(200) <= 12 && g.abs_diff(100) <= 12 && b.abs_diff(50) <= 12,
                "probe pixel ({r}, {g}, {b}), expected about (200, 100, 50)"
            );
        }
        Err(err) => assert!(!supported, "heic_supported() is true, yet decode failed: {err}"),
    }
}
