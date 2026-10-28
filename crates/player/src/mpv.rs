//! The only place that calls libmpv: a thin, private layer over the raw [`sys`](crate::sys) bindings.
//!
//! It owns the `mpv_handle`, converts Rust strings to C strings, maps return codes to [`Error`]s and decodes events
//! into owned values, so the rest of the crate contains no `unsafe`.

use crate::error::{Error, Result, mpv_message};
use crate::sys;
use std::ffi::{CStr, CString, c_char, c_int, c_void};
use std::ptr::{self, NonNull};

pub(crate) mod render;

/// Owns one `mpv_handle` and destroys it (`mpv_terminate_destroy`) when dropped.
///
/// Nominally `pub` only because the sealed [`PropertyValue`] methods take it; the module is private.
pub struct Handle(NonNull<sys::mpv_handle>);

// SAFETY: every libmpv client API function is thread-safe on a handle, except `mpv_wait_event`, which
// `Handle::wait_event` requires to be called from one thread at a time (see its safety contract).
unsafe impl Send for Handle {}
// SAFETY: as above.
unsafe impl Sync for Handle {}

/// A decoded `mpv_event`. Payloads are copied out, so the value stays valid after the next `mpv_wait_event`.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum MpvEvent {
    /// `mpv_wait_event` timed out or was woken by `mpv_wakeup`.
    None,
    Shutdown,
    PropertyChange {
        id: u64,
        value: PropertyData,
    },
    StartFile,
    FileLoaded,
    EndFile {
        reason: c_int,
        error: c_int,
    },
    Seek,
    PlaybackRestart,
    CommandReply {
        id: u64,
        error: c_int,
    },
    /// Any event this crate does not handle.
    Other,
}

/// The value carried by a property-change event, for the formats this crate observes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum PropertyData {
    /// The property is currently unavailable (for example `duration` with no media loaded).
    None,
    Flag(bool),
    Double(f64),
}

impl Handle {
    /// Creates an uninitialised mpv instance, or `None` if `mpv_create` failed.
    pub(crate) fn create() -> Option<Self> {
        // SAFETY: `mpv_create` has no preconditions; it returns null on failure.
        NonNull::new(unsafe { sys::mpv_create() }).map(Self)
    }

    fn as_ptr(&self) -> *mut sys::mpv_handle {
        self.0.as_ptr()
    }

    /// Sets an option with `mpv_set_option_string`. Before [`initialize`](Self::initialize) this also accepts
    /// options that cannot be changed at runtime, such as `vo`.
    pub(crate) fn set_option(&self, name: &str, value: &str) -> Result<()> {
        let invalid = |message| Error::InvalidOption { name: name.to_owned(), message };
        let c_name = c_string(name).map_err(invalid)?;
        let c_value = c_string(value).map_err(invalid)?;
        // SAFETY: the handle is valid and both strings are NUL-terminated and outlive the call; mpv copies them.
        let code = unsafe { sys::mpv_set_option_string(self.as_ptr(), c_name.as_ptr(), c_value.as_ptr()) };
        check(code).map_err(|code| invalid(mpv_message(code)))
    }

    /// Initialises the instance, returning mpv's message on failure.
    pub(crate) fn initialize(&self) -> std::result::Result<(), String> {
        // SAFETY: the handle is valid; `mpv_initialize` may be called once on an uninitialised handle.
        check(unsafe { sys::mpv_initialize(self.as_ptr()) }).map_err(mpv_message)
    }

    /// Reads a property.
    pub(crate) fn get<T: PropertyValue>(&self, name: &str) -> Result<T> {
        let failed = |message| Error::Property { name: name.to_owned(), message };
        let c_name = c_string(name).map_err(failed)?;
        T::get(self, &c_name).map_err(|code| failed(mpv_message(code)))
    }

    /// Writes a property.
    pub(crate) fn set<T: PropertyValue>(&self, name: &str, value: T) -> Result<()> {
        let failed = |message| Error::Property { name: name.to_owned(), message };
        let c_name = c_string(name).map_err(failed)?;
        value.set(self, &c_name).map_err(|code| failed(mpv_message(code)))
    }

    /// Runs a command and waits for it to finish.
    pub(crate) fn command(&self, args: &[&str]) -> Result<()> {
        // SAFETY: `argv` is a NULL-terminated array of NUL-terminated strings that outlive the call.
        self.with_argv(args, |argv| unsafe { sys::mpv_command(self.as_ptr(), argv) })
    }

