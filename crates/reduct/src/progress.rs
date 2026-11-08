//! The batch progress window: a small centred window, in the style of
//! Explorer's copy dialog, with a count, a progress bar, the current file and
//! a Cancel button.
//!
//! - A batch of [`ALWAYS_SHOW_FROM`] files or more always shows it: some ARM
//!   PCs are slow enough that even a few files take a while.
//! - A single file shows it only once it has run for [`SHOW_AFTER`], so a
//!   quick one shows nothing.
//! - Without errors it shows the result for [`RESULT_SHOWN_FOR`] and closes
//!   itself.
//! - With errors it stays open until closed, with buttons for the failures
//!   log and, when HEIC support is missing, the HEVC extension's Store page.
//!
//! The template is `IDD_PROGRESS` in `resources/reduct.rc`. The window is a
//! modeless dialog on the main thread; the batch runs on a worker thread and
//! a timer reads its [`Progress`]. The window state lives in a thread-local
//! and is never borrowed while a Win32 call could send a message back to
//! `dialog_proc`: [`Window::tick`] decides what to show, and the Win32 calls
//! happen after the borrow ends.

use std::cell::RefCell;
use std::path::PathBuf;
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use windows::core::PCWSTR;
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE};
use windows::Win32::UI::Controls::{
    InitCommonControlsEx, ICC_PROGRESS_CLASS, ICC_STANDARD_CLASSES, INITCOMMONCONTROLSEX, PBM_SETPOS,
    PBM_SETRANGE32, PBM_SETSTATE, PBST_ERROR,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateDialogParamW, DestroyWindow, DispatchMessageW, GetDlgItem, GetMessageW, IsDialogMessageW, KillTimer,
    PostQuitMessage, SendDlgItemMessageW, SendMessageW, SetDlgItemTextW, SetForegroundWindow, SetTimer,
    SetWindowTextW, ShowWindow, TranslateMessage, IDCANCEL, IDOK, MSG, SW_HIDE, SW_SHOW, SW_SHOWNORMAL, WM_CLOSE,
    WM_COMMAND, WM_DESTROY, WM_INITDIALOG, WM_NEXTDLGCTL, WM_TIMER,
};

use crate::batch::{Finished, Progress};
use crate::ui::{make_int_resource, module, open, set_icons, wide, wide_os, HEVC_STORE_URI};

// Resource IDs: keep in step with resources/resource.h.
const IDD_PROGRESS: u16 = 102;
const IDC_PROGRESS_STATUS: i32 = 1101;
const IDC_PROGRESS_BAR: i32 = 1102;
const IDC_PROGRESS_DETAIL: i32 = 1103;
const IDC_SHOW_LOG: i32 = 1104;
const IDC_GET_HEVC: i32 = 1105;

const ID_OK: i32 = IDOK.0;
const ID_CANCEL: i32 = IDCANCEL.0;

/// A batch of at least this many files shows the window straight away.
const ALWAYS_SHOW_FROM: usize = 2;
/// A smaller batch still running after this long shows the window.
const SHOW_AFTER: Duration = Duration::from_millis(500);
/// How long a result without errors stays on screen.
const RESULT_SHOWN_FOR: Duration = Duration::from_secs(1);
const TIMER_ID: usize = 1;
const TIMER_MS: u32 = 100;

/// The window's state between messages.
struct Window {
    progress: Arc<Progress>,
    /// The batch; `None` once it has been joined.
    worker: Option<JoinHandle<Finished>>,
    finished: Option<Finished>,
    started: Instant,
    shown: bool,
    cancelling: bool,
    /// When a result without errors closes the window.
    close_at: Option<Instant>,
    /// The (done, total, file) last shown, so unchanged text isn't redrawn.
    last: Option<(usize, usize, String)>,
}

/// What the window should do after a tick or a command.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Action {
    Show,
    Running { done: usize, total: usize, file: String },
    Cancelling,
    Result(ResultView),
    Open(Vec<u16>),
    Close,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ResultView {
    headline: String,
    detail: String,
    errors: bool,
    log: bool,
    hevc: bool,
}

impl Window {
    fn new(progress: Arc<Progress>, worker: JoinHandle<Finished>, started: Instant) -> Window {
        Window {
            progress,
            worker: Some(worker),
            finished: None,
            started,
            shown: false,
            cancelling: false,
            close_at: None,
            last: None,
        }
    }

