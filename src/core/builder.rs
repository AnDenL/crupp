use super::scanner::{TranslationUnit, discover_sources};
use crate::config::{BinTarget, CompilerType, Language, Package, load_manifest_or_autodetect};
use colored::*;
use futures::future::join_all;
use indicatif::{ProgressBar, ProgressStyle};
use petgraph::Direction;
use petgraph::algo::{tarjan_scc, toposort};
use petgraph::graph::{DiGraph, NodeIndex};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::process::Command;

struct BuildContext {
    package: Package,
    obj_dir: PathBuf,
    pb: ProgressBar,
}

#[derive(Serialize)]
struct CompileCommand {
    directory: String,
    arguments: Vec<String>,
    file: String,
    output: String,
}

pub async fn build_project(manifest_path: &str, target_name: Option<&str>) -> Result<(), String> {
    let (mut manifest, is_autodetected) = load_manifest_or_autodetect(manifest_path)?;
    let pkg = &mut manifest.package;

    if is_autodetected {
        println!(
            "{}",
            "  No Crub.toml found. Running in auto-detected mode...".yellow()
        );
    }

    if pkg.include_dirs.is_empty() {
        pkg.include_dirs = super::scanner::auto_discover_include_dirs(Path::new("."), &pkg.out_dir);
    }

    let build_dir = PathBuf::from(&pkg.out_dir);
    let obj_dir = build_dir.join("obj");
    fs::create_dir_all(&obj_dir).map_err(|e| format!("Failed to create build dir: {}", e))?;

    let mut units = discover_sources(&pkg.source_dir, pkg);

    for bin in &manifest.bin {
        let clean = bin.path.trim_start_matches("./");
        let p = PathBuf::from(clean);
        if p.exists() && !units.iter().any(|u| u.path.ends_with(&p)) {
            if let Some(unit) = super::scanner::scan_file(&p, pkg) {
                units.push(unit);
            }
        }
    }

    if units.is_empty() {
        return Err(format!(
            "No C or C++ source files found in '{}'.",
            pkg.source_dir
        ));
    }

    if manifest.bin.is_empty() {
        manifest.bin = auto_detect_bin_targets(&units, &pkg.name);
    }

    let targets_to_link: Vec<BinTarget> = if let Some(name) = target_name {
        let filtered: Vec<BinTarget> = manifest
            .bin
            .iter()
            .filter(|b| b.name == name)
            .cloned()
            .collect();
        if filtered.is_empty() {
            let available: Vec<String> = manifest
                .bin
                .iter()
                .map(|b| format!("'{}'", b.name))
                .collect();
            return Err(format!(
                "Target '{}' not found!\n    Available targets: {}",
                name.bright_red().bold(),
                if available.is_empty() {
                    "none".to_string()
                } else {
                    available.join(", ")
                }
            ));
        }
        filtered
    } else {
        manifest.bin.clone()
    };

    let mut module_to_node = HashMap::new();
    let mut path_to_node = HashMap::new();
    let mut node_indices = Vec::new();
    let mut graph = DiGraph::<usize, ()>::new();

    for (i, unit) in units.iter().enumerate() {
        let idx = graph.add_node(i);
        node_indices.push(idx);
        path_to_node.insert(unit.path.clone(), idx);
        if let Some(mod_name) = &unit.exported_module {
            module_to_node.insert(mod_name.clone(), idx);
        }
    }

    for (i, unit) in units.iter().enumerate() {
        for imp in &unit.imports {
            if let Some(&target_idx) = module_to_node.get(imp) {
                graph.add_edge(target_idx, node_indices[i], ());
            } else {
                return Err(format!(
                    "Module '{}' imported in '{}' does not exist!",
                    imp.bright_red().bold(),
                    unit.path.display().to_string().cyan()
                ));
            }
        }
    }

    let order = match toposort(&graph, None) {
        Ok(ord) => ord,
        Err(_) => {
            let sccs = tarjan_scc(&graph);
            let cycle = sccs
                .iter()
                .find(|scc| scc.len() > 1 || graph.contains_edge(scc[0], scc[0]))
                .expect("Cycle detected");

            let mut cycle_info = Vec::new();
            for &node_idx in cycle {
                let unit = &units[graph[node_idx]];
                cycle_info.push(format!(
                    "{} ({})",
                    unit.exported_module
                        .as_deref()
                        .unwrap_or("unit")
                        .bright_cyan(),
                    unit.path.display()
                ));
            }

            return Err(format!(
                "Circular module dependency:\n  {}",
                cycle_info.join(" -> ")
            ));
        }
    };

    let mut deep_hashes = HashMap::new();
    for &node_idx in &order {
        let unit_idx = graph[node_idx];
        let mut hasher = Sha256::new();
        hasher.update(&units[unit_idx].base_hash);
        for neighbor in graph.neighbors_directed(node_idx, Direction::Incoming) {
            if let Some(dep_hash) = deep_hashes.get(&neighbor) {
                hasher.update(dep_hash);
            }
        }
        deep_hashes.insert(node_idx, hex::encode(hasher.finalize()));
    }

    let _ = write_compdb(pkg, &units, &deep_hashes, &node_indices, &obj_dir);

    let pb = ProgressBar::new(graph.node_count() as u64);
    pb.set_style(
        ProgressStyle::default_bar()
            .template(
                "{spinner:.cyan.bold} [{bar:40.cyan/blue}] {pos}/{len} ({percent}%) {msg:.yellow}",
            )
            .unwrap_or_else(|_| ProgressStyle::default_bar())
            .progress_chars("=>-"),
    );
    pb.set_message("Compiling project...");

    let build_ctx = Arc::new(BuildContext {
        package: pkg.clone(),
        obj_dir: obj_dir.clone(),
        pb: pb.clone(),
    });

    let mut in_degrees: HashMap<NodeIndex, usize> = HashMap::new();
    for node in graph.node_indices() {
        in_degrees.insert(
            node,
            graph.edges_directed(node, Direction::Incoming).count(),
        );
    }

    let mut completed = HashSet::new();
    let mut obj_files_map: HashMap<NodeIndex, PathBuf> = HashMap::new();

    while completed.len() < graph.node_count() {
        let mut current_wave = Vec::new();
        for node in graph.node_indices() {
            if !completed.contains(&node) && in_degrees[&node] == 0 {
                current_wave.push(node);
            }
        }

        if current_wave.is_empty() {
            pb.finish_and_clear();
            return Err("Deadlock detected during task graph execution!".to_string());
        }

        let mut tasks = Vec::new();
        for &node_idx in &current_wave {
            let unit = units[graph[node_idx]].clone();
            let deep_hash = deep_hashes[&node_idx].clone();
            let ctx = Arc::clone(&build_ctx);
            tasks.push(tokio::spawn(async move {
                compile_unit(ctx, unit, deep_hash)
                    .await
                    .map(|obj_path| (node_idx, obj_path))
            }));
        }

        for res in join_all(tasks).await {
            match res {
                Ok(Ok((node_idx, obj_path))) => {
                    obj_files_map.insert(node_idx, obj_path);
                    completed.insert(node_idx);
                    pb.inc(1);
                    for neighbor in graph.neighbors_directed(node_idx, Direction::Outgoing) {
                        *in_degrees.get_mut(&neighbor).unwrap() -= 1;
                    }
                }
                Ok(Err(e)) => {
                    pb.finish_and_clear();
                    return Err(e);
                }
                Err(e) => {
                    pb.finish_and_clear();
                    return Err(format!("Compilation task failed: {}", e));
                }
            }
        }
    }
    pb.finish_and_clear();

    println!(
        "{}",
        "Compiled successfully. Linking targets...".bright_cyan()
    );

    let has_any_cpp = units.iter().any(|u| u.language == Language::Cpp);
    let linker_cmd = if has_any_cpp {
        pkg.compiler.cpp_compiler()
    } else {
        pkg.compiler.c_compiler()
    };

    let all_main_nodes: HashSet<NodeIndex> = units
        .iter()
        .enumerate()
        .filter(|(_, u)| u.has_main)
        .map(|(i, _)| node_indices[i])
        .collect();

    for bin in &targets_to_link {
        let clean_path = bin.path.trim_start_matches("./").trim_start_matches('/');
        let target_path = PathBuf::from(clean_path);
        let target_node = path_to_node
            .iter()
            .find(|(p, _)| p.ends_with(&target_path))
            .map(|(_, &node)| node);

        let out_path = build_dir.join(&bin.name);
        println!("{:>12} {}", "Linking".magenta().bold(), bin.name);

        let mut required_objects = Vec::new();
        for (node, obj_path) in &obj_files_map {
            if all_main_nodes.contains(node) && Some(*node) != target_node {
                continue;
            }
            required_objects.push(obj_path.clone());
        }

        let mut cmd = Command::new(&linker_cmd);
        for flag in &pkg.flags {
            for part in flag.split_whitespace() {
                cmd.arg(part);
            }
        }
        for obj in &required_objects {
            cmd.arg(obj);
        }
        cmd.arg("-o").arg(&out_path);
        for lib_dir in &pkg.lib_dirs {
            cmd.arg(format!("-L{}", lib_dir));
        }

        let mut effective_libs = pkg.libs.clone();
        if cfg!(target_family = "unix") && !effective_libs.iter().any(|l| l == "m") {
            effective_libs.push("m".to_string());
        }
        for lib in &effective_libs {
            cmd.arg(format!("-l{}", lib));
        }

        let output = cmd
            .output()
            .await
            .map_err(|e| explain_io_error(&linker_cmd, e))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            eprintln!("\n{}", stderr.dimmed());
            explain_linker_error(&stderr);
            return Err(format!("Failed to link target '{}'", bin.name));
        }
    }

    println!("{:>12} project!", "Finished".bright_green().bold());
    Ok(())
}

