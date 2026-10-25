//! Opt-in direct source I/O. This is a bounded file/HTTP(S) transport, not a
//! source parser. Proxy, cookie and credential policies remain explicit host
//! profiles; this direct profile sends no credentials or automatic cookies.
use std::{
    fmt,
    fs::OpenOptions,
    io::{self, Read},
    time::Duration,
};

pub struct SourceBytes {
    pub identity: String,
    pub bytes: Vec<u8>,
}

/// Direct readers can parse before native release, but native `File`/HTTP Drop
/// cannot report close failure. Use `OwnedInput` for a fallible host-close policy.
#[derive(Debug)]
pub enum InputParseError<E> {
    Read(SourceReadError),
    Parse(E),
}
impl<E: fmt::Display> fmt::Display for InputParseError<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Read(e) => e.fmt(f),
            Self::Parse(e) => e.fmt(f),
        }
    }
}
impl<E: std::error::Error + 'static> std::error::Error for InputParseError<E> {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(match self {
            Self::Read(e) => e,
            Self::Parse(e) => e,
        })
    }
}

#[derive(Debug)]
pub enum SourceReadError {
    Options,
    Protocol,
    Credentials,
    File(io::Error),
    Http(Option<u16>),
    Read,
    Bytes,
}
impl fmt::Display for SourceReadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Options => f.write_str("invalid direct-source limits"),
            Self::Protocol => f.write_str("unsupported direct-source URI"),
            Self::Credentials => {
                f.write_str("source URI credentials require a separate host policy")
            }
            Self::File(error) => error.fmt(f),
            Self::Http(Some(status)) => write!(f, "HTTP source returned status {status}"),
            Self::Http(None) => f.write_str("HTTP source could not be loaded"),
            Self::Read => f.write_str("source stream could not be read"),
            Self::Bytes => f.write_str("source byte limit exceeded"),
        }
    }
}
impl std::error::Error for SourceReadError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::File(error) => Some(error),
            _ => None,
        }
    }
}

pub struct DirectSourceReader {
    agent: ureq::Agent,
    maximum: usize,
}
impl DirectSourceReader {
    pub fn new(maximum: usize, timeout: Duration) -> Result<Self, SourceReadError> {
        if maximum > crate::MAX_X12_DOCUMENT_BYTES
            || timeout.is_zero()
            || timeout > Duration::from_secs(300)
        {
            return Err(SourceReadError::Options);
        }
        let config = ureq::Agent::config_builder()
            .timeout_global(Some(timeout))
            .max_redirects(50)
            .proxy(None)
            .user_agent(ureq::config::AutoHeaderValue::None)
            .accept(ureq::config::AutoHeaderValue::None)
            .accept_encoding(ureq::config::AutoHeaderValue::None)
            .build();
        Ok(Self {
            agent: config.into(),
            maximum,
        })
    }
    /// Trim only at loading, preserve the selected identity, and finish reading
    /// and releasing this input before returning bytes to a parser.
    pub fn read(&self, selected: &str) -> Result<SourceBytes, SourceReadError> {
        let identity = selected.trim().to_string();
        let url = identity
            .encode_utf16()
            .position(|unit| unit == b':' as u16)
            .is_some_and(|index| index > 1);
        let bytes = if url {
            let mut uri = url::Url::parse(&identity).map_err(|_| SourceReadError::Protocol)?;
            if !uri.username().is_empty() || uri.password().is_some() {
                return Err(SourceReadError::Credentials);
            }
            match uri.scheme() {
                "file" => self.file(uri.to_file_path().map_err(|_| SourceReadError::Protocol)?)?,
                "http" | "https" => {
                    uri.set_fragment(None);
                    let mut response =
                        self.agent.get(uri.as_str()).call().map_err(|e| match e {
                            ureq::Error::StatusCode(code) => SourceReadError::Http(Some(code)),
                            _ => SourceReadError::Http(None),
                        })?;
                    if !response.status().is_success() {
                        return Err(SourceReadError::Http(Some(response.status().as_u16())));
                    }
                    let legacy_chunked = response.version() == ureq::http::Version::HTTP_10
                        && response
                            .headers()
                            .get("transfer-encoding")
                            .and_then(|v| v.to_str().ok())
                            .and_then(|v| v.split(',').next_back())
                            .is_some_and(|v| v.trim().eq_ignore_ascii_case("chunked"));
                    if legacy_chunked {
                        self.bounded(LegacyChunks::new(response.body_mut().as_reader()))?
                    } else {
                        self.bounded(response.body_mut().as_reader())?
                    }
                }
                _ => return Err(SourceReadError::Protocol),
            }
        } else {
            self.file(std::path::PathBuf::from(&identity))?
        };
        Ok(SourceBytes { identity, bytes })
    }

