use crate::config::RootConfig;
use anyhow::{Result, bail};
use std::collections::HashSet;

/// Resolve a list of requested packages into a fully-expanded, dependency-ordered list.
/// Dependencies come before the packages that depend on them.
/// Circular dependencies produce an error.
pub fn resolve_packages(root: &RootConfig, requested: &[&str]) -> Result<Vec<String>> {
    let mut resolved: Vec<String> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();

    for pkg in requested {
        resolve_one(root, pkg, &mut resolved, &mut seen, &mut Vec::new())?;
    }

    Ok(resolved)
}

fn resolve_one(
    root: &RootConfig,
    pkg: &str,
    resolved: &mut Vec<String>,
    seen: &mut HashSet<String>,
    stack: &mut Vec<String>,
) -> Result<()> {
    if seen.contains(pkg) {
        return Ok(());
    }

    if stack.contains(&pkg.to_string()) {
        stack.push(pkg.to_string());
        bail!("circular dependency detected: {}", stack.join(" -> "));
    }

    let pkg_config = root.packages.get(pkg);
    if let Some(config) = pkg_config {
        stack.push(pkg.to_string());
        for dep in &config.depends {
            if !root.packages.contains_key(dep.as_str()) {
                bail!("package '{pkg}' depends on unknown package '{dep}'");
            }
            resolve_one(root, dep, resolved, seen, stack)?;
        }
        stack.pop();
    } else {
        bail!("unknown package: '{pkg}'");
    }

    seen.insert(pkg.to_string());
    resolved.push(pkg.to_string());
    Ok(())
}
