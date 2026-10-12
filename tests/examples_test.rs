use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;
use std::path::Path;
use std::time::Instant;

#[test]
fn test_all_examples() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let examples_dir = manifest_dir.join("tests").join("examples");

    assert!(
        examples_dir.exists(),
        "Directory tests/examples not found: {}",
        examples_dir.display()
    );

    let mut tested_count = 0;

    for entry in fs::read_dir(&examples_dir).expect("Failed to read tests/examples") {
        let entry = entry.expect("Invalid directory entry");
        let path = entry.path();

        if !path.is_dir() {
            continue;
        }

        let project_name = path.file_name().unwrap().to_string_lossy().to_string();
        println!("\n========================================================");
        println!("🚀 Testing project: {}", project_name);

        // 1. Clean previous build artifacts for a cold build
        let build_dir = path.join("build");
        if build_dir.exists() {
            fs::remove_dir_all(&build_dir).expect("Failed to clean build directory");
        }

        // 2. Cold build
        let cold_start = Instant::now();
        let mut build_cmd = Command::cargo_bin("crupp").unwrap();
        build_cmd
            .current_dir(&path)
            .arg("build")
            .assert()
            .success()
            .stdout(predicate::str::contains("Finished"));
        let cold_time = cold_start.elapsed();

        // 3. Run target
        let mut run_cmd = Command::cargo_bin("crupp").unwrap();
        run_cmd.current_dir(&path).arg("run").assert().success();

        // 4. Warm build (verifying cache validity)
        let warm_start = Instant::now();
        let mut warm_cmd = Command::cargo_bin("crupp").unwrap();
        warm_cmd
            .current_dir(&path)
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

    assert!(
        tested_count > 0,
        "No projects found in tests/examples to build!"
    );
}
