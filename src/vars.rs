use anyhow::{Context, Result};
use toml::Value;
use toml::map::Map;

use crate::loader::ConfigLoader;

/// Resolve merged variables for a given hostname: role vars merged in order,
/// then host vars override.
pub fn resolve_vars(loader: &ConfigLoader, hostname: &str) -> Result<Map<String, Value>> {
    let host = loader
        .load_host(hostname)
        .with_context(|| format!("failed to load host config for '{hostname}'"))?;

    let mut merged = Map::new();
    for role_name in &host.roles {
        let role = loader
            .load_role(role_name)
            .with_context(|| format!("failed to load role '{role_name}'"))?;
        merged = merge_vars(&merged, &role.vars);
    }
    merged = merge_vars(&merged, &host.vars);
    Ok(merged)
}

/// Deep-merge two TOML variable maps. Values in `overlay` take precedence.
/// Nested tables are merged recursively; all other types are replaced.
pub fn merge_vars(base: &Map<String, Value>, overlay: &Map<String, Value>) -> Map<String, Value> {
    let mut result = base.clone();

    for (key, overlay_val) in overlay {
        match (result.get(key), overlay_val) {
            (Some(Value::Table(base_table)), Value::Table(overlay_table)) => {
                let merged = merge_vars(base_table, overlay_table);
                result.insert(key.clone(), Value::Table(merged));
            }
            _ => {
                result.insert(key.clone(), overlay_val.clone());
            }
        }
    }

    result
}
