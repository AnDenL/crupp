use crate::core::scanner::TranslationUnit;
use colored::*;
use petgraph::Direction;
use petgraph::algo::{tarjan_scc, toposort};
use petgraph::graph::{DiGraph, NodeIndex};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::PathBuf;

pub struct BuildGraph {
    pub graph: DiGraph<usize, ()>,
    pub node_indices: Vec<NodeIndex>,
    pub path_to_node: HashMap<PathBuf, NodeIndex>,
}

pub fn build_dependency_graph(
    units: &[TranslationUnit],
    source_dir: &str,
) -> Result<BuildGraph, String> {
    let mut module_to_node = HashMap::with_capacity(units.len());
    let mut path_to_node = HashMap::with_capacity(units.len());
    let mut node_indices = Vec::with_capacity(units.len());
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
                    "Module '{}' imported in '{}' does not exist!\n  💡 Tip: Check that the file providing 'export module {};' is inside source_dir ('{}').",
                    imp.bright_red().bold(),
                    unit.path.display().to_string().cyan(),
                    imp,
                    source_dir
                ));
            }
        }
    }

    Ok(BuildGraph {
        graph,
        node_indices,
        path_to_node,
    })
}

pub fn sort_and_detect_cycles(
    graph: &DiGraph<usize, ()>,
    units: &[TranslationUnit],
) -> Result<Vec<NodeIndex>, String> {
    match toposort(graph, None) {
        Ok(ord) => Ok(ord),
        Err(_) => {
            let sccs = tarjan_scc(graph);
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

            Err(format!(
                "Circular module dependency detected:\n  {}\n  💡 Tip: Break cyclic dependencies using module interface partitions (:partition) or forward declarations.",
                cycle_info.join(" -> ")
            ))
        }
    }
}

pub fn compute_deep_hashes(
    graph: &DiGraph<usize, ()>,
    units: &[TranslationUnit],
    order: &[NodeIndex],
    profile_flags: &[String],
) -> HashMap<NodeIndex, String> {
    let mut deep_hashes = HashMap::with_capacity(order.len());

    for &node_idx in order {
        let unit_idx = graph[node_idx];
        let mut hasher = Sha256::new();
        hasher.update(&units[unit_idx].base_hash);
        // Include build profile flags into deep hash
        for flag in profile_flags {
            hasher.update(flag.as_bytes());
        }
        for neighbor in graph.neighbors_directed(node_idx, Direction::Incoming) {
            if let Some(dep_hash) = deep_hashes.get(&neighbor) {
                hasher.update(dep_hash);
            }
        }
        deep_hashes.insert(node_idx, hex::encode(hasher.finalize()));
    }

    deep_hashes
}
