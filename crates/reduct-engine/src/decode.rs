//! The WIC decoding pipeline (spec §4.3): decode frame 0, read the EXIF
//! orientation, convert to sRGB 32bpp BGRA, apply the orientation, optionally
//! scale down, then flatten alpha onto a background colour.

use std::os::windows::ffi::OsStrExt;
use std::path::Path;

use windows::core::{w, Interface, PCWSTR};
use windows::Win32::Foundation::{GENERIC_READ, WINCODEC_ERR_COMPONENTNOTFOUND};
use windows::Win32::Graphics::Imaging::{
    GUID_WICPixelFormat32bppBGRA, IWICBitmapFrameDecode, IWICBitmapSource, IWICColorContext,
    IWICImagingFactory, IWICPalette, WICBitmapDitherTypeNone,
    WICBitmapInterpolationModeHighQualityCubic, WICBitmapPaletteTypeCustom,
    WICBitmapTransformFlipHorizontal, WICBitmapTransformFlipVertical, WICBitmapTransformOptions,
    WICBitmapTransformRotate0, WICBitmapTransformRotate180, WICBitmapTransformRotate270,
    WICBitmapTransformRotate90, WICDecodeMetadataCacheOnDemand,
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
    let sized = scale_down(&factory, &oriented, max_long_edge)?;
    let (width, height, pixels) = copy_pixels(&sized)?;

    Ok(RgbImage {
        width,
        height,
        data: flatten_bgra(&pixels, background),
    })
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
fn transform_for(orientation: u16) -> WICBitmapTransformOptions {
    let combine = |a: WICBitmapTransformOptions, b: WICBitmapTransformOptions| {
        WICBitmapTransformOptions(a.0 | b.0)
    };
    match orientation {
        2 => WICBitmapTransformFlipHorizontal,
        3 => WICBitmapTransformRotate180,
        4 => WICBitmapTransformFlipVertical,
        5 => combine(WICBitmapTransformRotate90, WICBitmapTransformFlipHorizontal),
        6 => WICBitmapTransformRotate90,
        7 => combine(WICBitmapTransformRotate270, WICBitmapTransformFlipHorizontal),
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
/// when it has one; otherwise (or if the transform cannot be set up) assumes
/// the pixels are already sRGB and only converts the pixel format.
fn to_srgb_bgra(
    factory: &IWICImagingFactory,
    frame: &IWICBitmapFrameDecode,
) -> Result<IWICBitmapSource, EngineError> {
    let source: IWICBitmapSource = match color_transform(factory, frame) {
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

/// The first embedded colour context transformed to sRGB, or `None` if the
/// frame has none or WIC cannot transform it.
fn color_transform(
    factory: &IWICImagingFactory,
    frame: &IWICBitmapFrameDecode,
) -> Option<IWICBitmapSource> {
    // SAFETY: plain COM calls; the slice passed to GetColorContexts holds
    // exactly as many initialised contexts as its length says.
    unsafe {
        // The generated wrapper always passes a non-null array; WIC only
        // reports the count when the array is null, so call the vtable.
        let mut count = 0u32;
        (Interface::vtable(frame).GetColorContexts)(
            Interface::as_raw(frame),
            0,
            std::ptr::null_mut(),
            &mut count,
        )
        .ok()
        .ok()?;
        if count == 0 {
            return None;
        }
        let mut contexts: Vec<Option<IWICColorContext>> = Vec::with_capacity(count as usize);
        for _ in 0..count {
            contexts.push(Some(factory.CreateColorContext().ok()?));
        }
        let mut actual = 0u32;
        frame.GetColorContexts(&mut contexts, &mut actual).ok()?;
        let source_context = contexts.into_iter().next().flatten()?;

        let srgb = factory.CreateColorContext().ok()?;
        srgb.InitializeFromExifColorSpace(1).ok()?;

        let transform = factory.CreateColorTransformer().ok()?;
        transform
            .Initialize(frame, &source_context, &srgb, &GUID_WICPixelFormat32bppBGRA)
            .ok()?;
        Some(transform.into())
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

fn scale_down(
    factory: &IWICImagingFactory,
    source: &IWICBitmapSource,
    max_long_edge: Option<u32>,
) -> Result<IWICBitmapSource, EngineError> {
    let (width, height) = source_size(source)?;
    let Some((new_width, new_height)) = scaled_size(width, height, max_long_edge) else {
        return Ok(source.clone());
    };
    // SAFETY: plain COM calls.
    unsafe {
        let scaler = factory.CreateBitmapScaler().map_err(decode_err("scaler"))?;
        scaler
            .Initialize(
                source,
                new_width,
                new_height,
                WICBitmapInterpolationModeHighQualityCubic,
            )
            .map_err(decode_err("scaling"))?;
        Ok(scaler.into())
    }
}

/// Copies the 32bpp BGRA source into a tightly packed buffer.
fn copy_pixels(source: &IWICBitmapSource) -> Result<(u32, u32, Vec<u8>), EngineError> {
    let (width, height) = source_size(source)?;
    let stride = width
        .checked_mul(4)
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
        assert_eq!(transform_for(5).0, 1 | 8);
        assert_eq!(transform_for(7).0, 3 | 8);
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
