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

use crate::paths;

// Resource IDs: keep in step with resources/resource.h.
const IDI_APP: u16 = 1;
const IDD_SETTINGS: u16 = 101;
const IDC_QUALITY: i32 = 1002;
const IDC_QUALITY_SPIN: i32 = 1003;
const IDC_SAVE_TO: i32 = 1004;
const IDC_NAME_TAKEN: i32 = 1005;
const IDC_IMAGE_SIZE: i32 = 1006;
const IDC_BACKGROUND: i32 = 1007;
const IDC_KEEP_DATE: i32 = 1008;
const IDC_HEIC_STATUS: i32 = 1010;
const IDC_HEIC_STORE: i32 = 1011;
const IDC_LICENCES: i32 = 1012;
const IDC_SIZE_HINT: i32 = 1013;

const ID_OK: i32 = IDOK.0;
const ID_CANCEL: i32 = IDCANCEL.0;

// Combo box entries; the index of each entry is what the `*_index` and
// `*_from_index` functions below map to and from.
const SAVE_TO: [&str; 2] = ["Next to the original", "\"Optimised\" subfolder"];
const NAME_TAKEN: [&str; 2] = ["Add _2, _3, \u{2026}", "Add (optimised)"];
/// The `Image size` entries, their long edge (`None`: original size) and the
/// line shown below the list for each.
const IMAGE_SIZE: [&str; 5] = [
    "Original size",
    "Large: for big screens and TVs (3840 px)",
    "Medium: for sharing online (2048 px)",
    "Small: for email and chat apps (1280 px)",
    "Very small: for attachments and forms (800 px)",
];
const IMAGE_SIZE_EDGES: [Option<u32>; 5] = [None, Some(3840), Some(2048), Some(1280), Some(800)];
const IMAGE_SIZE_HINTS: [&str; 5] = [
    "Images keep their size. Only the compression makes the files smaller.",
    "Sharp on big screens and TVs, with somewhat smaller files. Smaller images stay as they are.",
    "Fine on any screen, with much smaller files. Smaller images stay as they are.",
    "Good for email and chat apps, with small files. Smaller images stay as they are.",
    "For attachments and upload forms with a size limit. Smaller images stay as they are.",
];
const BACKGROUND: [&str; 2] = ["White", "Black"];

const HEIC_INSTALLED: &str = "HEIC support: installed";
const HEIC_MISSING: &str = "HEIC support: missing. HEIC photos need Microsoft's HEVC Video Extensions.";
/// The Store page of Microsoft's HEVC Video Extensions.
pub(crate) const HEVC_STORE_URI: &str = "ms-windows-store://pdp/?ProductId=9NMZLZ57R3T7";
const NOTICES_FILE: &str = "THIRD-PARTY-NOTICES.txt";
/// A copy for when the file isn't next to the exe (an unpackaged build).
const NOTICES_TEXT: &str = include_str!("../../../THIRD-PARTY-NOTICES.txt");

/// What the dialog needs between messages: the settings it was opened with
/// (so values the dialog can't show survive) and whether the user picked a
/// `Image size` entry.
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
    let result =
        unsafe { DialogBoxParamW(module(), make_int_resource(IDD_SETTINGS), None, Some(dialog_proc), LPARAM(0)) };
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
                (IDC_IMAGE_SIZE, CBN_SELCHANGE) => {
                    STATE.with(|s| {
                        if let Some(state) = s.borrow_mut().as_mut() {
                            state.limit_changed = true;
                        }
                    });
                    update_size_hint(hwnd);
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
            (IDC_IMAGE_SIZE, &IMAGE_SIZE[..], limit_index(settings.max_long_edge)),
            (IDC_BACKGROUND, &BACKGROUND[..], background_index(settings.background)),
        ] {
            for entry in entries {
                let text = wide(entry);
                SendDlgItemMessageW(hwnd, id, CB_ADDSTRING, WPARAM(0), LPARAM(text.as_ptr() as isize));
            }
            SendDlgItemMessageW(hwnd, id, CB_SETCURSEL, WPARAM(selected), LPARAM(0));
        }
        let _ = CheckDlgButton(hwnd, IDC_KEEP_DATE, check_state(settings.keep_file_date));
    }
    update_size_hint(hwnd);

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

