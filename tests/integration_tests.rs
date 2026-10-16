// Integration tests for Crupp build tool
// Test suite covering multi-binary projects, root source_dir ("./"),
// vendor source deduplication, comptime metaprogramming (emit & printf fallback),
// standalone execution, manifest commands (add-bin, add-lib, check-libs),
// build profiles, clangd LSP comptime header generation, and NixOS integration.

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
        .env("CRUPP_NO_AUTO_NIX_SHELL", "1")
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
        .env("CRUPP_NO_AUTO_NIX_SHELL", "1")
        .arg("build")
        .assert()
        .success()
        .stdout(predicate::str::contains("Linking app"));

    let mut run_cmd = Command::cargo_bin("crupp").unwrap();
    run_cmd
        .current_dir(root)
        .env("CRUPP_NO_AUTO_NIX_SHELL", "1")
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
        .env("CRUPP_NO_AUTO_NIX_SHELL", "1")
        .arg("run")
        .assert()
        .success()
        .stdout(predicate::str::contains("COMPONENTS: 2"));
}

#[test]
fn test_comptime_printf_code_generation() {
    let dir = tempdir().unwrap();
    let root = dir.path();

    fs::create_dir_all(root.join("src")).unwrap();

    // Matches tests/examples/comptime/src/main.c: uses printf/math inside #if COMPTIME without emit()
    fs::write(
        root.join("src/main.c"),
        r#"
        #include <stdio.h>

        #if COMPTIME
        #include <stdio.h>
        #include <math.h>

        int main(void) {
            fprintf(stderr, "⚡ [crupp:comptime] Calculating table...\n");
            printf("static const float SINS[3] = { %.6ff, %.6ff, %.6ff };\n",
                sinf(0.0f), sinf(1.5707963f), sinf(3.1415926f));
            return 0;
        }
        #endif

        int main(void) {
            printf("Generated sine 90 deg: %.2f\n", SINS[1]);
            return 0;
        }
        "#,
    )
    .unwrap();

    fs::write(
        root.join("Crub.toml"),
        r#"
        [package]
        name = "comptime_printf"
        compiler = "gcc"
        standard = "-std=c17"
        source_dir = "./src"
        out_dir = "./build"
        "#,
    )
    .unwrap();

    let mut cmd = Command::cargo_bin("crupp").unwrap();
    cmd.current_dir(root)
        .env("CRUPP_NO_AUTO_NIX_SHELL", "1")
        .arg("run")
        .assert()
        .success()
        .stdout(predicate::str::contains("Generated sine 90 deg: 1.00"));
}

