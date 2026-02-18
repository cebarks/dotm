use crate::scanner::{EntryKind, FileAction};
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

/// Deploy a file action via staging: copy/render the real file into `staging_dir`,
/// then create a symlink from `target_dir` pointing to the staged file.
///
/// For all entry kinds (Base, Override, Template), the staged file is a real file.
/// The target path is always a symlink to the staged file's canonical path.
pub fn deploy_staged(
    action: &FileAction,
    staging_dir: &Path,
    target_dir: &Path,
    dry_run: bool,
    force: bool,
    rendered_content: Option<&str>,
) -> Result<DeployResult> {
    let staged_path = staging_dir.join(&action.target_rel_path);
    let target_path = target_dir.join(&action.target_rel_path);

    if dry_run {
        return Ok(DeployResult::DryRun);
    }

    // Check if the target already exists (managed symlink or file) before removing
    let was_existing = target_path.is_symlink() || target_path.exists();

    // Handle conflicts on the target path
    if target_path.exists() || target_path.is_symlink() {
        if target_path.is_symlink() {
            std::fs::remove_file(&target_path)
                .with_context(|| format!("failed to remove existing symlink: {}", target_path.display()))?;
        } else if force {
            std::fs::remove_file(&target_path)
                .with_context(|| format!("failed to remove existing file: {}", target_path.display()))?;
        } else {
            return Ok(DeployResult::Conflict(format!(
                "file already exists and is not managed by dotm: {}",
                target_path.display()
            )));
        }
    }

    // Create parent directories for both staged and target paths
    if let Some(parent) = staged_path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create staging directory: {}", parent.display()))?;
    }
    if let Some(parent) = target_path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create target directory: {}", parent.display()))?;
    }

    // Stage the file (always a real file in staging_dir)
    match action.kind {
        EntryKind::Template => {
            let content = rendered_content.unwrap_or("");
            std::fs::write(&staged_path, content)
                .with_context(|| format!("failed to write template to staging: {}", staged_path.display()))?;
        }
        EntryKind::Base | EntryKind::Override => {
            std::fs::copy(&action.source, &staged_path)
                .with_context(|| format!("failed to copy {} to staging: {}", action.source.display(), staged_path.display()))?;
            copy_permissions(&action.source, &staged_path)?;
        }
    }

    // Symlink from target to the staged file's canonical path
    let abs_staged = std::fs::canonicalize(&staged_path)
        .with_context(|| format!("failed to canonicalize staged path: {}", staged_path.display()))?;
    std::os::unix::fs::symlink(&abs_staged, &target_path)
        .with_context(|| format!("failed to create symlink: {} -> {}", target_path.display(), abs_staged.display()))?;

    if was_existing {
        Ok(DeployResult::Updated)
    } else {
        Ok(DeployResult::Created)
    }
}

/// Deploy a file action by copying directly to the target directory (no staging).
///
/// Used for packages with `strategy = "copy"`. Templates get rendered content
/// written; everything else is copied. Source permissions are preserved.
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

    // Check if the target already exists before removing
    let was_existing = target_path.is_symlink() || target_path.exists();

    // Handle conflicts on the target path
    if target_path.exists() || target_path.is_symlink() {
        if target_path.is_symlink() {
            std::fs::remove_file(&target_path)
                .with_context(|| format!("failed to remove existing symlink: {}", target_path.display()))?;
        } else if force {
            std::fs::remove_file(&target_path)
                .with_context(|| format!("failed to remove existing file: {}", target_path.display()))?;
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
        EntryKind::Template => {
            let content = rendered_content.unwrap_or("");
            std::fs::write(&target_path, content)
                .with_context(|| format!("failed to write template output: {}", target_path.display()))?;
        }
        EntryKind::Base | EntryKind::Override => {
            std::fs::copy(&action.source, &target_path)
                .with_context(|| format!("failed to copy {} to {}", action.source.display(), target_path.display()))?;
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
