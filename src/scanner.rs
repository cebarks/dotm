use anyhow::{Context, Result};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Describes what to do with a single file during deployment.
#[derive(Debug)]
pub struct FileAction {
    /// The source file in the dotfiles repo
    pub source: PathBuf,
    /// The relative path where this file should be deployed (relative to target dir)
    pub target_rel_path: PathBuf,
    /// Whether to copy (true) or symlink (false)
    pub is_copy: bool,
    /// Whether this file is a Tera template that needs rendering
    pub is_template: bool,
}

/// Scan a package directory and resolve overrides for the given host and roles.
///
/// Returns a list of FileActions describing what to deploy.
pub fn scan_package(pkg_dir: &Path, hostname: &str, roles: &[&str]) -> Result<Vec<FileAction>> {
    let mut files: HashMap<PathBuf, Vec<PathBuf>> = HashMap::new();

    collect_files(pkg_dir, pkg_dir, &mut files)
        .with_context(|| format!("failed to scan package directory: {}", pkg_dir.display()))?;

    let mut actions = Vec::new();

    for (target_path, variants) in &files {
        let action = resolve_variant(target_path, variants, hostname, roles);
        actions.push(action);
    }

    actions.sort_by(|a, b| a.target_rel_path.cmp(&b.target_rel_path));
    Ok(actions)
}

/// Recursively collect files, grouping override variants by their canonical path.
fn collect_files(
    base: &Path,
    dir: &Path,
    files: &mut HashMap<PathBuf, Vec<PathBuf>>,
) -> Result<()> {
    for entry in
        std::fs::read_dir(dir).with_context(|| format!("failed to read directory: {}", dir.display()))?
    {
        let entry = entry?;
        let path = entry.path();

        if path.is_dir() {
            collect_files(base, &path, files)?;
        } else {
            let rel_path = path.strip_prefix(base).unwrap().to_path_buf();
            let canonical = canonical_target_path(&rel_path);
            files.entry(canonical).or_default().push(path);
        }
    }
    Ok(())
}

/// Strip `##` suffix and `.tera` extension to get the canonical target path.
fn canonical_target_path(rel_path: &Path) -> PathBuf {
    let file_name = rel_path.file_name().unwrap().to_str().unwrap();

    // Strip ## suffix first
    let base_name = if let Some(idx) = file_name.find("##") {
        &file_name[..idx]
    } else {
        file_name
    };

    // Strip .tera extension
    let base_name = base_name.strip_suffix(".tera").unwrap_or(base_name);

    if let Some(parent) = rel_path.parent() {
        if parent == Path::new("") {
            PathBuf::from(base_name)
        } else {
            parent.join(base_name)
        }
    } else {
        PathBuf::from(base_name)
    }
}

/// Given all variants of a file, pick the best one for this host/roles.
fn resolve_variant(
    target_path: &Path,
    variants: &[PathBuf],
    hostname: &str,
    roles: &[&str],
) -> FileAction {
    let host_suffix = format!("##host.{hostname}");

    // Priority 1: host override
    if let Some(source) = variants
        .iter()
        .find(|v| v.file_name().unwrap().to_str().unwrap().contains(&host_suffix))
    {
        return FileAction {
            source: source.clone(),
            target_rel_path: target_path.to_path_buf(),
            is_copy: true,
            is_template: false,
        };
    }

    // Priority 2: role override (last matching role wins)
    for role in roles.iter().rev() {
        let role_suffix = format!("##role.{role}");
        if let Some(source) = variants
            .iter()
            .find(|v| v.file_name().unwrap().to_str().unwrap().contains(&role_suffix))
        {
            return FileAction {
                source: source.clone(),
                target_rel_path: target_path.to_path_buf(),
                is_copy: true,
                is_template: false,
            };
        }
    }

    // Priority 3: template (base file with .tera extension)
    if let Some(source) = variants.iter().find(|v| {
        let name = v.file_name().unwrap().to_str().unwrap();
        name.ends_with(".tera") && !name.contains("##")
    }) {
        return FileAction {
            source: source.clone(),
            target_rel_path: target_path.to_path_buf(),
            is_copy: true,
            is_template: true,
        };
    }

    // Priority 4: plain base file
    let source = variants
        .iter()
        .find(|v| {
            let name = v.file_name().unwrap().to_str().unwrap();
            !name.contains("##") && !name.ends_with(".tera")
        })
        .unwrap_or(&variants[0]);

    FileAction {
        source: source.clone(),
        target_rel_path: target_path.to_path_buf(),
        is_copy: false,
        is_template: false,
    }
}
