//! Image decoding, conversion and JPEG encoding for Reduct.

pub mod codec;
pub mod com;
pub mod decode;
pub mod encode;
pub mod error;
pub mod output;

pub use codec::heic_supported;
pub use com::{init_com_mta, ComGuard};
pub use decode::{decode, flatten_bgra, RgbImage};
pub use encode::encode_jpeg;
pub use error::EngineError;
pub use output::commit;
