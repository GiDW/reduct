//! The WIC decoding pipeline (spec §4.3): decode frame 0, read the EXIF
//! orientation, convert to sRGB 32bpp BGRA, apply the orientation, flatten
//! alpha onto a background colour, then optionally scale down.

use std::os::windows::ffi::OsStrExt;
use std::path::Path;

use windows::core::{w, Interface, PCWSTR};
use windows::Win32::Foundation::{
    GENERIC_READ, WINCODEC_ERR_COMPONENTNOTFOUND, WINCODEC_ERR_UNSUPPORTEDPIXELFORMAT,
};
use windows::Win32::Graphics::Imaging::{
    GUID_WICPixelFormat24bppRGB, GUID_WICPixelFormat32bppBGRA, IWICBitmapFrameDecode, IWICBitmapSource, IWICColorContext,
    IWICImagingFactory, IWICPalette, WICBitmapDitherTypeNone,
    WICBitmapInterpolationModeHighQualityCubic, WICBitmapPaletteTypeCustom,
    WICBitmapTransformFlipHorizontal, WICBitmapTransformFlipVertical, WICBitmapTransformOptions,
    WICBitmapTransformRotate0, WICBitmapTransformRotate180, WICBitmapTransformRotate270,
    WICBitmapTransformRotate90, WICColorContextExifColorSpace, WICColorContextProfile,
    WICDecodeMetadataCacheOnDemand,
};
use windows::Win32::System::Com::StructuredStorage::{PropVariantClear, PROPVARIANT};
use windows::Win32::System::Variant::VT_UI2;

use crate::com::wic_factory;
use crate::error::{describe, EngineError};

/// A decoded image: packed 8-bit RGB, rows tightly packed (`width * 3` bytes).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RgbImage {
    pub width: u32,
    pub height: u32,
    pub data: Vec<u8>,
}

/// Decodes frame 0 of `path` into upright sRGB RGB pixels.
///
/// COM must be initialised on the calling thread (see [`crate::init_com_mta`]).
/// `max_long_edge` scales the image down (never up) so that its longer side is
/// at most that many pixels. Transparent pixels are flattened onto `background`.
pub fn decode(
    path: &Path,
    max_long_edge: Option<u32>,
    background: [u8; 3],
) -> Result<RgbImage, EngineError> {
    let factory = wic_factory().map_err(|e| EngineError::Decode(describe("WIC factory", &e)))?;
    let frame = open_frame(&factory, path)?;

    let orientation = read_orientation(&frame);
    let bgra = to_srgb_bgra(&factory, &frame)?;
    let oriented = apply_orientation(&factory, &bgra, orientation)?;
    let (width, height, pixels) = copy_pixels(&oriented, 4)?;
    // Flatten before scaling: scaling straight alpha would bleed the colour
    // of transparent pixels into the edges.
    let upright = RgbImage {
        width,
        height,
        data: flatten_bgra(&pixels, background),
    };
    drop(pixels);
    scale_down(&factory, upright, max_long_edge)
}

/// Flattens straight-alpha BGRA pixels onto `background` (RGB) and returns
/// packed RGB. Per channel: `out = (c * a + bg * (255 - a) + 127) / 255`.
pub fn flatten_bgra(bgra: &[u8], background: [u8; 3]) -> Vec<u8> {
    let mut rgb = Vec::with_capacity(bgra.len() / 4 * 3);
    for px in bgra.chunks_exact(4) {
        let a = u32::from(px[3]);
        let mix = |c: u8, bg: u8| ((u32::from(c) * a + u32::from(bg) * (255 - a) + 127) / 255) as u8;
        rgb.push(mix(px[2], background[0]));
        rgb.push(mix(px[1], background[1]));
        rgb.push(mix(px[0], background[2]));
    }
    rgb
}

/// Maps an EXIF orientation (1..=8) to the WIC transform that makes it upright.
///
/// WIC flips before it rotates, so the transposed orientations 5 and 7 pair
/// the flip with the opposite rotation to the spec table (verified on the
/// orientation fixtures in CI).
fn transform_for(orientation: u16) -> WICBitmapTransformOptions {
    let combine = |a: WICBitmapTransformOptions, b: WICBitmapTransformOptions| {
        WICBitmapTransformOptions(a.0 | b.0)
    };
    match orientation {
        2 => WICBitmapTransformFlipHorizontal,
        3 => WICBitmapTransformRotate180,
        4 => WICBitmapTransformFlipVertical,
        5 => combine(WICBitmapTransformRotate270, WICBitmapTransformFlipHorizontal),
        6 => WICBitmapTransformRotate90,
        7 => combine(WICBitmapTransformRotate90, WICBitmapTransformFlipHorizontal),
        8 => WICBitmapTransformRotate270,
        _ => WICBitmapTransformRotate0,
    }
}

