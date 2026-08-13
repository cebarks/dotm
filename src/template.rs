use std::collections::HashMap;
use std::env;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{Context, Result};
use tera::Tera;
use toml::map::Map;
use toml::Value;

static EXEC_WARNING_SHOWN: AtomicBool = AtomicBool::new(false);

/// Tera function that executes a shell command and returns its stdout.
struct ExecFunction;

impl tera::Function for ExecFunction {
    fn call(&self, args: &HashMap<String, serde_json::Value>) -> tera::Result<serde_json::Value> {
        let command = args
            .get("command")
            .and_then(|v| v.as_str())
            .ok_or_else(|| tera::Error::msg("exec() requires a `command` argument"))?;

        if !EXEC_WARNING_SHOWN.swap(true, Ordering::Relaxed) {
            eprintln!(
                "warning: template uses exec() to run shell commands — review templates if this repo isn't yours"
            );
        }

        let shell = env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string());

        let output = Command::new(&shell)
            .arg("-c")
            .arg(command)
            .output()
            .map_err(|e| tera::Error::msg(format!("exec({command:?}): failed to run: {e}")))?;

        if !output.status.success() {
            let code = output
                .status
                .code()
                .map(|c| c.to_string())
                .unwrap_or_else(|| "signal".to_string());
            return Err(tera::Error::msg(format!(
                "exec({command:?}): exited with status {code}"
            )));
        }

        let stdout = String::from_utf8_lossy(&output.stdout)
            .trim_end()
            .to_string();

        Ok(serde_json::Value::String(stdout))
    }

    fn is_safe(&self) -> bool {
        false
    }
}

/// Render a Tera template string with the given variables.
pub fn render_template(template_str: &str, vars: &Map<String, Value>) -> Result<String> {
    let mut tera = Tera::default();
    tera.add_raw_template("__dotm_template", template_str)
        .context("failed to parse template")?;

    tera.register_function("exec", ExecFunction);

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
            let map: serde_json::Map<String, serde_json::Value> = table
                .iter()
                .map(|(k, v)| (k.clone(), toml_value_to_json(v)))
                .collect();
            serde_json::Value::Object(map)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_vars() -> Map<String, Value> {
        Map::new()
    }

    #[test]
    fn exec_captures_stdout_trimmed() {
        let result = render_template(r#"{{ exec(command="echo hello") }}"#, &empty_vars()).unwrap();
        assert_eq!(result, "hello");
    }

    #[test]
    fn exec_trims_trailing_whitespace_only() {
        let result = render_template(
            r#"{{ exec(command="printf '  hi  \n\n'") }}"#,
            &empty_vars(),
        )
        .unwrap();
        assert_eq!(result, "  hi");
    }

    #[test]
    fn exec_multiline_output() {
        let result =
            render_template(r#"{{ exec(command="printf 'a\nb\nc'") }}"#, &empty_vars()).unwrap();
        assert_eq!(result, "a\nb\nc");
    }

    #[test]
    fn exec_nonzero_exit_fails() {
        let result = render_template(r#"{{ exec(command="exit 42") }}"#, &empty_vars());
        let err = format!("{:?}", result.unwrap_err());
        assert!(err.contains("42"), "error should mention exit code: {err}");
    }

    #[test]
    fn exec_missing_command_arg_fails() {
        let result = render_template(r#"{{ exec() }}"#, &empty_vars());
        assert!(result.is_err());
    }

    #[test]
    fn exec_command_not_found_fails() {
        let result = render_template(
            r#"{{ exec(command="__dotm_nonexistent_cmd_42") }}"#,
            &empty_vars(),
        );
        assert!(result.is_err());
    }

    #[test]
    fn exec_mixed_with_vars() {
        let mut vars = Map::new();
        vars.insert("name".to_string(), Value::String("world".to_string()));
        let result =
            render_template(r#"{{ exec(command="echo hello") }} {{ name }}"#, &vars).unwrap();
        assert_eq!(result, "hello world");
    }
}
