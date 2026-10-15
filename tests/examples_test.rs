use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;
use std::path::Path;
use std::time::Instant;
use tempfile::tempdir;

fn copy_dir_writable(src: &Path, dst: &Path) {
    fs::create_dir_all(dst).expect("Failed to create temp example directory");
    for entry in fs::read_dir(src).expect("Failed to read source directory") {
        let entry = entry.expect("Invalid directory entry");
        let path = entry.path();
        let name = entry.file_name();
        let name_str = name.to_string_lossy();

        // Skip stale build artifacts or IDE compilation databases
        if matches!(
            name_str.as_ref(),
            "build" | "build_crupp" | "target" | ".git" | "compile_commands.json"
        ) {
            continue;
        }

        let dest_path = dst.join(&name);
        if path.is_dir() {
            copy_dir_writable(&path, &dest_path);
        } else if path.is_file() {
            fs::copy(&path, &dest_path).expect("Failed to copy example file");
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                if let Ok(meta) = fs::metadata(&dest_path) {
                    let mut perms = meta.permissions();
                    perms.set_mode(0o644);
                    let _ = fs::set_permissions(&dest_path, perms);
                }
            }
        }
    }
}

#[test]
fn test_all_examples() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let examples_dir = manifest_dir.join("tests").join("examples");

    if !examples_dir.exists() {
        println!(
            "ℹ️ Directory tests/examples not found ({}); skipping examples_test.",
            examples_dir.display()
        );
        return;
    }

    let mut tested_count = 0;

    let mut entries: Vec<_> = fs::read_dir(&examples_dir)
        .expect("Failed to read tests/examples")
        .filter_map(|e| e.ok())
        .collect();
    entries.sort_by_key(|e| e.file_name());

    for entry in entries {
        let src_path = entry.path();
        if !src_path.is_dir() {
            continue;
        }

        let project_name = src_path.file_name().unwrap().to_string_lossy().to_string();
        println!("\n========================================================");
        println!("🚀 Testing project: {}", project_name);

        // Copy example into an isolated writable tempdir (essential on NixOS where /nix/store or source dirs may be read-only)
        let tmp = tempdir().expect("Failed to create tempdir for example");
        let path = tmp.path().join(&project_name);
        copy_dir_writable(&src_path, &path);

        // Check if example requires optional host libraries that are not installed in current shell
        if path.join("Crub.toml").exists() || path.join("crupp.toml").exists() {
            let mut check_cmd = Command::cargo_bin("crupp").unwrap();
            if let Ok(out) = check_cmd
                .current_dir(&path)
                .env("CRUPP_NO_AUTO_NIX_SHELL", "1")
                .arg("check-libs")
                .output()
            {
                let stdout = String::from_utf8_lossy(&out.stdout);
                if stdout.contains("NOT FOUND") {
                    println!(
                        "  ⚠️ Skipping '{}' because an optional external system library is not installed in the current environment.",
                        project_name
                    );
                    continue;
                }
            }
        }

        // 1. Cold build
        let cold_start = Instant::now();
        let mut build_cmd = Command::cargo_bin("crupp").unwrap();
        build_cmd
            .current_dir(&path)
            .env("CRUPP_NO_AUTO_NIX_SHELL", "1")
            .arg("build")
            .assert()
            .success()
            .stdout(predicate::str::contains("Finished"));
        let cold_time = cold_start.elapsed();

        // 2. Run target
        let mut run_cmd = Command::cargo_bin("crupp").unwrap();
        run_cmd
            .current_dir(&path)
            .env("CRUPP_NO_AUTO_NIX_SHELL", "1")
            .arg("run")
            .assert()
            .success();

        // 3. Warm build (verifying cache validity)
        let warm_start = Instant::now();
        let mut warm_cmd = Command::cargo_bin("crupp").unwrap();
        warm_cmd
            .current_dir(&path)
            .env("CRUPP_NO_AUTO_NIX_SHELL", "1")
            .arg("build")
            .assert()
            .success()
            .stdout(predicate::str::contains("Cached"));
        let warm_time = warm_start.elapsed();

        let speedup = cold_time.as_secs_f64() / warm_time.as_secs_f64().max(0.00001);

        println!("  ⏱️ Cold build:    {:.3}s", cold_time.as_secs_f64());
        println!("  ⚡ Cached build:  {:.3}s", warm_time.as_secs_f64());
        println!("  📈 Cache speedup: {:.1}x", speedup);

        tested_count += 1;
    }

    println!(
        "\n✅ Successfully tested {} example project(s).",
        tested_count
    );
}
