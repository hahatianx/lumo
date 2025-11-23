use std::fmt::{Debug, Display, Formatter};

pub type Error = Box<dyn std::error::Error + Send + Sync>;

pub struct LumoError {
    err: String,
    file: &'static str,
    line: u32,
    // Store Send + Sync error for thread-safety; we can still expose it as `&dyn Error` in `source()`
    source: Option<Error>,
}

impl LumoError {
    pub fn new(
        err: impl Into<String>,
        file: &'static str,
        line: u32,
        source: Option<Error>,
    ) -> Self {
        Self {
            err: err.into(),
            file,
            line,
            source,
        }
    }
}

#[macro_export]
macro_rules! lumo_error {
    ($fmt:expr $(, $($args:tt)*)?) => {
        crate::err::LumoError::new(
            format!($fmt $(,$($args)*)?),
            file!(), line!(), None)
    };
}

#[macro_export]
macro_rules! lumo_error_with_source {
    ($source:expr, $fmt:expr $(, $($args:tt)*)?) => {
        crate::err::LumoError::new(
            format!($fmt $(,$($args)*)?),
            file!(), line!(), Some(Box::new($source) as crate::err::Error))
    }
}

impl Debug for LumoError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}]:{} {}", self.file, self.line, self.err)
    }
}

impl Display for LumoError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.err)
    }
}

impl std::error::Error for LumoError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.source
            .as_deref()
            .map(|e| e as &(dyn std::error::Error))
    }
}

/// This is defined as a convenience.
pub type Result<T> = std::result::Result<T, Error>;
