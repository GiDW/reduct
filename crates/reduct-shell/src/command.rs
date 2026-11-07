//! The "Optimise with Reduct" Explorer command.

use std::ffi::{OsStr, OsString};
use std::io;
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};

use reduct_common::{formats, listfile};
use windows::Win32::Foundation::{CloseHandle, E_FAIL, E_NOTIMPL, HMODULE};
use windows::Win32::System::Com::{CoTaskMemFree, IBindCtx};
use windows::Win32::System::LibraryLoader::{
    GetModuleFileNameW, GetModuleHandleExW, GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS,
    GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
};
use windows::Win32::System::SystemInformation::GetTickCount64;
use windows::Win32::System::Threading::{
    CreateProcessW, PROCESS_CREATION_FLAGS, PROCESS_INFORMATION, STARTUPINFOW,
};
use windows::Win32::UI::Shell::{
    FOLDERID_LocalAppData, IEnumExplorerCommand, IExplorerCommand, IExplorerCommand_Impl,
    IShellItem, IShellItemArray, SHGetKnownFolderPath, SHStrDupW, ECF_DEFAULT, ECS_ENABLED,
    ECS_HIDDEN, KF_FLAG_DEFAULT, SIGDN_FILESYSPATH,
};
use windows_core::{implement, Error, Ref, Result, BOOL, GUID, HRESULT, PCWSTR, PWSTR};

use crate::{guard, CLSID_OPTIMISE};

/// Context-menu text (British spelling, see the global constraints).
const TITLE: &str = "Optimise with Reduct";
/// Icon file next to `ReductShell.dll` in the package.
const ICON_FILE: &str = "reduct-menu.ico";
/// The app execution alias, relative to `%LOCALAPPDATA%`.
const ALIAS_PATH: &str = r"Microsoft\WindowsApps\reduct.exe";
/// The exe next to `ReductShell.dll` in the package, used when the alias is off.
const PACKAGED_EXE: &str = "Reduct.exe";

#[implement(IExplorerCommand)]
pub struct Command;

impl IExplorerCommand_Impl for Command_Impl {
    fn GetTitle(&self, _items: Ref<IShellItemArray>) -> Result<PWSTR> {
        guard::catch(|| sh_str_dup(OsStr::new(TITLE)))
    }

    fn GetIcon(&self, _items: Ref<IShellItemArray>) -> Result<PWSTR> {
        guard::catch(|| {
            let dll = module_path()?;
            let dir = dll.parent().ok_or_else(|| Error::from(E_FAIL))?;
            sh_str_dup(dir.join(ICON_FILE).as_os_str())
        })
    }

    fn GetToolTip(&self, _items: Ref<IShellItemArray>) -> Result<PWSTR> {
        guard::catch(|| Err(E_NOTIMPL.into()))
    }

    fn GetCanonicalName(&self) -> Result<GUID> {
        guard::catch(|| Ok(CLSID_OPTIMISE))
    }

    fn GetState(&self, items: Ref<IShellItemArray>, _ok_to_be_slow: BOOL) -> Result<u32> {
        // Names only: nothing here opens a file or waits on anything.
        guard::catch(|| {
            let show = match items.as_ref() {
                Some(array) => should_show(array),
                None => false,
            };
            let state = if show { ECS_ENABLED } else { ECS_HIDDEN };
            Ok(state.0 as u32)
        })
    }

    fn Invoke(&self, items: Ref<IShellItemArray>, _bind: Ref<IBindCtx>) -> Result<()> {
        guard::catch(|| {
            let Some(array) = items.as_ref() else {
                return Ok(());
            };
            let paths: Vec<PathBuf> = file_system_paths(array)?
                .into_iter()
                .flatten()
                // write_list needs valid Unicode; skip the rare path that isn't.
                .filter(|p| p.to_str().is_some() && formats::is_supported(p))
                .collect();
            if paths.is_empty() {
                return Ok(());
            }
            launch(&paths)
        })
    }

    fn GetFlags(&self) -> Result<u32> {
        guard::catch(|| Ok(ECF_DEFAULT.0 as u32))
    }

    fn EnumSubCommands(&self) -> Result<IEnumExplorerCommand> {
        guard::catch(|| Err(E_NOTIMPL.into()))
    }
}

/// `ECS_ENABLED` test: every item has a supported file-system path.
fn should_show(array: &IShellItemArray) -> bool {
    match file_system_paths(array) {
        Ok(paths) => formats::should_show(paths),
        Err(_) => false,
    }
}

/// The `SIGDN_FILESYSPATH` name of every item; `None` where it has none.
fn file_system_paths(array: &IShellItemArray) -> Result<Vec<Option<PathBuf>>> {
    // SAFETY: plain COM calls on a live interface.
    let count = unsafe { array.GetCount()? };
    let mut paths = Vec::with_capacity(count as usize);
    for i in 0..count {
        // SAFETY: `i` is below the count the array reported.
        let item = unsafe { array.GetItemAt(i) };
        paths.push(item.ok().and_then(|item| file_system_path(&item)));
    }
    Ok(paths)
}

fn file_system_path(item: &IShellItem) -> Option<PathBuf> {
    // SAFETY: GetDisplayName returns a CoTaskMemAlloc'd string that we own.
    unsafe {
        let name = item.GetDisplayName(SIGDN_FILESYSPATH).ok()?;
        let path = PathBuf::from(OsString::from_wide(name.as_wide()));
        CoTaskMemFree(Some(name.0 as *const _));
        Some(path)
    }
}

