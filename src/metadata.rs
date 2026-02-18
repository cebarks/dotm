use crate::config::PackageConfig;
use anyhow::{Context, Result};
use std::os::unix::fs::MetadataExt;
use std::path::Path;

/// Resolved metadata for a single file.
#[derive(Debug, Clone)]
pub struct ResolvedMetadata {
    pub owner: Option<String>,
    pub group: Option<String>,
    pub mode: Option<String>,
}

/// Resolve what metadata to apply for a file, following the resolution order:
/// 1. Per-file preserve -> keep existing (overrides package-level)
/// 2. Per-file ownership/permissions -> explicit override
/// 3. Package-level owner/group -> default for all files
/// 4. Nothing -> preserve existing (None)
pub fn resolve_metadata(pkg_config: &PackageConfig, rel_path: &str) -> ResolvedMetadata {
    let preserve_fields: Vec<&str> = pkg_config
        .preserve
        .get(rel_path)
        .map(|v| v.iter().map(|s| s.as_str()).collect())
        .unwrap_or_default();

    let owner = if preserve_fields.contains(&"owner") {
        None
    } else if let Some(ownership) = pkg_config.ownership.get(rel_path) {
        ownership.split(':').next().map(|s| s.to_string())
    } else {
        pkg_config.owner.clone()
    };

    let group = if preserve_fields.contains(&"group") {
        None
    } else if let Some(ownership) = pkg_config.ownership.get(rel_path) {
        ownership.split(':').nth(1).map(|s| s.to_string())
    } else {
        pkg_config.group.clone()
    };

    let mode = if preserve_fields.contains(&"mode") {
        None
    } else {
        pkg_config.permissions.get(rel_path).cloned()
    };

    ResolvedMetadata { owner, group, mode }
}

/// Read the current metadata of a file on disk. Returns (owner_name, group_name, octal_mode).
pub fn read_file_metadata(path: &Path) -> Result<(String, String, String)> {
    let meta = std::fs::metadata(path)
        .with_context(|| format!("failed to read metadata for {}", path.display()))?;

    let uid = meta.uid();
    let gid = meta.gid();

    let owner = nix::unistd::User::from_uid(nix::unistd::Uid::from_raw(uid))
        .ok()
        .flatten()
        .map(|u| u.name)
        .unwrap_or_else(|| uid.to_string());

    let group = nix::unistd::Group::from_gid(nix::unistd::Gid::from_raw(gid))
        .ok()
        .flatten()
        .map(|g| g.name)
        .unwrap_or_else(|| gid.to_string());

    let mode = format!("{:o}", meta.mode() & 0o7777);

    Ok((owner, group, mode))
}

/// Apply ownership (chown) to a file. Only applies fields that are Some.
pub fn apply_ownership(path: &Path, owner: Option<&str>, group: Option<&str>) -> Result<()> {
    let uid = match owner {
        Some(name) => {
            let user = nix::unistd::User::from_name(name)
                .with_context(|| format!("failed to look up user '{name}'"))?
                .with_context(|| format!("user '{name}' not found"))?;
            Some(user.uid)
        }
        None => None,
    };

    let gid = match group {
        Some(name) => {
            let grp = nix::unistd::Group::from_name(name)
                .with_context(|| format!("failed to look up group '{name}'"))?
                .with_context(|| format!("group '{name}' not found"))?;
            Some(grp.gid)
        }
        None => None,
    };

    nix::unistd::chown(path, uid, gid)
        .with_context(|| format!("failed to chown {}", path.display()))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::PackageConfig;

    fn make_pkg_config() -> PackageConfig {
        PackageConfig {
            target: Some("/etc/foo".into()),
            strategy: Some(crate::config::DeployStrategy::Copy),
            system: true,
            owner: Some("root".into()),
            group: Some("root".into()),
            ..Default::default()
        }
    }

    #[test]
    fn resolve_uses_package_level_defaults() {
        let pkg = make_pkg_config();
        let meta = resolve_metadata(&pkg, "some/file.conf");
        assert_eq!(meta.owner.as_deref(), Some("root"));
        assert_eq!(meta.group.as_deref(), Some("root"));
        assert!(meta.mode.is_none());
    }

    #[test]
    fn resolve_per_file_ownership_overrides_package() {
        let mut pkg = make_pkg_config();
        pkg.ownership
            .insert("file.conf".into(), "www:webgroup".into());
        let meta = resolve_metadata(&pkg, "file.conf");
        assert_eq!(meta.owner.as_deref(), Some("www"));
        assert_eq!(meta.group.as_deref(), Some("webgroup"));
    }

    #[test]
    fn resolve_preserve_overrides_package_level() {
        let mut pkg = make_pkg_config();
        pkg.preserve
            .insert("file.conf".into(), vec!["owner".into()]);
        let meta = resolve_metadata(&pkg, "file.conf");
        assert!(meta.owner.is_none());
        assert_eq!(meta.group.as_deref(), Some("root"));
    }

    #[test]
    fn resolve_preserve_mode_blocks_permission_override() {
        let mut pkg = make_pkg_config();
        pkg.permissions.insert("file.conf".into(), "640".into());
        pkg.preserve
            .insert("file.conf".into(), vec!["mode".into()]);
        let meta = resolve_metadata(&pkg, "file.conf");
        assert!(meta.mode.is_none());
    }

    #[test]
    fn resolve_no_config_preserves_everything() {
        let mut pkg = make_pkg_config();
        pkg.owner = None;
        pkg.group = None;
        let meta = resolve_metadata(&pkg, "file.conf");
        assert!(meta.owner.is_none());
        assert!(meta.group.is_none());
        assert!(meta.mode.is_none());
    }

    #[test]
    fn resolve_permissions_from_config() {
        let mut pkg = make_pkg_config();
        pkg.permissions.insert("file.conf".into(), "755".into());
        let meta = resolve_metadata(&pkg, "file.conf");
        assert_eq!(meta.mode.as_deref(), Some("755"));
    }
}
