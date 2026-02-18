# dotm

A dotfile manager with composable roles, Tera templates, host-specific overrides, and system-level package support.

dotm organizes config files into **packages** (directories mirroring your target directory structure), groups them into **roles** (e.g. "desktop", "dev", "gaming"), and assigns roles to **hosts**. Deployment creates symlinks for plain files and copies for overrides/templates, so your dotfiles repo stays the single source of truth.

## Installation

```bash
cargo install dotm-rs
```

Or to install from the latest source:

```bash
cargo install --git https://github.com/cebarks/dotm
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

A package is a directory under `packages/` that mirrors the target directory structure (usually `~`). Files inside are deployed to their corresponding locations.

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

Packages are declared in `dotm.toml`:

```toml
[packages.editor]
description = "Editor configuration"
depends = ["shell"]       # always pulled in
suggests = ["theme"]      # informational only
target = "/"              # override deploy target (default: ~)
strategy = "copy"         # "stage" (default) or "copy"
```

### Deployment Strategies

Each package uses one of two deployment strategies:

- **stage** (default) — files are copied to a `.staged/` directory, then symlinked from the target location. The dotfiles repo stays the source of truth and changes to the staged copy are detected as drift.
- **copy** — files are copied directly to the target location. No symlink, no staging directory. Useful for system files or contexts where symlinks aren't appropriate.

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
# hosts/workstation.toml
hostname = "workstation"
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
│   ├── workstation.toml
│   └── dev-server.toml
├── roles/
│   ├── desktop.toml
│   ├── dev.toml
│   └── gaming.toml
└── packages/
    ├── shell/
    │   ├── .bashrc              # plain file → symlinked
    │   ├── .bashrc##host.dev-server   # host override → copied
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

## File Permissions & Ownership

Packages can control file permissions and ownership. This is particularly useful for system packages but works for any package.

### Per-file permissions

```toml
[packages.bin.permissions]
"bin/myscript" = "755"
"bin/helper" = "700"
```

### Per-package ownership defaults

```toml
[packages.myservice]
owner = "root"
group = "root"
```

### Per-file ownership overrides

```toml
[packages.myservice.ownership]
"conf.d/app.conf" = "root:appgroup"
```

### Preserving existing metadata

When you want dotm to manage file content but leave existing ownership or permissions untouched on specific files:

```toml
[packages.myservice.preserve]
"dispatcher.d/hook.sh" = ["owner", "group"]
"conf.d/local.conf" = ["mode"]
```

### Resolution order

For each file, each metadata field (owner, group, mode) is resolved independently:

1. Per-file `preserve` — keep existing value on disk
2. Per-file `ownership` / `permissions` — explicit override
3. Package-level `owner` / `group` — default for all files in the package
4. Nothing configured — preserve existing value on disk

The default behavior is to preserve. Setting metadata is always opt-in.

## System Packages

dotm can deploy configuration files to system locations like `/etc/`. System packages are deployed separately from user packages, under root privileges.

### Configuration

Mark a package as system-level with `system = true`. System packages **must** explicitly set `target` and `strategy`:

```toml
[packages.networkmanager]
description = "NetworkManager configs"
system = true
target = "/etc/NetworkManager"
strategy = "copy"
owner = "root"
group = "root"

[packages.networkmanager.ownership]
"conf.d/custom-dns.conf" = "root:networkmanager"

[packages.networkmanager.permissions]
"conf.d/custom-dns.conf" = "640"
```

### Usage

System packages are deployed separately from user packages using the `--system` flag:

```bash
# Deploy user packages (system packages are skipped)
dotm deploy

# Deploy system packages (requires root)
sudo dotm deploy --system

# Check system package status
sudo dotm status --system

