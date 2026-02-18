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