    /// One timer tick at `now`.
    fn tick(&mut self, now: Instant) -> Vec<Action> {
        if self.finished.is_some() {
            return match self.close_at {
                Some(at) if now >= at => {
                    self.close_at = None;
                    vec![Action::Close]
                }
                _ => Vec::new(),
            };
        }

        if self.worker.as_ref().is_none_or(JoinHandle::is_finished) {
            let finished = self
                .worker
                .take()
                .and_then(|worker| worker.join().ok())
                .unwrap_or_else(Finished::crashed);
            let view = result_view(&finished);
            self.finished = Some(finished);
            if !view.errors && !self.shown && !self.always_show() {
                return vec![Action::Close];
            }
            if !view.errors {
                self.close_at = Some(now + RESULT_SHOWN_FOR);
            }
            let mut actions = vec![Action::Result(view)];
            if !self.shown {
                self.shown = true;
                actions.push(Action::Show);
            }
            return actions;
        }

        let mut actions = Vec::new();
        let show = !self.shown && (self.always_show() || now.duration_since(self.started) >= SHOW_AFTER);
        if (self.shown || show) && !self.cancelling {
            let state = (self.progress.done(), self.progress.total(), self.progress.current());
            if self.last.as_ref() != Some(&state) {
                let (done, total, file) = state.clone();
                self.last = Some(state);
                actions.push(Action::Running { done, total, file });
            }
        }
        if show {
            self.shown = true;
            actions.push(Action::Show);
        }
        actions
    }

    fn always_show(&self) -> bool {
        self.progress.total() >= ALWAYS_SHOW_FROM
    }

    /// Cancel, Esc or the close box: cancels a running batch, closes a
    /// finished one.
    fn on_cancel(&mut self) -> Vec<Action> {
        if self.finished.is_some() {
            return vec![Action::Close];
        }
        if self.cancelling {
            return Vec::new();
        }
        self.cancelling = true;
        self.progress.cancel();
        vec![Action::Cancelling]
    }

    /// Close or Enter: only closes a finished batch.
    fn on_ok(&mut self) -> Vec<Action> {
        if self.finished.is_some() {
            vec![Action::Close]
        } else {
            Vec::new()
        }
    }

    fn on_show_log(&self) -> Vec<Action> {
        let log: Option<&PathBuf> = self.finished.as_ref().and_then(|f| f.log.as_ref());
        log.map(|path| vec![Action::Open(wide_os(path.as_os_str()))]).unwrap_or_default()
    }
}

fn result_view(finished: &Finished) -> ResultView {
    let (headline, detail) = finished.text();
    ResultView {
        headline,
        detail,
        errors: finished.has_errors(),
        log: finished.log.is_some(),
        hevc: finished.codec_missing(),
    }
}

/// `Optimising N images`, the running window's title.
fn caption(total: usize) -> String {
    match total {
        0 => "Reduct".to_owned(),
        1 => "Optimising 1 image".to_owned(),
        n => format!("Optimising {n} images"),
    }
}

fn status(done: usize, total: usize) -> String {
    if total == 0 {
        "Preparing\u{2026}".to_owned()
    } else {
        format!("{done} of {total} done")
    }
}

thread_local! {
    static WINDOW: RefCell<Option<Window>> = const { RefCell::new(None) };
}