/// Target size for `max_long_edge`: `None` when no scaling is needed.
/// Keeps the aspect ratio, rounds the short side and never scales up.
fn scaled_size(width: u32, height: u32, max_long_edge: Option<u32>) -> Option<(u32, u32)> {
    let max = max_long_edge?.max(1);
    let long = width.max(height);
    if max >= long {
        return None;
    }
    let short = width.min(height);
    let new_short = ((u64::from(short) * u64::from(max) + u64::from(long) / 2) / u64::from(long))
        .max(1) as u32;
    Some(if width >= height {
        (max, new_short)
    } else {
        (new_short, max)
    })
}

// --- Thin wrappers around the WIC calls. All `unsafe` lives below. ---

fn decode_err(context: &str) -> impl FnOnce(windows::core::Error) -> EngineError + '_ {
    move |e| EngineError::Decode(describe(context, &e))
}

fn open_frame(factory: &IWICImagingFactory, path: &Path) -> Result<IWICBitmapFrameDecode, EngineError> {
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    // SAFETY: `wide` is NUL-terminated and outlives the call.
    let decoder = unsafe {
        factory.CreateDecoderFromFilename(
            PCWSTR(wide.as_ptr()),
            None,
            GENERIC_READ,
            WICDecodeMetadataCacheOnDemand,
        )
    }
    .map_err(|e| {
        let code = e.code();
        if code == WINCODEC_ERR_COMPONENTNOTFOUND {
            EngineError::CodecMissing
        } else if (code.0 as u32) >> 16 == 0x8007 {
            // FACILITY_WIN32: file not found, access denied, ...
            EngineError::Io(std::io::Error::from_raw_os_error((code.0 as u32 & 0xFFFF) as i32))
        } else {
            EngineError::Decode(describe("open", &e))
        }
    })?;
    // SAFETY: frame 0 always exists for a successfully opened decoder or the call fails.
    unsafe { decoder.GetFrame(0) }.map_err(decode_err("frame 0"))
}

/// Reads `System.Photo.Orientation`; anything missing, unreadable or out of
/// range counts as 1 (upright).
fn read_orientation(frame: &IWICBitmapFrameDecode) -> u16 {
    // SAFETY: plain COM call on a valid frame.
    let Ok(reader) = (unsafe { frame.GetMetadataQueryReader() }) else {
        return 1;
    };
    let mut value = PROPVARIANT::default();
    // SAFETY: `value` is a valid, zero-initialised PROPVARIANT that we clear afterwards.
    let orientation = unsafe {
        let got = reader.GetMetadataByName(w!("System.Photo.Orientation"), &mut value);
        let inner = &value.Anonymous.Anonymous;
        let result = if got.is_ok() && inner.vt == VT_UI2 {
            inner.Anonymous.uiVal
        } else {
            1
        };
        let _ = PropVariantClear(&mut value);
        result
    };
    if (1..=8).contains(&orientation) {
        orientation
    } else {
        1
    }
}

/// Converts the frame to sRGB 32bpp BGRA. Uses the frame's colour profile
/// when it has one (failing if WIC cannot apply it); without one the pixels
/// are taken as sRGB and only the pixel format is converted.
fn to_srgb_bgra(
    factory: &IWICImagingFactory,
    frame: &IWICBitmapFrameDecode,
) -> Result<IWICBitmapSource, EngineError> {
    let source: IWICBitmapSource = match color_transform(factory, frame)? {
        Some(transformed) => transformed,
        None => frame.clone().into(),
    };
    // SAFETY: plain COM calls; the converter is a no-op if the source is already BGRA.
    unsafe {
        let converter = factory
            .CreateFormatConverter()
            .map_err(decode_err("format converter"))?;
        converter
            .Initialize(
                &source,
                &GUID_WICPixelFormat32bppBGRA,
                WICBitmapDitherTypeNone,
                None::<&IWICPalette>,
                0.0,
                WICBitmapPaletteTypeCustom,
            )
            .map_err(decode_err("format conversion"))?;
        Ok(converter.into())
    }
}

