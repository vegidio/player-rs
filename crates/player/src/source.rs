//! Media to load: a URL or a local file.

use crate::error::{Error, Result};
use std::fmt;
use std::path::Path;

/// Media for [`Player::load`](crate::Player::load): a URL or a local file.
///
/// Creating a source only checks its form. Whether the media exists and can be played is reported later, through
/// [`Event::Ended`](crate::Event::Ended) with [`EndReason::Error`](crate::EndReason::Error).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Source(String);

impl Source {
    /// A URL: a scheme followed by `://`, such as `https://example.com/movie.mp4`, or one of mpv's input-device URLs
    /// such as `av://lavfi:testsrc`.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidSource`], naming the rejected string, if it does not start with a scheme (letters,
    /// digits, `+`, `-` or `.`, starting with a letter) followed by `://`, or if it contains a NUL byte.
    pub fn url(url: &str) -> Result<Self> {
        let scheme = url.split_once("://").map(|(scheme, _)| scheme);
        let valid = scheme.is_some_and(|scheme| {
            let mut chars = scheme.chars();
            chars.next().is_some_and(|c| c.is_ascii_alphabetic())
                && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
        });

        if !valid {
            return Err(Error::InvalidSource(format!("'{}' is not a URL (expected scheme://...)", url.escape_debug())));
        }
        if url.contains('\0') {
            return Err(Error::InvalidSource(format!("'{}' contains a NUL byte", url.escape_debug())));
        }
        Ok(Self(url.to_owned()))
    }

    /// A local file. The path is made absolute (without touching the filesystem), so it is always played as a file,
    /// even if its name looks like a URL, such as a file `clip.mp4` inside a directory named `http:`.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidSource`], naming the rejected path, if it is not valid UTF-8 (mpv takes UTF-8 paths on
    /// every platform), contains a NUL byte, or cannot be made absolute (for example because it is empty).
    pub fn file(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let invalid = |reason: &str| Error::InvalidSource(format!("'{}' {reason}", path.display()));

        if path.to_str().is_none() {
            return Err(invalid("is not valid UTF-8"));
        }
        let absolute = std::path::absolute(path).map_err(|e| invalid(&format!("cannot be made absolute: {e}")))?;
        let absolute = absolute.into_os_string().into_string().map_err(|_| invalid("is not valid UTF-8"))?;
        if absolute.contains('\0') {
            return Err(invalid("contains a NUL byte"));
        }
        Ok(Self(absolute))
    }

    /// The URL or absolute path passed to mpv.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Source {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_urls() {
        for url in ["https://example.com/movie.mp4", "av://lavfi:testsrc", "rtmp+tls://host/live", "a.b-c://x"] {
            assert_eq!(Source::url(url).unwrap().as_str(), url);
        }
    }

    #[test]
    fn rejects_non_urls() {
        for url in ["", "movie.mp4", "://host", "1http://host", "ht tp://host", "/tmp/movie.mp4", "c:\\movie.mp4"] {
            let Err(Error::InvalidSource(message)) = Source::url(url) else {
                panic!("{url:?} was accepted")
            };
            assert!(message.contains(&format!("'{url}'").replace('\\', "\\\\")), "{message}");
        }
    }

    // On Windows `http:` is not a valid directory name, so this case only exists on Unix.
    #[cfg(unix)]
    #[test]
    fn makes_files_absolute() {
        let source = Source::file("http://clip.mp4").unwrap();
        let path = Path::new(source.as_str());
        assert!(path.is_absolute());
        assert!(path.ends_with(Path::new("http:").join("clip.mp4")));
    }

    #[test]
    fn rejects_empty_file() {
        assert!(matches!(Source::file(""), Err(Error::InvalidSource(_))));
    }

    #[cfg(unix)]
    #[test]
    fn rejects_non_utf8_file() {
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt;

        let Err(Error::InvalidSource(message)) = Source::file(OsStr::from_bytes(b"movie-\xff.mp4")) else {
            panic!("a non-UTF-8 path was accepted")
        };
        assert!(message.contains("movie-"), "{message}");
    }
}
