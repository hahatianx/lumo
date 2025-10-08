
pub type Error = Box<dyn std::error::Error + Send + Sync>;

/// This is defined as a convenience.
pub type Result<T> = std::result::Result<T, Error>;