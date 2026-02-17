use crate::config::{HostConfig, RoleConfig, RootConfig};
use anyhow::{Context, Result, bail};
use std::path::{Path, PathBuf};

pub struct ConfigLoader {
    base_dir: PathBuf,
    root: RootConfig,
}

impl ConfigLoader {
    pub fn new(base_dir: &Path) -> Result<Self> {
        let config_path = base_dir.join("dotm.toml");
        let content = std::fs::read_to_string(&config_path)
            .with_context(|| format!("failed to read {}", config_path.display()))?;
        let root: RootConfig = toml::from_str(&content)
            .with_context(|| format!("failed to parse {}", config_path.display()))?;

        Ok(Self {
            base_dir: base_dir.to_path_buf(),
            root,
        })
    }

    pub fn root(&self) -> &RootConfig {
        &self.root
    }

    pub fn base_dir(&self) -> &Path {
        &self.base_dir
    }

    pub fn packages_dir(&self) -> PathBuf {
        self.base_dir.join(&self.root.dotm.packages_dir)
    }

    pub fn load_host(&self, hostname: &str) -> Result<HostConfig> {
        let path = self.base_dir.join("hosts").join(format!("{hostname}.toml"));
        if !path.exists() {
            bail!("host config not found: {}", path.display());
        }
        let content = std::fs::read_to_string(&path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        let config: HostConfig = toml::from_str(&content)
            .with_context(|| format!("failed to parse {}", path.display()))?;
        Ok(config)
    }

    pub fn load_role(&self, name: &str) -> Result<RoleConfig> {
        let path = self.base_dir.join("roles").join(format!("{name}.toml"));
        if !path.exists() {
            bail!("role config not found: {}", path.display());
        }
        let content = std::fs::read_to_string(&path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        let config: RoleConfig = toml::from_str(&content)
            .with_context(|| format!("failed to parse {}", path.display()))?;
        Ok(config)
    }
}
