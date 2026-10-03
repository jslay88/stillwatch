//! Errors from encoding and decoding wire payloads.

/// Why a wire payload couldn't be built, encoded, or decoded.
#[derive(Debug, thiserror::Error)]
pub enum IpcError {
    /// JSON encoding or decoding failed.
    #[error("invalid JSON payload: {0}")]
    Json(#[from] serde_json::Error),
    /// A `PromptAnswer` call had an unknown kind or invalid minutes.
    #[error("invalid prompt answer: {0}")]
    PromptAnswer(String),
    /// A probe block list doesn't match its grid dimensions.
    #[error("probe grid {columns}x{rows} needs {expected} blocks, got {actual}")]
    ProbeGrid {
        /// Grid columns.
        columns: u16,
        /// Grid rows.
        rows: u16,
        /// `columns * rows`.
        expected: usize,
        /// Blocks supplied.
        actual: usize,
    },
}