# Restore system files to pre-dotm state
sudo dotm restore --system
```

### State separation

User and system packages maintain separate state:

| Context | State directory | Staging directory |
|---------|-----------------|-------------------|
| User | `~/.local/state/dotm/` | `<dotfiles>/.staged/` |
| System | `/var/lib/dotm/` | `/var/lib/dotm/.staged/` |

## Drift Detection

dotm tracks the content hash and metadata of every deployed file. When files are modified externally, dotm detects the drift:

```bash
dotm status            # shows modified/missing files
dotm diff              # shows unified diffs for modified files
dotm adopt             # interactively adopt external changes back into source
```

Status markers:

| Marker | Meaning |
|--------|---------|
| `~` | File is OK (verbose mode only) |
| `M` | Content has been modified since last deploy |
| `!` | File is missing |
| `P` | File metadata (owner/group/permissions) has drifted |

If a file was modified externally, re-deploying will skip it with a warning. Use `--force` to overwrite, or `dotm adopt` to pull the changes back into your dotfiles repo.

## CLI Reference

```
dotm [OPTIONS] <COMMAND>

Options:
  -d, --dir <DIR>   Path to dotfiles directory [default: .]
  -V, --version     Print version

Commands:
  deploy     Deploy configs for the current host
  undeploy   Remove all managed symlinks and copies
  restore    Restore files to their pre-dotm state
  status     Show deployment status
  diff       Show diffs for files modified since last deploy
  adopt      Interactively adopt changes back into source
  check      Validate configuration
  init       Initialize a new package
  commit     Commit all changes in the dotfiles repo
  push       Push dotfiles repo to remote
  pull       Pull dotfiles repo from remote
  sync       Pull, deploy, and optionally push in one step
```

### deploy

```bash
dotm deploy                    # deploy for current hostname
dotm deploy --host dev-server  # deploy for a specific host
dotm deploy --dry-run          # show what would be done
dotm deploy --force            # overwrite modified/unmanaged files
dotm deploy --system           # deploy system packages (requires root)
```

### undeploy

```bash
dotm undeploy                  # remove all managed files
dotm undeploy --system         # remove managed system files
```

### restore

```bash
dotm restore                   # restore all files to pre-dotm state
dotm restore --package shell   # restore a specific package
dotm restore --dry-run         # show what would be restored
dotm restore --system          # restore system files
```

`restore` differs from `undeploy`: if dotm overwrote an existing file, `restore` puts the original back. `undeploy` just removes the file.

### status

```bash
dotm status                    # show managed files and their state
dotm status -v                 # show all files, including OK ones
dotm status -s                 # one-line summary for shell prompts
dotm status -p shell           # filter to a specific package
dotm status --system           # show system package status
```

### diff

```bash
dotm diff                      # show diffs for all modified files
dotm diff .bashrc              # filter to a specific path
dotm diff --system             # show diffs for system files
```

### adopt

```bash
dotm adopt                     # interactively adopt changes
dotm adopt --system            # adopt changes to system files
```

### check

```bash
dotm check                     # validate configuration
dotm check --warn-suggestions  # also warn about unresolved suggests
```

Validates package dependencies, host/role references, system package requirements (target and strategy must be set), ownership format, permission values, and preserve/override conflicts.

### init

```bash
dotm init mypackage            # create packages/mypackage/
```

### commit / push / pull / sync

```bash
dotm commit                    # auto-generate commit message
dotm commit -m "update shell"  # custom commit message
dotm push                      # push to remote
dotm pull                      # pull from remote
dotm sync                      # pull + deploy + push
dotm sync --no-push            # pull + deploy only
dotm sync --system             # sync system packages
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
| System file deployment | Yes | No | No | No |
| File ownership control | Yes | No | No | No |
| Drift detection | Yes | No | No | Yes |
| Pre-existing file backup | Yes | No | No | No |

*yadm templates require a separate `yadm alt` step.

## Disclaimer

Claude Code (Opus 4.6) was used for parts of the development of this tool, including some implementation, testing and documentation.

## License

GNU AGPLv3
