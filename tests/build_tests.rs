use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;
use std::time::Instant;
use tempfile::tempdir;

#[test]
fn test_zero_config_pure_c() {
    let dir = tempdir().unwrap();
    let root = dir.path();

    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("src/utils.c"),
        r#"
        int calculate(int a, int b) {
            return a * b + 42;
        }
        "#,
    )
    .unwrap();

    fs::write(
        root.join("src/main.c"),
        r#"
        #include <stdio.h>
        int calculate(int a, int b);

        int main(void) {
            printf("RESULT: %d\n", calculate(3, 7));
            return 0;
        }
        "#,
    )
    .unwrap();

    let mut cmd = Command::cargo_bin("crupp").unwrap();
    cmd.current_dir(root)
        .env("CRUPP_NO_AUTO_NIX_SHELL", "1")
        .arg("build")
        .assert()
        .success()
        .stdout(predicate::str::contains("Finished"));

    let mut run_cmd = Command::cargo_bin("crupp").unwrap();
    run_cmd
        .current_dir(root)
        .env("CRUPP_NO_AUTO_NIX_SHELL", "1")
        .arg("run")
        .assert()
        .success()
        .stdout(predicate::str::contains("RESULT: 63"));
}

#[test]
fn test_hybrid_c_and_cpp() {
    let dir = tempdir().unwrap();
    let root = dir.path();

    fs::create_dir_all(root.join("src")).unwrap();

    fs::write(
        root.join("src/c_logic.c"),
        r#"
        int c_square(int n) {
            return n * n;
        }
        "#,
    )
    .unwrap();

    fs::write(
        root.join("src/main.cpp"),
        r#"
        #include <iostream>

        extern "C" int c_square(int n);

        int main() {
            std::cout << "HYBRID_VAL: " << c_square(9) << std::endl;
            return 0;
        }
        "#,
    )
    .unwrap();

    let mut cmd = Command::cargo_bin("crupp").unwrap();
    cmd.current_dir(root)
        .env("CRUPP_NO_AUTO_NIX_SHELL", "1")
        .arg("run")
        .assert()
        .success()
        .stdout(predicate::str::contains("HYBRID_VAL: 81"));
}

#[test]
fn test_cache_and_benchmark() {
    let dir = tempdir().unwrap();
    let root = dir.path();

    fs::create_dir_all(root.join("src")).unwrap();
    for i in 0..10 {
        fs::write(
            root.join(format!("src/worker_{}.c", i)),
            format!("int work_{}() {{ return {}; }}", i, i),
        )
        .unwrap();
    }

    fs::write(
        root.join("src/main.c"),
        r#"
        #include <stdio.h>
        int main() { return 0; }
        "#,
    )
    .unwrap();

    let start_cold = Instant::now();
    let mut cmd_cold = Command::cargo_bin("crupp").unwrap();
    cmd_cold
        .current_dir(root)
        .env("CRUPP_NO_AUTO_NIX_SHELL", "1")
        .arg("build")
        .assert()
        .success();
    let cold_duration = start_cold.elapsed();

    let start_warm = Instant::now();
    let mut cmd_warm = Command::cargo_bin("crupp").unwrap();
    cmd_warm
        .current_dir(root)
        .env("CRUPP_NO_AUTO_NIX_SHELL", "1")
        .arg("build")
        .assert()
        .success()
        .stdout(predicate::str::contains("Cached"));
    let warm_duration = start_warm.elapsed();

    println!("\n  Cold build: {:?}", cold_duration);
    println!("  Cached build: {:?}", warm_duration);

    assert!(warm_duration < cold_duration);
}
