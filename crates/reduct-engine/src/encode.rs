//! JPEG encoding with mozjpeg (spec §3.3), using mediamill's settings.

use std::panic::{catch_unwind, AssertUnwindSafe};

use mozjpeg::{ColorSpace, Compress};

use crate::decode::RgbImage;
use crate::error::{panic_message, EngineError};

/// Encodes `img` as a progressive JPEG at `quality` (1-100).
///
/// Settings: mozjpeg's default `JCP_MAX_COMPRESSION` profile (trellis
/// quantisation, base quant table 3), progressive mode with optimised scans,
/// optimised Huffman coding and 4:2:0 chroma subsampling. No EXIF, ICC or
/// other APPn markers are written beyond mozjpeg's JFIF header.
pub fn encode_jpeg(img: &RgbImage, quality: u8) -> Result<Vec<u8>, EngineError> {
    let (width, height) = (img.width as usize, img.height as usize);
    if width == 0 || height == 0 {
        return Err(EngineError::Encode(format!("empty image ({width}x{height})")));
    }
    let expected = width
        .checked_mul(height)
        .and_then(|n| n.checked_mul(3))
        .ok_or_else(|| EngineError::Encode(format!("image too large ({width}x{height})")))?;
    if img.data.len() != expected {
        return Err(EngineError::Encode(format!(
            "pixel buffer is {} bytes, expected {expected} for {width}x{height} RGB",
            img.data.len()
        )));
    }
    let quality = quality.clamp(1, 100);

    // The output usually ends up well under an eighth of the RGB size. Reserve
    // that much fallibly so a PC short of memory fails this file instead of
    // aborting the process; growth past it (and mozjpeg's own allocations,
    // which unwind on failure) is not covered.
    let mut output = Vec::new();
    output
        .try_reserve_exact(expected / 8)
        .map_err(|_| EngineError::Encode("not enough memory for this image".into()))?;

    // mozjpeg reports errors by unwinding; catch them here.
    let result = catch_unwind(AssertUnwindSafe(|| -> std::io::Result<Vec<u8>> {
        // `new` calls jpeg_set_defaults under the default JCP_MAX_COMPRESSION
        // profile, which selects base quant table 3 for jpeg_set_quality.
        let mut comp = Compress::new(ColorSpace::JCS_RGB);
        comp.set_size(width, height);
        comp.set_quality(f32::from(quality));
        comp.set_progressive_mode();
        comp.set_optimize_scans(true);
        comp.set_optimize_coding(true);
        comp.set_chroma_sampling_pixel_sizes((2, 2), (2, 2));

        let mut started = comp.start_compress(output)?;
        started.write_scanlines(&img.data)?;
        started.finish()
    }));

    match result {
        Ok(Ok(bytes)) => Ok(bytes),
        Ok(Err(err)) => Err(EngineError::Encode(err.to_string())),
        Err(payload) => Err(EngineError::Encode(panic_message(payload.as_ref()))),
    }
}
