use toml::map::Map;
use toml::Value;

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