/// Shows the line for the selected `Image size` entry below the list.
fn update_size_hint(hwnd: HWND) {
    let hint = selection(hwnd, IDC_IMAGE_SIZE).and_then(|i| IMAGE_SIZE_HINTS.get(i)).copied().unwrap_or("");
    let hint = wide(hint);
    // SAFETY: own child control; `hint` outlives the call.
    let _ = unsafe { SetDlgItemTextW(hwnd, IDC_SIZE_HINT, PCWSTR(hint.as_ptr())) };
}

/// Title-bar and Alt+Tab icons at the sizes the current DPI needs.
pub(crate) fn set_icons(hwnd: HWND) {
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
        limit: selection(hwnd, IDC_IMAGE_SIZE),
        limit_changed,
        background: selection(hwnd, IDC_BACKGROUND),
        keep_date: is_checked(hwnd, IDC_KEEP_DATE),
    };
    match paths::save_settings(&apply_form(&original, &form)) {
        Ok(()) => end(hwnd, ID_OK),
        // The dialog stays open so the user can try again or cancel.
        Err(e) => message_box(hwnd, &format!("Couldn't save the settings: {e}")),
    }
}

/// Opens the notices next to the exe (as installed from the package). Without
/// them there, as with a bare CI build, writes the embedded copy to the data
/// folder and opens that.
fn open_licences(hwnd: HWND) {
    let beside_exe = std::env::current_exe().ok().and_then(|exe| Some(exe.parent()?.join(NOTICES_FILE)));
    let path = match beside_exe.filter(|p| p.is_file()) {
        Some(path) => path,
        None => {
            let path = paths::data_dir().join(NOTICES_FILE);
            if let Err(e) = std::fs::write(&path, NOTICES_TEXT) {
                message_box(hwnd, &format!("Couldn't write {}: {e}", path.display()));
                return;
            }
            path
        }
    };
    open(hwnd, &wide_os(path.as_os_str()));
}

/// Opens a file or URI (NUL-terminated UTF-16) with its default handler.
pub(crate) fn open(hwnd: HWND, target: &[u16]) {
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

pub(crate) fn module() -> Option<HINSTANCE> {
    // SAFETY: a NULL name returns the exe's own module handle.
    unsafe { GetModuleHandleW(None) }.ok().map(HINSTANCE::from)
}

/// `MAKEINTRESOURCEW`.
pub(crate) fn make_int_resource(id: u16) -> PCWSTR {
    PCWSTR(id as usize as *const u16)
}

/// NUL-terminated UTF-16.
pub(crate) fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

pub(crate) fn wide_os(s: &OsStr) -> Vec<u16> {
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
    /// Whether the user picked an `Image size` entry. A stored limit that
    /// isn't one of the entries shows as `Original size` and is kept unless
    /// this is set.
    limit_changed: bool,
    background: Option<usize>,
    keep_date: bool,
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

/// A limit that isn't one of the entries shows as `Original size`.
fn limit_index(limit: Option<u32>) -> usize {
    IMAGE_SIZE_EDGES.iter().position(|edge| *edge == limit).unwrap_or(0)
}

fn limit_from_index(index: usize) -> Option<Option<u32>> {
    IMAGE_SIZE_EDGES.get(index).copied()
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
        for l in IMAGE_SIZE_EDGES {
            assert_eq!(limit_from_index(limit_index(l)), Some(l));
        }
        // Settings saved before the new entries map onto the same sizes.
        assert_eq!(IMAGE_SIZE[limit_index(Some(2048))], "Medium: for sharing online (2048 px)");
        assert_eq!(IMAGE_SIZE[limit_index(Some(3840))], "Large: for big screens and TVs (3840 px)");
        for b in [Background::White, Background::Black] {
            assert_eq!(background_from_index(background_index(b)), Some(b));
            assert!(background_index(b) < BACKGROUND.len());
        }
        assert_eq!(limit_from_index(IMAGE_SIZE.len()), None);
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
        form.limit = Some(3);
        assert_eq!(apply_form(&s, &form).max_long_edge, Some(1280));
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
            limit: Some(1),
            limit_changed: true,
            background: Some(1),
            keep_date: true,
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
                skip_if_larger: true,
            }
        );
    }
}
