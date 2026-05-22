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

#[test]
fn release_workflow_includes_pkg_and_windows_installer() {
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let workflow = fs::read_to_string(repo_root.join(".github/workflows/release.yaml"))
        .expect("release workflow should be readable");

    assert!(
        workflow.contains("package-macos-universal-pkg"),
        "release workflow should include macOS universal pkg packaging job"
    );
    assert!(
        workflow.contains(".pkg"),
        "release workflow should package and publish a macOS pkg artifact"
    );
    assert!(
        workflow.contains("package-windows-installer"),
        "release workflow should include windows installer packaging job"
    );
    assert!(
        workflow.contains("ISCC.exe"),
        "release workflow should build a Windows installer exe with Inno Setup"
    );
    assert!(
        workflow.contains("Source: \"..\\build\\alchemy-x86_64-pc-windows-msvc\\alchemy.exe\""),
        "release workflow should reference the extracted binary with a path relative to dist\\alchemy-installer.iss"
    );
    assert!(
        workflow.contains("OutputDir=."),
        "release workflow should emit installer output next to dist\\alchemy-installer.iss so hash/upload paths resolve"
    );
}

#[test]
fn release_workflow_builds_x86_64_apple_darwin_on_macos_latest() {
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let workflow = fs::read_to_string(repo_root.join(".github/workflows/release.yaml"))
        .expect("release workflow should be readable");

    assert!(
        workflow.contains("target: x86_64-apple-darwin\n            archive_ext: tar.gz\n            binary_suffix: \"\"\n            use_cross: false"),
        "release workflow should build x86_64-apple-darwin with use_cross: false"
    );
    assert!(
        workflow.contains("runner: macos-latest\n            target: x86_64-apple-darwin"),
        "release workflow should run x86_64-apple-darwin build on macos-latest"
    );
}

#[test]
fn release_workflow_builds_aarch64_apple_darwin_on_macos_latest() {
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let workflow = fs::read_to_string(repo_root.join(".github/workflows/release.yaml"))
        .expect("release workflow should be readable");

    assert!(
        workflow.contains("target: aarch64-apple-darwin\n            archive_ext: tar.gz\n            binary_suffix: \"\"\n            use_cross: false"),
        "release workflow should build aarch64-apple-darwin with use_cross: false"
    );
    assert!(
        workflow.contains("runner: macos-latest\n            target: aarch64-apple-darwin"),
        "release workflow should run aarch64-apple-darwin build on macos-latest"
    );
}
