use dotm::list;
use dotm::loader::ConfigLoader;
use std::path::Path;

#[test]
fn list_packages_basic() {
    let loader = ConfigLoader::new(Path::new("tests/fixtures/basic")).unwrap();
    let output = list::render_packages(loader.root(), false);
    assert!(output.contains("shell"));
    assert!(output.contains("editor"));
}

#[test]
fn list_packages_verbose() {
    let loader = ConfigLoader::new(Path::new("tests/fixtures/basic")).unwrap();
    let output = list::render_packages(loader.root(), true);
    assert!(output.contains("depends"));
    assert!(output.contains("shell"));
}

#[test]
fn list_roles_basic() {
    let loader = ConfigLoader::new(Path::new("tests/fixtures/basic")).unwrap();
    let output = list::render_roles(&loader, false).unwrap();
    assert!(output.contains("desktop"));
    assert!(output.contains("dev"));
}

#[test]
fn list_hosts_basic() {
    let loader = ConfigLoader::new(Path::new("tests/fixtures/basic")).unwrap();
    let output = list::render_hosts(&loader, false).unwrap();
    assert!(output.contains("testhost"));
}

#[test]
fn list_tree_shows_hierarchy() {
    let loader = ConfigLoader::new(Path::new("tests/fixtures/basic")).unwrap();
    let output = list::render_tree(&loader).unwrap();
    assert!(output.contains("testhost"));
    assert!(output.contains("desktop"));
    assert!(output.contains("shell"));
}

#[test]
fn list_packages_verbose_shows_setup_command() {
    let toml_str = r#"
[dotm]
target = "~"

[packages.homebrew]
description = "Homebrew"
setup = "brew bundle --file=~/.Brewfile"
"#;
    let root: dotm::config::RootConfig = toml::from_str(toml_str).unwrap();
    let output = list::render_packages(&root, true);
    assert!(output.contains("Setup: brew bundle --file=~/.Brewfile"));
}

#[test]
fn list_packages_non_verbose_omits_setup_detail() {
    let toml_str = r#"
[dotm]
target = "~"

[packages.homebrew]
description = "Homebrew"
setup = "brew bundle --file=~/.Brewfile"
"#;
    let root: dotm::config::RootConfig = toml::from_str(toml_str).unwrap();
    let output = list::render_packages(&root, false);
    assert!(!output.contains("Setup:"));
}
