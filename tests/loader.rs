use dotm::loader::ConfigLoader;
use std::path::Path;

#[test]
fn load_root_config() {
    let loader = ConfigLoader::new(Path::new("tests/fixtures/basic")).unwrap();
    assert_eq!(loader.root().dotm.target, "~");
    assert_eq!(loader.root().packages.len(), 2);
}

#[test]
fn load_host_config() {
    let loader = ConfigLoader::new(Path::new("tests/fixtures/basic")).unwrap();
    let host = loader.load_host("testhost").unwrap();
    assert_eq!(host.hostname, "testhost");
    assert_eq!(host.roles, vec!["desktop", "dev"]);
}

#[test]
fn load_host_not_found() {
    let loader = ConfigLoader::new(Path::new("tests/fixtures/basic")).unwrap();
    let result = loader.load_host("nonexistent");
    assert!(result.is_err());
}

#[test]
fn load_role_config() {
    let loader = ConfigLoader::new(Path::new("tests/fixtures/basic")).unwrap();
    let role = loader.load_role("desktop").unwrap();
    assert_eq!(role.packages, vec!["shell"]);
}

#[test]
fn load_role_not_found() {
    let loader = ConfigLoader::new(Path::new("tests/fixtures/basic")).unwrap();
    let result = loader.load_role("nonexistent");
    assert!(result.is_err());
}
