//! Decodes the EXIF orientation fixtures through the real WIC pipeline.

use std::path::PathBuf;

use reduct_engine::{decode, init_com_mta, RgbImage};

const WHITE: [u8; 3] = [255, 255, 255];

fn fixture(n: u8) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/orientation")
        .join(format!("Landscape_{n}.jpg"))
}

fn load(n: u8, max_long_edge: Option<u32>) -> RgbImage {
    decode(&fixture(n), max_long_edge, WHITE)
        .unwrap_or_else(|e| panic!("decoding Landscape_{n}.jpg failed: {e}"))
}

fn mean_abs_diff(a: &RgbImage, b: &RgbImage) -> f64 {
    let total: u64 = a
        .data
        .iter()
        .zip(&b.data)
        .map(|(x, y)| u64::from(x.abs_diff(*y)))
        .sum();
    total as f64 / a.data.len() as f64
}

#[test]
fn orientation_fixtures_match_upright() {
    let _com = init_com_mta().expect("COM init");
    let upright = load(1, None);
    assert_eq!(
        upright.data.len(),
        (upright.width * upright.height * 3) as usize
    );
    for n in 2..=8 {
        let img = load(n, None);
        assert_eq!(
            (img.width, img.height),
            (upright.width, upright.height),
            "Landscape_{n} has a different size after orientation"
        );
        let diff = mean_abs_diff(&img, &upright);
        println!("Landscape_{n}: mean abs diff {diff:.3}");
        assert!(diff < 8.0, "Landscape_{n}: mean abs diff {diff:.3} >= 8.0");
    }
}

#[test]
fn max_long_edge_scales_down_only() {
    let _com = init_com_mta().expect("COM init");
    let original = load(1, None);

    let small = load(1, Some(600));
    assert_eq!(small.width.max(small.height), 600);
    assert_eq!(small.data.len(), (small.width * small.height * 3) as usize);

    let same = load(1, Some(100_000));
    assert_eq!((same.width, same.height), (original.width, original.height));
}

#[test]
fn heic_detection_runs() {
    // The result depends on the machine's Store codecs; this only checks that
    // both probes run to completion.
    let supported = reduct_engine::heic_supported();
    println!("heic_supported: {supported}");
}