pub fn clean_project(manifest_path: &str) -> Result<(), String> {
    let (manifest, _) = load_manifest_or_autodetect(manifest_path)?;
    let out_dir = PathBuf::from(&manifest.package.out_dir);
    if out_dir.exists() {
        fs::remove_dir_all(&out_dir)
            .map_err(|e| format!("Failed to remove '{}': {}", out_dir.display(), e))?;
        println!("{:>12} '{}'", "Removed".green().bold(), out_dir.display());
    } else {
        println!("{:>12} (already clean)", "Finished".bright_green().bold());
    }
    Ok(())
}

fn auto_detect_bin_targets(units: &[TranslationUnit], default_name: &str) -> Vec<BinTarget> {
    let main_units: Vec<&TranslationUnit> = units.iter().filter(|u| u.has_main).collect();
    if main_units.is_empty() {
        if units.len() == 1 {
            return vec![BinTarget {
                name: default_name.to_string(),
                path: units[0].path.to_string_lossy().to_string(),
            }];
        }
        return vec![];
    }

    if main_units.len() == 1 {
        return vec![BinTarget {
            name: default_name.to_string(),
            path: main_units[0].path.to_string_lossy().to_string(),
        }];
    }

    main_units
        .iter()
        .map(|u| {
            let file_stem = u.path.file_stem().and_then(|s| s.to_str()).unwrap_or("app");
            let target_name = if file_stem == "main" {
                u.path
                    .parent()
                    .and_then(|p| p.file_name())
                    .and_then(|n| n.to_str())
                    .unwrap_or(default_name)
                    .to_string()
            } else {
                file_stem.to_string()
            };
            BinTarget {
                name: target_name,
                path: u.path.to_string_lossy().to_string(),
            }
        })
        .collect()
}

