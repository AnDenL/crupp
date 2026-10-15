use super::types::{CtAttr, CtEnum, CtField, CtStruct, CtVariant, TypeKind};

pub fn is_valid_c_ident(s: &str) -> bool {
    if s.is_empty() {
        return false;
    }
    if matches!(
        s,
        "struct"
            | "enum"
            | "union"
            | "typedef"
            | "const"
            | "volatile"
            | "static"
            | "extern"
            | "inline"
            | "return"
            | "if"
            | "else"
            | "for"
            | "while"
            | "do"
            | "switch"
            | "case"
            | "default"
            | "break"
            | "continue"
            | "sizeof"
    ) {
        return false;
    }
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => (),
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Extracts `@attr`, `@attr(val)`, `@attr("val")`, `#[attr]`, `#[attr(val)]` from comment text.
/// Ignores `@ident` inside backticks (e.g. `` `@component` `` in prose comments) or mid-word (`a@b`).
pub fn extract_attributes(text: &str) -> Vec<CtAttr> {
    let mut attrs = Vec::new();
    let bytes = text.as_bytes();
    let len = bytes.len();
    let mut i = 0;

    while i < len {
        let is_at = bytes[i] == b'@';
        let is_rust_attr = bytes[i] == b'#' && i + 1 < len && bytes[i + 1] == b'[';

        if !is_at && !is_rust_attr {
            i += 1;
            continue;
        }

        // Ignore `@ident` if preceded by backtick, quote, or identifier char (e.g. `@component` in docs)
        if is_at && i > 0 {
            let prev = bytes[i - 1];
            if prev == b'`'
                || prev == b'\''
                || prev == b'"'
                || prev.is_ascii_alphanumeric()
                || prev == b'_'
            {
                i += 1;
                continue;
            }
        }

        if is_at {
            i += 1; // skip '@'
        } else {
            i += 2; // skip '#['
            while i < len && (bytes[i] == b' ' || bytes[i] == b'\t') {
                i += 1;
            }
        }

        // Parse attribute name
        let name_start = i;
        while i < len && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
            i += 1;
        }
        if i == name_start {
            continue;
        }
        let name = text[name_start..i].to_string();
        if !is_valid_c_ident(&name) {
            continue;
        }

        // If followed immediately by backtick (e.g. `@component`), ignore it as prose markdown
        if is_at && i < len && bytes[i] == b'`' {
            i += 1;
            continue;
        }

        // Skip horizontal whitespace
        while i < len && (bytes[i] == b' ' || bytes[i] == b'\t') {
            i += 1;
        }

        let mut value = String::new();

        if i < len && bytes[i] == b'(' {
            // Balanced parentheses extraction
            i += 1; // skip '('
            let val_start = i;
            let mut depth = 1usize;
            let mut in_quotes = false;
            while i < len && depth > 0 {
                let b = bytes[i];
                if in_quotes {
                    if b == b'\\' && i + 1 < len {
                        i += 2;
                        continue;
                    } else if b == b'"' {
                        in_quotes = false;
                    }
                } else if b == b'"' {
                    in_quotes = true;
                } else if b == b'(' {
                    depth += 1;
                } else if b == b')' {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                }
                i += 1;
            }
            let raw_val = text[val_start..i].trim();
            value = strip_outer_quotes(raw_val).to_string();
            if i < len && bytes[i] == b')' {
                i += 1;
            }
        } else if i < len && (bytes[i] == b'=' || bytes[i] == b':') {
            i += 1;
            while i < len && (bytes[i] == b' ' || bytes[i] == b'\t') {
                i += 1;
            }
            if i < len && bytes[i] == b'"' {
                i += 1;
                let val_start = i;
                while i < len && bytes[i] != b'"' && bytes[i] != b'\n' {
                    if bytes[i] == b'\\' && i + 1 < len {
                        i += 2;
                    } else {
                        i += 1;
                    }
                }
                value = text[val_start..i].to_string();
                if i < len && bytes[i] == b'"' {
                    i += 1;
                }
            } else {
                let val_start = i;
                while i < len
                    && !bytes[i].is_ascii_whitespace()
                    && bytes[i] != b']'
                    && bytes[i] != b','
                    && bytes[i] != b';'
                {
                    i += 1;
                }
                value = strip_outer_quotes(text[val_start..i].trim()).to_string();
            }
        }

        if is_rust_attr {
            while i < len && bytes[i] != b']' && bytes[i] != b'\n' {
                i += 1;
            }
            if i < len && bytes[i] == b']' {
                i += 1;
            }
        }

        if !attrs.iter().any(|a: &CtAttr| a.name == name) {
            attrs.push(CtAttr { name, value });
        }
    }

    attrs
}

