use crate::config::RootConfig;
use crate::loader::ConfigLoader;
use anyhow::Result;

pub fn render_packages(root: &RootConfig, verbose: bool) -> String {
    let mut names: Vec<&String> = root.packages.keys().collect();
    names.sort();

    let mut out = String::new();
    for name in names {
        if verbose {
            let pkg = &root.packages[name];
            out.push_str(name);
            if let Some(ref desc) = pkg.description {
                out.push_str(&format!(" — {desc}"));
            }
            out.push('\n');
            if !pkg.depends.is_empty() {
                out.push_str(&format!("  depends: {}\n", pkg.depends.join(", ")));
            }
            if !pkg.suggests.is_empty() {
                out.push_str(&format!("  suggests: {}\n", pkg.suggests.join(", ")));
            }
            if let Some(ref target) = pkg.target {
                out.push_str(&format!("  target: {target}\n"));
            }
            if let Some(strategy) = pkg.strategy {
                out.push_str(&format!("  strategy: {strategy:?}\n"));
            }
            if pkg.system {
                out.push_str("  system: true\n");
            }
        } else {
            out.push_str(name);
            if let Some(ref desc) = root.packages[name].description {
                out.push_str(&format!(" — {desc}"));
            }
            out.push('\n');
        }
    }
    out
}

pub fn render_roles(loader: &ConfigLoader, verbose: bool) -> Result<String> {
    let roles = loader.list_roles()?;
    let mut out = String::new();
    for name in &roles {
        out.push_str(name);
        if verbose {
            if let Ok(role) = loader.load_role(name) {
                out.push_str(&format!(" [{}]", role.packages.join(", ")));
            }
        }
        out.push('\n');
    }
    Ok(out)
}

pub fn render_hosts(loader: &ConfigLoader, verbose: bool) -> Result<String> {
    let hosts = loader.list_hosts()?;
    let mut out = String::new();
    for name in &hosts {
        out.push_str(name);
        if verbose {
            if let Ok(host) = loader.load_host(name) {
                out.push_str(&format!(" [{}]", host.roles.join(", ")));
            }
        }
        out.push('\n');
    }
    Ok(out)
}

pub fn render_tree(loader: &ConfigLoader) -> Result<String> {
    let hosts = loader.list_hosts()?;
    let mut out = String::new();

    for (hi, host_name) in hosts.iter().enumerate() {
        let is_last_host = hi == hosts.len() - 1;
        let host_prefix = if is_last_host { "└── " } else { "├── " };
        out.push_str(&format!("{host_prefix}{host_name}\n"));

        if let Ok(host) = loader.load_host(host_name) {
            for (ri, role_name) in host.roles.iter().enumerate() {
                let is_last_role = ri == host.roles.len() - 1;
                let branch = if is_last_host { "    " } else { "│   " };
                let role_prefix = if is_last_role { "└── " } else { "├── " };
                out.push_str(&format!("{branch}{role_prefix}{role_name}\n"));

                if let Ok(role) = loader.load_role(role_name) {
                    for (pi, pkg_name) in role.packages.iter().enumerate() {
                        let is_last_pkg = pi == role.packages.len() - 1;
                        let inner_branch = if is_last_role { "    " } else { "│   " };
                        let pkg_prefix = if is_last_pkg { "└── " } else { "├── " };
                        out.push_str(&format!("{branch}{inner_branch}{pkg_prefix}{pkg_name}\n"));
                    }
                }
            }
        }
    }
    Ok(out)
}
