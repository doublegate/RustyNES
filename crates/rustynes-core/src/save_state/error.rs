//! Save state error types.

use thiserror::Error;

/// Save state operation error
#[derive(Debug, Error)]
pub enum SaveStateError {
    /// Invalid magic bytes (expected 'RNES')
    #[error("Invalid magic bytes (expected 'RNES')")]
    InvalidMagic,

    /// Unsupported version
    #[error("Unsupported version: {0} (current: {CURRENT_VERSION})")]
    UnsupportedVersion(u32),

    /// ROM mismatch (save state doesn't match current ROM)
    #[error("ROM mismatch: expected {expected:x?}, got {actual:x?}")]
    RomMismatch {
        /// Expected ROM hash
        expected: [u8; 32],
        /// Actual ROM hash
        actual: [u8; 32],
    },

    /// Checksum mismatch (data corruption)
    #[error("Checksum mismatch: expected {expected:08x}, got {actual:08x}")]
    ChecksumMismatch {
        /// Expected checksum
        expected: u32,
        /// Actual checksum
        actual: u32,
    },

    /// Insufficient data
    #[error("Insufficient data: need {needed} bytes, got {available}")]
    InsufficientData {
        /// Bytes needed
        needed: usize,
        /// Bytes available
        available: usize,
    },

    /// Compression error
    #[error("Compression error: {0}")]
    Compression(String),

    /// Decompression error
    #[error("Decompression error: {0}")]
    Decompression(String),

    /// I/O error
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

const CURRENT_VERSION: u32 = super::SAVE_STATE_VERSION;
