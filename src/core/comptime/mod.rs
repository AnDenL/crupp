pub mod executor;
pub mod header;
pub mod parser;
pub mod types;

use std::path::Path;

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

    // 1. Extract static reflection metadata from C source file
    let (structs, enums) = parser::parse_c_declarations(content);

    // 2. Generate specialized <crupp/comptime.h> header with reflection database & code generators
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