async fn compile_unit(
    ctx: Arc<BuildContext>,
    unit: TranslationUnit,
    deep_hash: String,
) -> Result<PathBuf, String> {
    let safe_name = unit
        .path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("unit")
        .replace('.', "_");
    let hash_prefix = &deep_hash[0..8];
    let obj_name = format!("{}_{}.o", safe_name, hash_prefix);
    let obj_path = ctx.obj_dir.join(&obj_name);
    let cache_file = ctx.obj_dir.join(format!("{}.hash", safe_name));

    let is_cached = if let Ok(cached) = fs::read_to_string(&cache_file) {
        let mut valid = obj_path.exists();
        if let Language::Cpp = unit.language {
            if let CompilerType::Clang = ctx.package.compiler {
                if let Some(mod_name) = &unit.exported_module {
                    valid = valid && ctx.obj_dir.join(format!("{}.pcm", mod_name)).exists();
                }
            }
        }
        cached == deep_hash && valid
    } else {
        false
    };

    if is_cached {
        ctx.pb.suspend(|| {
            println!(
                "{:>12} {}",
                "Cached".bright_blue().bold(),
                unit.path.display()
            );
        });
        return Ok(obj_path);
    }

    ctx.pb.suspend(|| {
        println!("{:>12} {}", "Compiling".green().bold(), unit.path.display());
    });

    let raw_source = fs::read_to_string(&unit.path).map_err(|e| e.to_string())?;
    let file_to_compile = if super::comptime::has_comptime_markers(&raw_source) {
        let gen_dir = ctx.obj_dir.join("gen");
        fs::create_dir_all(&gen_dir).map_err(|e| e.to_string())?;

        let transformed_code = super::comptime::transform_comptime(
            &raw_source,
            &ctx.package.include_dirs,
            &ctx.obj_dir,
            &unit.path,
        )?;

        let shadow_file = gen_dir.join(unit.path.file_name().unwrap());
        let abs_orig = fs::canonicalize(&unit.path).unwrap_or_else(|_| unit.path.clone());
        let final_shadow_code = format!(
            "#line 1 \"{}\"\n{}",
            abs_orig.to_string_lossy(),
            transformed_code
        );

        fs::write(&shadow_file, final_shadow_code).map_err(|e| e.to_string())?;
        shadow_file
    } else {
        unit.path.clone()
    };

    let (compiler_bin, standard, lang_flags) = match unit.language {
        Language::C => (
            ctx.package.compiler.c_compiler(),
            &ctx.package.c_standard,
            &ctx.package.c_flags,
        ),
        Language::Cpp => (
            ctx.package.compiler.cpp_compiler(),
            &ctx.package.standard,
            &ctx.package.cpp_flags,
        ),
    };

    let mut cmd = Command::new(&compiler_bin);
    cmd.arg(standard).arg("-c");
    for flag in &ctx.package.flags {
        for part in flag.split_whitespace() {
            cmd.arg(part);
        }
    }
    for flag in lang_flags {
        for part in flag.split_whitespace() {
            cmd.arg(part);
        }
    }

    cmd.arg("-DCOMPTIME=0");
    cmd.arg("-Dcomptime(x)=(x)");
    cmd.arg("-Dtcc(x)=(x)");
    cmd.arg(&file_to_compile).arg("-o").arg(&obj_path);

    for inc in &ctx.package.include_dirs {
        cmd.arg(format!("-I{}", inc));
    }
    if let Some(parent) = unit.path.parent() {
        cmd.arg(format!("-I{}", parent.display()));
    }

    if unit.language == Language::Cpp {
        let is_export = unit.exported_module.is_some();
        let module_flags =
            ctx.package
                .compiler
                .get_module_flags(&ctx.obj_dir, is_export, &unit.exported_module);
        cmd.args(module_flags);
    }

    let output = cmd
        .output()
        .await
        .map_err(|e| explain_io_error(&compiler_bin, e))?;

    if output.status.success() {
        let _ = fs::write(cache_file, &deep_hash);
        Ok(obj_path)
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        ctx.pb.suspend(|| {
            eprintln!(
                "\n{} {}",
                "  Error compiling".red().bold(),
                unit.path.display()
            );
            eprintln!("{}", stderr);
        });
        Err(format!(
            "Compilation stopped due to errors in {}",
            unit.path.display()
        ))
    }
}

