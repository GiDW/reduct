//! Batch conversion (spec §4.3): `Reduct.exe --batch <listfile> [--no-window]`.
//! The conversion runs on a worker thread; the main thread shows the progress
//! window (progress.rs) unless `--no-window` is given.

use std::cell::RefCell;
use std::fs::{self, OpenOptions};
use std::io;
use std::panic::{self, catch_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use rayon::prelude::*;
use rayon::ThreadPoolBuilder;
use reduct_common::formats::{is_heif, is_jpeg, is_supported};
use reduct_common::lang::Lang;
use reduct_common::listfile::read_list;
use reduct_common::naming::{plan_outputs, Planned};
use reduct_common::settings::Settings;
use reduct_engine::{commit, decode, encode_jpeg, heic_supported, init_com_mta, panic_message, ComGuard, EngineError};

use crate::paths;
use crate::progress;
use crate::summary::{result_text, summarize, Outcome, Summary};

thread_local! {
    /// This thread's COM initialisation: the guard, or why it failed.
    static COM: RefCell<Option<Result<ComGuard, String>>> = const { RefCell::new(None) };
}

/// How far a running batch is, shared between the worker and the progress
/// window. The window also uses it to ask the worker to stop.
#[derive(Default)]
pub struct Progress {
    /// Files in the batch; `0` until the list is read and planned.
    total: AtomicUsize,
    done: AtomicUsize,
    cancelled: AtomicBool,
    /// The file name of the file a worker started most recently.
    current: Mutex<String>,
}

impl Progress {
    pub fn total(&self) -> usize {
        self.total.load(Ordering::Relaxed)
    }

    pub fn done(&self) -> usize {
        self.done.load(Ordering::Relaxed)
    }

    pub fn current(&self) -> String {
        self.current.lock().map(|c| c.clone()).unwrap_or_default()
    }

    /// Files that haven't started yet are left out; files already being
    /// converted finish.
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Relaxed)
    }

    pub(crate) fn set_total(&self, total: usize) {
        self.total.store(total, Ordering::Relaxed);
    }

    fn start(&self, source: &Path) {
        let name = source.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        if let Ok(mut current) = self.current.lock() {
            *current = name;
        }
    }

    fn finish_one(&self) {
        self.done.fetch_add(1, Ordering::Relaxed);
    }
}

/// How a batch ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finished {
    pub summary: Summary,
    /// The failures log, when something failed.
    pub log: Option<PathBuf>,
    /// Why the batch couldn't run at all.
    pub problem: Option<Problem>,
}

/// Why a batch couldn't run at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Problem {
    /// The list file couldn't be read.
    SelectionUnreadable,
    /// The worker thread died.
    Crashed,
}

impl Problem {
    fn text(self, lang: Lang) -> &'static str {
        match self {
            Problem::SelectionUnreadable => lang.text().selection_unreadable,
            Problem::Crashed => lang.text().something_went_wrong,
        }
    }
}

impl Finished {
    fn problem(problem: Problem, log: Option<PathBuf>) -> Finished {
        Finished { summary: Summary::default(), log, problem: Some(problem) }
    }

    /// For a worker thread that panicked outside the per-file guard.
    pub fn crashed() -> Finished {
        Finished::problem(Problem::Crashed, None)
    }

    pub fn has_errors(&self) -> bool {
        self.problem.is_some() || self.summary.failed > 0
    }

    /// `0` when nothing failed, `1` otherwise.
    pub fn exit_code(&self) -> i32 {
        i32::from(self.has_errors())
    }

    pub fn codec_missing(&self) -> bool {
        self.summary.codec_missing > 0
    }

    /// The result's headline and detail line.
    pub fn text(&self, lang: Lang) -> (String, String) {
        match self.problem {
            Some(problem) => (lang.optimised(0, None), problem.text(lang).to_owned()),
            None => result_text(&self.summary, lang),
        }
    }
}

/// Runs a batch from a list file and returns the process exit code: `0` when
/// nothing failed, `1` when at least one file failed or the list file could
/// not be read. With `window`, the progress window shows while it runs.
pub fn run(list: &Path, window: bool) -> i32 {
    let progress = Arc::new(Progress::default());
    let worker = {
        let list = list.to_path_buf();
        let progress = Arc::clone(&progress);
        std::thread::Builder::new()
            .name("reduct-batch".to_owned())
            .spawn(move || run_in(&list, &paths::data_dir(), &progress))
    };
    let finished = match worker {
        Ok(worker) if window => progress::run(progress, worker),
        Ok(worker) => worker.join().unwrap_or_else(|_| Finished::crashed()),
        // No thread: run here, without a window.
        Err(_) => run_in(list, &paths::data_dir(), &progress),
    };
    finished.exit_code()
}