fn strip_outer_quotes(s: &str) -> &str {
    let trimmed = s.trim();
    if trimmed.len() >= 2 && trimmed.starts_with('"') && trimmed.ends_with('"') {
        &trimmed[1..trimmed.len() - 1]
    } else {
        trimmed
    }
}

/// Collects only the contiguous block of comment lines immediately preceding `decl_byte_offset`.
/// Stops immediately on any blank line or non-comment code line so file header banners never leak into structs.
fn collect_preceding_comments(source: &str, decl_byte_offset: usize) -> String {
    let before = &source[..decl_byte_offset];
    let mut collected_lines = Vec::new();
    let mut in_block_comment = false;

    for line in before.lines().rev() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            if in_block_comment {
                collected_lines.push(line);
                continue;
            }
            // Stop at blank line boundary!
            break;
        }

        if in_block_comment {
            collected_lines.push(line);
            if trimmed.contains("/*") {
                in_block_comment = false;
            }
            continue;
        }

        if trimmed.starts_with("//") {
            collected_lines.push(line);
        } else if trimmed.ends_with("*/") {
            collected_lines.push(line);
            if !trimmed.contains("/*") {
                in_block_comment = true;
            }
        } else {
            // Reached a code line or preprocessor directive
            break;
        }
    }

    collected_lines.reverse();
    collected_lines.join("\n")
}

/// Strips any `#if comptime ... #endif` blocks from source text before scanning for C declarations,
/// so comptime generator code does not reflect over itself.
fn mask_comptime_blocks(source: &str) -> String {
    let mut out = String::with_capacity(source.len());
    let mut depth = 0usize;

    for line in source.lines() {
        let trimmed = line.trim_start();
        if depth == 0 {
            if let Some(after_hash) = trimmed.strip_prefix('#') {
                let directive = after_hash.trim_start().to_ascii_lowercase();
                if directive.starts_with("if comptime")
                    || directive.starts_with("ifdef comptime")
                    || directive.starts_with("if defined(comptime)")
                {
                    depth = 1;
                    out.push('\n');
                    continue;
                }
            }
            out.push_str(line);
            out.push('\n');
        } else {
            if let Some(after_hash) = trimmed.strip_prefix('#') {
                let directive = after_hash.trim_start();
                if directive.starts_with("if") {
                    depth += 1;
                } else if directive.starts_with("endif") {
                    depth -= 1;
                }
            }
            out.push('\n');
        }
    }

    out
}