/// The frame transformed from its embedded colour space to sRGB 32bpp BGRA.
///
/// `Ok(None)` means there is nothing to transform: no colour context, or only
/// an EXIF colour space that is sRGB or unknown. An ICC profile (or EXIF
/// Adobe RGB) that WIC cannot apply is an error, not a silent sRGB fallback.
fn color_transform(
    factory: &IWICImagingFactory,
    frame: &IWICBitmapFrameDecode,
) -> Result<Option<IWICBitmapSource>, EngineError> {
    let Some(context) = source_color_context(factory, frame)? else {
        return Ok(None);
    };
    // SAFETY: plain COM calls on valid objects.
    unsafe {
        let srgb = factory
            .CreateColorContext()
            .map_err(decode_err("colour transform: sRGB context"))?;
        srgb.InitializeFromExifColorSpace(1)
            .map_err(decode_err("colour transform: sRGB context"))?;

        let transform = factory
            .CreateColorTransformer()
            .map_err(decode_err("colour transform"))?;
        let first = transform.Initialize(frame, &context, &srgb, &GUID_WICPixelFormat32bppBGRA);
        match first {
            Ok(()) => return Ok(Some(transform.into())),
            Err(e) if e.code() != WINCODEC_ERR_UNSUPPORTEDPIXELFORMAT => {
                return Err(EngineError::Decode(describe("colour transform", &e)));
            }
            Err(_) => {}
        }
        // The transformer does not accept the decoder's pixel format: convert
        // to 32bpp BGRA first (pixel values unchanged) and transform BGRA -> BGRA.
        let converter = factory
            .CreateFormatConverter()
            .map_err(decode_err("colour transform: format converter"))?;
        converter
            .Initialize(
                frame,
                &GUID_WICPixelFormat32bppBGRA,
                WICBitmapDitherTypeNone,
                None::<&IWICPalette>,
                0.0,
                WICBitmapPaletteTypeCustom,
            )
            .map_err(decode_err("colour transform: format conversion"))?;
        let transform = factory
            .CreateColorTransformer()
            .map_err(decode_err("colour transform"))?;
        transform
            .Initialize(&converter, &context, &srgb, &GUID_WICPixelFormat32bppBGRA)
            .map_err(decode_err("colour transform"))?;
        Ok(Some(transform.into()))
    }
}

/// The colour context to convert from: the ICC profile if the frame has one,
/// otherwise the first context if it is an EXIF colour space other than sRGB
/// (1) or uncalibrated. `None` when there is nothing to convert.
fn source_color_context(
    factory: &IWICImagingFactory,
    frame: &IWICBitmapFrameDecode,
) -> Result<Option<IWICColorContext>, EngineError> {
    // SAFETY: plain COM calls; the array passed to GetColorContexts holds
    // exactly `count` initialised contexts.
    unsafe {
        // The generated wrapper always passes a non-null array; WIC only
        // reports the count when the array is null, so call the vtable.
        let mut count = 0u32;
        let counted = (Interface::vtable(frame).GetColorContexts)(
            Interface::as_raw(frame),
            0,
            std::ptr::null_mut(),
            &mut count,
        )
        .ok();
        if counted.is_err() || count == 0 {
            // Formats without colour contexts may report "unsupported" here.
            return Ok(None);
        }
        let mut contexts: Vec<Option<IWICColorContext>> = Vec::with_capacity(count as usize);
        for _ in 0..count {
            contexts.push(Some(
                factory
                    .CreateColorContext()
                    .map_err(decode_err("colour contexts"))?,
            ));
        }
        let mut actual = 0u32;
        frame
            .GetColorContexts(&mut contexts, &mut actual)
            .map_err(decode_err("colour contexts"))?;
        let contexts: Vec<IWICColorContext> = contexts
            .into_iter()
            .take(actual as usize)
            .flatten()
            .collect();

        if let Some(profile) = contexts
            .iter()
            .find(|c| c.GetType().is_ok_and(|t| t == WICColorContextProfile))
        {
            return Ok(Some(profile.clone()));
        }
        let Some(first) = contexts.into_iter().next() else {
            return Ok(None);
        };
        let is_exif = first.GetType().is_ok_and(|t| t == WICColorContextExifColorSpace);
        // EXIF colour space: 1 = sRGB, 2 = Adobe RGB, 0xFFFF = uncalibrated.
        if is_exif && first.GetExifColorSpace().is_ok_and(|v| v == 2) {
            Ok(Some(first))
        } else {
            Ok(None)
        }
    }
}

fn apply_orientation(
    factory: &IWICImagingFactory,
    source: &IWICBitmapSource,
    orientation: u16,
) -> Result<IWICBitmapSource, EngineError> {
    let options = transform_for(orientation);
    if options == WICBitmapTransformRotate0 {
        return Ok(source.clone());
    }
    // SAFETY: plain COM calls.
    unsafe {
        let rotator = factory
            .CreateBitmapFlipRotator()
            .map_err(decode_err("flip rotator"))?;
        rotator
            .Initialize(source, options)
            .map_err(decode_err("orientation"))?;
        Ok(rotator.into())
    }
}

