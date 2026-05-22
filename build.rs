use std::path::PathBuf;

fn main() {
    let vendor_dir = PathBuf::from("vendor/sqlite-vec");
    let c_src = vendor_dir.join("sqlite-vec.c");

    // Download sqlite-vec.c if not already vendored
    if !c_src.exists() {
        download_sqlite_vec_c(&c_src);
    }

    // Compile sqlite-vec.c as a static library
    cc::Build::new()
        .file(&c_src)
        .include(&vendor_dir)
        // Build as part of SQLite (uses sqlite3.h from libsqlite3-sys)
        .define("SQLITE_CORE", None)
        // Suppress warnings that aren't our problem
        .warnings(false)
        .compile("sqlite_vec0");

    // Tell Rust linker to link the compiled static lib
    println!("cargo:rerun-if-changed=vendor/sqlite-vec/sqlite-vec.c");
    println!("cargo:rerun-if-changed=vendor/sqlite-vec/sqlite-vec.h");
    println!("cargo:rerun-if-changed=build.rs");
}

fn download_sqlite_vec_c(dest: &PathBuf) {
    use std::io::Write;

    eprintln!("cargo:warning=Downloading sqlite-vec.c v0.1.9 from jsdelivr CDN...");

    let url = "https://cdn.jsdelivr.net/gh/asg017/sqlite-vec@v0.1.9/sqlite-vec.c";

    // Try curl first (available on most CI environments)
    let status = std::process::Command::new("curl")
        .args(["-fsSL", "-o"])
        .arg(dest)
        .arg(url)
        .status();

    if let Ok(s) = status {
        if s.success() {
            return;
        }
    }

    // Try wget as fallback
    let status = std::process::Command::new("wget")
        .args(["-q", "-O"])
        .arg(dest)
        .arg(url)
        .status();

    if let Ok(s) = status {
        if s.success() {
            return;
        }
    }

    // If both fail, write a stub that gives a compile error with instructions
    let stub = r#"
/* sqlite-vec.c was not found and could not be downloaded automatically.
 * Please download it manually:
 *   curl -o vendor/sqlite-vec/sqlite-vec.c \
 *     https://cdn.jsdelivr.net/gh/asg017/sqlite-vec@v0.1.9/sqlite-vec.c
 */
#error "sqlite-vec.c is missing. See the comment above for download instructions."
"#;
    let mut f = std::fs::File::create(dest).expect("Failed to create sqlite-vec.c stub");
    f.write_all(stub.as_bytes()).unwrap();
    panic!("sqlite-vec.c not found and automatic download failed. See vendor/sqlite-vec/sqlite-vec.c for instructions.");
}