fn explain_io_error(binary: &str, err: std::io::Error) -> String {
    if err.kind() == std::io::ErrorKind::NotFound {
        format!(
            "Compiler executable '{}' was not found in PATH!\n    Solution: Install it (`sudo pacman -S clang gcc`) or adjust 'compiler' in Crub.toml.",
            binary
        )
    } else {
        format!("Failed to execute '{}': {}", binary, err)
    }
}

fn explain_linker_error(stderr: &str) {
    let tips = [
        (
            "undefined reference to `sin'",
            "Missing math functions. Add libs = [\"m\"] in Crub.toml.",
        ),
        (
            "undefined reference to `cos'",
            "Missing math functions. Add libs = [\"m\"] in Crub.toml.",
        ),
        (
            "undefined reference to `sqrt'",
            "Missing math functions. Add libs = [\"m\"] in Crub.toml.",
        ),
        (
            "undefined reference to `pthread_",
            "Missing POSIX threads. Add flags = [\"-pthread\"] or libs = [\"pthread\"].",
        ),
        (
            "undefined reference to `dlopen'",
            "Missing dynamic loader. Add libs = [\"dl\"].",
        ),
        (
            "undefined reference to `main'",
            "No 'main' function found. Ensure a source file defines 'int main()'.",
        ),
        (
            "multiple definition of `main'",
            "Multiple 'main' functions linked together. Declare separate [[bin]] sections in Crub.toml.",
        ),
        (
            "undefined reference to `SDL_",
            "Missing SDL2 library. Add libs = [\"SDL2\"] in Crub.toml.",
        ),
        (
            "undefined reference to `glBegin'",
            "Missing OpenGL. Add libs = [\"GL\"] in Crub.toml.",
        ),
        (
            "undefined reference to `glCreateShader'",
            "Missing OpenGL. Add libs = [\"GL\"] in Crub.toml.",
        ),
        (
            "undefined reference to `vkCreateInstance'",
            "Missing Vulkan loader. Add libs = [\"vulkan\"] in Crub.toml.",
        ),
        (
            "undefined reference to `XOpenDisplay'",
            "Missing X11. Add libs = [\"X11\"] in Crub.toml.",
        ),
        (
            "undefined reference to `InitWindow'",
            "Missing Raylib. Add libs = [\"raylib\"] in Crub.toml.",
        ),
    ];
    for (needle, hint) in &tips {
        if stderr.contains(needle) {
            println!("    {} {}", "Tip:".yellow().bold(), hint.yellow());
        }
    }
}