fn source_size(source: &IWICBitmapSource) -> Result<(u32, u32), EngineError> {
    let (mut width, mut height) = (0u32, 0u32);
    // SAFETY: both out pointers are valid for the call.
    unsafe { source.GetSize(&mut width, &mut height) }.map_err(decode_err("size"))?;
    Ok((width, height))
}

/// Scales the flattened RGB image down to `max_long_edge` with
/// HighQualityCubic; returns it unchanged when no scaling is needed.
fn scale_down(
    factory: &IWICImagingFactory,
    image: RgbImage,
    max_long_edge: Option<u32>,
) -> Result<RgbImage, EngineError> {
    let Some((new_width, new_height)) = scaled_size(image.width, image.height, max_long_edge) else {
        return Ok(image);
    };
    // SAFETY: plain COM calls; CreateBitmapFromMemory copies `image.data`,
    // which holds exactly width * 3 * height bytes.
    let scaler = unsafe {
        let bitmap = factory
            .CreateBitmapFromMemory(
                image.width,
                image.height,
                &GUID_WICPixelFormat24bppRGB,
                image.width * 3,
                &image.data,
            )
            .map_err(decode_err("scaling: bitmap"))?;
        let scaler = factory.CreateBitmapScaler().map_err(decode_err("scaler"))?;
        scaler
            .Initialize(
                &bitmap,
                new_width,
                new_height,
                WICBitmapInterpolationModeHighQualityCubic,
            )
            .map_err(decode_err("scaling"))?;
        scaler
    };
    drop(image);
    let scaled: IWICBitmapSource = scaler.into();
    let (width, height, data) = copy_pixels(&scaled, 3)?;
    Ok(RgbImage {
        width,
        height,
        data,
    })
}

/// Copies the source (`bytes_per_pixel` bytes per pixel) into a tightly
/// packed buffer.
fn copy_pixels(
    source: &IWICBitmapSource,
    bytes_per_pixel: u32,
) -> Result<(u32, u32, Vec<u8>), EngineError> {
    let (width, height) = source_size(source)?;
    let stride = width
        .checked_mul(bytes_per_pixel)
        .ok_or_else(|| EngineError::Decode("image too large".into()))?;
    let len = u32::try_from(u64::from(stride) * u64::from(height))
        .map_err(|_| EngineError::Decode("image too large".into()))?;
    let mut buffer = vec![0u8; len as usize];
    // SAFETY: a null rect means the whole image; `buffer` holds stride * height bytes.
    unsafe { source.CopyPixels(std::ptr::null(), stride, &mut buffer) }
        .map_err(decode_err("copy pixels"))?;
    Ok((width, height, buffer))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flatten_opaque_and_transparent() {
        let white = [255, 255, 255];
        // BGRA: opaque red, fully transparent green, half-transparent blue.
        let bgra = [0, 0, 255, 255, 0, 255, 0, 0, 255, 0, 0, 128];
        let rgb = flatten_bgra(&bgra, white);
        assert_eq!(&rgb[0..3], &[255, 0, 0], "opaque red stays red");
        assert_eq!(&rgb[3..6], &[255, 255, 255], "transparent gives the background");
        // blue: (255*128 + 255*127 + 127)/255 = 255; red/green: (0 + 255*127 + 127)/255 = 127
        assert_eq!(&rgb[6..9], &[127, 127, 255], "half-transparent gives the rounded mix");

        let black_bg = flatten_bgra(&[0, 0, 255, 128], [0, 0, 0]);
        // (255*128 + 127)/255 = 128
        assert_eq!(black_bg, vec![128, 0, 0]);
    }

    #[test]
    fn orientation_table() {
        assert_eq!(transform_for(1), WICBitmapTransformRotate0);
        assert_eq!(transform_for(0), WICBitmapTransformRotate0);
        assert_eq!(transform_for(9), WICBitmapTransformRotate0);
        assert_eq!(transform_for(6), WICBitmapTransformRotate90);
        assert_eq!(transform_for(5).0, 3 | 8);
        assert_eq!(transform_for(7).0, 1 | 8);
    }

    #[test]
    fn scaled_size_keeps_aspect_and_never_upscales() {
        assert_eq!(scaled_size(1800, 1200, Some(600)), Some((600, 400)));
        assert_eq!(scaled_size(1200, 1800, Some(600)), Some((400, 600)));
        assert_eq!(scaled_size(1800, 1200, Some(1800)), None);
        assert_eq!(scaled_size(1800, 1200, Some(100_000)), None);
        assert_eq!(scaled_size(1800, 1200, None), None);
        assert_eq!(scaled_size(1000, 3, Some(10)), Some((10, 1)));
    }
}
