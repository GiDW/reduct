//! Headless batch conversion (spec §4.3): `Reduct.exe --batch <listfile>`.

use std::any::Any;
use std::cell::RefCell;
use std::fs::{self, OpenOptions};
use std::io;
use std::panic::{self, catch_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

use rayon::prelude::*;
use rayon::ThreadPoolBuilder;
use reduct_common::formats::{is_heif, is_jpeg, is_supported};
use reduct_common::listfile::read_list;
use reduct_common::naming::{plan_outputs, Planned};
use reduct_common::settings::Settings;
use reduct_engine::{commit, decode, encode_jpeg, heic_supported, init_com_mta, ComGuard, EngineError};

use crate::notify::{notify, notify_text};
use crate::paths;
use crate::summary::{summarize, Outcome};

thread_local! {
    /// This thread's COM initialisation: the guard, or why it failed.
    static COM: RefCell<Option<Result<ComGuard, String>>> = const { RefCell::new(None) };
}

/// Runs a batch from a list file and returns the process exit code: `0` when
/// nothing failed, `1` when at least one file failed or the list file could
/// not be read.
pub fn run(list: &Path) -> i32 {
    run_in(list, &paths::data_dir())
}

/// `run` with the data folder (settings and logs) passed in, so tests can
/// keep away from the real one.
fn run_in(list: &Path, data_dir: &Path) -> i32 {
    let sources = match read_list(list) {
        Ok(sources) => sources,
        Err(err) => {
            let reason = format!("could not read the list file: {err}");
            let log = write_failure_log(data_dir, &[(list, reason.as_str())]);
            notify_text("No images converted", "Couldn't read the selection", log.as_deref(), false);
            return 1;
        }
    };
    let _ = fs::remove_file(list);

    let settings = paths::load_settings_in(data_dir);
    let sources: Vec<PathBuf> = sources.into_iter().filter(|p| is_supported(p)).collect();
    let planned = plan_outputs(&sources, settings.suffix, settings.output, &|p: &Path| p.exists());

    let outcomes = convert_all(&planned, &settings);
    let summary = summarize(&outcomes);

    let failures: Vec<(&Path, &str)> = outcomes
        .iter()
        .filter_map(|o| match o {
            Outcome::Failed { source, reason, .. } => Some((source.as_path(), reason.as_str())),
            _ => None,
        })
        .collect();
    let log = if failures.is_empty() { None } else { write_failure_log(data_dir, &failures) };

    notify(&summary, log.as_deref(), summary.codec_missing > 0);

    if summary.failed > 0 {
        1
    } else {
        0
    }
}

/// Silences the default panic message for the batch. The exe has no
/// console, and panics (mozjpeg reports errors by unwinding) are caught per
/// file and recorded in the failures log instead.
pub fn install_quiet_panic_hook() {
    panic::set_hook(Box::new(|_| {}));
}

/// Converts every planned file on a rayon pool whose threads are COM MTA
/// threads. Falls back to the current thread if the pool cannot be built.
fn convert_all(planned: &[Planned], settings: &Settings) -> Vec<Outcome> {
    let heic = OnceLock::new();
    let work = || -> Vec<Outcome> {
        planned.par_iter().map(|p| convert_one(p, settings, &heic)).collect()
    };
    let pooled = ThreadPoolBuilder::new()
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
            let outcomes = planned.iter().map(|p| convert_one(p, settings, &heic)).collect();
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

    let written = commit(&bytes, planned).map_err(|err| (err.to_string(), false))?;
    if settings.keep_file_date {
        // Best effort: the output exists either way.
        if let Ok(modified) = source_meta.modified() {
            let _ = OpenOptions::new()
                .write(true)
                .open(&written)
                .and_then(|file| file.set_modified(modified));
        }
    }
    Ok(Outcome::Converted { source_bytes, output_bytes })
}

/// HEIF failures count as a missing codec when the HEIF/HEVC extensions are
/// absent (WIC reports a corrupt HEIC and a missing decoder alike, so the
/// error itself does not tell). `heic_supported` runs once per batch.
fn classify_decode_error(source: &Path, err: EngineError, heic: &OnceLock<bool>) -> Failure {
    if is_heif(source) && !*heic.get_or_init(heic_supported) {
        return ("HEIC needs Microsoft's HEIF and HEVC extensions".to_owned(), true);
    }
    match err {
        EngineError::CodecMissing => ("no decoder for this format".to_owned(), false),
        other => (other.to_string(), false),
    }
}

fn panic_message(payload: &(dyn Any + Send)) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        (*s).to_owned()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "unknown panic".to_owned()
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
        assert_eq!(
            text,
            "C:\\Foto's\\Été.heic\tno codec\r\nC:\\x\\broken.jpg\tdecode failed: 0x1 bad  data\r\n"
        );
    }

    #[test]
    fn missing_list_file_fails_with_exit_code_1_and_logs() {
        let data = std::env::temp_dir().join(format!("reduct-data-{}-missing-list", std::process::id()));
        let _ = fs::remove_dir_all(&data);
        fs::create_dir_all(&data).unwrap();
        let list = std::env::temp_dir().join("reduct-list-that-does-not-exist.txt");

        assert_eq!(run_in(&list, &data), 1);

        let logs: Vec<PathBuf> = fs::read_dir(data.join("logs"))
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect();
        let text = fs::read_to_string(&logs[0]).unwrap();
        let _ = fs::remove_dir_all(&data);
        assert_eq!(logs.len(), 1);
        assert!(text.contains("reduct-list-that-does-not-exist.txt\tcould not read the list file"), "{text}");
    }
}
