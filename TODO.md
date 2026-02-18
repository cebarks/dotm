- native submodule support
- speed up `dotm status`
- `dotm -d` doesn't work as intended when run outside of the dotfiles directory
    -d flag doesn't resolve paths independently of cwd

  dotm status reports all files as missing when cwd is outside the dotfiles repo, even with -d <path> specified.

  Repro:
  cd ~/dotfiles && dotm status --short   # works (exit 0)
  cd /tmp && dotm -d ~/dotfiles status --short   # "dotm: 321 missing" (exit 1)

  Root cause: Some path resolution in the status command is relative to cwd instead of the directory provided via -d. The -d flag should make all path
  operations relative to the specified directory.

  Workaround applied in dotfiles (packages/zsh/.zsh/0.core.zsh):
  (cd "$HOME/dotfiles" && dotm status --short)

