//! mozjpeg encoding and the no-overwrite output commit.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use reduct_common::naming::{plan_outputs, OutputLocation, SuffixStyle};
use reduct_engine::{commit, decode, encode_jpeg, init_com_mta};

const WHITE: [u8; 3] = [255, 255, 255];

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/orientation/Landscape_1.jpg")
}

/// A fresh, empty folder under the system temp dir, removed on drop.
struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> Self {
        static N: AtomicU32 = AtomicU32::new(0);
        let n = N.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("reduct-test-{name}-{}-{n}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("create temp dir");
        TempDir(dir)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn contains_marker(bytes: &[u8], marker: u8) -> bool {
    bytes.windows(2).any(|w| w == [0xFF, marker])
}

#[test]
fn encode_produces_progressive_jpeg() {
    let _com = init_com_mta().expect("COM init");
    let img = decode(&fixture(), None, WHITE).expect("decode Landscape_1.jpg");

    let bytes = encode_jpeg(&img, 75).expect("encode");

    assert_eq!(&bytes[..2], &[0xFF, 0xD8], "starts with SOI");
    assert!(contains_marker(&bytes, 0xC2), "has an SOF2 (progressive) marker");
    assert!(!contains_marker(&bytes, 0xE1), "has no APP1/EXIF marker");

    let tmp = TempDir::new("encode");
    let out = tmp.path().join("out.jpg");
    fs::write(&out, &bytes).expect("write encoded JPEG");
    let again = decode(&out, None, WHITE).expect("decode the encoded JPEG");
    assert_eq!((again.width, again.height), (img.width, img.height));
}

#[test]
fn commit_takes_next_suffix_when_target_appears() {
    let tmp = TempDir::new("commit");
    let dir = tmp.path();
    let source = dir.join("a.png");

    let planned =
        plan_outputs(&[source], SuffixStyle::Underscore, OutputLocation::NextToOriginal, &|p: &Path| p.exists())
            .remove(0);
    assert_eq!(planned.target(), dir.join("a.jpg"));

    // Someone else creates a.jpg between planning and committing.
    let existing = b"not ours";
    fs::write(dir.join("a.jpg"), existing).expect("create a.jpg");

    let written = commit(b"new jpeg bytes", &planned).expect("commit");

    assert_eq!(written, dir.join("a_2.jpg"));
    assert_eq!(fs::read(dir.join("a.jpg")).unwrap(), existing, "a.jpg is unchanged");
    assert_eq!(fs::read(&written).unwrap(), b"new jpeg bytes");
    let leftovers: Vec<_> = fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|name| name.contains(".tmp-") || name.ends_with(".reduct-tmp"))
        .collect();
    assert!(leftovers.is_empty(), "temp files left behind: {leftovers:?}");
}
