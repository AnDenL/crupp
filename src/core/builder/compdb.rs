use crate::config::{Language, Package, load_manifest_or_autodetect};
use crate::core::scanner::{TranslationUnit, auto_discover_include_dirs, discover_sources, scan_file};
use colored::*;
use petgraph::graph::NodeIndex;
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Serialize)]
pub struct CompileCommand {
    pub directory: String,
    pub arguments: Vec<String>,
    pub file: String,
    pub output: String,
}

pub fn write_compdb(
    pkg: &Package,
    units: &[TranslationUnit],
    deep_hashes: &HashMap<NodeIndex, String>,
    node_indices: &[NodeIndex],
    obj_dir: &Path,
) -> Result<(), String> {
    crate::core::nix::activate_nix_env_if_available();

    let current_dir = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let current_dir_str = current_dir.to_string_lossy().to_string();

    // Ensure tcc_home/include/comptime.h exists on disk so clangd/LSP can always resolve <comptime.h>
    let tcc_inc_dir = obj_dir.join("tcc_home").join("include");
    if !tcc_inc_dir.join("comptime.h").exists() {
        let default_comptime = crate::core::comptime::header::generate_comptime_header(&[], &[]);
        let _ = crate::core::comptime::executor::prepare_unit_headers(
            obj_dir,
            Path::new("compdb_init.c"),
            &default_comptime,
        );
    }
    let abs_tcc_inc = fs::canonicalize(&tcc_inc_dir).unwrap_or_else(|_| current_dir.join(&tcc_inc_dir));

    let pkg_cflags = crate::core::nix::pkg_config_cflags(&pkg.libs);
    let mut commands = Vec::with_capacity(units.len());

    for (i, unit) in units.iter().enumerate() {
        let node_idx = node_indices[i];
        let deep_hash = &deep_hashes[&node_idx];
        let rel_path = unit.path.strip_prefix(".").unwrap_or(&unit.path);
        let safe_name = rel_path
            .to_string_lossy()
            .replace(['/', '\\'], "_")
            .replace('.', "_");
        let hash_prefix = if deep_hash.len() >= 8 {
            &deep_hash[0..8]
        } else {
            deep_hash.as_str()
        };
        let obj_name = format!("{}_{}.o", safe_name, hash_prefix);
        let abs_file =
            fs::canonicalize(&unit.path).unwrap_or_else(|_| current_dir.join(&unit.path));
        let abs_output = current_dir.join(obj_dir).join(&obj_name);

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

        let resolved_compiler_path = crate::core::nix::resolve_executable_path(&compiler_bin);

        let mut args = vec![
            resolved_compiler_path,
            standard.clone(),
            "-c".to_string(),
            "-DCOMPTIME=0".to_string(),
            "-Dcomptime(x)=(x)".to_string(),
            "-Dtcc(x)=(x)".to_string(),
        ];

        for flag in &pkg.flags {
            for part in flag.split_whitespace() {
                args.push(part.to_string());
            }
        }
        for flag in lang_flags {
            for part in flag.split_whitespace() {
                args.push(part.to_string());
            }
        }
        for flag in &pkg_cflags {
            args.push(flag.clone());
        }

        let mut seen_includes = HashSet::new();
        for inc in &pkg.include_dirs {
            let clean = inc.trim().trim_start_matches("-I").trim();
            if clean.is_empty() {
                continue;
            }
            let p = Path::new(clean);
            let inc_path = if p.is_absolute() {
                p.to_path_buf()
            } else {
                current_dir.join(clean)
            };
            let inc_str = inc_path.to_string_lossy().to_string();
            if seen_includes.insert(inc_str.clone()) {
                args.push(format!("-I{}", inc_str));
            }
        }

        if let Some(parent) = unit.path.parent() {
            let p_str = parent.to_string_lossy();
            let clean = p_str.trim().trim_start_matches("./").trim();
            if !clean.is_empty() && clean != "." {
                let inc_path = current_dir.join(clean);
                let inc_str = inc_path.to_string_lossy().to_string();
                if seen_includes.insert(inc_str.clone()) {
                    args.push(format!("-I{}", inc_str));
                }
            }
        }

        // Include generated <comptime.h> directory so clangd resolves comptime APIs
        let tcc_inc_str = abs_tcc_inc.to_string_lossy().to_string();
        if seen_includes.insert(tcc_inc_str.clone()) {
            args.push(format!("-I{}", tcc_inc_str));
        }

        // Embed system & NixOS store include directories (-isystem) so clangd on NixOS finds
        // <stdio.h>, <iostream>, <vector>, and nix-shell library headers out of the box!
        for sys_inc in crate::core::nix::query_compiler_system_includes(&compiler_bin, unit.language) {
            let sys_str = sys_inc.to_string_lossy().to_string();
            if seen_includes.insert(sys_str.clone()) {
                args.push("-isystem".to_string());
                args.push(sys_str);
            }
        }

        args.push(abs_file.to_string_lossy().to_string());
        args.push("-o".to_string());
        args.push(abs_output.to_string_lossy().to_string());

        commands.push(CompileCommand {
            directory: current_dir_str.clone(),
            arguments: args,
            file: abs_file.to_string_lossy().to_string(),
            output: abs_output.to_string_lossy().to_string(),
        });
    }

    let json = serde_json::to_string_pretty(&commands).map_err(|e| e.to_string())?;
    fs::write(current_dir.join("compile_commands.json"), json).map_err(|e| e.to_string())?;
    Ok(())
}

pub async fn export_compdb(manifest_path: &str) -> Result<(), String> {
    crate::core::nix::activate_nix_env_if_available();

    let (mut manifest, _) = load_manifest_or_autodetect(manifest_path)?;
    if manifest.package.include_dirs.is_empty() {
        manifest.package.include_dirs =
            auto_discover_include_dirs(Path::new("."), &manifest.package.out_dir);
    } else {
        manifest
            .package
            .include_dirs
            .retain(|d| !d.trim().is_empty());
    }

    let pkg = &manifest.package;
    let obj_dir = PathBuf::from(&pkg.out_dir).join("obj");
    let _ = fs::create_dir_all(&obj_dir);

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

    let mut deep_hashes = HashMap::with_capacity(units.len());
    let mut node_indices = Vec::with_capacity(units.len());
    for (i, u) in units.iter().enumerate() {
        let n = NodeIndex::new(i);
        node_indices.push(n);
        deep_hashes.insert(n, u.base_hash.clone());
    }

    write_compdb(pkg, &units, &deep_hashes, &node_indices, &obj_dir)?;
    println!(
        "{}",
        "compile_commands.json generated successfully!".bright_green()
    );
    Ok(())
}
