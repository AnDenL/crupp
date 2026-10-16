use crate::config::{Language, load_manifest_or_autodetect};
use crate::core::builder::diagnostics::{explain_compiler_error, explain_io_error};
use crate::core::scanner::{auto_discover_include_dirs, discover_sources, scan_file};
use colored::*;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use tokio::process::Command;

pub async fn check_project(
    manifest_path: &str,
    target_name: Option<&str>,
    verbose: bool,
) -> Result<(), String> {
    crate::core::nix::activate_nix_env_if_available();

    let (mut manifest, _) = load_manifest_or_autodetect(manifest_path)?;
    let pkg = &mut manifest.package;

    if pkg.include_dirs.is_empty() {
        pkg.include_dirs = auto_discover_include_dirs(Path::new("."), &pkg.out_dir);
    } else {
        pkg.include_dirs.retain(|d| !d.trim().is_empty());
    }

    let current_dir = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let mut seen_canonical = HashSet::new();
    let mut units = discover_sources(&pkg.source_dir, pkg);

    units.retain_mut(|u| {
        let canon = fs::canonicalize(&u.path).unwrap_or_else(|_| u.path.clone());
        if !seen_canonical.insert(canon.clone()) {
            return false;
        }
        if let Ok(rel) = canon.strip_prefix(&current_dir) {
            u.path = rel.to_path_buf();
        }
        true
    });

    for bin in &manifest.bin {
        let clean = bin.path.trim_start_matches("./");
        let p = PathBuf::from(clean);
        if p.exists() {
            let canon = fs::canonicalize(&p).unwrap_or_else(|_| p.clone());
            if seen_canonical.insert(canon.clone()) {
                let clean_p = if let Ok(rel) = canon.strip_prefix(&current_dir) {
                    rel.to_path_buf()
                } else {
                    p
                };
                if let Some(unit) = scan_file(&clean_p, pkg) {
                    units.push(unit);
                }
            }
        }
    }

    if let Some(req) = target_name {
        let req_path = Path::new(req);
        if req_path.is_file() {
            if let Some(u) = scan_file(req_path, pkg) {
                units = vec![u];
            }
        }
    }

    if units.is_empty() {
        return Err(format!("No source files found in '{}'.", pkg.source_dir));
    }

    println!(
        "{}",
        "Checking syntax across translation units...".bright_cyan()
    );

    let obj_dir = PathBuf::from(&pkg.out_dir).join("obj");
    let _ = fs::create_dir_all(&obj_dir);
    let nix_cflags = crate::core::nix::extra_compiler_flags(&pkg.libs);

    for unit in &units {
        let raw_source = fs::read_to_string(&unit.path).map_err(|e| e.to_string())?;
        let file_to_check = if crate::core::comptime::has_comptime_markers(&raw_source) {
            let transformed = crate::core::comptime::transform_comptime(
                &raw_source,
                &pkg.include_dirs,
                &obj_dir,
                &unit.path,
            )?;
            if transformed != raw_source {
                let gen_dir = obj_dir.join("gen");
                fs::create_dir_all(&gen_dir).map_err(|e| e.to_string())?;
                let safe_name = unit
                    .path
                    .to_string_lossy()
                    .replace(['/', '\\', '.'], "_");
                let shadow_file = gen_dir.join(format!("check_{}.c", safe_name));
                let abs_orig = fs::canonicalize(&unit.path).unwrap_or_else(|_| unit.path.clone());
                let escaped_orig = abs_orig
                    .to_string_lossy()
                    .replace('\\', "\\\\")
                    .replace('"', "\\\"");
                let final_shadow = format!("#line 1 \"{}\"\n{}", escaped_orig, transformed);
                fs::write(&shadow_file, final_shadow).map_err(|e| e.to_string())?;
                shadow_file
            } else {
                unit.path.clone()
            }
        } else {
            unit.path.clone()
        };

        let (compiler_bin, standard, lang_flags) = match unit.language {
            Language::C => (
                pkg.compiler.resolve_c_compiler().0,
                &pkg.c_standard,
                &pkg.c_flags,
            ),
            Language::Cpp => (
                pkg.compiler.resolve_cpp_compiler().0,
                &pkg.standard,
                &pkg.cpp_flags,
            ),
        };

        let mut cmd = Command::new(&compiler_bin);
        cmd.arg(standard).arg("-fsyntax-only");
        for flag in &pkg.flags {
            for part in flag.split_whitespace() {
                cmd.arg(part);
            }
        }
        for flag in lang_flags {
            for part in flag.split_whitespace() {
                cmd.arg(part);
            }
        }
        for flag in &nix_cflags {
            cmd.arg(flag);
        }

        cmd.arg("-DCOMPTIME=0");
        cmd.arg("-Dcomptime(x)=(x)");
        cmd.arg("-Dtcc(x)=(x)");

        let mut seen_includes = HashSet::new();
        for inc in &pkg.include_dirs {
            let clean = inc.trim().trim_start_matches("-I").trim();
            if clean.is_empty() {
                continue;
            }
            if seen_includes.insert(clean.to_string()) {
                cmd.arg(format!("-I{}", clean));
            }
        }
        if let Some(parent) = unit.path.parent() {
            let p_str = parent.to_string_lossy();
            let clean = p_str.trim().trim_start_matches("./").trim();
            if !clean.is_empty() && clean != "." {
                if seen_includes.insert(clean.to_string()) {
                    cmd.arg(format!("-I{}", clean));
                }
            }
        }
        cmd.arg(&file_to_check);

        if verbose {
            println!("  Checking: {:?}", cmd);
        }

        let output = cmd
            .output()
            .await
            .map_err(|e| explain_io_error(&compiler_bin, &e))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            eprintln!(
                "\n{} {}",
                "Syntax error in".red().bold(),
                unit.path.display()
            );
            eprintln!("{}", stderr);
            explain_compiler_error(&stderr, unit.language);
            return Err(format!("Check failed in {}", unit.path.display()));
        }
    }

    println!(
        "{:>12} syntax check (no errors)",
        "Finished".bright_green().bold()
    );
    Ok(())
}

pub fn _clean_project(manifest_path: &str) -> Result<(), String> {
    clean_project_with_all(manifest_path, false)
}

pub fn clean_project_with_all(manifest_path: &str, clean_all: bool) -> Result<(), String> {
    let (manifest, _) = load_manifest_or_autodetect(manifest_path)?;
    let out_dir = PathBuf::from(&manifest.package.out_dir);
    if out_dir.exists() {
        fs::remove_dir_all(&out_dir)
            .map_err(|e| format!("Failed to remove '{}': {}", out_dir.display(), e))?;
        println!("{:>12} '{}'", "Removed".green().bold(), out_dir.display());
    } else {
        println!("{:>12} (already clean)", "Finished".bright_green().bold());
    }

    if clean_all {
        let compdb = Path::new("compile_commands.json");
        if compdb.exists() {
            let _ = fs::remove_file(compdb);
            println!("{:>12} 'compile_commands.json'", "Removed".green().bold());
        }
    }
    Ok(())
}
