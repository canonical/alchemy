use std::fs;
use std::path::Path;

#[test]
fn install_scripts_exist() {
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"));
    assert!(
        repo_root.join("install.sh").exists(),
        "install.sh should exist at repository root"
    );
    assert!(
        repo_root.join("install.ps1").exists(),
        "install.ps1 should exist at repository root"
    );
}

#[test]
fn readme_has_install_one_liners() {
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let readme = fs::read_to_string(repo_root.join("README.md")).expect("README.md should be readable");

    assert!(readme.contains(
        "curl -fsSL https://raw.githubusercontent.com/canonical/alchemy/refs/heads/main/install.sh | bash"
    ));
    assert!(readme.contains(
        "powershell -c \"irm https://raw.githubusercontent.com/canonical/alchemy/refs/heads/main/install.ps1 | iex\""
    ));
}

#[test]
fn installers_have_latest_resolution_fallback() {
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let install_sh =
        fs::read_to_string(repo_root.join("install.sh")).expect("install.sh should be readable");
    let install_ps1 =
        fs::read_to_string(repo_root.join("install.ps1")).expect("install.ps1 should be readable");

    assert!(
        install_sh.contains("url_effective"),
        "install.sh should include redirect-based fallback for latest release resolution"
    );
    assert!(
        install_ps1.contains("ResponseUri"),
        "install.ps1 should include redirect-based fallback for latest release resolution"
    );
}
