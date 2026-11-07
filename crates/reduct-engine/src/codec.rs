//! Detects whether HEIC can be decoded (spec §4.1): WIC needs both the HEIF
//! Image Extensions (a WIC decoder for the HEIF container) and an HEVC video
//! decoder (Media Foundation transform).

use windows::core::{IUnknown, Interface};
use windows::Win32::Foundation::S_OK;
use windows::Win32::Graphics::Imaging::{
    IWICBitmapCodecInfo, GUID_ContainerFormatHeif, WICComponentEnumerateDefault, WICDecoder,
};
use windows::Win32::Media::MediaFoundation::{
    IMFActivate, MFMediaType_Video, MFShutdown, MFStartup, MFTEnumEx, MFVideoFormat_HEVC,
    MFSTARTUP_LITE, MFT_CATEGORY_VIDEO_DECODER, MFT_ENUM_FLAG_ASYNCMFT, MFT_ENUM_FLAG_HARDWARE,
    MFT_ENUM_FLAG_SORTANDFILTER, MFT_ENUM_FLAG_SYNCMFT, MFT_REGISTER_TYPE_INFO, MF_VERSION,
};
use windows::Win32::System::Com::CoTaskMemFree;

use crate::com::{init_com_mta, wic_factory};

/// True when a WIC decoder for `GUID_ContainerFormatHeif` is installed and
/// Media Foundation has at least one HEVC video decoder.
///
/// Initialises COM (MTA) for the duration of the call if the thread has not
/// done so; a thread that is already an STA works as well.
pub fn heic_supported() -> bool {
    let _com = init_com_mta().ok();
    wic_heif_decoder_present() && hevc_decoder_present()
}

fn wic_heif_decoder_present() -> bool {
    let Ok(factory) = wic_factory() else {
        return false;
    };
    // SAFETY: plain COM calls; each enumerated element is an owned IUnknown.
    unsafe {
        let Ok(components) = factory
            .CreateComponentEnumerator(WICDecoder.0 as u32, WICComponentEnumerateDefault.0 as u32)
        else {
            return false;
        };
        loop {
            let mut item: [Option<IUnknown>; 1] = [None];
            let mut fetched = 0u32;
            if components.Next(&mut item, Some(&mut fetched as *mut u32)) != S_OK || fetched == 0 {
                return false;
            }
            let Some(unknown) = item[0].take() else {
                continue;
            };
            if let Ok(info) = unknown.cast::<IWICBitmapCodecInfo>() {
                if info.GetContainerFormat().is_ok_and(|g| g == GUID_ContainerFormatHeif) {
                    return true;
                }
            }
        }
    }
}

fn hevc_decoder_present() -> bool {
    let input = MFT_REGISTER_TYPE_INFO {
        guidMajorType: MFMediaType_Video,
        guidSubtype: MFVideoFormat_HEVC,
    };
    let flags = MFT_ENUM_FLAG_SYNCMFT
        | MFT_ENUM_FLAG_ASYNCMFT
        | MFT_ENUM_FLAG_HARDWARE
        | MFT_ENUM_FLAG_SORTANDFILTER;
    let mut activates: *mut Option<IMFActivate> = std::ptr::null_mut();
    let mut count = 0u32;
    // SAFETY: MFTEnumEx fills `activates` with `count` owned IMFActivate
    // pointers in a CoTaskMemAlloc'ed array; each is released by reading it out
    // and dropping it, then the array is freed.
    unsafe {
        let started = MFStartup(MF_VERSION, MFSTARTUP_LITE).is_ok();
        let result = MFTEnumEx(
            MFT_CATEGORY_VIDEO_DECODER,
            flags,
            Some(&input as *const _),
            None,
            &mut activates,
            &mut count,
        );
        if !activates.is_null() {
            for i in 0..count as usize {
                drop(std::ptr::read(activates.add(i)));
            }
            CoTaskMemFree(Some(activates as *const _));
        }
        if started {
            let _ = MFShutdown();
        }
        result.is_ok() && count > 0
    }
}