/// Shows the progress window for the batch running on `worker` until the
/// window closes, and returns how the batch ended. Without a window (it
/// couldn't be created) this just waits for the batch.
pub fn run(progress: Arc<Progress>, worker: JoinHandle<Finished>) -> Finished {
    // STA on the UI thread: ShellExecuteW (the log and Store buttons) may
    // hand the request to COM-based shell extensions.
    // SAFETY: plain initialisation calls; CoUninitialize is paired below.
    let com = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) };
    let icc = INITCOMMONCONTROLSEX {
        dwSize: std::mem::size_of::<INITCOMMONCONTROLSEX>() as u32,
        dwICC: ICC_STANDARD_CLASSES | ICC_PROGRESS_CLASS,
    };
    // SAFETY: `icc` is a valid, initialised struct.
    let _ = unsafe { InitCommonControlsEx(&icc) };

    WINDOW.with(|w| *w.borrow_mut() = Some(Window::new(progress, worker, Instant::now())));
    // SAFETY: the template is in this module's resources and dialog_proc has
    // the DLGPROC signature.
    let created =
        unsafe { CreateDialogParamW(module(), make_int_resource(IDD_PROGRESS), None, Some(dialog_proc), LPARAM(0)) };
    if let Ok(hwnd) = created {
        message_loop(hwnd);
    }
    let window = WINDOW.with(|w| w.borrow_mut().take());
    let finished = match window {
        Some(Window { finished: Some(finished), .. }) => finished,
        Some(Window { worker: Some(worker), .. }) => worker.join().unwrap_or_else(|_| Finished::crashed()),
        _ => Finished::crashed(),
    };

    if com.is_ok() {
        // SAFETY: balances the successful CoInitializeEx above.
        unsafe { CoUninitialize() };
    }
    finished
}