#[test]
fn test_comptime_multi_file_headers() {
    let dir = tempdir().unwrap();
    let root = dir.path();

    fs::create_dir_all(root.join("include")).unwrap();
    fs::create_dir_all(root.join("src")).unwrap();

    // 1. External header file with component definitions (including extern "C" wrapper)
    fs::write(
        root.join("include/components.h"),
        r#"
        #pragma once

        #ifdef __cplusplus
        extern "C" {
        #endif

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

        #ifdef __cplusplus
        }
        #endif
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
        .env("CRUPP_NO_AUTO_NIX_SHELL", "1")
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
        .env("CRUPP_NO_AUTO_NIX_SHELL", "1")
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
        .env("CRUPP_NO_AUTO_NIX_SHELL", "1")
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
        .env("CRUPP_NO_AUTO_NIX_SHELL", "1")
        .arg("add-lib")
        .arg("m")
        .assert()
        .success()
        .stdout(predicate::str::contains("Added library 'm'"));

    // 3. Check libraries
    let mut check_libs_cmd = Command::cargo_bin("crupp").unwrap();
    check_libs_cmd
        .current_dir(root)
        .env("CRUPP_NO_AUTO_NIX_SHELL", "1")
        .arg("check-libs")
        .assert()
        .success()
        .stdout(predicate::str::contains("m"));

    // 4. Build newly added binary
    let mut build_cmd = Command::cargo_bin("crupp").unwrap();
    build_cmd
        .current_dir(root)
        .env("CRUPP_NO_AUTO_NIX_SHELL", "1")
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
        .env("CRUPP_NO_AUTO_NIX_SHELL", "1")
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
        .env("CRUPP_NO_AUTO_NIX_SHELL", "1")
        .arg("run")
        .arg("--release")
        .assert()
        .success()
        .stdout(predicate::str::contains("RELEASE_PROFILE_OK"));

    // 2. Run with custom profile 'fast'
    let mut custom_cmd = Command::cargo_bin("crupp").unwrap();
    custom_cmd
        .current_dir(root)
        .env("CRUPP_NO_AUTO_NIX_SHELL", "1")
        .arg("run")
        .arg("--profile")
        .arg("fast")
        .assert()
        .success()
        .stdout(predicate::str::contains("CUSTOM_PROFILE_OK"));
}

#[test]
fn test_scene_comptime_with_comments_and_parallel_units() {
    let dir = tempdir().unwrap();
    let root = dir.path();

    fs::create_dir_all(root.join("include")).unwrap();
    fs::create_dir_all(root.join("src/core/scene")).unwrap();
    fs::create_dir_all(root.join("game")).unwrap();

    fs::write(
        root.join("include/en_components.h"),
        r#"
        #pragma once
        #ifdef __cplusplus
        extern "C" {
        #endif

        typedef float f32;
        typedef int i32;

        // How this works:
        // All structs marked with `@component` are reflected.

        typedef struct {
            int unused_internal;
        } EnInternalHelper;

        // @component @table("camera")
        typedef struct {
            f32 x, y;
            // @default(64.0f)
            f32 zoom;
        } EnCamera2D;

        // @component @table("transform")
        typedef struct {
            f32 x, y;
            f32 rotation;
            f32 scale_x; // @default(1.0f)
            f32 scale_y; // @default(1.0f)
        } EnTransform2D;

        #ifdef __cplusplus
        }
        #endif
        "#,
    )
    .unwrap();

    fs::write(
        root.join("src/core/scene/scene.c"),
        r#"
        #include "en_components.h"
        #include <stdio.h>
        #include <string.h>

        // ============================================================================
        // COMPTIME GENERATOR BLOCK
        // ============================================================================
        // How this works:
        // 1. Crupp extracts metadata from all structs marked with `@component`.
        // 2. TCC executes this block in RAM during compilation.
        // 3. `emit()` and `emitf()` replace this entire `#if comptime ... #endif` block
        //    with optimized, zero-overhead C code.
        // ============================================================================
        #if comptime
        #include <comptime.h>

        int main(void) {
            int total = struct_count();
            printf("Comptime debug: scanning %d structs\n", total);

            int registred = 0;
            for (int i = 0; i < total; i++) {
                const ct_struct* s = get_struct(i);
                printf("%s\n", s->name);
                if (!struct_has_attr(s, "component")) continue;
                registred++;

                const char* table = struct_attr_or(s, "table", s->name);
                emitf("static void init_%s(%s* out) {\n", table, s->name);
                emitf("    *out = (%s){\n", s->name);
                for (int j = 0; j < s->field_count; j++) {
                    const ct_field* f = &s->fields[j];
                    const char* def = field_get_attr(f, "def");
                    // Also verify strcmp(def, ...) is NULL-safe even when def is NULL!
                    if (strcmp(def, "") != 0 && strlen(def) > 0) {
                        printf("        .%s = %s,\n", f->name, def);
                        emitf("        .%s = %s,\n", f->name, def);
                    }
                }
                emit("    };\n}\n\n");
            }

            emit("void populate_scene_bundles(int* out_count) {\n");
            emit("    EnCamera2D cam; init_camera(&cam);\n");
            emit("    EnTransform2D tr; init_transform(&tr);\n");
            emit("    int c = 0;\n");
            for (int i = 0; i < total; i++) {
                const ct_struct* s = get_struct(i);
                if (!struct_has_attr(s, "component")) continue;
                emitf("    c++; // %s\n", s->name);
            }
            emit("    if (cam.zoom == 64.0f && tr.scale_x == 1.0f) { *out_count = c; }\n");
            emit("}\n");
            printf("Registred: %i\n", registred);
            return 0;
        }
        #endif

        int run_scene_loader(void) {
            int count = 0;
            populate_scene_bundles(&count);
            return count;
        }
        "#,
    )
    .unwrap();

    fs::write(
        root.join("game/main.c"),
        r#"
        #include <stdio.h>
        int run_scene_loader(void);

        #if comptime
        #include <comptime.h>
        int main(void) {
            emit("int get_magic(void) { return 77; }\n");
            return 0;
        }
        #endif

        int main(void) {
            printf("SCENE_COMPONENTS: %d, MAGIC: %d\n", run_scene_loader(), get_magic());
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
        flags = ["-Wall", "-Wextra", "-Werror"]

        [[bin]]
        name = "en_runner"
        path = "game/main.c"
        "#,
    )
    .unwrap();

    let mut cmd = Command::cargo_bin("crupp").unwrap();
    cmd.current_dir(root)
        .env("CRUPP_NO_AUTO_NIX_SHELL", "1")
        .arg("run")
        .assert()
        .success()
        .stdout(predicate::str::contains("SCENE_COMPONENTS: 2, MAGIC: 77"));
}

#[test]
fn test_compdb_generation_and_nixos_includes() {
    let dir = tempdir().unwrap();
    let root = dir.path();

    let mock_nix_inc = root.join("nix_store_mock_include");
    fs::create_dir_all(&mock_nix_inc).unwrap();
    fs::write(
        mock_nix_inc.join("nix_mock_hdr.h"),
        "#pragma once\n#define NIX_STORE_VAL 123\n",
    )
    .unwrap();

    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("src/main.c"),
        r#"
        #include <stdio.h>
        #include <nix_mock_hdr.h>
        int main(void) {
            printf("NIX_VAL: %d\n", NIX_STORE_VAL);
            return 0;
        }
        "#,
    )
    .unwrap();

    let nix_cflags = format!("-isystem {}", mock_nix_inc.display());

    // 1. Generate compile_commands.json via 'crupp compdb' with NIX_CFLAGS_COMPILE
    let mut compdb_cmd = Command::cargo_bin("crupp").unwrap();
    compdb_cmd
        .current_dir(root)
        .env("CRUPP_NO_AUTO_NIX_SHELL", "1")
        .env("NIX_CFLAGS_COMPILE", &nix_cflags)
        .arg("compdb")
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "compile_commands.json generated successfully",
        ));

    let compdb_content = fs::read_to_string(root.join("compile_commands.json")).unwrap();
    assert!(compdb_content.contains("-isystem"));
    assert!(compdb_content.contains(&mock_nix_inc.to_string_lossy().to_string()));
    assert!(compdb_content.contains("tcc_home/include"));
    assert!(root.join("build/obj/tcc_home/include/comptime.h").exists());

    // 2. Also verify 'crupp run' honors NIX_CFLAGS_COMPILE
    let mut run_cmd = Command::cargo_bin("crupp").unwrap();
    run_cmd
        .current_dir(root)
        .env("CRUPP_NO_AUTO_NIX_SHELL", "1")
        .env("NIX_CFLAGS_COMPILE", &nix_cflags)
        .arg("run")
        .assert()
        .success()
        .stdout(predicate::str::contains("NIX_VAL: 123"));
}

