// Integration tests for Crupp build tool
// Test suite covering multi-binary projects, root source_dir ("./"),
// vendor source deduplication, and comptime metaprogramming.

use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;
use tempfile::tempdir;

#[test]
fn test_multi_binary_root_source_dir() {
    let dir = tempdir().unwrap();
    let root = dir.path();

    // Create engine directory structure
    fs::create_dir_all(root.join("game")).unwrap();
    fs::create_dir_all(root.join("editor")).unwrap();
    fs::create_dir_all(root.join("src/core")).unwrap();
    fs::create_dir_all(root.join("include")).unwrap();

    // Core library files (without main)
    fs::write(
        root.join("src/core/world.c"),
        r#"
        void en_world(void) {}
        void en_create(void) {}
        "#,
    )
    .unwrap();

    fs::write(
        root.join("include/en.h"),
        r#"
        #pragma once
        void en_world(void);
        void en_create(void);
        "#,
    )
    .unwrap();

    // Two independent binaries with their own main()
    fs::write(
        root.join("game/main.c"),
        r#"
        #include <stdio.h>
        #include "../include/en.h"

        int main(void) {
            en_world();
            en_create();
            printf("RUNNER_OK\n");
            return 0;
        }
        "#,
    )
    .unwrap();

    fs::write(
        root.join("editor/main.c"),
        r#"
        #include <stdio.h>
        #include "../include/en.h"

        int main(void) {
            en_world();
            printf("EDITOR_OK\n");
            return 0;
        }
        "#,
    )
    .unwrap();

    fs::write(
        root.join("Crub.toml"),
        r#"
        [package]
        name = "en"
        compiler = "gcc"
        standard = "-std=c17"
        source_dir = "./"
        out_dir = "./build_crupp"

        include_dirs = ["include"]

        [[bin]]
        name = "en_editor"
        path = "editor/main.c"

        [[bin]]
        name = "en_runner"
        path = "game/main.c"
        "#,
    )
    .unwrap();

    let mut cmd = Command::cargo_bin("crupp").unwrap();
    cmd.current_dir(root)
        .arg("build")
        .assert()
        .success()
        .stdout(predicate::str::contains("Linking en_editor"))
        .stdout(predicate::str::contains("Linking en_runner"));
}

#[test]
fn test_vendor_source_deduplication() {
    let dir = tempdir().unwrap();
    let root = dir.path();

    fs::create_dir_all(root.join("game")).unwrap();
    fs::create_dir_all(root.join("vendor/tomlc99")).unwrap();

    // Vendor library: toml.c and toml.h
    fs::write(
        root.join("vendor/tomlc99/toml.h"),
        r#"
        #pragma once
        int toml_parse(const char* str);
        "#,
    )
    .unwrap();

    fs::write(
        root.join("vendor/tomlc99/toml.c"),
        r#"
        #include "toml.h"
        int toml_parse(const char* str) {
            return (str != (void*)0) ? 42 : 0;
        }
        "#,
    )
    .unwrap();

    // Main file that includes vendor/tomlc99/toml.h
    fs::write(
        root.join("game/main.c"),
        r#"
        #include <stdio.h>
        #include "../vendor/tomlc99/toml.h"

        int main(void) {
            printf("TOML_RES: %d\n", toml_parse("key = 1"));
            return 0;
        }
        "#,
    )
    .unwrap();

    fs::write(
        root.join("Crub.toml"),
        r#"
        [package]
        name = "app"
        compiler = "gcc"
        standard = "-std=c17"
        source_dir = "./"
        out_dir = "./build"

        include_dirs = ["vendor/tomlc99"]

        [[bin]]
        name = "app"
        path = "game/main.c"
        "#,
    )
    .unwrap();

    // Build must succeed with NO duplicate symbol definitions
    let mut cmd = Command::cargo_bin("crupp").unwrap();
    cmd.current_dir(root)
        .arg("build")
        .assert()
        .success()
        .stdout(predicate::str::contains("Linking app"));

    let mut run_cmd = Command::cargo_bin("crupp").unwrap();
    run_cmd
        .current_dir(root)
        .arg("run")
        .assert()
        .success()
        .stdout(predicate::str::contains("TOML_RES: 42"));
}

#[test]
fn test_comptime_reflection_and_generation() {
    let dir = tempdir().unwrap();
    let root = dir.path();

    fs::create_dir_all(root.join("src")).unwrap();

    fs::write(
        root.join("src/main.c"),
        r#"
        #include <stdio.h>

        // @component
        typedef struct {
            float x, y;
        } Position;

        // @component
        typedef struct {
            float vx, vy;
        } Velocity;

        #if comptime
        #include <comptime.h>

        int main(void) {
            emit("int get_component_count(void) {\n");
            int count = 0;
            for (int i = 0; i < struct_count(); i++) {
                const ct_struct* s = get_struct(i);
                if (struct_has_attr(s, "component")) {
                    count++;
                }
            }
            emitf("    return %d;\n", count);
            emit("}\n");
            return 0;
        }
        #endif

        int main(void) {
            printf("COMPONENTS: %d\n", get_component_count());
            return 0;
        }
        "#,
    )
    .unwrap();

    fs::write(
        root.join("Crub.toml"),
        r#"
        [package]
        name = "comptime_test"
        compiler = "gcc"
        standard = "-std=c17"
        source_dir = "./src"
        out_dir = "./build"

        [[bin]]
        name = "comptime_test"
        path = "src/main.c"
        "#,
    )
    .unwrap();

    let mut cmd = Command::cargo_bin("crupp").unwrap();
    cmd.current_dir(root)
        .arg("run")
        .assert()
        .success()
        .stdout(predicate::str::contains("COMPONENTS: 2"));
}
