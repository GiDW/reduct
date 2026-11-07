//! The settings dialog, shown when Reduct.exe starts without arguments (from
//! the Start menu). The dialog template lives in `resources/reduct.rc`; this
//! module fills it from `settings.ini` and writes it back on OK.

use std::cell::RefCell;
use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;

use reduct_common::naming::{OutputLocation, SuffixStyle};
use reduct_common::settings::{Background, Settings};
use windows::core::{w, BOOL, PCWSTR};
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, WPARAM};
use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Controls::{
    CheckDlgButton, InitCommonControlsEx, IsDlgButtonChecked, LoadIconMetric, BST_CHECKED, BST_UNCHECKED,
    ICC_STANDARD_CLASSES, ICC_UPDOWN_CLASS, INITCOMMONCONTROLSEX, LIM_LARGE, LIM_SMALL, UDM_SETPOS32, UDM_SETRANGE32,
};
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::{
    DialogBoxParamW, EndDialog, GetDlgItem, GetDlgItemInt, MessageBoxW, SendDlgItemMessageW, SendMessageW,
    SetDlgItemTextW, ShowWindow, BN_CLICKED, CBN_SELCHANGE, CB_ADDSTRING, CB_GETCURSEL, CB_SETCURSEL, ICON_BIG,
    ICON_SMALL, IDCANCEL, IDOK, MB_ICONERROR, MB_OK, SW_HIDE, SW_SHOWNORMAL, WM_CLOSE, WM_COMMAND, WM_INITDIALOG,
    WM_SETICON,
};

use crate::notify::HEVC_STORE_URI;
use crate::paths;

// Resource IDs: keep in step with resources/resource.h.
const IDI_APP: u16 = 1;
const IDD_SETTINGS: u16 = 101;
const IDC_QUALITY: i32 = 1002;
const IDC_QUALITY_SPIN: i32 = 1003;
const IDC_SAVE_TO: i32 = 1004;
const IDC_NAME_TAKEN: i32 = 1005;
const IDC_LIMIT_SIZE: i32 = 1006;
const IDC_BACKGROUND: i32 = 1007;
const IDC_KEEP_DATE: i32 = 1008;
const IDC_SKIP_LARGER: i32 = 1009;
const IDC_HEIC_STATUS: i32 = 1010;
const IDC_HEIC_STORE: i32 = 1011;
const IDC_LICENCES: i32 = 1012;

const ID_OK: i32 = IDOK.0;
const ID_CANCEL: i32 = IDCANCEL.0;

// Combo box entries; the index of each entry is what the `*_index` and
// `*_from_index` functions below map to and from.
const SAVE_TO: [&str; 2] = ["Next to the original", "\"Optimised\" subfolder"];
const NAME_TAKEN: [&str; 2] = ["Add _2, _3, \u{2026}", "Add (optimised)"];
const LIMIT_SIZE: [&str; 3] = ["Off", "2048 px long edge", "3840 px long edge"];
const BACKGROUND: [&str; 2] = ["White", "Black"];

const HEIC_INSTALLED: &str = "HEIC support: installed";
const HEIC_MISSING: &str = "HEIC support: missing. HEIC photos need Microsoft's HEVC Video Extensions.";
const NOTICES_FILE: &str = "THIRD-PARTY-NOTICES.txt";

/// What the dialog needs between messages: the settings it was opened with
/// (so values the dialog can't show survive) and whether the user picked a
/// `Limit size` entry.
struct State {
    original: Settings,
    limit_changed: bool,
}

thread_local! {
    static STATE: RefCell<Option<State>> = const { RefCell::new(None) };
}

