//! Keeps panics from unwinding across the COM boundary.

use std::panic::{catch_unwind, AssertUnwindSafe};

use windows::Win32::Foundation::E_FAIL;
use windows_core::Result;

/// Runs `f`, turning a panic into `E_FAIL`. Every COM method body goes
/// through this: unwinding out of an `extern "system"` function aborts the
/// host process, which here is Explorer's surrogate.
pub fn catch<T>(f: impl FnOnce() -> Result<T>) -> Result<T> {
    catch_unwind(AssertUnwindSafe(f)).unwrap_or_else(|_| Err(E_FAIL.into()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::Foundation::E_NOTIMPL;

    #[test]
    fn passes_results_through() {
        assert_eq!(catch(|| Ok(7)).unwrap(), 7);
        assert_eq!(catch::<()>(|| Err(E_NOTIMPL.into())).unwrap_err().code(), E_NOTIMPL);
    }

    #[test]
    fn turns_a_panic_into_e_fail() {
        let r = catch::<()>(|| panic!("boom"));
        assert_eq!(r.unwrap_err().code(), E_FAIL);
    }
}
