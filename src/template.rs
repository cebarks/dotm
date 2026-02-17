use anyhow::{Context, Result};
use tera::Tera;
use toml::map::Map;
use toml::Value;

/// Render a Tera template string with the given variables.
pub fn render_template(template_str: &str, vars: &Map<String, Value>) -> Result<String> {
    let mut tera = Tera::default();
    tera.add_raw_template("__dotm_template", template_str)
        .context("failed to parse template")?;

    let context = toml_map_to_tera_context(vars);

    tera.render("__dotm_template", &context)
        .context("failed to render template")
}

fn toml_map_to_tera_context(vars: &Map<String, Value>) -> tera::Context {
    let mut context = tera::Context::new();
    for (key, value) in vars {
        context.insert(key, &toml_value_to_json(value));
    }
    context
}

fn toml_value_to_json(value: &Value) -> serde_json::Value {
    match value {
        Value::String(s) => serde_json::Value::String(s.clone()),
        Value::Integer(i) => serde_json::json!(*i),
        Value::Float(f) => serde_json::json!(*f),
        Value::Boolean(b) => serde_json::Value::Bool(*b),
        Value::Datetime(dt) => serde_json::Value::String(dt.to_string()),
        Value::Array(arr) => serde_json::Value::Array(arr.iter().map(toml_value_to_json).collect()),
        Value::Table(table) => {
            let map: serde_json::Map<String, serde_json::Value> =
                table.iter().map(|(k, v)| (k.clone(), toml_value_to_json(v))).collect();
            serde_json::Value::Object(map)
        }
    }
}
