//! Writing an encoded JPEG to its final name without overwriting anything
//! (spec §4.5).

use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use reduct_common::naming::{candidate, Planned};
use windows::core::PCWSTR;
use windows::Win32::Foundation::{ERROR_ALREADY_EXISTS, ERROR_FILE_EXISTS, WIN32_ERROR};
use windows::Win32::Storage::FileSystem::{MoveFileExW, MOVEFILE_WRITE_THROUGH};

use crate::error::EngineError;

/// How many suffixes past `planned.k` are tried before giving up.
const MAX_RETRIES: u32 = 1000;

/// How many temp names are tried before giving up.
const MAX_TEMP_ATTEMPTS: u32 = 100;

/// Process-wide counter that makes every temp name unique within the process.
static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Writes `bytes` to the planned output and returns the path actually used.
///
/// This is the real collision guard. Planning compares lowercased paths,
/// which only approximates NTFS case folding and cannot see files created
/// after planning; here the file system decides. `planned.dir` is created if
/// needed, the bytes go to a temp file `<stem>.tmp-<pid>-<n>.reduct-tmp` in that
/// folder (`n` is a process-wide counter; a temp name that already exists
/// is left alone and the next `n` is tried), and the temp file is moved to
/// `candidate(stem, style, k)` with `MoveFileExW(MOVEFILE_WRITE_THROUGH)`, never with
/// `MOVEFILE_REPLACE_EXISTING`. If the name is taken, the next `k` is tried,
/// up to `planned.k + 1000`. The temp file is deleted on every failure.
pub fn commit(bytes: &[u8], planned: &Planned) -> Result<PathBuf, EngineError> {
    fs::create_dir_all(&planned.dir)?;

    let tmp = write_temp(bytes, planned)?;

    let tmp_wide = wide(&tmp);
    let last = planned.k.saturating_add(MAX_RETRIES);
    for k in planned.k..=last {
        let target = planned.dir.join(candidate(&planned.stem, planned.style, k));
        let target_wide = wide(&target);
        // SAFETY: both buffers are NUL-terminated UTF-16 and outlive the call.
        let moved = unsafe {
            MoveFileExW(
                PCWSTR(tmp_wide.as_ptr()),
                PCWSTR(target_wide.as_ptr()),
                MOVEFILE_WRITE_THROUGH,
            )
        };
        match moved {
            Ok(()) => return Ok(target),
            Err(err) => match WIN32_ERROR::from_error(&err) {
                Some(code) if code == ERROR_ALREADY_EXISTS || code == ERROR_FILE_EXISTS => continue,
                Some(code) => {
                    let _ = fs::remove_file(&tmp);
                    return Err(EngineError::Io(io::Error::from_raw_os_error(code.0 as i32)));
                }
                None => {
                    let _ = fs::remove_file(&tmp);
                    return Err(EngineError::Io(io::Error::other(format!(
                        "MoveFileExW: 0x{:08X} {}",
                        err.code().0 as u32,
                        err.message().trim_end()
                    ))));
                }
            },
        }
    }

    let _ = fs::remove_file(&tmp);
    Err(EngineError::Io(io::Error::new(
        io::ErrorKind::AlreadyExists,
        format!(
            "every output name from {} to {} is taken",
            candidate(&planned.stem, planned.style, planned.k),
            candidate(&planned.stem, planned.style, last)
        ),
    )))
}

/// Writes `bytes` to a fresh temp file next to the planned output and returns
/// its path. A name that is already taken belongs to someone else: it is left
/// alone and the next counter value is tried.
fn write_temp(bytes: &[u8], planned: &Planned) -> Result<PathBuf, EngineError> {
    let pid = std::process::id();
    for _ in 0..MAX_TEMP_ATTEMPTS {
        let n = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
        let tmp = planned.dir.join(format!("{}.tmp-{pid}-{n}.reduct-tmp", planned.stem));
        match write_new(&tmp, bytes) {
            Ok(()) => return Ok(tmp),
            Err(err) if err.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(err) => {
                let _ = fs::remove_file(&tmp);
                return Err(err.into());
            }
        }
    }
    Err(EngineError::Io(io::Error::new(
        io::ErrorKind::AlreadyExists,
        format!("no free temp name for {} after {MAX_TEMP_ATTEMPTS} attempts", planned.stem),
    )))
}

/// Writes `bytes` to a file that must not exist yet, flushed to disk.
fn write_new(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
    file.write_all(bytes)?;
    file.sync_all()
}

fn wide(path: &Path) -> Vec<u16> {
    path.as_os_str().encode_wide().chain(Some(0)).collect()
}