pub fn parse_c_declarations(raw_content: &str) -> (Vec<CtStruct>, Vec<CtEnum>) {
    let content_owned = mask_comptime_blocks(raw_content);
    let content = content_owned.as_str();
    let bytes = content.as_bytes();
    let len = bytes.len();

    let mut structs = Vec::new();
    let mut enums = Vec::new();

    let mut i = 0;
    let mut brace_depth = 0usize;

    while i < len {
        // 1. Skip line comments
        if bytes[i] == b'/' && i + 1 < len && bytes[i + 1] == b'/' {
            i += 2;
            while i < len && bytes[i] != b'\n' {
                i += 1;
            }
            continue;
        }

        // 2. Skip block comments
        if bytes[i] == b'/' && i + 1 < len && bytes[i + 1] == b'*' {
            i += 2;
            while i + 1 < len && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                i += 1;
            }
            i += 2;
            continue;
        }

        // 3. Skip string and char literals
        if bytes[i] == b'"' || bytes[i] == b'\'' {
            let quote = bytes[i];
            i += 1;
            while i < len {
                if bytes[i] == b'\\' && i + 1 < len {
                    i += 2;
                    continue;
                }
                if bytes[i] == quote {
                    i += 1;
                    break;
                }
                i += 1;
            }
            continue;
        }

        // 4. Track brace depth
        if bytes[i] == b'{' {
            brace_depth += 1;
            i += 1;
            continue;
        }
        if bytes[i] == b'}' {
            brace_depth = brace_depth.saturating_sub(1);
            i += 1;
            continue;
        }

        // Only inspect top-level declarations
        if brace_depth == 0 && (bytes[i].is_ascii_alphabetic() || bytes[i] == b'_') {
            let word_start = i;
            while i < len && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                i += 1;
            }
            let word = &content[word_start..i];

            let mut is_typedef = false;
            let mut decl_kind = "";
            let decl_start = word_start;

            if word == "typedef" {
                let saved = skip_ws_and_comments(content, i);
                let kw_start = saved;
                let mut kw_end = kw_start;
                while kw_end < len && (bytes[kw_end].is_ascii_alphanumeric() || bytes[kw_end] == b'_') {
                    kw_end += 1;
                }
                let kw = &content[kw_start..kw_end];
                if kw == "struct" || kw == "enum" {
                    is_typedef = true;
                    decl_kind = kw;
                    i = kw_end;
                }
            } else if word == "struct" || word == "enum" {
                decl_kind = word;
            }

            if !decl_kind.is_empty() {
                if let Some((tag_name, body_str, alias_name, next_idx)) =
                    try_parse_braced_declaration(content, i)
                {
                    let name = if is_typedef && !alias_name.is_empty() {
                        alias_name
                    } else if !tag_name.is_empty() {
                        tag_name
                    } else {
                        alias_name
                    };

                    if is_valid_c_ident(&name) {
                        let line_start = content[..decl_start]
                            .rfind('\n')
                            .map(|idx| idx + 1)
                            .unwrap_or(0);
                        let prefix_comments = collect_preceding_comments(content, line_start);
                        let attrs = extract_attributes(&prefix_comments);

                        if decl_kind == "struct" {
                            let fields = parse_struct_fields(&body_str);
                            structs.push(CtStruct {
                                name,
                                fields,
                                attrs,
                            });
                        } else if decl_kind == "enum" {
                            let variants = parse_enum_variants(&body_str);
                            enums.push(CtEnum {
                                name,
                                variants,
                                attrs,
                            });
                        }
                    }

                    i = next_idx;
                    continue;
                }
            }
            continue;
        }

        i += 1;
    }

    (structs, enums)
}

fn skip_ws_and_comments(content: &str, mut i: usize) -> usize {
    let bytes = content.as_bytes();
    let len = bytes.len();
    while i < len {
        if bytes[i].is_ascii_whitespace() {
            i += 1;
        } else if bytes[i] == b'/' && i + 1 < len && bytes[i + 1] == b'/' {
            i += 2;
            while i < len && bytes[i] != b'\n' {
                i += 1;
            }
        } else if bytes[i] == b'/' && i + 1 < len && bytes[i + 1] == b'*' {
            i += 2;
            while i + 1 < len && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                i += 1;
            }
            i += 2;
        } else {
            break;
        }
    }
    i
}