/// Shows the settings dialog and returns the process exit code: `0` once it
/// is closed, `1` if it couldn't be created.
pub fn run_settings() -> i32 {
    // STA on the UI thread: ShellExecuteW may hand the request to COM-based
    // shell extensions. heic_supported() runs on its own thread (MTA).
    // SAFETY: plain initialisation calls; CoUninitialize is paired below.
    let com = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) };
    let icc = INITCOMMONCONTROLSEX {
        dwSize: std::mem::size_of::<INITCOMMONCONTROLSEX>() as u32,
        dwICC: ICC_STANDARD_CLASSES | ICC_UPDOWN_CLASS,
    };
    // SAFETY: `icc` is a valid, initialised struct.
    let _ = unsafe { InitCommonControlsEx(&icc) };

    STATE.with(|s| *s.borrow_mut() = Some(State { original: paths::load_settings(), limit_changed: false }));
    // SAFETY: the template is in this module's resources and dialog_proc has
    // the DLGPROC signature.
    let result = unsafe {
        DialogBoxParamW(module(), make_int_resource(IDD_SETTINGS), None, Some(dialog_proc), LPARAM(0))
    };
    STATE.with(|s| s.borrow_mut().take());

    if com.is_ok() {
        // SAFETY: balances the successful CoInitializeEx above.
        unsafe { CoUninitialize() };
    }
    if result <= 0 {
        1
    } else {
        0
    }
}

unsafe extern "system" fn dialog_proc(hwnd: HWND, msg: u32, wparam: WPARAM, _lparam: LPARAM) -> isize {
    match msg {
        WM_INITDIALOG => {
            init_dialog(hwnd);
            // TRUE: let the dialog manager set the focus to the first control.
            1
        }
        WM_COMMAND => {
            let id = (wparam.0 & 0xffff) as i32;
            let code = ((wparam.0 >> 16) & 0xffff) as u32;
            match (id, code) {
                // Enter and Esc arrive as IDOK/IDCANCEL too; take any code.
                (ID_OK, _) => on_ok(hwnd),
                (ID_CANCEL, _) => end(hwnd, ID_CANCEL),
                (IDC_LIMIT_SIZE, CBN_SELCHANGE) => {
                    STATE.with(|s| {
                        if let Some(state) = s.borrow_mut().as_mut() {
                            state.limit_changed = true;
                        }
                    });
                }
                (IDC_HEIC_STORE, BN_CLICKED) => open(hwnd, &wide(HEVC_STORE_URI)),
                (IDC_LICENCES, BN_CLICKED) => open_licences(hwnd),
                _ => return 0,
            }
            1
        }
        WM_CLOSE => {
            end(hwnd, ID_CANCEL);
            1
        }
        _ => 0,
    }
}

fn init_dialog(hwnd: HWND) {
    set_icons(hwnd);
    let settings = STATE.with(|s| s.borrow().as_ref().map(|st| st.original.clone())).unwrap_or_default();

    // SAFETY (whole block): messages to this dialog's own child controls,
    // with arguments as documented for each message. CB_ADDSTRING copies the
    // string before it returns.
    unsafe {
        SendDlgItemMessageW(hwnd, IDC_QUALITY_SPIN, UDM_SETRANGE32, WPARAM(1), LPARAM(100));
        SendDlgItemMessageW(hwnd, IDC_QUALITY_SPIN, UDM_SETPOS32, WPARAM(0), LPARAM(settings.quality as isize));
        for (id, entries, selected) in [
            (IDC_SAVE_TO, &SAVE_TO[..], output_index(settings.output)),
            (IDC_NAME_TAKEN, &NAME_TAKEN[..], suffix_index(settings.suffix)),
            (IDC_LIMIT_SIZE, &LIMIT_SIZE[..], limit_index(settings.max_long_edge)),
            (IDC_BACKGROUND, &BACKGROUND[..], background_index(settings.background)),
        ] {
            for entry in entries {
                let text = wide(entry);
                SendDlgItemMessageW(hwnd, id, CB_ADDSTRING, WPARAM(0), LPARAM(text.as_ptr() as isize));
            }
            SendDlgItemMessageW(hwnd, id, CB_SETCURSEL, WPARAM(selected), LPARAM(0));
        }
        let _ = CheckDlgButton(hwnd, IDC_KEEP_DATE, check_state(settings.keep_file_date));
        let _ = CheckDlgButton(hwnd, IDC_SKIP_LARGER, check_state(settings.skip_if_larger));
    }

    // heic_supported() initialises COM as MTA on the calling thread, so it
    // runs on a short-lived thread of its own and leaves this STA alone.
    let supported = std::thread::spawn(reduct_engine::heic_supported).join().unwrap_or(false);
    let status = wide(if supported { HEIC_INSTALLED } else { HEIC_MISSING });
    // SAFETY: own child controls; `status` outlives the call.
    unsafe {
        let _ = SetDlgItemTextW(hwnd, IDC_HEIC_STATUS, PCWSTR(status.as_ptr()));
        if supported {
            if let Ok(button) = GetDlgItem(Some(hwnd), IDC_HEIC_STORE) {
                let _ = ShowWindow(button, SW_HIDE);
            }
        }
    }
}

