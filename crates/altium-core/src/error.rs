//! Typed errors. Every failure an agent can see has a stable `code`, a human
//! `message`, and (when we know one) a `hint` telling the caller what to do next.

use serde::Serialize;
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ErrorCode {
    /// Bad arguments or an unknown command.
    Usage,
    /// The input file does not exist or cannot be read.
    FileNotFound,
    /// The file exists but is not the expected Altium document type.
    WrongFileType,
    /// The file is an Altium document but its contents could not be parsed.
    Parse,
    /// A named thing (designator, net, stream, footprint...) does not exist.
    NotFound,
    /// A query matched more than one object where exactly one was required.
    Ambiguous,
    /// A write was refused because it would be unsafe (no backup, file locked...).
    Unsafe,
    /// A successful analysis matched an explicit design gate.
    GateFailed,
    /// This operation needs the Designer backend.
    UnsupportedNative,
    /// Altium Designer is not installed, not reachable or not configured.
    DesignerUnavailable,
    /// Altium Designer ran but the bridge script reported a failure.
    DesignerFailed,
    /// Altium Designer did not answer within the timeout.
    Timeout,
    /// Generic I/O failure.
    Io,
    /// A bug in this tool.
    Internal,
}

impl ErrorCode {
    /// Process exit code. Stable, so scripts can branch on it.
    pub fn exit_code(self) -> i32 {
        match self {
            ErrorCode::Usage => 2,
            ErrorCode::FileNotFound => 3,
            ErrorCode::WrongFileType => 4,
            ErrorCode::Parse => 5,
            ErrorCode::NotFound => 6,
            ErrorCode::Ambiguous => 7,
            ErrorCode::Unsafe => 8,
            ErrorCode::GateFailed => 9,
            ErrorCode::UnsupportedNative => 14,
            ErrorCode::DesignerUnavailable => 10,
            ErrorCode::DesignerFailed => 11,
            ErrorCode::Timeout => 12,
            ErrorCode::Io => 13,
            ErrorCode::Internal => 70,
        }
    }

    /// Shared categories across ace and kce; numeric exits are tool-specific.
    pub fn category(self) -> &'static str {
        match self {
            Self::Usage | Self::FileNotFound | Self::WrongFileType | Self::NotFound | Self::Ambiguous | Self::UnsupportedNative => "input",
            Self::Parse | Self::GateFailed => "design",
            Self::Unsafe => "validation",
            Self::DesignerUnavailable | Self::Io => "environment",
            Self::DesignerFailed => "crash",
            Self::Timeout => "timeout",
            Self::Internal => "internal",
        }
    }

    pub fn all() -> &'static [ErrorCode] {
        use ErrorCode::*;
        &[
            Usage,
            FileNotFound,
            WrongFileType,
            Parse,
            NotFound,
            Ambiguous,
            Unsafe,
            GateFailed,
            UnsupportedNative,
            DesignerUnavailable,
            DesignerFailed,
            Timeout,
            Io,
            Internal,
        ]
    }

    pub fn name(self) -> String {
        serde_json::to_value(self)
            .ok()
            .and_then(|v| v.as_str().map(str::to_owned))
            .unwrap_or_default()
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Error {
    pub code: ErrorCode,
    pub category: &'static str,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<Box<serde_json::Value>>,
}

impl Error {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Error { code, category: code.category(), message: message.into(), hint: None, details: None }
    }
    pub fn hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = Some(hint.into());
        self
    }
    pub fn details(mut self, details: serde_json::Value) -> Self {
        self.details = Some(Box::new(details));
        self
    }

    pub fn usage(m: impl Into<String>) -> Self {
        Self::new(ErrorCode::Usage, m)
    }
    pub fn parse(m: impl Into<String>) -> Self {
        Self::new(ErrorCode::Parse, m)
    }
    pub fn not_found(m: impl Into<String>) -> Self {
        Self::new(ErrorCode::NotFound, m)
    }
    pub fn internal(m: impl Into<String>) -> Self {
        Self::new(ErrorCode::Internal, m)
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code.name(), self.message)?;
        if let Some(h) = &self.hint {
            write!(f, " (hint: {h})")?;
        }
        Ok(())
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        let code = if e.kind() == std::io::ErrorKind::NotFound {
            ErrorCode::FileNotFound
        } else {
            ErrorCode::Io
        };
        Error::new(code, e.to_string())
    }
}

impl From<serde_json::Error> for Error {
    fn from(e: serde_json::Error) -> Self {
        Error::internal(format!("json: {e}"))
    }
}

pub type Result<T> = std::result::Result<T, Error>;
