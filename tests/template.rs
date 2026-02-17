use dotm::template::render_template;
use toml::map::Map;
use toml::Value;

fn vars_from_str(s: &str) -> Map<String, Value> {
    let val: Value = toml::from_str(s).unwrap();
    val.as_table().unwrap().clone()
}

#[test]
fn render_simple_variable() {
    let template = "color={{ color }}";
    let vars = vars_from_str(r#"color = "blue""#);
    let result = render_template(template, &vars).unwrap();
    assert_eq!(result, "color=blue");
}

#[test]
fn render_nested_variable() {
    let template = "resolution={{ display.resolution }}";
    let vars = vars_from_str(
        r#"
[display]
resolution = "3840x2160"
"#,
    );
    let result = render_template(template, &vars).unwrap();
    assert_eq!(result, "resolution=3840x2160");
}

#[test]
fn render_conditional() {
    let template = r#"{% if gpu.vendor == "amd" %}amd=true{% else %}amd=false{% endif %}"#;
    let vars = vars_from_str(
        r#"
[gpu]
vendor = "amd"
"#,
    );
    let result = render_template(template, &vars).unwrap();
    assert_eq!(result, "amd=true");
}

#[test]
fn render_missing_variable_errors() {
    let template = "value={{ nonexistent }}";
    let vars = Map::new();
    let result = render_template(template, &vars);
    assert!(result.is_err());
}
