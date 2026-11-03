//! What a bundle check reports. Never a panic: a damaged bundle is reported and the game
//! falls back to the next language in the chain.

use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    /// Informational (a key that only this bundle overrides, an untranslated entry counted).
    Info,
    /// The text works but is probably wrong (a placeholder dropped, a plural form missing).
    Warning,
    /// The entry cannot be used (syntax) or would show wrong text.
    Error,
}

impl Severity {
    pub fn name(self) -> &'static str {
        match self {
            Severity::Info => "info",
            Severity::Warning => "warning",
            Severity::Error => "error",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub severity: Severity,
    pub file: String,
    pub line: u32,
    pub key: String,
    /// A short code: `syntax`, `missing`, `unused`, `placeholder`, `plural`, `reference`,
    /// `duplicate`, `empty`, `meta`.
    pub kind: &'static str,
    pub message: String,
}

impl Diagnostic {
    pub fn new(
        severity: Severity,
        file: impl Into<String>,
        line: u32,
        key: impl Into<String>,
        kind: &'static str,
        message: String,
    ) -> Self {
        Diagnostic {
            severity,
            file: file.into(),
            line,
            key: key.into(),
            kind,
            message,
        }
    }
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: ", self.severity.name())?;
        if !self.file.is_empty() {
            write!(f, "{}", self.file)?;
            if self.line > 0 {
                write!(f, ":{}", self.line)?;
            }
            write!(f, ": ")?;
        }
        if !self.key.is_empty() {
            write!(f, "[{}] ", self.key)?;
        }
        write!(f, "{} ({})", self.message, self.kind)
    }
}
