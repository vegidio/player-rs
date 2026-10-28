//! Error types for the safe API.
//!
//! libmpv reports failures as negative `int` return codes. The internal helpers turn those into a [`Result`],
//! decoding the human-readable message with `mpv_error_string`.

use crate::sys;
use std::ffi::{CStr, c_int};

/// The crate-wide result type.
pub type Result<T> = std::result::Result<T, Error>;

/// Errors returned by the safe API.
///
/// Every variant carries a message: either this crate's own description of what was rejected, or mpv's message for
/// the error code it returned.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// mpv could not be created or initialised.
    #[error("could not initialise mpv: {0}")]
    Init(String),

    /// A builder value was out of range, or mpv rejected an option's name or value.
    #[error("invalid option '{name}': {message}")]
    InvalidOption {
        /// The option's name.
        name: String,
        /// Why it was rejected.
        message: String,
    },

    /// A [`Source`](crate::Source) could not be created. The message includes the rejected URL or path.
    #[error("invalid source: {0}")]
    InvalidSource(String),

    /// Reading or writing a property failed.
    #[error("property '{name}': {message}")]
    Property {
        /// The property's name.
        name: String,
        /// mpv's message.
        message: String,
    },

    /// Running a command failed.
    #[error("command '{name}': {message}")]
    Command {
        /// The command's name (its first argument).
        name: String,
        /// mpv's message.
        message: String,
    },
}

/// Decodes an mpv error code into a human-readable string via `mpv_error_string`.
pub(crate) fn mpv_message(code: c_int) -> String {
    // SAFETY: `mpv_error_string` accepts any value and returns a pointer to a static, NUL-terminated string
    // ("unknown error" for codes it does not know).
    let message = unsafe { CStr::from_ptr(sys::mpv_error_string(code)) };
    message.to_string_lossy().into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_mpv_codes() {
        assert_eq!(mpv_message(sys::MPV_ERROR_SUCCESS), "success");
        assert_eq!(mpv_message(sys::MPV_ERROR_PROPERTY_NOT_FOUND), "property not found");
    }
}
