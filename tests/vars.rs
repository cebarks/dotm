use dotm::vars::merge_vars;
use toml::map::Map;
use toml::Value;

fn map_from_str(s: &str) -> Map<String, Value> {
    let val: Value = toml::from_str(s).unwrap();
    val.as_table().unwrap().clone()
}

#[test]
fn merge_empty_into_empty() {
    let base = Map::new();
    let overlay = Map::new();
    let result = merge_vars(&base, &overlay);
    assert!(result.is_empty());
}

#[test]
fn merge_adds_new_keys() {
    let base = map_from_str(r#"a = 1"#);
    let overlay = map_from_str(r#"b = 2"#);
    let result = merge_vars(&base, &overlay);
    assert_eq!(result.get("a").unwrap().as_integer().unwrap(), 1);
    assert_eq!(result.get("b").unwrap().as_integer().unwrap(), 2);
}

#[test]
fn merge_overlay_wins() {
    let base = map_from_str(r#"a = 1"#);
    let overlay = map_from_str(r#"a = 99"#);
    let result = merge_vars(&base, &overlay);
    assert_eq!(result.get("a").unwrap().as_integer().unwrap(), 99);
}

#[test]
fn merge_nested_tables() {
    let base = map_from_str(
        r#"
[display]
resolution = "1920x1080"
refresh = 60
"#,
    );
    let overlay = map_from_str(
        r#"
[display]
resolution = "3840x2160"
"#,
    );
    let result = merge_vars(&base, &overlay);
    let display = result.get("display").unwrap().as_table().unwrap();
    assert_eq!(display.get("resolution").unwrap().as_str().unwrap(), "3840x2160");
    assert_eq!(display.get("refresh").unwrap().as_integer().unwrap(), 60);
}
