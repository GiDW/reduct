//! COM initialisation and the per-thread WIC imaging factory.

use std::cell::RefCell;
use std::marker::PhantomData;

use windows::Win32::Graphics::Imaging::{CLSID_WICImagingFactory, IWICImagingFactory};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED,
};

thread_local! {
    static FACTORY: RefCell<Option<IWICImagingFactory>> = const { RefCell::new(None) };
}

/// Keeps COM initialised on the current thread. Dropping it releases the
/// thread's cached WIC factory and calls `CoUninitialize`.
///
/// Not `Send`: COM initialisation belongs to the thread that made it.
pub struct ComGuard {
    _not_send: PhantomData<*const ()>,
}

/// Initialises COM as a multithreaded apartment on the current thread.
///
/// Fails with `RPC_E_CHANGED_MODE` if the thread is already in an STA.
pub fn init_com_mta() -> windows::core::Result<ComGuard> {
    // SAFETY: plain COM initialisation; S_OK and S_FALSE both need a matching
    // CoUninitialize, which the guard's Drop provides.
    unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.ok()?;
    Ok(ComGuard {
        _not_send: PhantomData,
    })
}

impl Drop for ComGuard {
    fn drop(&mut self) {
        // Release the factory while COM is still initialised; a Release after
        // the last CoUninitialize could call into an unloaded DLL.
        let _ = FACTORY.try_with(|f| f.borrow_mut().take());
        // SAFETY: pairs with the successful CoInitializeEx in init_com_mta.
        unsafe { CoUninitialize() };
    }
}

/// Returns this thread's WIC imaging factory, creating it on first use.
/// COM must be initialised on the thread.
pub(crate) fn wic_factory() -> windows::core::Result<IWICImagingFactory> {
    FACTORY.with(|cell| {
        if let Some(factory) = cell.borrow().as_ref() {
            return Ok(factory.clone());
        }
        // SAFETY: creates an in-process COM object; the caller has COM initialised.
        let factory: IWICImagingFactory =
            unsafe { CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER) }?;
        *cell.borrow_mut() = Some(factory.clone());
        Ok(factory)
    })
}