    /// Starts a command without waiting for it. Its outcome arrives as an [`MpvEvent::CommandReply`] carrying `id`.
    pub(crate) fn command_async(&self, id: u64, args: &[&str]) -> Result<()> {
        // SAFETY: as in `command`; mpv copies the arguments before returning.
        self.with_argv(args, |argv| unsafe { sys::mpv_command_async(self.as_ptr(), id, argv) })
    }

    fn with_argv(&self, args: &[&str], call: impl FnOnce(*mut *const c_char) -> c_int) -> Result<()> {
        let name = args.first().copied().unwrap_or_default();
        let failed = |message| Error::Command { name: name.to_owned(), message };
        let owned = args.iter().map(|arg| c_string(arg)).collect::<std::result::Result<Vec<_>, _>>();
        let owned = owned.map_err(failed)?;
        let mut argv: Vec<*const c_char> = owned.iter().map(|arg| arg.as_ptr()).chain([ptr::null()]).collect();
        check(call(argv.as_mut_ptr())).map_err(|code| failed(mpv_message(code)))
    }

    /// Subscribes to changes of a property, delivered as [`MpvEvent::PropertyChange`] with `id`. Only
    /// `MPV_FORMAT_FLAG` and `MPV_FORMAT_DOUBLE` are decoded.
    pub(crate) fn observe(&self, id: u64, name: &str, format: sys::mpv_format) -> std::result::Result<(), String> {
        let c_name = c_string(name)?;
        // SAFETY: the handle is valid and the name is NUL-terminated and outlives the call; mpv copies it.
        check(unsafe { sys::mpv_observe_property(self.as_ptr(), id, c_name.as_ptr(), format) }).map_err(mpv_message)
    }

    /// Waits up to `timeout` seconds (negative: forever) for the next event.
    ///
    /// # Safety
    ///
    /// Must not be called from more than one thread at a time: libmpv reuses the returned event's memory on the next
    /// call, which is why the payload is copied out before returning.
    pub(crate) unsafe fn wait_event(&self, timeout: f64) -> MpvEvent {
        // SAFETY: the handle is valid; the caller guarantees no concurrent `mpv_wait_event`. The returned pointer is
        // never null and stays valid until the next call on this handle, which cannot happen during this function.
        let event = unsafe { &*sys::mpv_wait_event(self.as_ptr(), timeout) };
        match event.event_id {
            sys::MPV_EVENT_NONE => MpvEvent::None,
            sys::MPV_EVENT_SHUTDOWN => MpvEvent::Shutdown,
            sys::MPV_EVENT_START_FILE => MpvEvent::StartFile,
            sys::MPV_EVENT_FILE_LOADED => MpvEvent::FileLoaded,
            sys::MPV_EVENT_SEEK => MpvEvent::Seek,
            sys::MPV_EVENT_PLAYBACK_RESTART => MpvEvent::PlaybackRestart,
            sys::MPV_EVENT_COMMAND_REPLY => MpvEvent::CommandReply { id: event.reply_userdata, error: event.error },
            sys::MPV_EVENT_END_FILE => {
                // SAFETY: for MPV_EVENT_END_FILE, `data` points to an `mpv_event_end_file`.
                let end = unsafe { &*event.data.cast::<sys::mpv_event_end_file>() };
                MpvEvent::EndFile { reason: end.reason, error: end.error }
            }
            sys::MPV_EVENT_PROPERTY_CHANGE => {
                // SAFETY: for MPV_EVENT_PROPERTY_CHANGE, `data` points to an `mpv_event_property`.
                let property = unsafe { &*event.data.cast::<sys::mpv_event_property>() };
                // SAFETY: for these formats `data` points to a value of the matching C type (`int` for FLAG,
                // `double` for DOUBLE).
                let value = unsafe {
                    match property.format {
                        sys::MPV_FORMAT_FLAG => PropertyData::Flag(*property.data.cast::<c_int>() != 0),
                        sys::MPV_FORMAT_DOUBLE => PropertyData::Double(*property.data.cast::<f64>()),
                        _ => PropertyData::None,
                    }
                };
                MpvEvent::PropertyChange { id: event.reply_userdata, value }
            }
            _ => MpvEvent::Other,
        }
    }

    /// Makes a pending or the next [`wait_event`](Self::wait_event) return [`MpvEvent::None`].
    pub(crate) fn wakeup(&self) {
        // SAFETY: the handle is valid; `mpv_wakeup` is thread-safe.
        unsafe { sys::mpv_wakeup(self.as_ptr()) }
    }
}