/// Title-bar and Alt+Tab icons at the sizes the current DPI needs.
fn set_icons(hwnd: HWND) {
    for (which, metric) in [(ICON_SMALL, LIM_SMALL), (ICON_BIG, LIM_LARGE)] {
        // SAFETY: IDI_APP is an icon resource of this module. The icon stays
        // alive for the process's lifetime, like the dialog.
        if let Ok(icon) = unsafe { LoadIconMetric(module(), make_int_resource(IDI_APP), metric) } {
            unsafe { SendMessageW(hwnd, WM_SETICON, Some(WPARAM(which as usize)), Some(LPARAM(icon.0 as isize))) };
        }
    }
}

fn on_ok(hwnd: HWND) {
    let Some((original, limit_changed)) =
        STATE.with(|s| s.borrow().as_ref().map(|st| (st.original.clone(), st.limit_changed)))
    else {
        end(hwnd, ID_CANCEL);
        return;
    };
    let mut translated = BOOL(0);
    // SAFETY: own child control; `translated` outlives the call.
    let quality = unsafe { GetDlgItemInt(hwnd, IDC_QUALITY, Some(&mut translated), false) };
    let form = Form {
        quality: translated.as_bool().then_some(quality),
        save_to: selection(hwnd, IDC_SAVE_TO),
        name_taken: selection(hwnd, IDC_NAME_TAKEN),
        limit: selection(hwnd, IDC_LIMIT_SIZE),
        limit_changed,
        background: selection(hwnd, IDC_BACKGROUND),
        keep_date: is_checked(hwnd, IDC_KEEP_DATE),
        skip_larger: is_checked(hwnd, IDC_SKIP_LARGER),
    };
    match paths::save_settings(&apply_form(&original, &form)) {
        Ok(()) => end(hwnd, ID_OK),
        // The dialog stays open so the user can try again or cancel.
        Err(e) => message_box(hwnd, &format!("Couldn't save the settings: {e}")),
    }
}

fn open_licences(hwnd: HWND) {
    let Some(path) = std::env::current_exe().ok().and_then(|exe| Some(exe.parent()?.join(NOTICES_FILE))) else {
        message_box(hwnd, &format!("Couldn't find {NOTICES_FILE}."));
        return;
    };
    open(hwnd, &wide_os(path.as_os_str()));
}

/// Opens a file or URI (NUL-terminated UTF-16) with its default handler.
fn open(hwnd: HWND, target: &[u16]) {
    // SAFETY: `target` is NUL-terminated and outlives the call.
    let result = unsafe {
        ShellExecuteW(Some(hwnd), w!("open"), PCWSTR(target.as_ptr()), PCWSTR::null(), PCWSTR::null(), SW_SHOWNORMAL)
    };
    // ShellExecuteW reports success with a value above 32.
    if result.0 as isize <= 32 {
        let shown = String::from_utf16_lossy(target.strip_suffix(&[0]).unwrap_or(target));
        message_box(hwnd, &format!("Couldn't open {shown}."));
    }
}

fn end(hwnd: HWND, result: i32) {
    // SAFETY: `hwnd` is the modal dialog this procedure belongs to.
    let _ = unsafe { EndDialog(hwnd, result as isize) };
}