    /// Read completely and invoke `parse` before the native source is dropped.
    /// This preserves identity and transport limits, but does not report close
    /// failure: standard safe File/HTTP Drop offers no such result. The existing
    /// byte-only `read` still releases the source before returning bytes.
    pub fn read_with<T, E>(
        &self,
        selected: &str,
        parse: impl FnOnce(&str, &[u8]) -> Result<T, E>,
    ) -> Result<T, InputParseError<E>> {
        let identity = selected.trim().to_string();
        let url = identity
            .encode_utf16()
            .position(|u| u == b':' as u16)
            .is_some_and(|i| i > 1);
        if url {
            let mut uri = url::Url::parse(&identity)
                .map_err(|_| InputParseError::Read(SourceReadError::Protocol))?;
            if !uri.username().is_empty() || uri.password().is_some() {
                return Err(InputParseError::Read(SourceReadError::Credentials));
            }
            match uri.scheme() {
                "file" => self.file_with(
                    uri.to_file_path()
                        .map_err(|_| InputParseError::Read(SourceReadError::Protocol))?,
                    &identity,
                    parse,
                ),
                "http" | "https" => {
                    uri.set_fragment(None);
                    let mut response = self.agent.get(uri.as_str()).call().map_err(|e| {
                        InputParseError::Read(match e {
                            ureq::Error::StatusCode(code) => SourceReadError::Http(Some(code)),
                            _ => SourceReadError::Http(None),
                        })
                    })?;
                    if !response.status().is_success() {
                        return Err(InputParseError::Read(SourceReadError::Http(Some(
                            response.status().as_u16(),
                        ))));
                    }
                    let legacy_chunked = response.version() == ureq::http::Version::HTTP_10
                        && response
                            .headers()
                            .get("transfer-encoding")
                            .and_then(|v| v.to_str().ok())
                            .and_then(|v| v.split(',').next_back())
                            .is_some_and(|v| v.trim().eq_ignore_ascii_case("chunked"));
                    let bytes = if legacy_chunked {
                        self.bounded(LegacyChunks::new(response.body_mut().as_reader()))
                    } else {
                        self.bounded(response.body_mut().as_reader())
                    }
                    .map_err(InputParseError::Read)?;
                    let result = parse(&identity, &bytes).map_err(InputParseError::Parse);
                    drop(response);
                    result
                }
                _ => Err(InputParseError::Read(SourceReadError::Protocol)),
            }
        } else {
            self.file_with(std::path::PathBuf::from(&identity), &identity, parse)
        }
    }
    fn file_with<T, E>(
        &self,
        path: std::path::PathBuf,
        identity: &str,
        parse: impl FnOnce(&str, &[u8]) -> Result<T, E>,
    ) -> Result<T, InputParseError<E>> {
        let mut options = OpenOptions::new();
        options.read(true);
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            options.share_mode(1);
        }
        let file = options
            .open(path)
            .map_err(|e| InputParseError::Read(SourceReadError::File(e)))?;
        let bytes = self.bounded(&file).map_err(InputParseError::Read)?;
        let result = parse(identity, &bytes).map_err(InputParseError::Parse);
        drop(file);
        result
    }
    fn file(&self, path: std::path::PathBuf) -> Result<Vec<u8>, SourceReadError> {
        let mut options = OpenOptions::new();
        options.read(true);
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            options.share_mode(1);
        }
        let file = options.open(path).map_err(SourceReadError::File)?;
        self.bounded(file)
    }
    fn bounded(&self, reader: impl Read) -> Result<Vec<u8>, SourceReadError> {
        let mut bytes = Vec::new();
        reader
            .take(self.maximum as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| SourceReadError::Read)?;
        if bytes.len() > self.maximum {
            return Err(SourceReadError::Bytes);
        }
        Ok(bytes)
    }
}

