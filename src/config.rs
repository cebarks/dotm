use serde::Deserialize;
use std::collections::HashMap;
use toml::map::Map;
use toml::Value;

#[derive(Debug, Deserialize)]
pub struct RootConfig {
    pub dotm: DotmSettings,
    #[serde(default)]
    pub packages: HashMap<String, PackageConfig>,
}

#[derive(Debug, Deserialize)]
pub struct DotmSettings {
    pub target: String,
    #[serde(default = "default_packages_dir")]
    pub packages_dir: String,
    #[serde(default)]
    pub auto_prune: bool,
}

fn default_packages_dir() -> String {
    "packages".to_string()
}

#[derive(Debug, Deserialize, PartialEq, Eq, Clone, Copy)]
#[serde(rename_all = "lowercase")]
pub enum DeployStrategy {
    Stage,
    Copy,
}

#[derive(Debug, Default, Deserialize)]
pub struct PackageConfig {
    pub description: Option<String>,
    #[serde(default)]
    pub depends: Vec<String>,
    #[serde(default)]
    pub suggests: Vec<String>,
    pub target: Option<String>,
    pub strategy: Option<DeployStrategy>,
    #[serde(default)]
    pub permissions: HashMap<String, String>,
    #[serde(default)]
    pub system: bool,
    pub owner: Option<String>,
    pub group: Option<String>,
    #[serde(default)]
    pub ownership: HashMap<String, String>,
    #[serde(default)]
    pub preserve: HashMap<String, Vec<String>>,
    pub pre_deploy: Option<String>,
    pub post_deploy: Option<String>,
    pub pre_undeploy: Option<String>,
    pub post_undeploy: Option<String>,
}

pub fn validate_system_packages(root: &RootConfig) -> Vec<String> {
    let mut errors = Vec::new();
    for (name, pkg) in &root.packages {
        if pkg.system && pkg.target.is_none() {
            errors.push(format!(
                "system package '{name}' must specify a target directory"
            ));
        }
        // Validate ownership format
        for (path, value) in &pkg.ownership {
            if value.split(':').count() != 2 {
                errors.push(format!(
                    "package '{name}': invalid ownership format for '{path}': expected 'user:group', got '{value}'"
                ));
            }
        }
        // Validate permissions format
        for (path, value) in &pkg.permissions {
            if u32::from_str_radix(value, 8).is_err() {
                errors.push(format!(
                    "package '{name}': invalid permission for '{path}': '{value}' is not valid octal"
                ));
            }
        }
        // Validate preserve entries don't conflict
        for (path, preserve_fields) in &pkg.preserve {
            for field in preserve_fields {
                match field.as_str() {
                    "owner" | "group" => {
                        if pkg.ownership.contains_key(path) {
                            errors.push(format!(
                                "package '{name}': file '{path}' has both preserve {field} and ownership override"
                            ));
                        }
                    }
                    "mode" => {
                        if pkg.permissions.contains_key(path) {
                            errors.push(format!(
                                "package '{name}': file '{path}' has both preserve mode and permission override"
                            ));
                        }
                    }
                    other => {
                        errors.push(format!(
                            "package '{name}': file '{path}': unknown preserve field '{other}'"
                        ));
                    }
                }
            }
        }
    }
    errors
}

pub fn deprecated_strategy_warnings(root: &RootConfig) -> Vec<String> {
    let mut warnings = Vec::new();
    for (name, pkg) in &root.packages {
        if pkg.strategy.is_some() {
            warnings.push(format!(
                "warning: 'strategy' field on package '{name}' is deprecated and ignored; deployment mode is now determined automatically"
            ));
        }
    }
    warnings
}

#[derive(Debug, Deserialize)]
pub struct HostConfig {
    pub hostname: String,
    pub roles: Vec<String>,
    #[serde(default)]
    pub vars: Map<String, Value>,
}

#[derive(Debug, Deserialize)]
pub struct RoleConfig {
    pub packages: Vec<String>,
    #[serde(default)]
    pub vars: Map<String, Value>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_system_packages_does_not_require_strategy() {
        let toml_str = r#"
[dotm]
target = "~"

[packages.sys]
system = true
target = "/etc/sys"
"#;
        let root: RootConfig = toml::from_str(toml_str).unwrap();
        let errors = validate_system_packages(&root);
        assert!(errors.is_empty(), "unexpected errors: {:?}", errors);
    }

    #[test]
    fn strategy_field_still_parses() {
        let toml_str = r#"
[dotm]
target = "~"

[packages.sys]
system = true
target = "/etc/sys"
strategy = "copy"
"#;
        let root: RootConfig = toml::from_str(toml_str).unwrap();
        assert!(root.packages["sys"].strategy.is_some());
    }

    #[test]
    fn deprecated_strategy_warning_emitted() {
        let toml_str = r#"
[dotm]
target = "~"

[packages.shell]
strategy = "stage"
"#;
        let root: RootConfig = toml::from_str(toml_str).unwrap();
        let warnings = deprecated_strategy_warnings(&root);
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("shell"));
        assert!(warnings[0].contains("deprecated"));
    }

    #[test]
    fn no_deprecation_warning_without_strategy() {
        let toml_str = r#"
[dotm]
target = "~"

[packages.shell]
"#;
        let root: RootConfig = toml::from_str(toml_str).unwrap();
        let warnings = deprecated_strategy_warnings(&root);
        assert!(warnings.is_empty());
    }
}