/// Starting right after `struct` or `enum`, parses optional tag name, `{ body }`, optional alias name, and `;`.
/// Returns `None` if this is a forward declaration (`struct Foo;`) or variable/function return type without `{ ... }`.
fn try_parse_braced_declaration(
    content: &str,
    start_after_kw: usize,
) -> Option<(String, String, String, usize)> {
    let bytes = content.as_bytes();
    let len = bytes.len();
    let mut i = skip_ws_and_comments(content, start_after_kw);

    // Collect tokens before `{` or `;`
    let mut tag_name = String::new();
    while i < len && bytes[i] != b'{' && bytes[i] != b';' && bytes[i] != b'(' {
        if bytes[i] == b'/' && i + 1 < len && (bytes[i + 1] == b'/' || bytes[i + 1] == b'*') {
            i = skip_ws_and_comments(content, i);
            continue;
        }
        if bytes[i].is_ascii_alphabetic() || bytes[i] == b'_' {
            let w_start = i;
            while i < len && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                i += 1;
            }
            let w = &content[w_start..i];
            if w == "__attribute__" || w == "__declspec" {
                i = skip_ws_and_comments(content, i);
                if i < len && bytes[i] == b'(' {
                    let mut p_depth = 1;
                    i += 1;
                    while i < len && p_depth > 0 {
                        if bytes[i] == b'(' {
                            p_depth += 1;
                        } else if bytes[i] == b')' {
                            p_depth -= 1;
                        }
                        i += 1;
                    }
                }
            } else {
                tag_name = w.to_string();
            }
        } else if bytes[i].is_ascii_whitespace() {
            i += 1;
        } else {
            // Unexpected token before `{` (e.g., pointer `struct Foo* bar()`)
            return None;
        }
    }

    if i >= len || bytes[i] != b'{' {
        return None;
    }

    // Find matching `}` for `{`, ignoring comments and strings!
    let body_start = i + 1;
    let mut depth = 1usize;
    i += 1;

    while i < len && depth > 0 {
        if bytes[i] == b'/' && i + 1 < len && bytes[i + 1] == b'/' {
            i += 2;
            while i < len && bytes[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        if bytes[i] == b'/' && i + 1 < len && bytes[i + 1] == b'*' {
            i += 2;
            while i + 1 < len && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                i += 1;
            }
            i += 2;
            continue;
        }
        if bytes[i] == b'"' || bytes[i] == b'\'' {
            let quote = bytes[i];
            i += 1;
            while i < len {
                if bytes[i] == b'\\' && i + 1 < len {
                    i += 2;
                    continue;
                }
                if bytes[i] == quote {
                    i += 1;
                    break;
                }
                i += 1;
            }
            continue;
        }
        if bytes[i] == b'{' {
            depth += 1;
        } else if bytes[i] == b'}' {
            depth -= 1;
            if depth == 0 {
                break;
            }
        }
        i += 1;
    }

    if depth != 0 {
        return None;
    }

    let body_end = i;
    let body_str = content[body_start..body_end].to_string();
    i += 1; // skip '}'

    // Parse optional alias name before ';'
    let mut alias_name = String::new();
    while i < len && bytes[i] != b';' {
        if bytes[i] == b'/' && i + 1 < len && (bytes[i + 1] == b'/' || bytes[i + 1] == b'*') {
            i = skip_ws_and_comments(content, i);
            continue;
        }
        if bytes[i].is_ascii_alphabetic() || bytes[i] == b'_' {
            let w_start = i;
            while i < len && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                i += 1;
            }
            let w = &content[w_start..i];
            if w != "__attribute__" && w != "packed" && w != "aligned" {
                alias_name = w.to_string();
            }
        } else {
            i += 1;
        }
    }

    if i < len && bytes[i] == b';' {
        i += 1;
    }

    Some((tag_name, body_str, alias_name, i))
}

