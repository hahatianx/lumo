//! Filesystem helpers.
//!
//! Cross-platform directory permission checks for read, write, and execute (traverse).
//!
//! Notes
//! -----
//! - On Unix-like systems:
//!   - Read means being able to list entries (opendir/readdir) -> tested via read_dir.
//!   - Write means being able to create a new file in the directory -> tested via create_new.
//!   - Execute on a directory means being able to traverse (search) it -> approximated via
//!     canonicalize, which requires traverse permissions for each path component.
//! - On Windows, ACL semantics differ, but these concrete operations are a practical way to
//!   probe effective permissions for the current process token.
//!
//! The checks are best-effort and based on attempting real operations. They should work on Linux,
//! macOS, and Windows.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// Result of probing directory permissions for the current process.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DirPermissions {
    pub read: bool,
    pub write: bool,
    pub execute: bool, // "traverse" on Unix; ability to canonicalize/enter the dir
}

impl DirPermissions {
    /// Convenience: all permissions are granted.
    pub const fn all() -> Self {
        Self {
            read: true,
            write: true,
            execute: true,
        }
    }
}

/// Check read, write, and execute (traverse) permissions for a given directory path by
/// attempting real operations.
///
/// This function is cross-platform and does not rely on platform-specific permission bits; it
/// simply tries the operations and reports whether they succeeded.
pub fn check_dir_permissions<P: AsRef<Path>>(dir: P) -> DirPermissions {
    let dir = dir.as_ref();

    // Must exist and be a directory; otherwise, everything is false.
    match fs::metadata(dir) {
        Ok(md) if md.is_dir() => {}
        _ => {
            return DirPermissions {
                read: false,
                write: false,
                execute: false,
            };
        }
    }

    // READ: attempt to open the directory for reading its entries.
    let read_ok = fs::read_dir(dir).is_ok();

    // EXECUTE (traverse): attempt to canonicalize the directory path. This generally requires
    // traverse permissions on Unix-like systems and is a reasonable proxy on Windows.
    let exec_ok = fs::canonicalize(dir).is_ok();

    // WRITE: attempt to create and then remove a unique temporary file inside the directory.
    let write_ok = try_create_ephemeral_file(dir).unwrap_or(false);

    DirPermissions {
        read: read_ok,
        write: write_ok,
        execute: exec_ok,
    }
}

fn try_create_ephemeral_file(dir: &Path) -> io::Result<bool> {
    // Build a fairly unique file name to avoid collisions.
    let mut filename = String::from(".perm_check_");
    filename.push_str(&std::process::id().to_string());
    filename.push('_');
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    filename.push_str(&millis.to_string());
    filename.push_str(".tmp");

    let path: PathBuf = dir.join(filename);

    // Try to create a brand new file (fails if we cannot write or already exists).
    match fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
    {
        Ok(file) => {
            // Ensure the handle is dropped before we delete on Windows.
            drop(file);
            let _ = fs::remove_file(&path); // best-effort cleanup
            Ok(true)
        }
        Err(e) => {
            // If file already exists (very unlikely), try with a random suffix once more.
            if e.kind() == io::ErrorKind::AlreadyExists {
                let alt = path.with_extension("alt.tmp");
                match fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&alt)
                {
                    Ok(f2) => {
                        drop(f2);
                        let _ = fs::remove_file(&alt);
                        Ok(true)
                    }
                    Err(_) => Ok(false),
                }
            } else {
                Ok(false)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn check_current_dir_permissions() {
        let perms = check_dir_permissions(".");
        // We expect at least execute (traverse) to be true for the current working directory
        // in typical test environments. Read and write may vary; don't assert them strictly.
        assert!(
            perms.execute,
            "Expected to be able to traverse current directory"
        );
    }
}
