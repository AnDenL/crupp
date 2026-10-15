pub mod executor;
pub mod header;
pub mod parser;
pub mod types;

use regex::Regex;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

pub fn has_comptime_markers(content: &str) -> bool {
    content.contains("comptime")
        || content.contains("COMPTIME")
        || content.contains("Comptime")
        || content.contains("tcc(")
        || content.contains("TCC(")
        || content.lines().any(|l| {
            let t = l.trim_start();
            t.starts_with('#') && {
                let lower = t.to_ascii_lowercase();
                lower.contains("comptime") || lower.contains("tcc(")
            }
        })
}

pub fn transform_comptime(
    content: &str,
    include_dirs: &[String],
    obj_dir: &Path,
    origin_file: &Path,
) -> Result<String, String> {
    if !has_comptime_markers(content) {
        return Ok(content.to_string());
    }

    // 1. Extract static reflection metadata across current file and all transitively included headers
    let (structs, enums) = collect_all_declarations(content, origin_file, include_dirs);

    // 2. Generate specialized <comptime.h> header with reflection database & code generators
    let comptime_h = header::generate_comptime_header(&structs, &enums);
    let tcc_home = executor::prepare_embedded_headers(obj_dir, Some(&comptime_h));

    let mut result = content.to_string();

    // 3. Process '#if comptime ... #endif' metaprogramming blocks
    while let Some((start_idx, end_idx, comptime_code)) = find_next_comptime_block(&result) {
        let generated_c = executor::execute_c_code(&comptime_code, include_dirs, &tcc_home, origin_file)?;
        result.replace_range(start_idx..end_idx, &generated_c);
    }

    // 4. Process inline 'comptime(expr)' / 'tcc(expr)' evaluation macros
    while let Some((start_idx, end_idx, expr)) = find_next_comptime_expr(&result) {
        let wrapped_code = format!(
            r#"
            #include <stdio.h>
            #include <stdint.h>
            #include <math.h>

            int main(void) {{
                #define _crupp_print_val(x) _Generic((x), \
                    float: printf("%ff", (double)(x)), \
                    double: printf("%f", (double)(x)), \
                    char*: printf("\"%s\"", (char*)(x)), \
                    const char*: printf("\"%s\"", (const char*)(x)), \
                    default: printf("%lld", (long long)(x)) \
                )
                _crupp_print_val({expr});
                return 0;
            }}
            "#
        );

        let generated_val = executor::execute_c_code(&wrapped_code, include_dirs, &tcc_home, origin_file)?;
        result.replace_range(start_idx..end_idx, generated_val.trim());
    }

    Ok(result)
}

