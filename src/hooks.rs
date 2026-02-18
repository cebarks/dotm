use anyhow::{bail, Result};
use std::path::Path;
use std::process::Command;

/// Run a hook command via `sh -c`. Empty hooks are no-ops.
/// Sets DOTM_PACKAGE, DOTM_TARGET, DOTM_ACTION environment variables.
pub fn run_hook(command: &str, cwd: &Path, package: &str, action: &str) -> Result<()> {
    if command.is_empty() {
        return Ok(());
    }

    let status = Command::new("sh")
        .arg("-c")
        .arg(command)
        .current_dir(cwd)
        .env("DOTM_PACKAGE", package)
        .env("DOTM_TARGET", cwd.to_str().unwrap_or(""))
        .env("DOTM_ACTION", action)
        .status()?;

    if !status.success() {
        bail!(
            "hook failed for package '{}' ({}): command '{}' exited with {}",
            package,
            action,
            command,
            status
        );
    }

    Ok(())
}
