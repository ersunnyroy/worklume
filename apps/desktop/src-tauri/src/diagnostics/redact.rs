//! Shortens paths under the member's home folder to `~`, so logs and crash reports do not carry
//! the account name (`/Users/alex`, `C:\Users\alex`).

use std::io::{self, Write};
use std::path::Path;
use std::sync::Arc;

use tracing_subscriber::fmt::MakeWriter;

/// Replaces the home folder, as written and as JSON escapes it, with `~`.
#[derive(Debug, Clone, Default)]
pub struct HomeRedactor {
    /// Longest first, so the JSON-escaped form (longer on Windows) wins.
    patterns: Arc<[String]>,
}

impl HomeRedactor {
    /// A redactor for `home`. A missing or trivially short home folder redacts nothing.
    #[must_use]
    pub fn new(home: Option<&Path>) -> Self {
        let Some(home) = home.and_then(Path::to_str) else {
            return Self::default();
        };
        let home = home.trim_end_matches(['/', '\\']);
        // "/" or "C:" would match far too much.
        if home.len() < 3 {
            return Self::default();
        }
        let mut patterns = vec![home.to_owned()];
        let escaped = home.replace('\\', "\\\\").replace('"', "\\\"");
        if escaped != home {
            patterns.push(escaped);
        }
        patterns.sort_by_key(|pattern| std::cmp::Reverse(pattern.len()));
        Self {
            patterns: patterns.into(),
        }
    }

    /// The redactor for the current user's home folder.
    #[must_use]
    pub fn current() -> Self {
        Self::new(dirs::home_dir().as_deref())
    }

    /// Returns `text` with the home folder replaced.
    #[must_use]
    pub fn redact(&self, text: &str) -> String {
        let mut out = text.to_owned();
        for pattern in self.patterns.iter() {
            if out.contains(pattern.as_str()) {
                out = out.replace(pattern.as_str(), "~");
            }
        }
        out
    }

    fn matches(&self, text: &str) -> bool {
        self.patterns
            .iter()
            .any(|pattern| text.contains(pattern.as_str()))
    }
}

/// Wraps a tracing writer so every formatted line is redacted before it is written.
#[derive(Debug, Clone)]
pub struct RedactingMakeWriter<M> {
    inner: M,
    redactor: HomeRedactor,
}

impl<M> RedactingMakeWriter<M> {
    pub fn new(inner: M, redactor: HomeRedactor) -> Self {
        Self { inner, redactor }
    }
}

impl<'a, M: MakeWriter<'a>> MakeWriter<'a> for RedactingMakeWriter<M> {
    type Writer = RedactingWriter<M::Writer>;

    fn make_writer(&'a self) -> Self::Writer {
        RedactingWriter {
            inner: self.inner.make_writer(),
            redactor: self.redactor.clone(),
        }
    }
}

/// The formatter writes each event with a single `write_all`, so each call holds whole lines.
#[derive(Debug)]
pub struct RedactingWriter<W> {
    inner: W,
    redactor: HomeRedactor,
}

impl<W: Write> Write for RedactingWriter<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match std::str::from_utf8(buf) {
            Ok(text) if self.redactor.matches(text) => {
                self.inner
                    .write_all(self.redactor.redact(text).as_bytes())?;
            }
            _ => self.inner.write_all(buf)?,
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}