impl Drop for Handle {
    fn drop(&mut self) {
        // SAFETY: the handle is valid and owned by this value, so no other call can follow; the owner guarantees no
        // other thread is inside a call on it (the event thread is joined first). Uninitialised handles are allowed.
        unsafe { sys::mpv_terminate_destroy(self.as_ptr()) }
    }
}

/// A value that can be read from or written to an mpv property: `bool`, `i64`, `f64` or `String`, mapped to mpv's
/// `FLAG`, `INT64`, `DOUBLE` and `STRING` formats.
///
/// This trait is sealed: it cannot be implemented outside this crate.
pub trait PropertyValue: sealed::Sealed {}

mod sealed {
    use super::Handle;
    use std::ffi::{CStr, c_int};

    pub trait Sealed: Sized {
        fn get(handle: &Handle, name: &CStr) -> Result<Self, c_int>;
        fn set(self, handle: &Handle, name: &CStr) -> Result<(), c_int>;
    }
}

/// Implements [`PropertyValue`] for a type passed to mpv as-is, through a C value of type `$c`.
macro_rules! plain_property {
    ($ty:ty, $c:ty, $format:expr, $to_c:expr, $from_c:expr) => {
        impl PropertyValue for $ty {}

        impl sealed::Sealed for $ty {
            fn get(handle: &Handle, name: &CStr) -> std::result::Result<Self, c_int> {
                let mut value = <$c>::default();
                // SAFETY: the handle is valid, the name is NUL-terminated, and for this format mpv writes exactly
                // one `$c` into `value`.
                check(unsafe {
                    sys::mpv_get_property(handle.as_ptr(), name.as_ptr(), $format, (&raw mut value).cast())
                })?;
                Ok($from_c(value))
            }

            fn set(self, handle: &Handle, name: &CStr) -> std::result::Result<(), c_int> {
                let mut value: $c = $to_c(self);
                // SAFETY: the handle is valid, the name is NUL-terminated, and for this format mpv reads exactly one
                // `$c` from `value`, which outlives the call.
                check(unsafe {
                    sys::mpv_set_property(handle.as_ptr(), name.as_ptr(), $format, (&raw mut value).cast())
                })
            }
        }
    };
}

plain_property!(bool, c_int, sys::MPV_FORMAT_FLAG, c_int::from, |value: c_int| value != 0);
plain_property!(i64, i64, sys::MPV_FORMAT_INT64, std::convert::identity, std::convert::identity);
plain_property!(f64, f64, sys::MPV_FORMAT_DOUBLE, std::convert::identity, std::convert::identity);

impl PropertyValue for String {}

impl sealed::Sealed for String {
    fn get(handle: &Handle, name: &CStr) -> std::result::Result<Self, c_int> {
        let mut value: *mut c_char = ptr::null_mut();
        // SAFETY: the handle is valid, the name is NUL-terminated, and for MPV_FORMAT_STRING mpv writes one
        // `char *` it allocated into `value`.
        check(unsafe {
            sys::mpv_get_property(handle.as_ptr(), name.as_ptr(), sys::MPV_FORMAT_STRING, (&raw mut value).cast())
        })?;
        // SAFETY: on success `value` is a NUL-terminated string allocated by mpv.
        let string = unsafe { CStr::from_ptr(value) }.to_string_lossy().into_owned();
        // SAFETY: `value` was allocated by mpv and is not used after this point.
        unsafe { sys::mpv_free(value.cast::<c_void>()) };
        Ok(string)
    }

    fn set(self, handle: &Handle, name: &CStr) -> std::result::Result<(), c_int> {
        let value = CString::new(self).map_err(|_| sys::MPV_ERROR_INVALID_PARAMETER)?;
        let mut pointer = value.as_ptr();
        // SAFETY: the handle is valid, the name is NUL-terminated, and for MPV_FORMAT_STRING mpv reads one
        // `const char *` from `pointer`; the string it points to outlives the call and mpv copies it.
        check(unsafe {
            sys::mpv_set_property(handle.as_ptr(), name.as_ptr(), sys::MPV_FORMAT_STRING, (&raw mut pointer).cast())
        })
    }
}

/// Converts a string for mpv, rejecting interior NUL bytes.
fn c_string(value: &str) -> std::result::Result<CString, String> {
    CString::new(value).map_err(|_| format!("'{}' contains a NUL byte", value.escape_debug()))
}

/// Maps an mpv return code: negative is an error, anything else success.
fn check(code: c_int) -> std::result::Result<(), c_int> {
    if code < 0 { Err(code) } else { Ok(()) }
}
