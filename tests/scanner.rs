use dotm::scanner::scan_package;
use std::path::Path;

#[test]
fn scan_resolves_host_override_over_base() {
    let pkg_dir = Path::new("tests/fixtures/overrides/packages/configs");
    let actions = scan_package(pkg_dir, "myhost", &["desktop"]).unwrap();

    let app_conf = actions
        .iter()
        .find(|a| a.target_rel_path.to_str() == Some(".config/app.conf"))
        .unwrap();
    assert!(app_conf
        .source
        .to_str()
        .unwrap()
        .ends_with("app.conf##host.myhost"));
    assert!(app_conf.is_copy, "overrides should be copied, not symlinked");
}

#[test]
fn scan_resolves_role_override_when_no_host_override() {
    let pkg_dir = Path::new("tests/fixtures/overrides/packages/configs");
    let actions = scan_package(pkg_dir, "anotherhost", &["desktop"]).unwrap();

    let app_conf = actions
        .iter()
        .find(|a| a.target_rel_path.to_str() == Some(".config/app.conf"))
        .unwrap();
    assert!(app_conf
        .source
        .to_str()
        .unwrap()
        .ends_with("app.conf##role.desktop"));
    assert!(app_conf.is_copy);
}

#[test]
fn scan_uses_base_when_no_overrides_match() {
    let pkg_dir = Path::new("tests/fixtures/overrides/packages/configs");
    let actions = scan_package(pkg_dir, "anotherhost", &["server"]).unwrap();

    let app_conf = actions
        .iter()
        .find(|a| a.target_rel_path.to_str() == Some(".config/app.conf"))
        .unwrap();
    assert!(app_conf.source.to_str().unwrap().ends_with("app.conf"));
    assert!(!app_conf.source.to_str().unwrap().contains("##"));
    assert!(!app_conf.is_copy, "plain files should be symlinked");
}

#[test]
fn scan_plain_file_is_symlinked() {
    let pkg_dir = Path::new("tests/fixtures/overrides/packages/configs");
    let actions = scan_package(pkg_dir, "myhost", &["desktop"]).unwrap();

    let profile = actions
        .iter()
        .find(|a| a.target_rel_path.to_str() == Some(".profile"))
        .unwrap();
    assert!(!profile.is_copy);
    assert!(!profile.is_template);
}

#[test]
fn scan_template_is_detected() {
    let pkg_dir = Path::new("tests/fixtures/overrides/packages/configs");
    let actions = scan_package(pkg_dir, "myhost", &["desktop"]).unwrap();

    let tmpl = actions
        .iter()
        .find(|a| a.target_rel_path.to_str() == Some(".config/templated.conf"))
        .unwrap();
    assert!(tmpl.is_template);
    assert!(tmpl.is_copy, "templates are always copied");
    assert!(tmpl.source.to_str().unwrap().ends_with(".tera"));
}

#[test]
fn scan_excludes_non_matching_overrides() {
    let pkg_dir = Path::new("tests/fixtures/overrides/packages/configs");
    let actions = scan_package(pkg_dir, "myhost", &["desktop"]).unwrap();

    assert!(actions
        .iter()
        .all(|a| !a.source.to_str().unwrap().contains("##host.other")));
    let app_count = actions
        .iter()
        .filter(|a| a.target_rel_path.to_str() == Some(".config/app.conf"))
        .count();
    assert_eq!(app_count, 1);
}

#[test]
fn scan_theme_conf_has_no_override() {
    let pkg_dir = Path::new("tests/fixtures/overrides/packages/configs");
    let actions = scan_package(pkg_dir, "myhost", &["desktop"]).unwrap();

    let theme = actions
        .iter()
        .find(|a| a.target_rel_path.to_str() == Some(".config/theme.conf"))
        .unwrap();
    assert!(!theme.is_copy);
    assert!(!theme.is_template);
}
