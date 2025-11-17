use crate::err::Result;
use crate::global_var::ENV_VAR;
use std::path::{Component, Path, PathBuf};

// Normalize a path without touching the filesystem: remove '.' and resolve '..' components.
fn normalize_no_fs<P: AsRef<Path>>(p: P) -> PathBuf {
    let mut out = PathBuf::new();
    let mut is_abs = false;
    for comp in p.as_ref().components() {
        match comp {
            Component::RootDir | Component::Prefix(_) => {
                // Preserve absolute or prefixed roots (Windows prefixes too)
                out.push(comp.as_os_str());
                is_abs = true;
            }
            Component::CurDir => {}
            Component::ParentDir => {
                // Pop one component if possible and not at root
                if out.file_name().is_some() {
                    out.pop();
                }
                // If already at root (absolute), keep as is; for relative, leading .. is kept
                else if !is_abs {
                    out.push("..");
                }
            }
            Component::Normal(s) => out.push(s),
        }
    }
    out
}

// Shared path helpers to eliminate duplication between path checkers.
#[inline]
fn working_dir_path() -> Option<PathBuf> {
    ENV_VAR.get().map(|ev| PathBuf::from(ev.get_working_dir()))
}

#[inline]
fn safe_canon_or_normalize(p: &Path) -> PathBuf {
    std::fs::canonicalize(p).unwrap_or_else(|_| normalize_no_fs(p))
}

#[inline]
fn build_abs_under(base: &Path, p: &Path) -> PathBuf {
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        base.join(p)
    }
}

#[inline]
fn is_under(base: &Path, candidate: &Path) -> bool {
    candidate.starts_with(base)
}

#[inline]
fn check_under_base<P: AsRef<Path>>(base: &Path, p: P) -> bool {
    let base_canon = safe_canon_or_normalize(base);
    let candidate_abs = build_abs_under(base, p.as_ref());
    let candidate_norm = safe_canon_or_normalize(&candidate_abs);
    is_under(&base_canon, &candidate_norm)
}

#[inline]
pub fn check_path_inbound<P: AsRef<Path>>(p: P) -> bool {
    let base = match working_dir_path() {
        Some(b) => b,
        None => return false,
    };
    check_under_base(&base, p)
}

#[inline]
fn check_path_disc_meta<P: AsRef<Path>>(p: P) -> bool {
    let base_meta = match working_dir_path() {
        Some(b) => b.join(".disc"),
        None => return false,
    };
    check_under_base(&base_meta, p)
}

pub async fn async_fs_rename<P: AsRef<Path>>(from_path: P, to_path: P) -> Result<()> {
    // Resolve relative paths against working_dir
    let base = match working_dir_path() {
        Some(b) => b,
        None => return Err("ENV_VAR not initialized".into()),
    };
    let from_abs = build_abs_under(&base, from_path.as_ref());
    let to_abs = build_abs_under(&base, to_path.as_ref());

    // Both paths must be inbound (under working_dir)
    if !check_path_inbound(&from_abs) || !check_path_inbound(&to_abs) {
        return Err("Path not inbound".into());
    }

    tokio::fs::rename(&from_abs, &to_abs).await?;
    Ok(())
}

pub fn fs_rename<P: AsRef<Path>>(from_path: P, to_path: P) -> Result<()> {
    // Resolve relative paths against working_dir
    let base = match working_dir_path() {
        Some(b) => b,
        None => return Err("ENV_VAR not initialized".into()),
    };
    let from_abs = build_abs_under(&base, from_path.as_ref());
    let to_abs = build_abs_under(&base, to_path.as_ref());

    if !check_path_inbound(&from_abs) || !check_path_inbound(&to_abs) {
        return Err("Path not inbound".into());
    }

    std::fs::rename(&from_abs, &to_abs)?;
    Ok(())
}

pub async fn async_fs_copy<P: AsRef<Path>>(from_path: P, to_path: P) -> Result<()> {
    // Resolve relative paths against working_dir
    let base = match working_dir_path() {
        Some(b) => b,
        None => return Err("ENV_VAR not initialized".into()),
    };
    let from_abs = build_abs_under(&base, from_path.as_ref());
    let to_abs = build_abs_under(&base, to_path.as_ref());

    if !check_path_inbound(&from_abs) || !check_path_inbound(&to_abs) {
        return Err("Path not inbound".into());
    }

    tokio::fs::copy(&from_abs, &to_abs).await?;
    Ok(())
}

pub fn fs_copy<P: AsRef<Path>>(from_path: P, to_path: P) -> Result<()> {
    // Resolve relative paths against working_dir
    let base = match working_dir_path() {
        Some(b) => b,
        None => return Err("ENV_VAR not initialized".into()),
    };
    let from_abs = build_abs_under(&base, from_path.as_ref());
    let to_abs = build_abs_under(&base, to_path.as_ref());

    if !check_path_inbound(&from_abs) || !check_path_inbound(&to_abs) {
        return Err("Path not inbound".into());
    }

    std::fs::copy(&from_abs, &to_abs)?;
    Ok(())
}