#[test]
fn test_nix_shell_and_flake_generation() {
    let dir = tempdir().unwrap();
    let root = dir.path();

    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("src/main.c"), "int main(void) { return 0; }\n").unwrap();
    fs::write(
        root.join("Crub.toml"),
        r#"
        [package]
        name = "nix_demo"
        compiler = "gcc"
        standard = "-std=c17"
        libs = ["z", "SDL2", "sqlite3"]
        "#,
    )
    .unwrap();

    let mut nix_cmd = Command::cargo_bin("crupp").unwrap();
    nix_cmd
        .current_dir(root)
        .env("CRUPP_NO_AUTO_NIX_SHELL", "1")
        .arg("nix")
        .arg("--flake")
        .arg("--envrc")
        .assert()
        .success()
        .stdout(predicate::str::contains("Created 'shell.nix'"))
        .stdout(predicate::str::contains("Created 'flake.nix'"))
        .stdout(predicate::str::contains("Created '.envrc'"));

    let shell_nix = fs::read_to_string(root.join("shell.nix")).unwrap();
    assert!(shell_nix.contains("pkg-config"));
    assert!(shell_nix.contains("clang-tools"));
    assert!(shell_nix.contains("zlib"));
    assert!(shell_nix.contains("SDL2"));
    assert!(shell_nix.contains("sqlite"));

    let flake_nix = fs::read_to_string(root.join("flake.nix")).unwrap();
    assert!(flake_nix.contains("devShells"));
    assert!(flake_nix.contains("zlib"));
}
