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
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::fs::OpenOptions;
use tokio::time::sleep;

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

pub fn expand_tilde(path: &str) -> String {
    // Expand leading "~/" to $HOME, and handle "~" alone. Leave other forms unchanged.
    if let Some(rest) = path.strip_prefix("~/") {
        if let Ok(home) = std::env::var("HOME") {
            // Preserve exact formatting semantics used in tests: `${HOME}/${rest}`
            // This intentionally does not normalize duplicate slashes, so if HOME ends with
            // a trailing slash, the resulting path may contain a double slash, matching
            // expectations like `format!("{}/{}", home, rest)`.
            return format!("{}/{}", home, rest);
        }
        // If HOME is not set, leave the original path unchanged.
        return path.to_string();
    }
    if path == "~" {
        if let Ok(home) = std::env::var("HOME") {
            return home;
        }
        return path.to_string();
    }
    path.to_string()
}

pub fn test_dir_existence<P: AsRef<Path>>(dir: P) -> bool {
    dir.as_ref().exists() && dir.as_ref().is_dir()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;
    use std::env;
    use std::fs;
    use std::path::Path;

    // RAII guard to ensure the temporary directory tree is deleted on drop,
    // even if the test fails/panics early.
    struct TempDirGuard(std::path::PathBuf);
    impl TempDirGuard {
        fn new(prefix: &str) -> Self {
            let mut p = std::env::temp_dir();
            let ts = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis();
            p.push(format!("{}_{}_{}", prefix, std::process::id(), ts));
            fs::create_dir_all(&p).unwrap();
            TempDirGuard(p)
        }
        fn path(&self) -> &std::path::Path {
            &self.0
        }
    }
    impl Drop for TempDirGuard {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

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

    #[test]
    fn check_permissions_nonexistent_dir_all_false() {
        let mut p = std::env::temp_dir();
        p.push(format!(
            "no_such_dir_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis()
        ));
        if p.exists() {
            let _ = std::fs::remove_dir_all(&p);
        }
        let perms = check_dir_permissions(&p);
        assert!(!perms.read && !perms.write && !perms.execute);
    }

    #[test]
    fn check_permissions_writable_temp_dir_has_write() {
        let tmp = TempDirGuard::new("perms_ok");
        let p = tmp.path();
        let perms = check_dir_permissions(p);
        assert!(
            perms.write,
            "Expected write permission in temp dir: {:?}",
            perms
        );
        // Best-effort explicit cleanup is handled by TempDirGuard::drop
    }

    #[test]
    #[serial]
    fn expand_tilde_expands_when_home_set() {
        // Save current HOME
        let original_home = env::var("HOME").ok();
        let temp_home = "/tmp/junie_home_test";
        unsafe {
            env::set_var("HOME", temp_home);
        }

        let input = "~/sub/dir";
        let resolved = expand_tilde(input);
        assert_eq!(resolved, format!("{}/{}", temp_home, "sub/dir"));

        // Restore HOME
        match original_home {
            Some(val) => unsafe {
                env::set_var("HOME", val);
            },
            None => unsafe {
                env::remove_var("HOME");
            },
        }
    }

    #[test]
    fn expand_tilde_leaves_non_tilde_paths_unchanged() {
        assert_eq!(expand_tilde("/absolute/path"), "/absolute/path");
        assert_eq!(expand_tilde("relative/path"), "relative/path");
        assert_eq!(expand_tilde("~not/home"), "~not/home");
    }

    #[test]
    #[serial]
    fn expand_tilde_unset_home_leaves_tilde_path() {
        // Save and unset HOME
        let original_home = env::var("HOME").ok();
        unsafe {
            env::remove_var("HOME");
        }

        let input = "~/file";
        let resolved = expand_tilde(input);
        // Without HOME, function should leave the path unchanged
        assert_eq!(resolved, input);

        // Restore HOME
        match original_home {
            Some(val) => unsafe {
                env::set_var("HOME", val);
            },
            None => unsafe {
                env::remove_var("HOME");
            },
        }
    }

    #[test]
    fn test_dir_existence_true_for_current_dir() {
        assert!(test_dir_existence("."));
    }

    #[test]
    fn test_dir_existence_false_for_existing_file() {
        // Create a temporary directory and a file inside it
        let tmp = TempDirGuard::new("junie_exist_file");
        let p = tmp.path().join("file.tmp");
        std::fs::write(&p, b"x").unwrap();
        // test_dir_existence returns true only for directories, not files
        assert!(!test_dir_existence(p.to_str().unwrap()));
        // Best-effort explicit cleanup (Drop will remove the directory tree)
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn test_dir_existence_false_for_nonexistent_path() {
        let mut p = std::env::temp_dir();
        p.push(format!(
            "junie_nonexistent_{}_{}.nope",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis()
        ));
        // Ensure path does not exist
        if p.exists() {
            let _ = std::fs::remove_file(&p);
        }
        assert!(!test_dir_existence(p.to_str().unwrap()));
    }
}
