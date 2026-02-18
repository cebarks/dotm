use dotm::config::{DotmSettings, PackageConfig, RootConfig};
use dotm::resolver::resolve_packages;
use std::collections::HashMap;

fn make_root(packages: Vec<(&str, Vec<&str>, Vec<&str>)>) -> RootConfig {
    let mut pkg_map = HashMap::new();
    for (name, deps, suggests) in packages {
        pkg_map.insert(
            name.to_string(),
            PackageConfig {
                description: None,
                depends: deps.into_iter().map(String::from).collect(),
                suggests: suggests.into_iter().map(String::from).collect(),
                target: None,
                strategy: None,
                permissions: Default::default(),
                system: false,
                owner: None,
                group: None,
                ownership: Default::default(),
                preserve: Default::default(),
            },
        );
    }
    RootConfig {
        dotm: DotmSettings {
            target: "~".to_string(),
            packages_dir: "packages".to_string(),
            auto_prune: false,
        },
        packages: pkg_map,
    }
}

#[test]
fn resolve_single_package_no_deps() {
    let root = make_root(vec![("zsh", vec![], vec![])]);
    let result = resolve_packages(&root, &["zsh"]).unwrap();
    assert_eq!(result, vec!["zsh"]);
}

#[test]
fn resolve_package_with_dep() {
    let root = make_root(vec![
        ("kde", vec!["util"], vec![]),
        ("util", vec![], vec![]),
    ]);
    let result = resolve_packages(&root, &["kde"]).unwrap();
    assert_eq!(result, vec!["util", "kde"]);
}

#[test]
fn resolve_transitive_deps() {
    let root = make_root(vec![
        ("a", vec!["b"], vec![]),
        ("b", vec!["c"], vec![]),
        ("c", vec![], vec![]),
    ]);
    let result = resolve_packages(&root, &["a"]).unwrap();
    assert_eq!(result, vec!["c", "b", "a"]);
}

#[test]
fn resolve_deduplicates() {
    let root = make_root(vec![
        ("kde", vec!["util"], vec![]),
        ("dev", vec!["util"], vec![]),
        ("util", vec![], vec![]),
    ]);
    let result = resolve_packages(&root, &["kde", "dev"]).unwrap();
    assert!(result.iter().filter(|p| *p == "util").count() == 1);
    let util_pos = result.iter().position(|p| p == "util").unwrap();
    let kde_pos = result.iter().position(|p| p == "kde").unwrap();
    let dev_pos = result.iter().position(|p| p == "dev").unwrap();
    assert!(util_pos < kde_pos);
    assert!(util_pos < dev_pos);
}

#[test]
fn resolve_circular_dep_errors() {
    let root = make_root(vec![
        ("a", vec!["b"], vec![]),
        ("b", vec!["a"], vec![]),
    ]);
    let result = resolve_packages(&root, &["a"]);
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("circular"),
        "expected circular dep error, got: {err}"
    );
}

#[test]
fn resolve_unknown_package_errors() {
    let root = make_root(vec![("a", vec!["nonexistent"], vec![])]);
    let result = resolve_packages(&root, &["a"]);
    assert!(result.is_err());
}

#[test]
fn resolve_suggests_not_included() {
    let root = make_root(vec![
        ("kde", vec![], vec!["gaming"]),
        ("gaming", vec![], vec![]),
    ]);
    let result = resolve_packages(&root, &["kde"]).unwrap();
    assert_eq!(result, vec!["kde"]);
}
