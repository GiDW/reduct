//! The list file that hands a selection from the shell DLL to `Reduct.exe`.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// Writes `items` to `path` as UTF-8, one path per line, each ending in `\n`.
///
/// Fails with `InvalidData` if a path is not valid Unicode.
pub fn write_list(path: &Path, items: &[PathBuf]) -> io::Result<()> {
    let mut text = String::new();
    for item in items {
        let s = item.to_str().ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "path is not valid Unicode"))?;
        text.push_str(s);
        text.push('\n');
    }
    fs::write(path, text)
}

/// Reads a list file written by [`write_list`]. Strips a trailing `\r` from
/// each line and skips empty lines.
pub fn read_list(path: &Path) -> io::Result<Vec<PathBuf>> {
    let text = fs::read_to_string(path)?;
    Ok(text
        .split('\n')
        .map(|l| l.strip_suffix('\r').unwrap_or(l))
        .filter(|l| !l.is_empty())
        .map(PathBuf::from)
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_file(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("reduct-{}-{}.txt", name, std::process::id()))
    }

    #[test]
    fn list_file_round_trips_unicode() {
        let file = temp_file("roundtrip");
        let items = vec![
            PathBuf::from(r"C:\Foto's\Été à Gent.HEIC"),
            PathBuf::from(r"C:\写真\猫.jpg"),
            PathBuf::from(r"C:\x\🙂.png"),
        ];
        write_list(&file, &items).unwrap();
        let back = read_list(&file).unwrap();
        let _ = fs::remove_file(&file);
        assert_eq!(back, items);
    }

    #[test]
    fn read_list_tolerates_crlf_and_blank_lines() {
        let file = temp_file("crlf");
        fs::write(&file, "C:\\a.jpg\r\n\r\n\nC:\\b c\\d.png\r\n\n").unwrap();
        let back = read_list(&file).unwrap();
        let _ = fs::remove_file(&file);
        assert_eq!(back, vec![PathBuf::from(r"C:\a.jpg"), PathBuf::from(r"C:\b c\d.png")]);
    }
}
