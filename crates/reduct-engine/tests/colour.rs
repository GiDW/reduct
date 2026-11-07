//! Colour management and alpha handling through the real WIC pipeline.
//! Fixtures: fixtures/colour (see its README.md for how they were made).

use std::path::PathBuf;

use reduct_engine::{decode, init_com_mta, RgbImage};

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/colour")
        .join(name)
}

fn load(name: &str, max_long_edge: Option<u32>) -> RgbImage {
    decode(&fixture(name), max_long_edge, [255, 255, 255])
        .unwrap_or_else(|e| panic!("decoding {name} failed: {e}"))
}

fn mean_rgb(img: &RgbImage) -> [f64; 3] {
    let mut sum = [0u64; 3];
    for px in img.data.chunks_exact(3) {
        for c in 0..3 {
            sum[c] += u64::from(px[c]);
        }
    }
    let n = (img.data.len() / 3) as f64;
    [sum[0] as f64 / n, sum[1] as f64 / n, sum[2] as f64 / n]
}

/// Stored pixels are (100, 200, 100) tagged as Display P3. Converted to sRGB
/// (relative colorimetric, D65 both sides) that is about (44, 203, 86); left
/// untransformed it would stay (100, 200, 100).
fn assert_p3_converted(name: &str) {
    let img = load(name, None);
    assert_eq!((img.width, img.height), (8, 8));
    let [r, g, b] = mean_rgb(&img);
    println!("{name}: sRGB mean ({r:.1}, {g:.1}, {b:.1})");
    assert!((r - 44.0).abs() <= 12.0, "{name}: red {r:.1}, expected about 44");
    assert!((g - 203.0).abs() <= 10.0, "{name}: green {g:.1}, expected about 203");
    assert!((b - 86.0).abs() <= 12.0, "{name}: blue {b:.1}, expected about 86");
}

#[test]
fn p3_png_is_converted_to_srgb() {
    let _com = init_com_mta().expect("COM init");
    assert_p3_converted("p3-green-8x8.png");
}

#[test]
fn p3_jpeg_is_converted_to_srgb() {
    let _com = init_com_mta().expect("COM init");
    assert_p3_converted("p3-green-8x8.jpg");
}

/// Left half opaque white, right half fully transparent black, on white.
/// Flattened first, every pixel is white, so the scaled result must be white
/// too. Scaling straight alpha first would mix in the black (about 191).
#[test]
fn alpha_is_flattened_before_scaling() {
    let _com = init_com_mta().expect("COM init");
    let full = load("half-transparent-8x8.png", None);
    assert!(full.data.iter().all(|&v| v == 255), "unscaled flatten is white");

    let small = load("half-transparent-8x8.png", Some(2));
    assert_eq!((small.width, small.height), (2, 2));
    let min = *small.data.iter().min().unwrap();
    assert!(min >= 250, "scaled pixels darkened to {min}: alpha bled into the colour");
}
