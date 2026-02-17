# dotm

A dotfile manager with composable roles, Tera templates, and host-specific overrides.

dotm organizes config files into **packages** (directories mirroring your home directory structure), groups them into **roles** (e.g. "desktop", "dev", "gaming"), and assigns roles to **hosts**. Deployment creates symlinks for plain files and copies for overrides/templates, so your dotfiles repo stays the single source of truth.

## Installation

```bash
cargo install --path .
```

## Quick Start

```bash
# Initialize a dotm project
mkdir ~/dotfiles && cd ~/dotfiles

# Create the root config
cat > dotm.toml << 'EOF'
[dotm]
target = "~"

[packages.shell]
description = "Shell configuration"

[packages.editor]
description = "Editor configuration"
depends = ["shell"]
EOF

# Create a package
dotm init shell
cp ~/.bashrc packages/shell/.bashrc

# Create a role
mkdir roles
echo 'packages = ["shell", "editor"]' > roles/dev.toml

# Create a host config
mkdir hosts
cat > hosts/$(hostname).toml << EOF
hostname = "$(hostname)"
roles = ["dev"]
EOF

# Deploy (dry run first)
dotm deploy --dry-run
dotm deploy
```

## Core Concepts

### Packages

A package is a directory under `packages/` that mirrors the target directory structure (usually `~`). Files inside are symlinked to their corresponding locations during deployment.

```
packages/
├── shell/
│   ├── .bashrc
│   └── .bash_profile
└── editor/
    └── .config/
        └── nvim/
            └── init.lua
```

Packages can declare dependencies and suggestions in `dotm.toml`:

```toml
[packages.editor]
description = "Editor configuration"
depends = ["shell"]       # always pulled in
suggests = ["theme"]      # informational only
target = "/"              # override deploy target (default: ~)
```

### Roles

A role groups packages together and can define variables for template rendering. Role configs live in `roles/<name>.toml`:

```toml
# roles/desktop.toml
packages = ["shell", "editor", "kde"]

[vars]
shell.prompt = "fancy"
display.resolution = "3840x2160"
```

### Hosts

A host config selects which roles to apply and can override variables. Host configs live in `hosts/<hostname>.toml`:

```toml
# hosts/relativity.toml
hostname = "relativity"
roles = ["desktop", "gaming", "dev"]

[vars]
display.resolution = "3840x2160"
gpu.vendor = "amd"
```

Variable precedence: **host vars > role vars** (last role listed wins among roles).

## Directory Structure

```
~/dotfiles/
├── dotm.toml                    # root config: package declarations
├── hosts/
│   ├── relativity.toml          # workstation
│   └── mars.toml                # server
├── roles/
│   ├── desktop.toml
│   ├── dev.toml
│   └── gaming.toml
└── packages/
    ├── shell/
    │   ├── .bashrc              # plain file → symlinked
    │   ├── .bashrc##host.mars   # host override → copied
    │   └── .bashrc##role.dev    # role override → copied
    ├── editor/
    │   └── .config/nvim/
    │       └── init.lua
    └── kde/
        └── .config/
            ├── rc.conf
            └── rc.conf.tera     # template → rendered & copied
```

## File Overrides

Override files sit next to the base file with a `##` suffix:

| Pattern | Priority | Description |
|---------|----------|-------------|
| `file##host.<hostname>` | 1 (highest) | Used only on the named host |
| `file##role.<rolename>` | 2 | Used when the role is active |
| `file.tera` | 3 | Tera template, rendered with vars |
| `file` | 4 (lowest) | Base file, symlinked |

- Override and template files are **copied**, not symlinked
- Only the highest-priority matching variant is deployed
- Non-matching overrides are ignored entirely

## Templates

Files ending in `.tera` are rendered using [Tera](https://keats.github.io/tera/) (a Jinja2-like template engine). Variables come from role and host configs:

```
# .config/app.conf.tera
resolution={{ display.resolution }}
{% if gpu.vendor == "amd" %}
driver=amdgpu
{% else %}
driver=modesetting
{% endif %}
```

The `.tera` extension is stripped from the deployed filename.

## CLI Reference

```
dotm [OPTIONS] <COMMAND>

Options:
  -d, --dir <DIR>   Path to dotfiles directory [default: .]
  -V, --version     Print version

Commands:
  deploy     Deploy configs for the current host
  undeploy   Remove all managed symlinks and copies
  status     Show deployment status
  check      Validate configuration
  init       Initialize a new package
```

### deploy

```bash
dotm deploy                    # deploy for current hostname
dotm deploy --host mars        # deploy for a specific host
dotm deploy --dry-run          # show what would be done
dotm deploy --force            # overwrite existing unmanaged files
```

### undeploy

```bash
dotm undeploy                  # remove all managed files
```

### status

```bash
dotm status                    # show managed files and their state
```

### check

```bash
dotm check                     # validate configuration
dotm check --warn-suggestions  # also warn about unresolved suggests
```

### init

```bash
dotm init mypackage            # create packages/mypackage/
```

## Comparison

| Feature | dotm | GNU stow | yadm | dotter |
|---------|------|----------|------|--------|
| Symlink-based | Yes | Yes | Yes | Yes |
| Role/profile system | Yes | No | No | Yes |
| Host-specific overrides | Yes | No | Alt files | Yes |
| Template rendering | Tera | No | Jinja2* | Handlebars |
| Dependency resolution | Yes | No | No | No |
| Per-package target dirs | Yes | Yes | No | No |

*yadm templates require a separate `yadm alt` step.

## License

MIT