/// Writes the list file and starts `reduct.exe --batch <list>` without
/// waiting for it. Tries the app execution alias first; when that fails (the
/// user can turn the alias off in Settings) it starts the `Reduct.exe` next
/// to this DLL in the package folder.
fn launch(paths: &[PathBuf]) -> Result<()> {
    let list = std::env::temp_dir().join(format!(
        "reduct-{}-{}.txt",
        std::process::id(),
        // SAFETY: no preconditions.
        unsafe { GetTickCount64() }
    ));
    listfile::write_list(&list, paths).map_err(io_error)?;

    let result = local_app_data()
        .and_then(|dir| start(&dir.join(ALIAS_PATH), &list))
        .or_else(|_| {
            let dll = module_path()?;
            let dir = dll.parent().ok_or_else(|| Error::from(E_FAIL))?;
            start(&dir.join(PACKAGED_EXE), &list)
        });
    if result.is_err() {
        // The exe deletes the list once it has read it; nobody will now.
        let _ = std::fs::remove_file(&list);
    }
    result
}

fn start(exe: &Path, list: &Path) -> Result<()> {
    let application = wide(exe.as_os_str());
    let mut command_line = wide(&command_line(exe.as_os_str(), list.as_os_str()));
    let startup = STARTUPINFOW {
        cb: size_of::<STARTUPINFOW>() as u32,
        ..Default::default()
    };
    let mut process = PROCESS_INFORMATION::default();
    // SAFETY: both strings are NUL-terminated and outlive the call; the
    // command line buffer is mutable as CreateProcessW requires.
    unsafe {
        CreateProcessW(
            PCWSTR(application.as_ptr()),
            Some(PWSTR(command_line.as_mut_ptr())),
            None,
            None,
            false,
            // Reduct.exe is a GUI-subsystem exe: no console, no flags needed.
            PROCESS_CREATION_FLAGS(0),
            None,
            PCWSTR::null(),
            &startup,
            &mut process,
        )?;
        let _ = CloseHandle(process.hThread);
        let _ = CloseHandle(process.hProcess);
    }
    Ok(())
}

/// `"<exe>" --batch "<list>"`. Windows paths can't contain `"`, and neither
/// path ends in a backslash, so plain quoting is enough.
fn command_line(exe: &OsStr, list: &OsStr) -> OsString {
    let mut line = OsString::from("\"");
    line.push(exe);
    line.push("\" --batch \"");
    line.push(list);
    line.push("\"");
    line
}

fn local_app_data() -> Result<PathBuf> {
    // SAFETY: the returned string is CoTaskMemAlloc'd and freed here.
    unsafe {
        let raw = SHGetKnownFolderPath(&FOLDERID_LocalAppData, KF_FLAG_DEFAULT, None)?;
        let path = PathBuf::from(OsString::from_wide(raw.as_wide()));
        CoTaskMemFree(Some(raw.0 as *const _));
        Ok(path)
    }
}

/// Full path of `ReductShell.dll`.
fn module_path() -> Result<PathBuf> {
    static ANCHOR: u8 = 0;
    let mut module = HMODULE::default();
    // SAFETY: FROM_ADDRESS takes any address inside the module; with
    // UNCHANGED_REFCOUNT there is no handle to release.
    unsafe {
        GetModuleHandleExW(
            GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
            PCWSTR(&ANCHOR as *const u8 as *const u16),
            &mut module,
        )?;
    }
    let mut buf = vec![0u16; 260];
    loop {
        // SAFETY: `buf` is a valid, writable buffer of the given length.
        let len = unsafe { GetModuleFileNameW(Some(module), &mut buf) } as usize;
        if len == 0 {
            return Err(Error::from_thread());
        }
        if len < buf.len() {
            return Ok(PathBuf::from(OsString::from_wide(&buf[..len])));
        }
        // Truncated: retry with more room, up to the longest Win32 path.
        if buf.len() >= 32_768 {
            return Err(E_FAIL.into());
        }
        buf.resize(buf.len() * 2, 0);
    }
}

/// Copies `s` into a CoTaskMemAlloc'd string the caller (the shell) frees.
fn sh_str_dup(s: &OsStr) -> Result<PWSTR> {
    let w = wide(s);
    // SAFETY: `w` is NUL-terminated and lives across the call.
    unsafe { SHStrDupW(PCWSTR(w.as_ptr())) }
}

fn wide(s: &OsStr) -> Vec<u16> {
    s.encode_wide().chain(Some(0)).collect()
}

fn io_error(e: io::Error) -> Error {
    match e.raw_os_error() {
        Some(code) => HRESULT::from_win32(code as u32).into(),
        None => E_FAIL.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_line_quotes_both_paths() {
        let line = command_line(
            OsStr::new(r"C:\Users\a b\AppData\Local\Microsoft\WindowsApps\reduct.exe"),
            OsStr::new(r"C:\Users\a b\AppData\Local\Temp\reduct-1-2.txt"),
        );
        assert_eq!(
            line,
            OsString::from(
                r#""C:\Users\a b\AppData\Local\Microsoft\WindowsApps\reduct.exe" --batch "C:\Users\a b\AppData\Local\Temp\reduct-1-2.txt""#
            )
        );
    }

    #[test]
    fn wide_is_nul_terminated() {
        assert_eq!(wide(OsStr::new("ab")), vec![b'a' as u16, b'b' as u16, 0]);
    }
}
