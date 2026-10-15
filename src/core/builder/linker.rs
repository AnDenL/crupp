use super::diagnostics::{detect_missing_libs_from_stderr, explain_io_error, explain_linker_error};
use crate::config::{BinTarget, Language, Package};
use crate::core::scanner::TranslationUnit;
use colored::*;
use petgraph::graph::NodeIndex;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use tokio::process::Command;

pub fn auto_detect_bin_targets(units: &[TranslationUnit], default_name: &str) -> Vec<BinTarget> {
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

pub async fn link_targets(
    pkg: &Package,
    targets_to_link: &[BinTarget],
    units: &[TranslationUnit],
    node_indices: &[NodeIndex],
    path_to_node: &HashMap<PathBuf, NodeIndex>,
    obj_files_map: &HashMap<NodeIndex, PathBuf>,
    build_dir: &Path,
    profile_flags: &[String],
    verbose: bool,
) -> Result<(), String> {
    let has_any_cpp = units.iter().any(|u| u.language == Language::Cpp);
    let (linker_cmd, fallback_note) = if has_any_cpp {
        pkg.compiler.resolve_cpp_compiler()
    } else {
        pkg.compiler.resolve_c_compiler()
    };

    if let Some(note) = fallback_note {
        println!("{}", note);
    }

    let all_main_nodes: HashSet<NodeIndex> = units
        .iter()
        .enumerate()
        .filter(|(_, u)| u.has_main)
        .map(|(i, _)| node_indices[i])
        .collect();

    for bin in targets_to_link {
        let clean_path = bin.path.trim_start_matches("./").trim_start_matches('/');
        let target_path = PathBuf::from(clean_path);
        let target_node = path_to_node
            .iter()
            .find(|(p, _)| p.ends_with(&target_path))
            .map(|(_, &node)| node);

        let out_path = build_dir.join(&bin.name);
        println!("{:>12} {}", "Linking".magenta().bold(), bin.name);

        let mut required_objects = Vec::with_capacity(obj_files_map.len());
        for (node, obj_path) in obj_files_map {
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
        for flag in profile_flags {
            for part in flag.split_whitespace() {
                cmd.arg(part);
            }
        }
        for obj in &required_objects {
            cmd.arg(obj);
        }
        cmd.arg("-o").arg(&out_path);

        for lib_dir in &pkg.lib_dirs {
            let clean = lib_dir.trim().trim_start_matches("-L").trim();
            if !clean.is_empty() {
                cmd.arg(format!("-L{}", clean));
            }
        }

        let mut effective_libs = pkg.libs.clone();
        if cfg!(target_family = "unix") && !effective_libs.iter().any(|l| l == "m") {
            effective_libs.push("m".to_string());
        }
        for lib in &effective_libs {
            let clean = lib.trim().trim_start_matches("-l").trim();
            if !clean.is_empty() {
                cmd.arg(format!("-l{}", clean));
            }
        }

        if verbose {
            println!("  [Linker Command]: {:?}", cmd);
        }

        let output = cmd
            .output()
            .await
            .map_err(|e| explain_io_error(&linker_cmd, &e))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let missing_libs = detect_missing_libs_from_stderr(&stderr);
            let mut recovered = false;

            // Attempt automatic link recovery if missing standard libraries were identified
            if !missing_libs.is_empty() {
                let mut retry_cmd = Command::new(&linker_cmd);
                for flag in &pkg.flags {
                    for part in flag.split_whitespace() {
                        retry_cmd.arg(part);
                    }
                }
                for flag in profile_flags {
                    for part in flag.split_whitespace() {
                        retry_cmd.arg(part);
                    }
                }
                for obj in &required_objects {
                    retry_cmd.arg(obj);
                }
                retry_cmd.arg("-o").arg(&out_path);

                for lib_dir in &pkg.lib_dirs {
                    let clean = lib_dir.trim().trim_start_matches("-L").trim();
                    if !clean.is_empty() {
                        retry_cmd.arg(format!("-L{}", clean));
                    }
                }
                for lib in &effective_libs {
                    let clean = lib.trim().trim_start_matches("-l").trim();
                    if !clean.is_empty() {
                        retry_cmd.arg(format!("-l{}", clean));
                    }
                }
                for missing in &missing_libs {
                    if !effective_libs.iter().any(|l| l == missing) {
                        retry_cmd.arg(format!("-l{}", missing));
                    }
                }

                if verbose {
                    println!("  [Linker Auto-Recovery Command]: {:?}", retry_cmd);
                }

                if let Ok(retry_output) = retry_cmd.output().await {
                    if retry_output.status.success() {
                        println!(
                            "    {} Auto-linked missing library: {}",
                            "ℹ️".bright_blue().bold(),
                            missing_libs
                                .iter()
                                .map(|l| format!("'-l{}'", l))
                                .collect::<Vec<_>>()
                                .join(", ")
                                .bright_green()
                                .bold()
                        );
                        println!(
                            "    {} Add {} to your Crub.toml to make this explicit.",
                            "💡 Tip:".yellow().bold(),
                            format!("libs = {:?}", missing_libs).yellow()
                        );
                        recovered = true;
                    }
                }
            }

            if !recovered {
                eprintln!("\n{}", stderr.dimmed());
                explain_linker_error(&stderr);
                return Err(format!("Failed to link target '{}'", bin.name));
            }
        }
    }

    Ok(())
}
