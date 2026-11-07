//! The end-of-batch notification. A no-op until the toast lands (Task 12).

use std::path::Path;

use crate::summary::Summary;

/// Tells the user how the batch went. Ignores every error.
pub fn notify(_s: &Summary, _log: Option<&Path>, _codec_missing: bool) {}
