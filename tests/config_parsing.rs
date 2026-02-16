use dotm::config::{HostConfig, RoleConfig, RootConfig};

#[test]
fn parse_minimal_root_config() {
    let toml_str = r#"
[dotm]
target = "~"
"#;
    let config: RootConfig = toml::from_str(toml_str).unwrap();
    assert_eq!(config.dotm.target, "~");
    assert_eq!(config.dotm.packages_dir, "packages"); // default
    assert!(config.packages.is_empty());
}

#[test]
fn parse_root_config_with_packages() {
    let toml_str = r#"
[dotm]
target = "~"
packages_dir = "pkgs"

[packages.zsh]
description = "Zsh shell configuration"

[packages.kde]
description = "KDE Plasma desktop configs"
depends = ["util"]
suggests = ["gaming"]

[packages.util]
description = "General utility configs"
"#;
    let config: RootConfig = toml::from_str(toml_str).unwrap();
    assert_eq!(config.dotm.packages_dir, "pkgs");
    assert_eq!(config.packages.len(), 3);

    let kde = &config.packages["kde"];
    assert_eq!(kde.description.as_deref(), Some("KDE Plasma desktop configs"));
    assert_eq!(kde.depends, vec!["util"]);
    assert_eq!(kde.suggests, vec!["gaming"]);

    let zsh = &config.packages["zsh"];
    assert!(zsh.depends.is_empty());
    assert!(zsh.suggests.is_empty());
}

#[test]
fn parse_root_config_with_package_target_override() {
    let toml_str = r#"
[dotm]
target = "~"

[packages.system]
description = "System-level configs"
target = "/"
"#;
    let config: RootConfig = toml::from_str(toml_str).unwrap();
    let system = &config.packages["system"];
    assert_eq!(system.target.as_deref(), Some("/"));
}

#[test]
fn parse_host_config() {
    let toml_str = r#"
hostname = "relativity"
roles = ["desktop", "gaming", "dev"]

[vars]
display.resolution = "3840x2160"
display.refresh_rate = 120
gpu.vendor = "amd"
"#;
    let config: HostConfig = toml::from_str(toml_str).unwrap();
    assert_eq!(config.hostname, "relativity");
    assert_eq!(config.roles, vec!["desktop", "gaming", "dev"]);

    let display = config.vars.get("display").unwrap().as_table().unwrap();
    assert_eq!(display.get("resolution").unwrap().as_str().unwrap(), "3840x2160");
    assert_eq!(display.get("refresh_rate").unwrap().as_integer().unwrap(), 120);
}

#[test]
fn parse_host_config_no_vars() {
    let toml_str = r#"
hostname = "minimal"
roles = ["dev"]
"#;
    let config: HostConfig = toml::from_str(toml_str).unwrap();
    assert_eq!(config.hostname, "minimal");
    assert!(config.vars.is_empty());
}

#[test]
fn parse_role_config() {
    let toml_str = r#"
packages = ["games", "gamemode"]

[vars]
gamemode.renice = 10
"#;
    let config: RoleConfig = toml::from_str(toml_str).unwrap();
    assert_eq!(config.packages, vec!["games", "gamemode"]);
    let gamemode = config.vars.get("gamemode").unwrap().as_table().unwrap();
    assert_eq!(gamemode.get("renice").unwrap().as_integer().unwrap(), 10);
}

#[test]
fn parse_role_config_no_vars() {
    let toml_str = r#"
packages = ["zsh", "ssh"]
"#;
    let config: RoleConfig = toml::from_str(toml_str).unwrap();
    assert_eq!(config.packages, vec!["zsh", "ssh"]);
    assert!(config.vars.is_empty());
}
