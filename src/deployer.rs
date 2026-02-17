use crate::scanner::FileAction;
use anyhow::{Context, Result};
use std::path::Path;

#[derive(Debug)]
pub enum DeployResult {
    Created,
    Updated,
    Unchanged,
    Conflict(String),
    DryRun,
}

/// Deploy a single file action to the target directory.
///
/// - `dry_run`: if true, don't actually create/modify anything
/// - `force`: if true, overwrite existing unmanaged files
/// - `rendered_content`: for templates, the pre-rendered content to write
pub fn deploy_file(
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

    // Check for conflicts
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

    if action.is_template {
        let content = rendered_content.unwrap_or("");
        std::fs::write(&target_path, content)
            .with_context(|| format!("failed to write template output: {}", target_path.display()))?;
    } else if action.is_copy {
        std::fs::copy(&action.source, &target_path)
            .with_context(|| format!("failed to copy {} to {}", action.source.display(), target_path.display()))?;
    } else {
        let abs_source = std::fs::canonicalize(&action.source)
            .with_context(|| format!("failed to resolve source path: {}", action.source.display()))?;
        std::os::unix::fs::symlink(&abs_source, &target_path)
            .with_context(|| format!("failed to create symlink: {} -> {}", target_path.display(), abs_source.display()))?;
    }

    Ok(DeployResult::Created)
}