fn collect_all_declarations(
    content: &str,
    origin_file: &Path,
    include_dirs: &[String],
) -> (Vec<types::CtStruct>, Vec<types::CtEnum>) {
    let mut all_structs: Vec<types::CtStruct> = Vec::new();
    let mut all_enums: Vec<types::CtEnum> = Vec::new();
    let mut seen_struct_names: HashSet<String> = HashSet::new();
    let mut seen_enum_names: HashSet<String> = HashSet::new();

    let mut visited_headers: HashSet<PathBuf> = HashSet::new();
    let mut header_queue: Vec<PathBuf> = Vec::new();

    // A. Parse the current file itself
    let (curr_structs, curr_enums) = parser::parse_c_declarations(content);
    for s in curr_structs {
        seen_struct_names.insert(s.name.clone());
        all_structs.push(s);
    }
    for e in curr_enums {
        seen_enum_names.insert(e.name.clone());
        all_enums.push(e);
    }

    // B. Find direct headers from current file
    find_included_headers(content, origin_file, include_dirs, &mut header_queue, &mut visited_headers);

    // C. Scan configured include directories for project headers (.h, .hpp)
    let current_dir = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    for inc_dir in include_dirs {
        let clean = inc_dir.trim().trim_start_matches("-I").trim();
        if clean.is_empty() {
            continue;
        }
        let clean_lower = clean.to_ascii_lowercase();
        if clean_lower.contains("vendor") || clean_lower.contains("third_party") || clean_lower.contains("external") {
            continue;
        }
        let p = Path::new(clean);
        let search_path = if p.is_relative() {
            current_dir.join(p)
        } else {
            p.to_path_buf()
        };

        if search_path.is_dir() {
            for entry in walkdir::WalkDir::new(&search_path)
                .max_depth(5)
                .into_iter()
                .filter_entry(|e| {
                    if e.depth() == 0 {
                        return true;
                    }
                    let name = e.file_name().to_string_lossy();
                    if name.starts_with('.') {
                        return false;
                    }
                    let name_lower = name.to_ascii_lowercase();
                    if name == "target" || name == "build" || name == "build_crupp"
                        || name_lower == "vendor" || name_lower == "third_party" || name_lower == "external" {
                        return false;
                    }
                    true
                })
                .filter_map(|e| e.ok())
            {
                let path = entry.path();
                if path.is_file() {
                    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                        if matches!(ext, "h" | "hpp" | "hxx" | "hh" | "inl") {
                            let canon = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
                            if visited_headers.insert(canon.clone()) {
                                header_queue.push(canon);
                            }
                        }
                    }
                }
            }
        }
    }

    // D. Transitive traversal of all discovered headers
    while let Some(hdr_path) = header_queue.pop() {
        if let Ok(hdr_bytes) = fs::read(&hdr_path) {
            let hdr_content = String::from_utf8_lossy(&hdr_bytes);
            let (h_structs, h_enums) = parser::parse_c_declarations(&hdr_content);

            for s in h_structs {
                if let Some(pos) = all_structs.iter().position(|x| x.name == s.name) {
                    // If previous definition had no fields (forward declaration) and this one has fields, replace it!
                    if all_structs[pos].fields.is_empty() && !s.fields.is_empty() {
                        all_structs[pos] = s;
                    } else if !s.attrs.is_empty() {
                        // Merge attributes if new ones found
                        for a in s.attrs {
                            if !all_structs[pos].attrs.iter().any(|x| x.name == a.name) {
                                all_structs[pos].attrs.push(a);
                            }
                        }
                    }
                } else {
                    seen_struct_names.insert(s.name.clone());
                    all_structs.push(s);
                }
            }

            for e in h_enums {
                if let Some(pos) = all_enums.iter().position(|x| x.name == e.name) {
                    if all_enums[pos].variants.is_empty() && !e.variants.is_empty() {
                        all_enums[pos] = e;
                    }
                } else {
                    seen_enum_names.insert(e.name.clone());
                    all_enums.push(e);
                }
            }

            // Inspect nested #includes in this header
            find_included_headers(&hdr_content, &hdr_path, include_dirs, &mut header_queue, &mut visited_headers);
        }
    }

    (all_structs, all_enums)
}