fn message_box(hwnd: HWND, text: &str) {
    let text = wide(text);
    // SAFETY: both strings are NUL-terminated and outlive the call.
    unsafe { MessageBoxW(Some(hwnd), PCWSTR(text.as_ptr()), w!("Reduct"), MB_OK | MB_ICONERROR) };
}

fn selection(hwnd: HWND, id: i32) -> Option<usize> {
    // SAFETY: own child control. CB_ERR (-1) means nothing is selected.
    let index = unsafe { SendDlgItemMessageW(hwnd, id, CB_GETCURSEL, WPARAM(0), LPARAM(0)) }.0;
    usize::try_from(index).ok()
}

fn is_checked(hwnd: HWND, id: i32) -> bool {
    // SAFETY: own child control.
    unsafe { IsDlgButtonChecked(hwnd, id) == BST_CHECKED.0 }
}

fn check_state(checked: bool) -> windows::Win32::UI::Controls::DLG_BUTTON_CHECK_STATE {
    if checked {
        BST_CHECKED
    } else {
        BST_UNCHECKED
    }
}

fn module() -> Option<HINSTANCE> {
    // SAFETY: a NULL name returns the exe's own module handle.
    unsafe { GetModuleHandleW(None) }.ok().map(HINSTANCE::from)
}

/// `MAKEINTRESOURCEW`.
fn make_int_resource(id: u16) -> PCWSTR {
    PCWSTR(id as usize as *const u16)
}

/// NUL-terminated UTF-16.
fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

fn wide_os(s: &OsStr) -> Vec<u16> {
    s.encode_wide().chain(Some(0)).collect()
}

/// The dialog's values as read on OK. `None` is a field the dialog couldn't
/// read (an empty quality box, a combo box without a selection): it keeps the
/// stored value.
#[derive(Debug, Default)]
struct Form {
    quality: Option<u32>,
    save_to: Option<usize>,
    name_taken: Option<usize>,
    limit: Option<usize>,
    /// Whether the user picked a `Limit size` entry. A stored limit other
    /// than 2048 or 3840 shows as `Off` and is kept unless this is set.
    limit_changed: bool,
    background: Option<usize>,
    keep_date: bool,
    skip_larger: bool,
}

/// The settings to save: `original` with the dialog's values applied.
fn apply_form(original: &Settings, form: &Form) -> Settings {
    let mut s = original.clone();
    if let Some(q) = form.quality {
        s.quality = q.clamp(1, 100) as u8;
    }
    if let Some(output) = form.save_to.and_then(output_from_index) {
        s.output = output;
    }
    if let Some(suffix) = form.name_taken.and_then(suffix_from_index) {
        s.suffix = suffix;
    }
    if form.limit_changed {
        if let Some(limit) = form.limit.and_then(limit_from_index) {
            s.max_long_edge = limit;
        }
    }
    if let Some(background) = form.background.and_then(background_from_index) {
        s.background = background;
    }
    s.keep_file_date = form.keep_date;
    s.skip_if_larger = form.skip_larger;
    s
}

fn output_index(output: OutputLocation) -> usize {
    match output {
        OutputLocation::NextToOriginal => 0,
        OutputLocation::Subfolder => 1,
    }
}

fn output_from_index(index: usize) -> Option<OutputLocation> {
    match index {
        0 => Some(OutputLocation::NextToOriginal),
        1 => Some(OutputLocation::Subfolder),
        _ => None,
    }
}

fn suffix_index(suffix: SuffixStyle) -> usize {
    match suffix {
        SuffixStyle::Underscore => 0,
        SuffixStyle::Optimised => 1,
    }
}

fn suffix_from_index(index: usize) -> Option<SuffixStyle> {
    match index {
        0 => Some(SuffixStyle::Underscore),
        1 => Some(SuffixStyle::Optimised),
        _ => None,
    }
}

/// Any limit other than 2048 or 3840 shows as `Off`.
fn limit_index(limit: Option<u32>) -> usize {
    match limit {
        Some(2048) => 1,
        Some(3840) => 2,
        _ => 0,
    }
}

