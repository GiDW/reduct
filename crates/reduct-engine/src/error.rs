//! Errors returned by the engine.

use std::fmt;

/// Why decoding or encoding one image failed.
#[derive(Debug)]
pub enum EngineError {
    /// No WIC decoder is installed for the file's format (for example HEIC
    /// without the HEIF and HEVC Store extensions).
    CodecMissing,
    /// WIC could not read or convert the image.
    Decode(String),
    /// The JPEG encoder failed.
    Encode(String),
    /// A file system operation failed.
    Io(std::io::Error),
}

impl fmt::Display for EngineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EngineError::CodecMissing => f.write_str("no decoder is installed for this image format"),
            EngineError::Decode(msg) => write!(f, "decode failed: {msg}"),
            EngineError::Encode(msg) => write!(f, "encode failed: {msg}"),
            EngineError::Io(err) => write!(f, "I/O error: {err}"),
        }
    }
}

impl std::error::Error for EngineError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            EngineError::Io(err) => Some(err),
            _ => None,
        }
    }
}

impl From<std::io::Error> for EngineError {
    fn from(err: std::io::Error) -> Self {
        EngineError::Io(err)
    }
}

/// Formats a Windows error as `context: 0xHRESULT message` for `Decode`.
pub(crate) fn describe(context: &str, err: &windows::core::Error) -> String {
    let message = err.message();
    let code = err.code().0 as u32;
    if message.is_empty() {
        format!("{context}: 0x{code:08X}")
    } else {
        format!("{context}: 0x{code:08X} {}", message.trim_end())
    }
}