fn message_loop(hwnd: HWND) {
    let mut msg = MSG::default();
    // SAFETY: standard message loop on this thread; `msg` outlives each call.
    // GetMessageW returns 0 for WM_QUIT and -1 on error; both end the loop.
    unsafe {
        while GetMessageW(&mut msg, None, 0, 0).0 > 0 {
            if !IsDialogMessageW(hwnd, &msg).as_bool() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
    }
}

unsafe extern "system" fn dialog_proc(hwnd: HWND, msg: u32, wparam: WPARAM, _lparam: LPARAM) -> isize {
    match msg {
        WM_INITDIALOG => {
            set_icons(hwnd);
            for id in [IDC_SHOW_LOG, IDC_GET_HEVC, ID_OK] {
                show_item(hwnd, id, false);
            }
            // SAFETY: a timer on this dialog; KillTimer in WM_DESTROY.
            unsafe { SetTimer(Some(hwnd), TIMER_ID, TIMER_MS, None) };
            1
        }
        WM_TIMER if wparam.0 == TIMER_ID => {
            let actions = with_window(|w| w.tick(Instant::now()));
            apply(hwnd, actions);
            1
        }
        WM_COMMAND => {
            let id = (wparam.0 & 0xffff) as i32;
            let actions = match id {
                ID_CANCEL => with_window(Window::on_cancel),
                ID_OK => with_window(Window::on_ok),
                IDC_SHOW_LOG => with_window(|w| w.on_show_log()),
                IDC_GET_HEVC => vec![Action::Open(wide(HEVC_STORE_URI))],
                _ => return 0,
            };
            apply(hwnd, actions);
            1
        }
        WM_CLOSE => {
            let actions = with_window(Window::on_cancel);
            apply(hwnd, actions);
            1
        }
        WM_DESTROY => {
            // SAFETY: the timer set in WM_INITDIALOG.
            let _ = unsafe { KillTimer(Some(hwnd), TIMER_ID) };
            // SAFETY: ends this thread's message loop.
            unsafe { PostQuitMessage(0) };
            1
        }
        _ => 0,
    }
}

/// Runs `f` on the window state; no actions when the state is missing or
/// already borrowed (a message sent back during a Win32 call).
fn with_window(f: impl FnOnce(&mut Window) -> Vec<Action>) -> Vec<Action> {
    WINDOW.with(|w| match w.try_borrow_mut() {
        Ok(mut window) => window.as_mut().map(f).unwrap_or_default(),
        Err(_) => Vec::new(),
    })
}

fn apply(hwnd: HWND, actions: Vec<Action>) {
    for action in actions {
        match action {
            Action::Show => {
                // SAFETY: this dialog's own window.
                unsafe {
                    let _ = ShowWindow(hwnd, SW_SHOWNORMAL);
                    let _ = SetForegroundWindow(hwnd);
                }
            }
            Action::Running { done, total, file } => {
                set_caption(hwnd, &caption(total));
                set_text(hwnd, IDC_PROGRESS_STATUS, &status(done, total));
                set_text(hwnd, IDC_PROGRESS_DETAIL, &file);
                set_bar(hwnd, done, total);
            }
            Action::Cancelling => {
                set_text(hwnd, IDC_PROGRESS_STATUS, "Cancelling\u{2026}");
                set_text(hwnd, IDC_PROGRESS_DETAIL, "");
                show_item(hwnd, ID_CANCEL, false);
            }
            Action::Result(view) => show_result(hwnd, &view),
            Action::Open(target) => open(hwnd, &target),
            Action::Close => {
                // SAFETY: this dialog's own window; WM_DESTROY ends the loop.
                let _ = unsafe { DestroyWindow(hwnd) };
                return;
            }
        }
    }
}

fn show_result(hwnd: HWND, view: &ResultView) {
    set_caption(hwnd, "Reduct");
    set_text(hwnd, IDC_PROGRESS_STATUS, &view.headline);
    set_text(hwnd, IDC_PROGRESS_DETAIL, &view.detail);
    set_bar(hwnd, 1, 1);
    show_item(hwnd, ID_CANCEL, false);
    if view.errors {
        // SAFETY: own child control; PBM_SETSTATE takes the state in wParam.
        unsafe { SendDlgItemMessageW(hwnd, IDC_PROGRESS_BAR, PBM_SETSTATE, WPARAM(PBST_ERROR as usize), LPARAM(0)) };
        show_item(hwnd, IDC_SHOW_LOG, view.log);
        show_item(hwnd, IDC_GET_HEVC, view.hevc);
        show_item(hwnd, ID_OK, true);
        // Cancel had the focus and is now hidden: move it to Close.
        // SAFETY: own child control; lParam 1 means wParam is a window.
        if let Ok(close) = unsafe { GetDlgItem(Some(hwnd), ID_OK) } {
            unsafe { SendMessageW(hwnd, WM_NEXTDLGCTL, Some(WPARAM(close.0 as usize)), Some(LPARAM(1))) };
        }
    }
}

fn set_caption(hwnd: HWND, text: &str) {
    let text = wide(text);
    // SAFETY: own window; `text` is NUL-terminated and outlives the call.
    let _ = unsafe { SetWindowTextW(hwnd, PCWSTR(text.as_ptr())) };
}

fn set_text(hwnd: HWND, id: i32, text: &str) {
    let text = wide(text);
    // SAFETY: own child control; `text` is NUL-terminated and outlives the call.
    let _ = unsafe { SetDlgItemTextW(hwnd, id, PCWSTR(text.as_ptr())) };
}

fn set_bar(hwnd: HWND, done: usize, total: usize) {
    let total = total.max(1);
    // SAFETY: own child control, with arguments as documented for each message.
    unsafe {
        SendDlgItemMessageW(hwnd, IDC_PROGRESS_BAR, PBM_SETRANGE32, WPARAM(0), LPARAM(total as isize));
        SendDlgItemMessageW(hwnd, IDC_PROGRESS_BAR, PBM_SETPOS, WPARAM(done.min(total)), LPARAM(0));
    }
}

fn show_item(hwnd: HWND, id: i32, show: bool) {
    // SAFETY: own child control.
    unsafe {
        if let Ok(item) = GetDlgItem(Some(hwnd), id) {
            let _ = ShowWindow(item, if show { SW_SHOW } else { SW_HIDE });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::summary::Summary;

    fn finished_worker(finished: Finished) -> JoinHandle<Finished> {
        let worker = std::thread::spawn(move || finished);
        while !worker.is_finished() {
            std::thread::yield_now();
        }
        worker
    }

    fn ok(converted: u32) -> Finished {
        Finished { summary: Summary { converted, ..Summary::default() }, log: None, problem: None }
    }

    fn failed() -> Finished {
        Finished {
            summary: Summary { converted: 8, failed: 2, codec_missing: 1, ..Summary::default() },
            log: Some(PathBuf::from(r"C:\logs\failures-1-2.txt")),
            problem: None,
        }
    }

    #[test]
    fn quick_batch_without_errors_never_shows() {
        let start = Instant::now();
        let mut w = Window::new(Arc::default(), finished_worker(ok(3)), start);
        assert_eq!(w.tick(start + Duration::from_millis(100)), vec![Action::Close]);
        assert!(!w.shown);
    }

    #[test]
    fn two_files_show_straight_away_even_when_quick() {
        let start = Instant::now();
        let progress = Arc::new(Progress::default());
        progress.set_total(2);

        // Still running at the first tick: shown at once.
        let (release, wait) = std::sync::mpsc::channel::<()>();
        let worker = std::thread::spawn(move || {
            let _ = wait.recv();
            ok(2)
        });
        let mut w = Window::new(Arc::clone(&progress), worker, start);
        assert_eq!(
            w.tick(start + Duration::from_millis(100)),
            vec![Action::Running { done: 0, total: 2, file: String::new() }, Action::Show]
        );
        release.send(()).unwrap();
        while !w.worker.as_ref().unwrap().is_finished() {
            std::thread::yield_now();
        }
        drop(w);

        // Already done at the first tick: the result still shows, briefly.
        let mut w = Window::new(progress, finished_worker(ok(2)), start);
        let at = start + Duration::from_millis(100);
        let actions = w.tick(at);
        assert!(matches!(&actions[..], [Action::Result(view), Action::Show] if !view.errors), "{actions:?}");
        assert_eq!(w.tick(at + RESULT_SHOWN_FOR), vec![Action::Close]);
    }

    #[test]
    fn quick_batch_with_errors_shows_the_result_and_stays() {
        let start = Instant::now();
        let mut w = Window::new(Arc::default(), finished_worker(failed()), start);
        let actions = w.tick(start + Duration::from_millis(100));
        assert_eq!(
            actions,
            vec![
                Action::Result(ResultView {
                    headline: "8 images optimised".to_owned(),
                    detail: "2 failed, HEIC needs Microsoft's HEVC extension".to_owned(),
                    errors: true,
                    log: true,
                    hevc: true,
                }),
                Action::Show,
            ]
        );
        assert_eq!(w.tick(start + Duration::from_secs(60)), Vec::new());
        assert_eq!(w.on_ok(), vec![Action::Close]);
    }

    #[test]
    fn slow_batch_shows_after_the_delay_and_closes_after_the_result() {
        let start = Instant::now();
        let (release, wait) = std::sync::mpsc::channel::<()>();
        let worker = std::thread::spawn(move || {
            let _ = wait.recv();
            ok(10)
        });
        let mut w = Window::new(Arc::default(), worker, start);

        assert_eq!(w.tick(start + Duration::from_millis(100)), Vec::new());
        assert_eq!(
            w.tick(start + SHOW_AFTER),
            vec![Action::Running { done: 0, total: 0, file: String::new() }, Action::Show]
        );
        // Nothing changed: nothing to redraw.
        assert_eq!(w.tick(start + Duration::from_millis(700)), Vec::new());

        release.send(()).unwrap();
        while !w.worker.as_ref().unwrap().is_finished() {
            std::thread::yield_now();
        }
        let done_at = start + Duration::from_secs(3);
        let actions = w.tick(done_at);
        assert!(matches!(&actions[..], [Action::Result(view)] if !view.errors), "{actions:?}");
        assert_eq!(w.tick(done_at + Duration::from_millis(500)), Vec::new());
        assert_eq!(w.tick(done_at + RESULT_SHOWN_FOR), vec![Action::Close]);
    }

    #[test]
    fn cancel_stops_a_running_batch_and_closes_a_finished_one() {
        let start = Instant::now();
        let (release, wait) = std::sync::mpsc::channel::<()>();
        let worker = std::thread::spawn(move || {
            let _ = wait.recv();
            ok(1)
        });
        let progress: Arc<Progress> = Arc::default();
        let mut w = Window::new(Arc::clone(&progress), worker, start);

        assert_eq!(w.on_cancel(), vec![Action::Cancelling]);
        assert!(progress.is_cancelled());
        assert_eq!(w.on_cancel(), Vec::new());
        assert_eq!(w.on_ok(), Vec::new());

        release.send(()).unwrap();
        while !w.worker.as_ref().unwrap().is_finished() {
            std::thread::yield_now();
        }
        w.tick(start + Duration::from_millis(100));
        assert_eq!(w.on_cancel(), vec![Action::Close]);
    }

    #[test]
    fn running_copy() {
        assert_eq!(caption(0), "Reduct");
        assert_eq!(caption(1), "Optimising 1 image");
        assert_eq!(caption(10), "Optimising 10 images");
        assert_eq!(status(0, 0), "Preparing\u{2026}");
        assert_eq!(status(4, 10), "4 of 10 done");
    }
}
