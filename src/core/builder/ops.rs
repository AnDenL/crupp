use crate::config::{Language, load_manifest_or_autodetect};
use crate::core::builder::diagnostics::{explain_compiler_error, explain_io_error};
use crate::core::scanner::{auto_discover_include_dirs, discover_sources};
use colored::*;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use tokio::process::Command;

pub async fn check_project(
    manifest_path: &str,
    _target_name: Option<&str>,
    verbose: bool,
) -> Result<(), String> {
    let (mut manifest, _) = load_manifest_or_autodetect(manifest_path)?;
    let pkg = &mut manifest.package;

    if pkg.include_dirs.is_empty() {
        pkg.include_dirs = auto_discover_include_dirs(Path::new("."), &pkg.out_dir);
    } else {
        pkg.include_dirs.retain(|d| !d.trim().is_empty());
    }

    let units = discover_sources(&pkg.source_dir, pkg);
    if units.is_empty() {
        return Err(format!("No source files found in '{}'.", pkg.source_dir));
    }

    println!("{}", "Checking syntax across translation units...".bright_cyan());

    for unit in &units {
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
        cmd.arg(&unit.path);

        if verbose {
            println!("  Checking: {:?}", cmd);
        }

        let output = cmd.output().await.map_err(|e| explain_io_error(&compiler_bin, &e))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            eprintln!("\n{} {}", "Syntax error in".red().bold(), unit.path.display());
            eprintln!("{}", stderr);
            explain_compiler_error(&stderr);
            return Err(format!("Check failed in {}", unit.path.display()));
        }
    }

    println!("{:>12} syntax check (no errors)", "Finished".bright_green().bold());
    Ok(())
}

pub fn clean_project(manifest_path: &str) -> Result<(), String> {
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