/// The batch itself, with the data folder (settings and logs) passed in so
/// tests can keep away from the real one.
fn run_in(list: &Path, data_dir: &Path, progress: &Progress) -> Finished {
    let sources = match read_list(list) {
        Ok(sources) => sources,
        Err(err) => {
            let reason = format!("could not read the list file: {err}");
            let log = write_failure_log(data_dir, &[(list, reason.as_str())]);
            return Finished::problem(Problem::SelectionUnreadable, log);
        }
    };
    let _ = fs::remove_file(list);

    let settings = paths::load_settings_in(data_dir);
    let sources: Vec<PathBuf> = sources.into_iter().filter(|p| is_supported(p)).collect();
    let planned = plan_outputs(&sources, settings.suffix, settings.output, Lang::current(), &|p: &Path| p.exists());

    progress.set_total(planned.len());
    let outcomes = convert_all(&planned, &settings, progress);
    let mut summary = summarize(&outcomes);
    summary.cancelled = planned.len().saturating_sub(outcomes.len()) as u32;

    let failures: Vec<(&Path, &str)> = outcomes
        .iter()
        .filter_map(|o| match o {
            Outcome::Failed { source, reason, .. } => Some((source.as_path(), reason.as_str())),
            _ => None,
        })
        .collect();
    let log = if failures.is_empty() { None } else { write_failure_log(data_dir, &failures) };
    Finished { summary, log, problem: None }
}

/// Silences the default panic message for the batch. The exe has no
/// console, and panics (mozjpeg reports errors by unwinding) are caught per
/// file and recorded in the failures log instead.
pub fn install_quiet_panic_hook() {
    panic::set_hook(Box::new(|_| {}));
}

/// At most this many files are converted at once. Each worker holds the
/// decoded, flattened and encoded copies of one image (0.5 to 0.8 GB for a
/// 48 MP HEIC), so one worker per logical CPU could run a PC out of memory.
const MAX_WORKERS: usize = 4;

/// Converts every planned file on a rayon pool whose threads are COM MTA
/// threads. Falls back to the current thread if the pool cannot be built.
/// After a cancel, files that haven't started are left out of the result.
fn convert_all(planned: &[Planned], settings: &Settings, progress: &Progress) -> Vec<Outcome> {
    let heic = OnceLock::new();
    let step = |p: &Planned| -> Option<Outcome> {
        if progress.is_cancelled() {
            return None;
        }
        progress.start(&p.source);
        let outcome = convert_one(p, settings, &heic);
        progress.finish_one();
        Some(outcome)
    };
    let work = || -> Vec<Outcome> { planned.par_iter().filter_map(step).collect() };
    let workers = std::thread::available_parallelism().map_or(1, |n| n.get()).min(MAX_WORKERS);
    let pooled = ThreadPoolBuilder::new()
        .num_threads(workers)
        .thread_name(|i| format!("reduct-worker-{i}"))
        .start_handler(|_| com_start())
        .exit_handler(|_| com_exit())
        // build_scoped joins the workers before returning, so every exit
        // handler has run (and COM is uninitialised) when the batch ends.
        .build_scoped(|thread| thread.run(), |pool| pool.install(work));
    match pooled {
        Ok(outcomes) => outcomes,
        Err(_) => {
            com_start();
            let outcomes = planned.iter().filter_map(step).collect();
            com_exit();
            outcomes
        }
    }
}

fn com_start() {
    let state = init_com_mta().map_err(|err| {
        format!("COM initialisation failed: 0x{:08X} {}", err.code().0 as u32, err.message().trim_end())
    });
    let _ = COM.try_with(|com| *com.borrow_mut() = Some(state));
}

/// Drops this thread's guard explicitly, so the cached WIC factory is
/// released before `CoUninitialize`, whatever order thread-local destructors
/// would run in.
fn com_exit() {
    let guard = COM.try_with(|com| com.borrow_mut().take()).ok().flatten();
    drop(guard);
}

fn com_ready() -> Result<(), String> {
    COM.with(|com| match &*com.borrow() {
        Some(Ok(_)) => Ok(()),
        Some(Err(reason)) => Err(reason.clone()),
        None => Err("COM is not initialised on this thread".to_owned()),
    })
}

/// Converts one file; never panics.
fn convert_one(planned: &Planned, settings: &Settings, heic: &OnceLock<bool>) -> Outcome {
    let result = catch_unwind(AssertUnwindSafe(|| try_convert(planned, settings, heic)));
    let (reason, codec_missing) = match result {
        Ok(Ok(outcome)) => return outcome,
        Ok(Err(failure)) => failure,
        Err(payload) => (format!("internal error: {}", panic_message(payload.as_ref())), false),
    };
    Outcome::Failed { source: planned.source.clone(), reason, codec_missing }
}

/// A failure reason and whether it is a missing HEIC codec.
type Failure = (String, bool);