// .NET accepts chunk framing on HTTP/1.0 responses. ureq intentionally treats
// those responses as close-delimited, so decode only that retained wire profile.
struct LegacyChunks<R> {
    inner: R,
    remaining: usize,
    separator: bool,
    done: bool,
}
impl<R: Read> LegacyChunks<R> {
    fn new(inner: R) -> Self {
        Self {
            inner,
            remaining: 0,
            separator: false,
            done: false,
        }
    }
    fn line(&mut self) -> io::Result<Vec<u8>> {
        let mut line = Vec::new();
        loop {
            let mut byte = [0];
            self.inner.read_exact(&mut byte)?;
            line.push(byte[0]);
            if line.len() > 4096 {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "chunk header limit",
                ));
            }
            if line.ends_with(b"\r\n") {
                line.truncate(line.len() - 2);
                return Ok(line);
            }
        }
    }
}
impl<R: Read> Read for LegacyChunks<R> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if buffer.is_empty() || self.done {
            return Ok(0);
        }
        if self.remaining == 0 {
            if self.separator {
                let mut bytes = [0; 2];
                self.inner.read_exact(&mut bytes)?;
                if bytes != *b"\r\n" {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "invalid chunk separator",
                    ));
                }
            }
            let line = self.line()?;
            let size = line.split(|b| *b == b';').next().unwrap();
            if size.is_empty() || !size.iter().all(u8::is_ascii_hexdigit) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "invalid chunk size",
                ));
            }
            let size = std::str::from_utf8(size)
                .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid chunk size"))?;
            self.remaining = usize::from_str_radix(size, 16)
                .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "chunk size overflow"))?;
            if self.remaining == 0 {
                let mut trailers = 0;
                loop {
                    let line = self.line()?;
                    trailers += line.len() + 2;
                    if trailers > 64 * 1024 {
                        return Err(io::Error::new(
                            io::ErrorKind::InvalidData,
                            "chunk trailer limit",
                        ));
                    }
                    if line.is_empty() {
                        break;
                    }
                }
                self.done = true;
                return Ok(0);
            }
            self.separator = true;
        }
        let limit = buffer.len().min(self.remaining);
        let count = self.inner.read(&mut buffer[..limit])?;
        if count == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "incomplete chunk",
            ));
        }
        self.remaining -= count;
        Ok(count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn source_limits_and_credentials_fail_without_a_request() {
        assert!(matches!(
            DirectSourceReader::new(0, Duration::ZERO),
            Err(SourceReadError::Options)
        ));
        let reader = DirectSourceReader::new(0, Duration::from_secs(1)).unwrap();
        assert!(matches!(
            reader.read("http://user:secret@127.0.0.1:1/"),
            Err(SourceReadError::Credentials)
        ));
        assert!(matches!(
            reader.read("ftp://127.0.0.1:1/a"),
            Err(SourceReadError::Protocol)
        ));
        assert!(matches!(
            reader.bounded(&b"x"[..]),
            Err(SourceReadError::Bytes)
        ));
    }

    #[test]
    fn legacy_chunks_are_bounded_and_require_complete_framing() {
        let mut reader =
            LegacyChunks::new(&b"2\r\nhi\r\n1;ignored=yes\r\n!\r\n0\r\nX-Test: value\r\n\r\n"[..]);
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).unwrap();
        assert_eq!(bytes, b"hi!");
        for input in [&b"2\r\nh"[..], &b"1\r\naXX0\r\n\r\n"[..], &b"x\r\n"[..]] {
            assert!(
                LegacyChunks::new(input)
                    .read_to_end(&mut Vec::new())
                    .is_err()
            );
        }
    }
}
