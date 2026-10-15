// Integration tests for Crupp build tool
// Test suite covering multi-binary projects, root source_dir ("./"),
// vendor source deduplication, comptime metaprogramming, standalone execution,
// manifest commands (add-bin, add-lib, check-libs), and build profiles.

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

#[test]
fn test_comptime_multi_file_headers() {
    let dir = tempdir().unwrap();
    let root = dir.path();

    fs::create_dir_all(root.join("include")).unwrap();
    fs::create_dir_all(root.join("src")).unwrap();

    // 1. External header file with component definitions
    fs::write(
        root.join("include/components.h"),
        r#"
        #pragma once

        // @component @table("camera")
        typedef struct {
            float x, y;
            float zoom;
        } Camera2D;

        // @component @table("transform")
        typedef struct {
            float x, y;
            float rotation;
        } Transform2D;

        typedef struct {
            int score;
        } UnreflectedState;
        "#,
    )
    .unwrap();

    // 2. Source file that includes components.h and uses comptime reflection over it
    fs::write(
        root.join("src/main.c"),
        r#"
        #include <stdio.h>
        #include "components.h"

        #if comptime
        #include <comptime.h>

        int main(void) {
            emit("void print_components(void) {\n");
            for (int i = 0; i < struct_count(); i++) {
                const ct_struct* s = get_struct(i);
                if (struct_has_attr(s, "component")) {
                    emitf("    printf(\"FOUND: %%s\\n\", \"%s\");\n", s->name);
                }
            }
            emit("}\n");
            return 0;
        }
        #endif

        int main(void) {
            print_components();
            return 0;
        }
        "#,
    )
    .unwrap();

    fs::write(
        root.join("Crub.toml"),
        r#"
        [package]
        name = "multi_header_test"
        compiler = "gcc"
        standard = "-std=c17"
        source_dir = "./src"
        out_dir = "./build"

        include_dirs = ["include"]

        [[bin]]
        name = "multi_header_test"
        path = "src/main.c"
        "#,
    )
    .unwrap();

    let mut cmd = Command::cargo_bin("crupp").unwrap();
    cmd.current_dir(root)
        .arg("run")
        .assert()
        .success()
        .stdout(predicate::str::contains("FOUND: Camera2D"))
        .stdout(predicate::str::contains("FOUND: Transform2D"));
}

#[test]
fn test_standalone_single_file_run() {
    let dir = tempdir().unwrap();
    let root = dir.path();

    // Standalone file that is NOT mentioned in any Crub.toml
    fs::write(
        root.join("scratchpad.c"),
        r#"
        #include <stdio.h>
        int main(int argc, char** argv) {
            printf("STANDALONE_OK: %d\n", argc);
            return 0;
        }
        "#,
    )
    .unwrap();

    let mut cmd = Command::cargo_bin("crupp").unwrap();
    cmd.current_dir(root)
        .arg("run")
        .arg("scratchpad.c")
        .arg("--")
        .arg("arg1")
        .arg("arg2")
        .assert()
        .success()
        .stdout(predicate::str::contains("STANDALONE_OK: 3"));
}

#[test]
fn test_manifest_management_add_bin_and_lib() {
    let dir = tempdir().unwrap();
    let root = dir.path();

    fs::create_dir_all(root.join("src")).unwrap();
    fs::create_dir_all(root.join("tools")).unwrap();

    fs::write(root.join("src/main.c"), "int main(void) { return 0; }\n").unwrap();

    fs::write(
        root.join("tools/inspector.c"),
        r#"
        #include <stdio.h>
        int main(void) {
            printf("INSPECTOR_ACTIVE\n");
            return 0;
        }
        "#,
    )
    .unwrap();

    fs::write(
        root.join("Crub.toml"),
        r#"
        [package]
        name = "my_suite"
        compiler = "gcc"
        standard = "-std=c17"
        source_dir = "./src"
        out_dir = "./build"
        libs = []

        [[bin]]
        name = "my_suite"
        path = "src/main.c"
        "#,
    )
    .unwrap();

    // 1. Add binary target
    let mut add_bin_cmd = Command::cargo_bin("crupp").unwrap();
    add_bin_cmd
        .current_dir(root)
        .arg("add-bin")
        .arg("inspector")
        .arg("tools/inspector.c")
        .assert()
        .success()
        .stdout(predicate::str::contains("Added binary target 'inspector'"));

    // 2. Add library 'm'
    let mut add_lib_cmd = Command::cargo_bin("crupp").unwrap();
    add_lib_cmd
        .current_dir(root)
        .arg("add-lib")
        .arg("m")
        .assert()
        .success()
        .stdout(predicate::str::contains("Added library 'm'"));

    // 3. Check libraries
    let mut check_libs_cmd = Command::cargo_bin("crupp").unwrap();
    check_libs_cmd
        .current_dir(root)
        .arg("check-libs")
        .assert()
        .success()
        .stdout(predicate::str::contains("m"));

    // 4. Build newly added binary
    let mut build_cmd = Command::cargo_bin("crupp").unwrap();
    build_cmd
        .current_dir(root)
        .arg("build")
        .arg("--bin")
        .arg("inspector")
        .assert()
        .success()
        .stdout(predicate::str::contains("Linking inspector"));

    // 5. Run newly added binary
    let mut run_cmd = Command::cargo_bin("crupp").unwrap();
    run_cmd
        .current_dir(root)
        .arg("run")
        .arg("--bin")
        .arg("inspector")
        .assert()
        .success()
        .stdout(predicate::str::contains("INSPECTOR_ACTIVE"));
}

#[test]
fn test_compilation_profiles_and_custom_flags() {
    let dir = tempdir().unwrap();
    let root = dir.path();

    fs::create_dir_all(root.join("src")).unwrap();

    fs::write(
        root.join("src/main.c"),
        r#"
        #include <stdio.h>
        int main(void) {
            #ifdef CUSTOM_PROFILE_ACTIVE
            printf("CUSTOM_PROFILE_OK\n");
            #elif defined(NDEBUG)
            printf("RELEASE_PROFILE_OK\n");
            #else
            printf("DEFAULT_PROFILE_OK\n");
            #endif
            return 0;
        }
        "#,
    )
    .unwrap();

    fs::write(
        root.join("Crub.toml"),
        r#"
        [package]
        name = "prof_app"
        compiler = "gcc"
        standard = "-std=c17"
        source_dir = "./src"
        out_dir = "./build"

        [profile.fast]
        flags = ["-O2", "-DCUSTOM_PROFILE_ACTIVE=1"]
        "#,
    )
    .unwrap();

    // 1. Run with release profile
    let mut rel_cmd = Command::cargo_bin("crupp").unwrap();
    rel_cmd
        .current_dir(root)
        .arg("run")
        .arg("--release")
        .assert()
        .success()
        .stdout(predicate::str::contains("RELEASE_PROFILE_OK"));

    // 2. Run with custom profile 'fast'
    let mut custom_cmd = Command::cargo_bin("crupp").unwrap();
    custom_cmd
        .current_dir(root)
        .arg("run")
        .arg("--profile")
        .arg("fast")
        .assert()
        .success()
        .stdout(predicate::str::contains("CUSTOM_PROFILE_OK"));
}
