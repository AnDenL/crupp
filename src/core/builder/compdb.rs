use crate::config::{Language, Package, load_manifest_or_autodetect};
use crate::core::scanner::{TranslationUnit, discover_sources};
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
    let current_dir = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let current_dir_str = current_dir.to_string_lossy().to_string();
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

        let (compiler_bin, standard) = match unit.language {
            Language::C => (pkg.compiler.resolve_c_compiler().0, &pkg.c_standard),
            Language::Cpp => (pkg.compiler.resolve_cpp_compiler().0, &pkg.standard),
        };

        let mut args = vec![
            compiler_bin,
            standard.clone(),
            "-c".to_string(),
            "-DCOMPTIME=0".to_string(),
            "-Dcomptime(x)=(x)".to_string(),
            "-Dtcc(x)=(x)".to_string(),
        ];
        args.extend(pkg.flags.clone());

        let mut seen_includes = HashSet::new();
        for inc in &pkg.include_dirs {
            let clean = inc.trim().trim_start_matches("-I").trim();
            if clean.is_empty() {
                continue;
            }
            let inc_path = current_dir.join(clean);
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
    let _ = fs::write(current_dir.join("compile_commands.json"), json);
    Ok(())
}

pub async fn export_compdb(manifest_path: &str) -> Result<(), String> {
    let (manifest, _) = load_manifest_or_autodetect(manifest_path)?;
    let pkg = &manifest.package;
    let obj_dir = PathBuf::from(&pkg.out_dir).join("obj");
    let units = discover_sources(&pkg.source_dir, pkg);

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