fn limit_from_index(index: usize) -> Option<Option<u32>> {
    match index {
        0 => Some(None),
        1 => Some(Some(2048)),
        2 => Some(Some(3840)),
        _ => None,
    }
}

fn background_index(background: Background) -> usize {
    match background {
        Background::White => 0,
        Background::Black => 1,
    }
}

fn background_from_index(index: usize) -> Option<Background> {
    match index {
        0 => Some(Background::White),
        1 => Some(Background::Black),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The form as the dialog shows `s`, with nothing changed.
    fn form_for(s: &Settings) -> Form {
        Form {
            quality: Some(s.quality as u32),
            save_to: Some(output_index(s.output)),
            name_taken: Some(suffix_index(s.suffix)),
            limit: Some(limit_index(s.max_long_edge)),
            limit_changed: false,
            background: Some(background_index(s.background)),
            keep_date: s.keep_file_date,
            skip_larger: s.skip_if_larger,
        }
    }

    #[test]
    fn indexes_round_trip_and_match_the_entries() {
        for o in [OutputLocation::NextToOriginal, OutputLocation::Subfolder] {
            assert_eq!(output_from_index(output_index(o)), Some(o));
            assert!(output_index(o) < SAVE_TO.len());
        }
        for x in [SuffixStyle::Underscore, SuffixStyle::Optimised] {
            assert_eq!(suffix_from_index(suffix_index(x)), Some(x));
            assert!(suffix_index(x) < NAME_TAKEN.len());
        }
        for l in [None, Some(2048), Some(3840)] {
            assert_eq!(limit_from_index(limit_index(l)), Some(l));
        }
        for b in [Background::White, Background::Black] {
            assert_eq!(background_from_index(background_index(b)), Some(b));
            assert!(background_index(b) < BACKGROUND.len());
        }
        assert_eq!(limit_from_index(LIMIT_SIZE.len()), None);
        assert_eq!(output_from_index(SAVE_TO.len()), None);
    }

    #[test]
    fn unchanged_form_keeps_the_settings() {
        let s = Settings {
            quality: 90,
            output: OutputLocation::Subfolder,
            suffix: SuffixStyle::Optimised,
            max_long_edge: Some(3840),
            background: Background::Black,
            keep_file_date: true,
            skip_if_larger: false,
        };
        assert_eq!(apply_form(&s, &form_for(&s)), s);
    }

    #[test]
    fn odd_limit_shows_off_and_is_kept_unless_changed() {
        let s = Settings { max_long_edge: Some(1600), ..Settings::default() };
        let mut form = form_for(&s);
        assert_eq!(form.limit, Some(0));
        assert_eq!(apply_form(&s, &form).max_long_edge, Some(1600));
        form.limit_changed = true;
        assert_eq!(apply_form(&s, &form).max_long_edge, None);
        form.limit = Some(1);
        assert_eq!(apply_form(&s, &form).max_long_edge, Some(2048));
    }

    #[test]
    fn quality_is_clamped_and_an_empty_box_keeps_it() {
        let s = Settings::default();
        let mut form = form_for(&s);
        form.quality = Some(0);
        assert_eq!(apply_form(&s, &form).quality, 1);
        form.quality = Some(250);
        assert_eq!(apply_form(&s, &form).quality, 100);
        form.quality = None;
        assert_eq!(apply_form(&s, &form).quality, s.quality);
    }

    #[test]
    fn form_values_are_applied() {
        let s = Settings::default();
        let form = Form {
            quality: Some(60),
            save_to: Some(1),
            name_taken: Some(1),
            limit: Some(2),
            limit_changed: true,
            background: Some(1),
            keep_date: true,
            skip_larger: false,
        };
        assert_eq!(
            apply_form(&s, &form),
            Settings {
                quality: 60,
                output: OutputLocation::Subfolder,
                suffix: SuffixStyle::Optimised,
                max_long_edge: Some(3840),
                background: Background::Black,
                keep_file_date: true,
                skip_if_larger: false,
            }
        );
    }
}
