use dotm::orchestrator::Orchestrator;
use tempfile::TempDir;

#[test]
fn add_moves_file_into_package_and_deploys() {
    let dotfiles = TempDir::new().unwrap();
    let target = TempDir::new().unwrap();
    let state_dir = TempDir::new().unwrap();

    // Create a minimal dotm setup with an absolute target
    std::fs::write(
        dotfiles.path().join("dotm.toml"),
        format!(
            "[dotm]\ntarget = \"{}\"\n\n[packages.shell]\ndescription = \"Shell\"",
            target.path().display()
        ),
    )
    .unwrap();
    std::fs::create_dir_all(dotfiles.path().join("packages/shell")).unwrap();
    std::fs::create_dir_all(dotfiles.path().join("hosts")).unwrap();
    std::fs::write(
        dotfiles.path().join("hosts/testhost.toml"),
        "hostname = \"testhost\"\nroles = [\"base\"]\n",
    )
    .unwrap();
    std::fs::create_dir_all(dotfiles.path().join("roles")).unwrap();
    std::fs::write(
        dotfiles.path().join("roles/base.toml"),
        "packages = [\"shell\"]\n",
    )
    .unwrap();

    // Create an existing file in the target dir (simulating a real config)
    let existing_file = target.path().join(".bashrc");
    std::fs::write(&existing_file, "# my bashrc").unwrap();

    // Simulate `dotm add shell .bashrc` — move file into package
    let pkg_dir = dotfiles.path().join("packages/shell");
    let dest = pkg_dir.join(".bashrc");
    std::fs::rename(&existing_file, &dest).unwrap();
    assert!(dest.exists());
    assert!(!existing_file.exists());

    // Deploy — should create symlink back to original location
    let mut orch = Orchestrator::new(dotfiles.path(), target.path())
        .unwrap()
        .with_state_dir(state_dir.path());
    let report = orch.deploy("testhost", false, false).unwrap();

    assert!(report.conflicts.is_empty());
    assert!(existing_file.is_symlink()); // symlink back in place
    assert_eq!(
        std::fs::read_to_string(&existing_file).unwrap(),
        "# my bashrc"
    );
}