fn write_compdb(
    pkg: &Package,
    units: &[TranslationUnit],
    deep_hashes: &HashMap<NodeIndex, String>,
    node_indices: &[NodeIndex],
    obj_dir: &Path,
) -> Result<(), String> {
    let current_dir = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let current_dir_str = current_dir.to_string_lossy().to_string();
    let mut commands = Vec::new();

    for (i, unit) in units.iter().enumerate() {
        let node_idx = node_indices[i];
        let deep_hash = &deep_hashes[&node_idx];
        let safe_name = unit
            .path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unit")
            .replace('.', "_");
        let obj_name = format!("{}_{}.o", safe_name, &deep_hash[0..8]);
        let abs_file =
            fs::canonicalize(&unit.path).unwrap_or_else(|_| current_dir.join(&unit.path));
        let abs_output = current_dir.join(obj_dir).join(&obj_name);

        let (compiler_bin, standard) = match unit.language {
            Language::C => (pkg.compiler.c_compiler(), &pkg.c_standard),
            Language::Cpp => (pkg.compiler.cpp_compiler(), &pkg.standard),
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
        for inc in &pkg.include_dirs {
            args.push(format!("-I{}", current_dir.join(inc).to_string_lossy()));
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

    let mut deep_hashes = HashMap::new();
    let mut node_indices = Vec::new();
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