fn find_included_headers(
    source_content: &str,
    current_file: &Path,
    include_dirs: &[String],
    queue: &mut Vec<PathBuf>,
    visited: &mut HashSet<PathBuf>,
) {
    let re_inc = Regex::new(r#"^\s*#\s*include\s*["<]([^">]+)[">]"#).unwrap();

    for line in source_content.lines() {
        let trimmed = line.trim_start();
        if !trimmed.starts_with('#') || !trimmed.contains("include") {
            continue;
        }

        if let Some(caps) = re_inc.captures(trimmed) {
            let inc_name = caps.get(1).unwrap().as_str();
            if let Some(resolved) = resolve_include_path(inc_name, current_file, include_dirs) {
                let canon = fs::canonicalize(&resolved).unwrap_or(resolved);
                if visited.insert(canon.clone()) {
                    queue.push(canon);
                }
            }
        }
    }
}

fn resolve_include_path(inc: &str, current_file: &Path, include_dirs: &[String]) -> Option<PathBuf> {
    // 1. Relative to current file's parent
    if let Some(parent) = current_file.parent() {
        let candidate = parent.join(inc);
        if candidate.is_file() {
            return Some(candidate);
        }
    }

    // 2. In current directory
    let cand_cwd = Path::new(".").join(inc);
    if cand_cwd.is_file() {
        return Some(cand_cwd);
    }

    // 3. In each configured include directory
    let current_dir = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    for dir in include_dirs {
        let clean = dir.trim().trim_start_matches("-I").trim();
        if clean.is_empty() {
            continue;
        }
        let base = Path::new(clean);
        let candidate = if base.is_relative() {
            current_dir.join(base).join(inc)
        } else {
            base.join(inc)
        };
        if candidate.is_file() {
            return Some(candidate);
        }
    }

    None
}

fn find_ascii_case_insensitive(haystack: &str, needle: &str) -> Option<usize> {
    if needle.is_empty() {
        return Some(0);
    }
    let needle_bytes = needle.as_bytes();
    let haystack_bytes = haystack.as_bytes();
    if needle_bytes.len() > haystack_bytes.len() {
        return None;
    }

    haystack_bytes.windows(needle_bytes.len()).position(|window| {
        window
            .iter()
            .zip(needle_bytes)
            .all(|(&h, &n)| h.to_ascii_lowercase() == n.to_ascii_lowercase())
    })
}

fn find_next_comptime_block(source: &str) -> Option<(usize, usize, String)> {
    let marker_candidates = ["#if comptime", "#ifdef comptime", "#if defined(comptime)"];

    let mut found_start = None;
    let mut marker_len = 0;

    for marker in &marker_candidates {
        if let Some(idx) = find_ascii_case_insensitive(source, marker) {
            if found_start.map_or(true, |curr| idx < curr) {
                found_start = Some(idx);
                marker_len = marker.len();
            }
        }
    }

    let start_idx = found_start?;
    let body_start = source[start_idx + marker_len..]
        .find('\n')
        .map(|nl| start_idx + marker_len + nl + 1)?;

    let mut depth = 1;
    let mut current_idx = body_start;
    let mut else_pos = None;

    while depth > 0 && current_idx < source.len() {
        let slice = &source[current_idx..];
        if let Some(nl_rel) = slice.find('\n') {
            let line = slice[..nl_rel].trim();
            if line.starts_with("#if") {
                depth += 1;
            } else if depth == 1 && (line.starts_with("#else") || line.starts_with("#elif")) {
                else_pos = Some(current_idx);
            } else if line.starts_with("#endif") {
                depth -= 1;
                if depth == 0 {
                    let end_idx = current_idx + nl_rel + 1;
                    let comptime_src = if let Some(e_idx) = else_pos {
                        source[body_start..e_idx].to_string()
                    } else {
                        source[body_start..current_idx].to_string()
                    };
                    return Some((start_idx, end_idx, comptime_src));
                }
            }
            current_idx += nl_rel + 1;
        } else {
            break;
        }
    }

    None
}

fn find_next_comptime_expr(source: &str) -> Option<(usize, usize, String)> {
    let tags = ["comptime(", "tcc("];
    let mut search_from = 0;

    while search_from < source.len() {
        let mut found = None;

        for tag in &tags {
            if let Some(idx) = source[search_from..].find(tag) {
                let abs_idx = search_from + idx;
                if found.map_or(true, |(curr, _)| abs_idx < curr) {
                    found = Some((abs_idx, tag.len()));
                }
            }
        }

        let (start_pos, tag_len) = found?;
        let expr_start = start_pos + tag_len;

        let line_start = source[..start_pos].rfind('\n').map(|i| i + 1).unwrap_or(0);
        let line_prefix = source[line_start..start_pos].trim();
        if line_prefix.starts_with('#') {
            search_from = expr_start;
            continue;
        }

        let mut depth = 1;
        let mut end_pos = expr_start;

        for (i, c) in source[expr_start..].char_indices() {
            match c {
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 {
                        end_pos = expr_start + i;
                        break;
                    }
                }
                _ => {}
            }
        }

        if depth == 0 {
            let expr = source[expr_start..end_pos].trim().to_string();
            return Some((start_pos, end_pos + 1, expr));
        } else {
            search_from = expr_start;
        }
    }

    None
}
