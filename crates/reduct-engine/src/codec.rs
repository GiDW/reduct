//! Detects whether HEIC can be decoded (spec §4.1): WIC needs both the HEIF
//! Image Extensions (a WIC decoder for the HEIF container) and an HEVC video
//! decoder. Rather than looking for installed components (a GPU driver's HEVC
//! decoder can be registered without the HEVC Video Extensions that WIC's HEIF
//! decoder actually needs), the check decodes a tiny embedded HEIC.

use windows::Win32::Foundation::E_FAIL;
use windows::Win32::Graphics::Imaging::{
    GUID_WICPixelFormat32bppBGRA, IWICPalette, WICBitmapDitherTypeNone,
    WICBitmapPaletteTypeCustom, WICDecodeMetadataCacheOnDemand,
};

use crate::com::{init_com_mta, wic_factory};

/// A 64x64 solid-colour HEIC (HEVC Main Still Picture), 487 bytes. Made on
/// macOS from a PNG with `sips -s format heic`; see `assets/README.md`.
static PROBE_HEIC: &[u8] = include_bytes!("../assets/probe.heic");

/// The probe's size in pixels; a decoder reporting anything else failed.
const PROBE_SIZE: u32 = 64;

/// True when WIC can decode a HEIC: the embedded probe image opens, its first
/// frame decodes and its pixels can be copied out.
///
/// Initialises COM (MTA) for the duration of the call if the thread has not
/// done so; a thread that is already an STA works as well.
pub fn heic_supported() -> bool {
    let _com = init_com_mta().ok();
    probe_decode().is_ok()
}

fn probe_decode() -> windows::core::Result<()> {
    let factory = wic_factory()?;
    // SAFETY: plain COM calls. The stream reads from a 'static buffer, and
    // `pixels` holds exactly stride * height bytes.
    unsafe {
        let stream = factory.CreateStream()?;
        stream.InitializeFromMemory(PROBE_HEIC)?;
        let decoder =
            factory.CreateDecoderFromStream(&stream, std::ptr::null(), WICDecodeMetadataCacheOnDemand)?;
        let frame = decoder.GetFrame(0)?;
        let converter = factory.CreateFormatConverter()?;
        converter.Initialize(
            &frame,
            &GUID_WICPixelFormat32bppBGRA,
            WICBitmapDitherTypeNone,
            None::<&IWICPalette>,
            0.0,
            WICBitmapPaletteTypeCustom,
        )?;
        let (mut width, mut height) = (0u32, 0u32);
        converter.GetSize(&mut width, &mut height)?;
        if (width, height) != (PROBE_SIZE, PROBE_SIZE) {
            return Err(E_FAIL.into());
        }
        let stride = PROBE_SIZE * 4;
        let mut pixels = vec![0u8; (stride * PROBE_SIZE) as usize];
        converter.CopyPixels(std::ptr::null(), stride, &mut pixels)?;
    }
    Ok(())
}
