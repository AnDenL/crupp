pub mod compdb;
pub mod compiler;
pub mod diagnostics;
pub mod graph;
pub mod linker;
pub mod ops;
pub mod options;

pub use compdb::export_compdb;
pub use ops::{check_project, clean_project_with_all};
pub use options::BuildOptions;

use self::compiler::compile_unit;
use self::graph::{build_dependency_graph, compute_deep_hashes, sort_and_detect_cycles};
use self::linker::{auto_detect_bin_targets, link_targets};
use self::options::BuildContext;
use crate::config::{BinTarget, BuildProfile, load_manifest_or_autodetect};
use crate::core::scanner::{auto_discover_include_dirs, discover_sources};
use colored::*;
use indicatif::{ProgressBar, ProgressStyle};
use petgraph::Direction;
use petgraph::graph::NodeIndex;
use std::collections::{HashMap, HashSet, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::{Semaphore, mpsc};

pub async fn _build_project(manifest_path: &str, target_name: Option<&str>) -> Result<(), String> {
    build_project_with_options(BuildOptions {
        manifest_path,
        target_name,
        ..Default::default()
    })
    .await
}

pub async fn build_project_with_options(opts: BuildOptions<'_>) -> Result<(), String> {
    let start_time = Instant::now();
    let (mut manifest, is_autodetected) = load_manifest_or_autodetect(opts.manifest_path)?;

    if is_autodetected {
        println!(
            "{}",
            "  ℹ️  No Crub.toml found. Running in smart auto-detected mode...".yellow()
        );
    }

    if manifest.package.include_dirs.is_empty() {
        manifest.package.include_dirs =
            auto_discover_include_dirs(Path::new("."), &manifest.package.out_dir);
    } else {
        manifest
            .package
            .include_dirs
            .retain(|d| !d.trim().is_empty());
    }
    manifest.package.lib_dirs.retain(|d| !d.trim().is_empty());
    manifest.package.libs.retain(|l| !l.trim().is_empty());

    let build_dir = PathBuf::from(&manifest.package.out_dir);
    let obj_dir = build_dir.join("obj");
    fs::create_dir_all(&obj_dir).map_err(|e| format!("Failed to create build dir: {}", e))?;

    let current_dir = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let mut seen_canonical = HashSet::new();
    let mut units = discover_sources(&manifest.package.source_dir, &manifest.package);

    // 1. Initial canonical deduplication and relative path normalization for discovered units
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

    // 2. Discover bin paths if not already in units
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
                if let Some(unit) = crate::core::scanner::scan_file(&clean_p, &manifest.package) {
                    units.push(unit);
                }
            }
        }
    }

    // 3. Auto-discover sibling .c / .cpp files for included local headers (strictly deduplicated!)
    let mut extra_sources = Vec::new();
    for u in &units {
        for hdr in &u.direct_headers {
            if let Some(stem) = hdr.file_stem().and_then(|s| s.to_str()) {
                if let Some(parent) = hdr.parent() {
                    for ext in &["c", "cpp", "cc", "cxx", "C"] {
                        let candidate = parent.join(format!("{}.{}", stem, ext));
                        if candidate.is_file() {
                            let canon =
                                fs::canonicalize(&candidate).unwrap_or_else(|_| candidate.clone());
                            if seen_canonical.insert(canon.clone()) {
                                let clean_cand = if let Ok(rel) = canon.strip_prefix(&current_dir) {
                                    rel.to_path_buf()
                                } else {
                                    candidate
                                };
                                if let Some(tu) =
                                    crate::core::scanner::scan_file(&clean_cand, &manifest.package)
                                {
                                    extra_sources.push(tu);
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    units.extend(extra_sources);

    if units.is_empty() {
        return Err(format!(
            "No C or C++ source files found in '{}'.\n  💡 Tip: Create your source files in 'src/' or specify 'source_dir' in Crub.toml.",
            manifest.package.source_dir
        ));
    }

    if manifest.bin.is_empty() {
        manifest.bin = auto_detect_bin_targets(&units, &manifest.package.name);
    }

    let targets_to_link: Vec<BinTarget> = if let Some(name) = opts.target_name {
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

    let profile_flags = manifest.resolve_profile_flags(&opts.profile);

    // Construct DAG & perform topological sort with cycle detection
    let build_graph = build_dependency_graph(&units, &manifest.package.source_dir)?;
    let order = sort_and_detect_cycles(&build_graph.graph, &units)?;
    let deep_hashes = compute_deep_hashes(&build_graph.graph, &units, &order, &profile_flags);

    // Auto-sync compile_commands.json
    let _ = compdb::write_compdb(
        &manifest.package,
        &units,
        &deep_hashes,
        &build_graph.node_indices,
        &obj_dir,
    );

    let max_concurrency = opts.jobs.unwrap_or_else(|| {
        std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4)
    });

    let pb = ProgressBar::new(build_graph.graph.node_count() as u64);
    pb.set_style(
        ProgressStyle::default_bar()
            .template(
                "{spinner:.cyan.bold} [{bar:40.cyan/blue}] {pos}/{len} ({percent}%) {msg:.yellow}",
            )
            .unwrap_or_else(|_| ProgressStyle::default_bar())
            .progress_chars("=>-"),
    );

    let profile_badge = match opts.profile {
        BuildProfile::Release => " [Release]".bright_magenta().bold(),
        BuildProfile::Debug => " [Debug]".bright_blue().bold(),
        BuildProfile::Sanitize => " [Sanitize]".bright_red().bold(),
        BuildProfile::Custom(ref s) => format!(" [{}]", s).bright_yellow().bold(),
        BuildProfile::Default => "".normal(),
    };
    pb.set_message(format!("Compiling project{}...", profile_badge));

    let build_ctx = Arc::new(BuildContext {
        package: manifest.package.clone(),
        obj_dir: obj_dir.clone(),
        pb: pb.clone(),
        profile: opts.profile.clone(),
        profile_flags: profile_flags.clone(),
        verbose: opts.verbose,
        force: opts.force,
    });

    // True Asynchronous DAG Worker Queue with direct indexing
    let total_nodes = build_graph.graph.node_count();
    let semaphore = Arc::new(Semaphore::new(max_concurrency));
    let (tx, mut rx) = mpsc::unbounded_channel();

    let mut in_degrees: Vec<usize> = Vec::with_capacity(total_nodes);
    for node in build_graph.graph.node_indices() {
        in_degrees.push(
            build_graph
                .graph
                .edges_directed(node, Direction::Incoming)
                .count(),
        );
    }

    let mut ready_queue = VecDeque::new();
    for (i, &deg) in in_degrees.iter().enumerate() {
        if deg == 0 {
            ready_queue.push_back(NodeIndex::new(i));
        }
    }

    let mut in_flight = 0;
    let mut completed_count = 0;
    let mut obj_files_map: HashMap<NodeIndex, PathBuf> = HashMap::with_capacity(total_nodes);

    while completed_count < total_nodes {
        // Schedule all currently ready nodes
        while let Some(node) = ready_queue.pop_front() {
            in_flight += 1;
            let permit = semaphore.clone().acquire_owned().await.unwrap();
            let tx_clone = tx.clone();
            let ctx = Arc::clone(&build_ctx);
            let unit = units[build_graph.graph[node]].clone();
            let deep_hash = deep_hashes[&node].clone();

            tokio::spawn(async move {
                let res = compile_unit(ctx, unit, deep_hash).await;
                drop(permit);
                let _ = tx_clone.send((node, res));
            });
        }

        if in_flight == 0 && completed_count < total_nodes {
            pb.finish_and_clear();
            return Err("Deadlock detected in compilation dependency graph!".to_string());
        }

        // Wait for any compilation unit to finish
        if let Some((finished_node, res)) = rx.recv().await {
            in_flight -= 1;
            match res {
                Ok(obj_path) => {
                    obj_files_map.insert(finished_node, obj_path);
                    completed_count += 1;
                    pb.inc(1);

                    // Unblock outgoing dependencies immediately
                    for neighbor in build_graph
                        .graph
                        .neighbors_directed(finished_node, Direction::Outgoing)
                    {
                        let deg = &mut in_degrees[neighbor.index()];
                        *deg -= 1;
                        if *deg == 0 {
                            ready_queue.push_back(neighbor);
                        }
                    }
                }
                Err(e) => {
                    pb.finish_and_clear();
                    return Err(e);
                }
            }
        }
    }

    pb.finish_and_clear();

    // Link targets
    link_targets(
        &manifest.package,
        &targets_to_link,
        &units,
        &build_graph.node_indices,
        &build_graph.path_to_node,
        &obj_files_map,
        &build_dir,
        &profile_flags,
        opts.verbose,
    )
    .await?;

    let elapsed = start_time.elapsed();
    println!(
        "{:>12} project! (in {:.2}s)",
        "Finished".bright_green().bold(),
        elapsed.as_secs_f64()
    );
    Ok(())
}
