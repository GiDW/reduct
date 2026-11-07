//! File Explorer context-menu handler (IExplorerCommand) for Reduct.
//!
//! The package manifest registers [`CLSID_OPTIMISE`] as a surrogate COM
//! server, so this DLL runs in `dllhost.exe` on behalf of Explorer. It only
//! looks at the names of the selected items and hands the selection to
//! `reduct.exe --batch <listfile>`.

// The library is named `ReductShell` so the output is `ReductShell.dll`, and
// the COM exports use their Win32 names.
#![allow(non_snake_case)]

mod command;
mod factory;
pub mod guard;

use core::ffi::c_void;

use windows::Win32::Foundation::{CLASS_E_CLASSNOTAVAILABLE, E_POINTER, S_FALSE, S_OK};
use windows::Win32::System::Com::IClassFactory;
use windows_core::{Interface, GUID, HRESULT};

/// CLSID of the "Optimise with Reduct" command, registered by the manifest.
pub const CLSID_OPTIMISE: GUID = GUID::from_u128(0x3AEBAC59_D582_488D_8254_15CBBF19C662);

/// Returns the class factory for [`CLSID_OPTIMISE`].
///
/// # Safety
///
/// Called by COM with valid `rclsid` and `riid` pointers and a writable `ppv`.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn DllGetClassObject(
    rclsid: *const GUID,
    riid: *const GUID,
    ppv: *mut *mut c_void,
) -> HRESULT {
    let result = guard::catch(|| {
        if ppv.is_null() || rclsid.is_null() || riid.is_null() {
            return Err(E_POINTER.into());
        }
        // SAFETY: checked for null above; COM passes valid pointers.
        unsafe {
            *ppv = core::ptr::null_mut();
            if *rclsid != CLSID_OPTIMISE {
                return Err(CLASS_E_CLASSNOTAVAILABLE.into());
            }
            let factory: IClassFactory = factory::Factory.into();
            factory.query(riid, ppv).ok()
        }
    });
    match result {
        Ok(()) => S_OK,
        Err(e) => e.code(),
    }
}

/// Always `S_FALSE`: the DLL stays loaded until the surrogate process ends.
#[unsafe(no_mangle)]
pub extern "system" fn DllCanUnloadNow() -> HRESULT {
    S_FALSE
}
