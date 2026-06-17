use crate::scanner::FileAction;
use anyhow::{Context, Result};
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

#[derive(Debug)]
pub enum DeployResult {
    Created,
    Updated,
    Unchanged,
    Conflict(String),
    DryRun,
}

/// Deploy a file by creating a symlink from target to the source file.
///
/// Used for user-mode Base and Override files. The symlink points to the
/// canonicalized absolute path of the source file in packages/.
pub fn deploy_symlink(
    action: &FileAction,
    target_dir: &Path,
    dry_run: bool,
    force: bool,
) -> Result<DeployResult> {
    let target_path = target_dir.join(&action.target_rel_path);

    if dry_run {
        return Ok(DeployResult::DryRun);
    }

    let was_existing = target_path.is_symlink() || target_path.exists();

    // Directory at target is an error
    if target_path.is_dir() && !target_path.is_symlink() {
        return Ok(DeployResult::Conflict(format!(
            "target is a directory (remove it manually): {}",
            target_path.display()
        )));
    }

    // Existing symlink (broken or pointing elsewhere): remove it
    if target_path.is_symlink() {
        std::fs::remove_file(&target_path).with_context(|| {
            format!(
                "failed to remove existing symlink: {}",
                target_path.display()
            )
        })?;
    } else if target_path.exists() {
        // Regular file: conflict unless force
        if force {
            std::fs::remove_file(&target_path).with_context(|| {
                format!("failed to remove existing file: {}", target_path.display())
            })?;
        } else {
            return Ok(DeployResult::Conflict(format!(
                "file already exists and is not managed by dotm: {}",
                target_path.display()
            )));
        }
    }

    // Create parent directories
    if let Some(parent) = target_path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create target directory: {}", parent.display()))?;
    }

    // Create symlink to canonicalized source path
    let abs_source = std::fs::canonicalize(&action.source).with_context(|| {
        format!(
            "failed to canonicalize source path: {}",
            action.source.display()
        )
    })?;
    std::os::unix::fs::symlink(&abs_source, &target_path).with_context(|| {
        format!(
            "failed to create symlink: {} -> {}",
            target_path.display(),
            abs_source.display()
        )
    })?;

    if was_existing {
        Ok(DeployResult::Updated)
    } else {
        Ok(DeployResult::Created)
    }
}

/// Deploy a file by copying content directly to the target.
///
/// Used for templates (rendered content) and system-mode files.
/// Templates get rendered content written; base/override files are copied from source.
pub fn deploy_copy(
    action: &FileAction,
    target_dir: &Path,
    dry_run: bool,
    force: bool,
    rendered_content: Option<&str>,
) -> Result<DeployResult> {
    let target_path = target_dir.join(&action.target_rel_path);

    if dry_run {
        return Ok(DeployResult::DryRun);
    }

    let was_existing = target_path.is_symlink() || target_path.exists();

    // Directory at target is an error
    if target_path.is_dir() && !target_path.is_symlink() {
        return Ok(DeployResult::Conflict(format!(
            "target is a directory (remove it manually): {}",
            target_path.display()
        )));
    }

    if target_path.exists() || target_path.is_symlink() {
        if target_path.is_symlink() {
            std::fs::remove_file(&target_path).with_context(|| {
                format!(
                    "failed to remove existing symlink: {}",
                    target_path.display()
                )
            })?;
        } else if force {
            std::fs::remove_file(&target_path).with_context(|| {
                format!("failed to remove existing file: {}", target_path.display())
            })?;
        } else {
            return Ok(DeployResult::Conflict(format!(
                "file already exists and is not managed by dotm: {}",
                target_path.display()
            )));
        }
    }

    // Create parent directories
    if let Some(parent) = target_path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create directory: {}", parent.display()))?;
    }

    match action.kind {
        crate::scanner::EntryKind::Template => {
            let content = rendered_content.context("template has no rendered content")?;
            std::fs::write(&target_path, content).with_context(|| {
                format!("failed to write template output: {}", target_path.display())
            })?;
        }
        crate::scanner::EntryKind::Base | crate::scanner::EntryKind::Override => {
            std::fs::copy(&action.source, &target_path).with_context(|| {
                format!(
                    "failed to copy {} to {}",
                    action.source.display(),
                    target_path.display()
                )
            })?;
            copy_permissions(&action.source, &target_path)?;
        }
    }

    if was_existing {
        Ok(DeployResult::Updated)
    } else {
        Ok(DeployResult::Created)
    }
}

/// Parse an octal mode string (e.g. "755") and apply it to the file at `path`.
pub fn apply_permission_override(path: &Path, mode_str: &str) -> Result<()> {
    let mode = u32::from_str_radix(mode_str, 8)
        .with_context(|| format!("invalid octal permission string: '{mode_str}'"))?;
    let permissions = std::fs::Permissions::from_mode(mode);
    std::fs::set_permissions(path, permissions)
        .with_context(|| format!("failed to set permissions {mode_str} on {}", path.display()))?;
    Ok(())
}

/// Copy the Unix file permissions from `source` to `dest`.
fn copy_permissions(source: &Path, dest: &Path) -> Result<()> {
    let metadata = std::fs::metadata(source)
        .with_context(|| format!("failed to read metadata from {}", source.display()))?;
    std::fs::set_permissions(dest, metadata.permissions())
        .with_context(|| format!("failed to set permissions on {}", dest.display()))?;
    Ok(())
}