fn parse_struct_fields(body: &str) -> Vec<CtField> {
    let mut fields = Vec::new();
    let bytes = body.as_bytes();
    let len = bytes.len();

    let mut pending_comments = String::new();
    let mut current_code = String::new();
    let mut nested_brace_depth = 0usize;
    let mut i = 0;

    while i < len {
        // 1. Line comment
        if bytes[i] == b'/' && i + 1 < len && bytes[i + 1] == b'/' {
            let c_start = i;
            i += 2;
            while i < len && bytes[i] != b'\n' {
                i += 1;
            }
            if nested_brace_depth == 0 {
                pending_comments.push_str(&body[c_start..i]);
                pending_comments.push('\n');
            }
            if i < len && bytes[i] == b'\n' {
                i += 1;
            }
            continue;
        }

        // 2. Block comment
        if bytes[i] == b'/' && i + 1 < len && bytes[i + 1] == b'*' {
            let c_start = i;
            i += 2;
            while i + 1 < len && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                i += 1;
            }
            if i + 1 < len {
                i += 2;
            }
            if nested_brace_depth == 0 {
                pending_comments.push_str(&body[c_start..i]);
                pending_comments.push('\n');
            }
            continue;
        }

        // 3. Nested braces (e.g. anonymous struct/union inside struct)
        if bytes[i] == b'{' {
            nested_brace_depth += 1;
            i += 1;
            continue;
        }
        if bytes[i] == b'}' {
            nested_brace_depth = nested_brace_depth.saturating_sub(1);
            if nested_brace_depth == 0 {
                // Skip up to ';' of nested union/struct
                while i < len && bytes[i] != b';' {
                    i += 1;
                }
                if i < len && bytes[i] == b';' {
                    i += 1;
                }
                pending_comments.clear();
                current_code.clear();
            } else {
                i += 1;
            }
            continue;
        }

        if nested_brace_depth > 0 {
            i += 1;
            continue;
        }

        // 4. Semicolon ending a field statement at top level of struct
        if bytes[i] == b';' {
            i += 1; // skip ';'

            // Check if there is a trailing comment on the SAME line right after ';'
            while i < len && (bytes[i] == b' ' || bytes[i] == b'\t') {
                i += 1;
            }
            if i + 1 < len && bytes[i] == b'/' && bytes[i + 1] == b'/' {
                let c_start = i;
                i += 2;
                while i < len && bytes[i] != b'\n' {
                    i += 1;
                }
                pending_comments.push_str(&body[c_start..i]);
                pending_comments.push('\n');
                if i < len && bytes[i] == b'\n' {
                    i += 1;
                }
            } else if i + 1 < len && bytes[i] == b'/' && bytes[i + 1] == b'*' {
                let c_start = i;
                i += 2;
                while i + 1 < len && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                    i += 1;
                }
                if i + 1 < len {
                    i += 2;
                }
                pending_comments.push_str(&body[c_start..i]);
                pending_comments.push('\n');
            }

            let stmt = current_code.trim();
            if !stmt.is_empty() && !stmt.contains('(') {
                let field_attrs = extract_attributes(&pending_comments);
                parse_single_field_stmt(stmt, &field_attrs, &mut fields);
            }

            pending_comments.clear();
            current_code.clear();
            continue;
        }

        current_code.push(bytes[i] as char);
        i += 1;
    }

    fields
}

fn parse_single_field_stmt(stmt: &str, field_attrs: &[CtAttr], fields: &mut Vec<CtField>) {
    let comma_parts: Vec<&str> = stmt.split(',').map(|s| s.trim()).collect();
    if comma_parts.is_empty() {
        return;
    }

    let first = comma_parts[0];
    // Handle bitfield `: width` on first declarator
    let first_no_bitfield = first.split(':').next().unwrap_or(first).trim();
    let words: Vec<&str> = first_no_bitfield.split_whitespace().collect();
    if words.len() < 2 {
        return;
    }

    let first_var = words[words.len() - 1];
    let base_type = words[..words.len() - 1].join(" ");

    let mut all_vars = vec![first_var];
    for part in &comma_parts[1..] {
        let no_bf = part.split(':').next().unwrap_or(part).trim();
        all_vars.push(no_bf);
    }

    for v in all_vars {
        let mut var_str = v.trim();
        if var_str.is_empty() {
            continue;
        }

        let mut is_ptr = base_type.contains('*');
        while var_str.starts_with('*') {
            is_ptr = true;
            var_str = var_str[1..].trim();
        }

        let mut array_len = 0;
        if let Some(open_bracket) = var_str.find('[') {
            if let Some(close_bracket) = var_str.find(']') {
                if close_bracket > open_bracket {
                    let num_str = var_str[open_bracket + 1..close_bracket].trim();
                    if let Ok(len) = num_str.parse::<usize>() {
                        array_len = len;
                    }
                }
            }
            var_str = var_str[..open_bracket].trim();
        }

        if !is_valid_c_ident(var_str) {
            continue;
        }

        let clean_base = base_type.trim_end_matches('*').trim();
        let final_type = if is_ptr {
            format!("{}*", clean_base)
        } else {
            clean_base.to_string()
        };

        let kind = TypeKind::from_c_type(clean_base, is_ptr, array_len > 0);

        fields.push(CtField {
            name: var_str.to_string(),
            type_name: final_type,
            kind,
            is_pointer: is_ptr,
            array_len,
            attrs: field_attrs.to_vec(),
        });
    }
}

