//! Class factory for the Reduct command.

use core::ffi::c_void;

use windows::Win32::Foundation::{CLASS_E_NOAGGREGATION, E_POINTER};
use windows::Win32::System::Com::{IClassFactory, IClassFactory_Impl};
use windows::Win32::UI::Shell::IExplorerCommand;
use windows_core::{implement, Interface, Ref, Result, BOOL, GUID, IUnknown};

use crate::command::Command;
use crate::guard;

#[implement(IClassFactory)]
pub struct Factory;

impl IClassFactory_Impl for Factory_Impl {
    fn CreateInstance(
        &self,
        punkouter: Ref<IUnknown>,
        riid: *const GUID,
        ppvobject: *mut *mut c_void,
    ) -> Result<()> {
        guard::catch(|| {
            if ppvobject.is_null() || riid.is_null() {
                return Err(E_POINTER.into());
            }
            // SAFETY: checked for null above.
            unsafe { *ppvobject = core::ptr::null_mut() };
            if !punkouter.is_null() {
                return Err(CLASS_E_NOAGGREGATION.into());
            }
            let command: IExplorerCommand = Command.into();
            // SAFETY: valid `riid` and `ppvobject`, checked above.
            unsafe { command.query(riid, ppvobject).ok() }
        })
    }

    fn LockServer(&self, _flock: BOOL) -> Result<()> {
        // DllCanUnloadNow always says no, so there's nothing to count.
        guard::catch(|| Ok(()))
    }
}