fn try_convert(planned: &Planned, settings: &Settings, heic: &OnceLock<bool>) -> Result<Outcome, Failure> {
    com_ready().map_err(|reason| (reason, false))?;
    let source = &planned.source;

    let source_meta = fs::metadata(source).map_err(|err| (format!("cannot read the file: {err}"), false))?;
    let image = decode(source, settings.max_long_edge, settings.background.rgb())
        .map_err(|err| classify_decode_error(source, err, heic))?;
    let bytes = encode_jpeg(&image, settings.quality).map_err(|err| (err.to_string(), false))?;
    drop(image);

    let (source_bytes, output_bytes) = (source_meta.len(), bytes.len() as u64);
    if is_jpeg(source) && settings.skip_if_larger && output_bytes >= source_bytes {
        return Ok(Outcome::SkippedLarger);
    }

    let written = commit(&bytes, planned).map_err(|err| (commit_reason(&err), false))?;
    if settings.keep_file_date {
        // Best effort: the output exists either way.
        if let Ok(modified) = source_meta.modified() {
            let _ = OpenOptions::new().write(true).open(&written).and_then(|file| file.set_modified(modified));
        }
    }
    Ok(Outcome::Converted { source_bytes, output_bytes })
}

/// A `.heic`/`.heif` failure counts as a missing codec when the HEIC probe
/// (`heic_supported`, a real decode of a tiny embedded HEIC) fails: WIC
/// reports a corrupt HEIC and a missing decoder alike, so the error itself
/// does not tell. The probe runs at most once per batch.
fn classify_decode_error(source: &Path, err: EngineError, heic: &OnceLock<bool>) -> Failure {
    if is_heif(source) && !*heic.get_or_init(heic_supported) {
        return ("HEIC needs Microsoft's HEIF and HEVC extensions".to_owned(), true);
    }
    match err {
        EngineError::CodecMissing => ("no decoder for this format".to_owned(), false),
        other => (other.to_string(), false),
    }
}

/// The failure-log reason for a failed write or move. Access denied gets a
/// plain sentence; anything else keeps the error's own text.
fn commit_reason(err: &EngineError) -> String {
    match err {
        EngineError::Io(e) if e.raw_os_error() == Some(5) || e.kind() == io::ErrorKind::PermissionDenied => {
            "can't write to this folder (access denied)".to_owned()
        }
        other => other.to_string(),
    }
}

/// Writes `<data_dir>\logs\failures-<unix-secs>-<pid>.txt` with one
/// `<path>\t<reason>` line per failure and returns its path.
fn write_failure_log(data_dir: &Path, failures: &[(&Path, &str)]) -> Option<PathBuf> {
    let dir = data_dir.join("logs");
    fs::create_dir_all(&dir).ok()?;
    let secs = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let path = dir.join(format!("failures-{secs}-{}.txt", std::process::id()));
    write_log(&path, failures).ok()?;
    Some(path)
}

fn write_log(path: &Path, failures: &[(&Path, &str)]) -> io::Result<()> {
    let mut text = String::new();
    for (source, reason) in failures {
        text.push_str(&one_line(&source.to_string_lossy()));
        text.push('\t');
        text.push_str(&one_line(reason));
        text.push_str("\r\n");
    }
    fs::write(path, text)
}

/// Replaces tabs and line breaks so each failure stays on one line.
fn one_line(s: &str) -> String {
    s.chars().map(|c| if matches!(c, '\t' | '\r' | '\n') { ' ' } else { c }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn log_lines_are_tab_separated_and_single_line() {
        let path = std::env::temp_dir().join(format!("reduct-failures-{}.txt", std::process::id()));
        let a = PathBuf::from(r"C:\Foto's\Été.heic");
        let b = PathBuf::from(r"C:\x\broken.jpg");
        write_log(&path, &[(&a, "no\tcodec"), (&b, "decode failed: 0x1 bad\r\ndata")]).unwrap();
        let text = fs::read_to_string(&path).unwrap();
        let _ = fs::remove_file(&path);
        assert_eq!(text, "C:\\Foto's\\Été.heic\tno codec\r\nC:\\x\\broken.jpg\tdecode failed: 0x1 bad  data\r\n");
    }

    #[test]
    fn access_denied_reads_as_a_plain_sentence() {
        let denied = EngineError::Io(io::Error::from_raw_os_error(5));
        assert_eq!(commit_reason(&denied), "can't write to this folder (access denied)");
        let full = EngineError::Io(io::Error::from_raw_os_error(112));
        assert_eq!(commit_reason(&full), full.to_string());
    }

    #[test]
    fn missing_list_file_fails_with_exit_code_1_and_logs() {
        let data = std::env::temp_dir().join(format!("reduct-data-{}-missing-list", std::process::id()));
        let _ = fs::remove_dir_all(&data);
        fs::create_dir_all(&data).unwrap();
        let list = std::env::temp_dir().join("reduct-list-that-does-not-exist.txt");

        let finished = run_in(&list, &data, &Progress::default());
        assert_eq!(finished.exit_code(), 1);
        assert_eq!(finished.text(Lang::EnGb).1, "Couldn't read the selection");
        assert_eq!(finished.text(Lang::Nl).1, "Kan de selectie niet lezen");

        let logs: Vec<PathBuf> = fs::read_dir(data.join("logs")).unwrap().map(|entry| entry.unwrap().path()).collect();
        let text = fs::read_to_string(&logs[0]).unwrap();
        let _ = fs::remove_dir_all(&data);
        assert_eq!(logs.len(), 1);
        assert!(text.contains("reduct-list-that-does-not-exist.txt\tcould not read the list file"), "{text}");
    }
}