fn parse_enum_variants(body: &str) -> Vec<CtVariant> {
    let mut variants = Vec::new();
    let mut current_val = 0i64;
    let bytes = body.as_bytes();
    let len = bytes.len();

    let mut pending_comments = String::new();
    let mut current_code = String::new();
    let mut i = 0;

    while i <= len {
        if i < len && bytes[i] == b'/' && i + 1 < len && bytes[i + 1] == b'/' {
            let c_start = i;
            i += 2;
            while i < len && bytes[i] != b'\n' {
                i += 1;
            }
            pending_comments.push_str(&body[c_start..i]);
            pending_comments.push('\n');
            if i < len && bytes[i] == b'\n' {
                i += 1;
            }
            continue;
        }
        if i < len && bytes[i] == b'/' && i + 1 < len && bytes[i + 1] == b'*' {
            let c_start = i;
            i += 2;
            while i + 1 < len && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                i += 1;
            }
            if i + 1 < len {
                i += 2;
            }
            pending_comments.push_str(&body[c_start..i]);
            pending_comments.push('\n');
            continue;
        }

        if i == len || bytes[i] == b',' {
            if i < len {
                i += 1; // skip ','
                while i < len && (bytes[i] == b' ' || bytes[i] == b'\t') {
                    i += 1;
                }
                if i + 1 < len && bytes[i] == b'/' && bytes[i + 1] == b'/' {
                    let c_start = i;
                    i += 2;
                    while i < len && bytes[i] != b'\n' {
                        i += 1;
                    }
                    pending_comments.push_str(&body[c_start..i]);
                    pending_comments.push('\n');
                }
            }

            let clean = current_code.trim();
            if !clean.is_empty() {
                let attrs = extract_attributes(&pending_comments);
                if let Some(eq_idx) = clean.find('=') {
                    let name = clean[..eq_idx].trim().to_string();
                    let val_str = clean[eq_idx + 1..].trim();
                    if let Ok(v) = val_str.parse::<i64>() {
                        current_val = v;
                    } else if let Some(hex_str) =
                        val_str.strip_prefix("0x").or_else(|| val_str.strip_prefix("0X"))
                    {
                        if let Ok(v) = i64::from_str_radix(hex_str, 16) {
                            current_val = v;
                        }
                    }
                    if is_valid_c_ident(&name) {
                        variants.push(CtVariant {
                            name,
                            value: current_val,
                            attrs,
                        });
                        current_val += 1;
                    }
                } else {
                    let name = clean.split_whitespace().next().unwrap_or(clean).to_string();
                    if is_valid_c_ident(&name) {
                        variants.push(CtVariant {
                            name,
                            value: current_val,
                            attrs,
                        });
                        current_val += 1;
                    }
                }
            }

            pending_comments.clear();
            current_code.clear();
            if i == len {
                break;
            }
            continue;
        }

        current_code.push(bytes[i] as char);
        i += 1;
    }

    variants
}
