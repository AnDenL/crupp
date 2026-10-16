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

/// Validates attribute names. Unlike C keywords, attribute names CAN match C keywords
/// (e.g. `@default`, `@const`, `@static`, `@case`, `@inline`), because they are stored
/// as string metadata in `ct_attr` (`.name = "default"`).
pub fn is_valid_attr_ident(s: &str) -> bool {
    if s.is_empty() {
        return false;
    }
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => (),
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn push_unique_attr(attrs: &mut Vec<CtAttr>, name: String, value: String) {
    if let Some(existing) = attrs.iter_mut().find(|a| a.name == name) {
        if existing.value.is_empty() && !value.is_empty() {
            existing.value = value;
        }
    } else {
        attrs.push(CtAttr { name, value });
    }
}

/// Parses comma-separated `key = val` or `key("val")` or `flag` sub-attributes inside `@attr(...)` or `#[...]`.
fn parse_inner_sub_attrs(inner: &str, attrs: &mut Vec<CtAttr>) {
    let bytes = inner.as_bytes();
    let len = bytes.len();
    let mut i = 0;

    while i < len {
        while i < len && (bytes[i].is_ascii_whitespace() || bytes[i] == b',') {
            i += 1;
        }
        if i >= len {
            break;
        }

        let name_start = i;
        while i < len && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
            i += 1;
        }
        if i == name_start {
            i += 1;
            continue;
        }
        let sub_name = inner[name_start..i].to_string();
        if !is_valid_attr_ident(&sub_name) {
            continue;
        }

        while i < len && (bytes[i] == b' ' || bytes[i] == b'\t') {
            i += 1;
        }

        if i < len && (bytes[i] == b'=' || bytes[i] == b':') {
            i += 1;
            while i < len && (bytes[i] == b' ' || bytes[i] == b'\t') {
                i += 1;
            }
            if i < len && bytes[i] == b'"' {
                i += 1;
                let v_start = i;
                while i < len && bytes[i] != b'"' {
                    if bytes[i] == b'\\' && i + 1 < len {
                        i += 2;
                    } else {
                        i += 1;
                    }
                }
                let val = inner[v_start..i].to_string();
                if i < len && bytes[i] == b'"' {
                    i += 1;
                }
                push_unique_attr(attrs, sub_name, val);
            } else {
                let v_start = i;
                let mut depth = 0usize;
                while i < len && (depth > 0 || (bytes[i] != b',' && bytes[i] != b']')) {
                    if bytes[i] == b'(' || bytes[i] == b'{' {
                        depth += 1;
                    } else if bytes[i] == b')' || bytes[i] == b'}' {
                        depth = depth.saturating_sub(1);
                    }
                    i += 1;
                }
                let val = strip_outer_quotes(inner[v_start..i].trim()).to_string();
                push_unique_attr(attrs, sub_name, val);
            }
        } else if i < len && bytes[i] == b'(' {
            i += 1;
            let v_start = i;
            let mut depth = 1usize;
            let mut in_quotes = false;
            while i < len && depth > 0 {
                if in_quotes {
                    if bytes[i] == b'\\' && i + 1 < len {
                        i += 2;
                        continue;
                    } else if bytes[i] == b'"' {
                        in_quotes = false;
                    }
                } else if bytes[i] == b'"' {
                    in_quotes = true;
                } else if bytes[i] == b'(' {
                    depth += 1;
                } else if bytes[i] == b')' {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                }
                i += 1;
            }
            let val = strip_outer_quotes(inner[v_start..i].trim()).to_string();
            if i < len && bytes[i] == b')' {
                i += 1;
            }
            push_unique_attr(attrs, sub_name, val);
        }
    }
}

/// Extracts `@attr`, `@attr(val)`, `@attr("val")`, `#[attr]`, `#[attr(val)]`, `#[attr, key = "val"]` from comment text.
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
            if i < len && bytes[i] == b'[' {
                // Support @[component] syntax too
                let bracket_start = i + 1;
                let mut b_depth = 1usize;
                i += 1;
                while i < len && b_depth > 0 {
                    if bytes[i] == b'[' {
                        b_depth += 1;
                    } else if bytes[i] == b']' {
                        b_depth -= 1;
                        if b_depth == 0 {
                            break;
                        }
                    }
                    i += 1;
                }
                let inner = &text[bracket_start..i];
                parse_inner_sub_attrs(inner, &mut attrs);
                // Also extract first word if bare flag like @[component]
                let first_word: String = inner
                    .trim_start()
                    .chars()
                    .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                    .collect();
                if is_valid_attr_ident(&first_word) {
                    push_unique_attr(&mut attrs, first_word, String::new());
                }
                if i < len && bytes[i] == b']' {
                    i += 1;
                }
                continue;
            }
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
        if !is_valid_attr_ident(&name) {
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
            // If raw_val contains key = value pairs (e.g. @component(table = "camera")), extract them too!
            if raw_val.contains('=') || raw_val.contains(':') {
                parse_inner_sub_attrs(raw_val, &mut attrs);
            }
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
            let rem_start = i;
            while i < len && bytes[i] != b']' && bytes[i] != b'\n' {
                i += 1;
            }
            let rem = text[rem_start..i].trim();
            if !rem.is_empty() {
                parse_inner_sub_attrs(rem, &mut attrs);
            }
            if i < len && bytes[i] == b']' {
                i += 1;
            }
        }

        // If user wrote `@component("camera")`, also populate `table = "camera"` if not already present
        if name == "component" && !value.is_empty() && !value.contains('=') && !value.contains(':') {
            let tbl_val = value.clone();
            push_unique_attr(&mut attrs, name, value);
            push_unique_attr(&mut attrs, "table".to_string(), tbl_val);
        } else {
            push_unique_attr(&mut attrs, name, value);
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

fn line_is_direct_annotation(trimmed: &str) -> bool {
    let body = trimmed
        .trim_start_matches("//")
        .trim_start_matches("/*")
        .trim_start_matches('*')
        .trim_start();
    body.starts_with('@') || body.starts_with("#[")
}

/// Collects the contiguous block of comment lines immediately preceding `decl_byte_offset`.
/// Stops on blank lines for prose comments so file header banners never leak into structs,
/// while tolerating a single blank line if the comment line directly above it starts with an explicit `@attr` or `#[attr]`.
fn collect_preceding_comments(source: &str, decl_byte_offset: usize) -> String {
    let before = &source[..decl_byte_offset];
    let rev_lines: Vec<&str> = before.lines().rev().collect();
    let mut collected_lines = Vec::new();
    let mut in_block_comment = false;
    let mut idx = 0;

    while idx < rev_lines.len() {
        let line = rev_lines[idx];
        let trimmed = line.trim();
        if trimmed.is_empty() {
            if in_block_comment {
                collected_lines.push(line);
                idx += 1;
                continue;
            }
            // If there is a single blank line and the line immediately above it is an explicit `@attr` line, keep scanning
            if idx + 1 < rev_lines.len() && line_is_direct_annotation(rev_lines[idx + 1].trim()) {
                idx += 1;
                continue;
            }
            break;
        }

        if in_block_comment {
            collected_lines.push(line);
            if trimmed.contains("/*") {
                in_block_comment = false;
            }
            idx += 1;
            continue;
        }

        if trimmed.starts_with("//") {
            collected_lines.push(line);
        } else if trimmed.ends_with("*/") || (trimmed.starts_with("/*") && trimmed.contains("*/")) {
            collected_lines.push(line);
            if !trimmed.contains("/*") {
                in_block_comment = true;
            }
        } else {
            // Reached a code line or preprocessor directive
            break;
        }
        idx += 1;
    }

    collected_lines.reverse();
    collected_lines.join("\n")
}

/// Strips any `#if comptime ... #endif` blocks from source text before scanning for C declarations,
/// strictly ignoring `#if comptime` inside `//` or `/* ... */` comments.
pub fn mask_comptime_blocks(source: &str) -> String {
    let mut out = String::with_capacity(source.len());
    let mut depth = 0usize;
    let mut in_block_comment = false;

    for line in source.lines() {
        let bytes = line.as_bytes();
        let len = bytes.len();
        let mut i = 0;

        if in_block_comment {
            while i + 1 < len {
                if bytes[i] == b'*' && bytes[i + 1] == b'/' {
                    in_block_comment = false;
                    i += 2;
                    break;
                }
                i += 1;
            }
            if in_block_comment {
                if depth == 0 {
                    out.push_str(line);
                }
                out.push('\n');
                continue;
            }
        }

        while i < len && (bytes[i] == b' ' || bytes[i] == b'\t' || bytes[i] == b'\r') {
            i += 1;
        }

        let is_directive = i < len && bytes[i] == b'#';
        let mut clean_dir = String::new();

        let mut scan_i = i;
        while scan_i < len {
            if in_block_comment {
                if scan_i + 1 < len && bytes[scan_i] == b'*' && bytes[scan_i + 1] == b'/' {
                    in_block_comment = false;
                    scan_i += 2;
                } else {
                    scan_i += 1;
                }
                continue;
            }
            if scan_i + 1 < len && bytes[scan_i] == b'/' && bytes[scan_i + 1] == b'/' {
                break;
            }
            if scan_i + 1 < len && bytes[scan_i] == b'/' && bytes[scan_i + 1] == b'*' {
                in_block_comment = true;
                scan_i += 2;
                continue;
            }
            if is_directive && bytes[scan_i] != b'\r' {
                clean_dir.push(bytes[scan_i] as char);
            }
            scan_i += 1;
        }

        if depth == 0 {
            if is_directive {
                let after_hash = clean_dir.strip_prefix('#').unwrap_or(&clean_dir).trim_start();
                let lower = after_hash.to_ascii_lowercase();
                let tokens: Vec<&str> = lower.split_whitespace().collect();
                if tokens.len() >= 2 && (tokens[0] == "if" || tokens[0] == "ifdef") {
                    let cond = tokens[1..].join("");
                    if cond == "comptime"
                        || cond == "defined(comptime)"
                        || cond == "comptime!=0"
                        || cond == "comptime==1"
                    {
                        depth = 1;
                        out.push('\n');
                        continue;
                    }
                }
            }
            out.push_str(line);
            out.push('\n');
        } else {
            if is_directive {
                let after_hash = clean_dir.strip_prefix('#').unwrap_or(&clean_dir).trim_start();
                let kw_end = after_hash
                    .find(|c: char| !c.is_ascii_alphabetic())
                    .unwrap_or(after_hash.len());
                match &after_hash[..kw_end] {
                    "if" | "ifdef" | "ifndef" => depth += 1,
                    "endif" => depth = depth.saturating_sub(1),
                    _ => {}
                }
            }
            out.push('\n');
        }
    }

    out
}

pub fn merge_struct_into(all_structs: &mut Vec<CtStruct>, s: CtStruct) {
    if let Some(existing) = all_structs.iter_mut().find(|x| x.name == s.name) {
        if existing.fields.is_empty() && !s.fields.is_empty() {
            let mut combined_attrs = existing.attrs.clone();
            for a in s.attrs {
                push_unique_attr(&mut combined_attrs, a.name, a.value);
            }
            *existing = CtStruct {
                name: s.name,
                fields: s.fields,
                attrs: combined_attrs,
            };
        } else {
            for a in s.attrs {
                push_unique_attr(&mut existing.attrs, a.name, a.value);
            }
        }
    } else {
        all_structs.push(s);
    }
}

pub fn merge_enum_into(all_enums: &mut Vec<CtEnum>, e: CtEnum) {
    if let Some(existing) = all_enums.iter_mut().find(|x| x.name == e.name) {
        if existing.variants.is_empty() && !e.variants.is_empty() {
            let mut combined_attrs = existing.attrs.clone();
            for a in e.attrs {
                push_unique_attr(&mut combined_attrs, a.name, a.value);
            }
            *existing = CtEnum {
                name: e.name,
                variants: e.variants,
                attrs: combined_attrs,
            };
        } else {
            for a in e.attrs {
                push_unique_attr(&mut existing.attrs, a.name, a.value);
            }
        }
    } else {
        all_enums.push(e);
    }
}

pub fn parse_c_declarations(raw_content: &str) -> (Vec<CtStruct>, Vec<CtEnum>) {
    let content_owned = mask_comptime_blocks(raw_content);
    let content = content_owned.as_str();
    let bytes = content.as_bytes();
    let len = bytes.len();

    let mut structs = Vec::new();
    let mut enums = Vec::new();

    let mut i = 0;
    let mut at_line_start = true;

    while i < len {
        if bytes[i] == b'\n' {
            at_line_start = true;
            i += 1;
            continue;
        }

        // Skip preprocessor directives (#define, #pragma, #include, #error, etc.) including \-continued lines,
        // while keeping conditional directives (#if, #ifdef, #ifndef, #else, #elif, #endif) transparent.
        if at_line_start {
            if bytes[i] == b' ' || bytes[i] == b'\t' || bytes[i] == b'\r' {
                i += 1;
                continue;
            }
            if bytes[i] == b'#' {
                let hash_pos = i;
                i += 1;
                while i < len && (bytes[i] == b' ' || bytes[i] == b'\t') {
                    i += 1;
                }
                let kw_start = i;
                while i < len && bytes[i].is_ascii_alphabetic() {
                    i += 1;
                }
                let kw = &content[kw_start..i];
                // Always skip the preprocessor line itself (including any backslash continuations for #define)
                i = hash_pos;
                while i < len {
                    if bytes[i] == b'\\' && i + 1 < len {
                        if bytes[i + 1] == b'\n' {
                            i += 2;
                            continue;
                        } else if bytes[i + 1] == b'\r' && i + 2 < len && bytes[i + 2] == b'\n' {
                            i += 3;
                            continue;
                        }
                    }
                    if bytes[i] == b'\n' {
                        break;
                    }
                    i += 1;
                }
                let _ = kw;
                continue;
            }
            at_line_start = false;
        }

        // 1. Skip line comments
        if bytes[i] == b'/' && i + 1 < len && bytes[i + 1] == b'/' {
            i += 2;
            while i < len && bytes[i] != b'\n' {
                i += 1;
            }
            continue;
        }

        // 2. Skip block comments (bounds-safe!)
        if bytes[i] == b'/' && i + 1 < len && bytes[i + 1] == b'*' {
            i += 2;
            while i + 1 < len && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                i += 1;
            }
            if i + 1 < len {
                i += 2;
            } else {
                i = len;
            }
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

        // Inspect identifiers for struct / enum / typedef declarations.
        // Note: we intentionally do NOT gate on `brace_depth == 0` because C headers wrapped in
        // `#ifdef __cplusplus \n extern "C" { \n #endif` or C++ `namespace { ... }` have outer `{`,
        // and `try_parse_braced_declaration` strictly matches only real `[typedef] struct/enum [Tag] { ... }`
        // type definitions while skipping over nested struct bodies via `i = next_idx`.
        if bytes[i].is_ascii_alphabetic() || bytes[i] == b'_' {
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
                while kw_end < len
                    && (bytes[kw_end].is_ascii_alphanumeric() || bytes[kw_end] == b'_')
                {
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
                if let Some((
                    tag_name,
                    body_str,
                    alias_name,
                    open_brace_idx,
                    close_brace_idx,
                    next_idx,
                )) = try_parse_braced_declaration(content, i)
                {
                    let name = if is_typedef && !alias_name.is_empty() {
                        alias_name.clone()
                    } else if !tag_name.is_empty() {
                        tag_name.clone()
                    } else {
                        alias_name.clone()
                    };

                    if is_valid_c_ident(&name) {
                        let line_start = content[..decl_start]
                            .rfind('\n')
                            .map(|idx| idx + 1)
                            .unwrap_or(0);
                        let mut combined_comment_text =
                            collect_preceding_comments(content, line_start);

                        // Also include any inline comments on the `typedef struct` line before `{`
                        if open_brace_idx > line_start {
                            combined_comment_text.push('\n');
                            combined_comment_text.push_str(&content[line_start..open_brace_idx]);
                        }

                        // Check if there is a trailing comment on the opening `{` line itself (before any `;`)
                        let mut clean_body = body_str;
                        if let Some(first_nl) = clean_body.find('\n') {
                            let first_line = &clean_body[..first_nl];
                            if !first_line.contains(';') {
                                if let Some(c_pos) =
                                    first_line.find("//").or_else(|| first_line.find("/*"))
                                {
                                    combined_comment_text.push('\n');
                                    combined_comment_text.push_str(&first_line[c_pos..]);
                                    clean_body = format!(
                                        "{}{}",
                                        &first_line[..c_pos],
                                        &clean_body[first_nl..]
                                    );
                                }
                            }
                        }

                        // Also include any trailing comment between `}` and the end of the `} Name;` line
                        let stmt_line_end = content[close_brace_idx..]
                            .find('\n')
                            .map(|off| close_brace_idx + off)
                            .unwrap_or(len);
                        if stmt_line_end > close_brace_idx {
                            combined_comment_text.push('\n');
                            combined_comment_text
                                .push_str(&content[close_brace_idx..stmt_line_end]);
                        }

                        let attrs = extract_attributes(&combined_comment_text);

                        if decl_kind == "struct" {
                            let fields = parse_struct_fields(&clean_body);
                            merge_struct_into(
                                &mut structs,
                                CtStruct {
                                    name: name.clone(),
                                    fields,
                                    attrs: attrs.clone(),
                                },
                            );
                        } else if decl_kind == "enum" {
                            let variants = parse_enum_variants(&clean_body);
                            merge_enum_into(
                                &mut enums,
                                CtEnum {
                                    name: name.clone(),
                                    variants,
                                    attrs: attrs.clone(),
                                },
                            );
                        }
                    }

                    i = next_idx;
                    continue;
                } else if is_typedef {
                    // Check for annotated forward typedef: `// @component \n typedef struct Tag Alias;`
                    if let Some((tag_name, alias_name, next_idx)) =
                        try_parse_forward_typedef(content, i)
                    {
                        let line_start = content[..decl_start]
                            .rfind('\n')
                            .map(|idx| idx + 1)
                            .unwrap_or(0);
                        let prefix_comments = collect_preceding_comments(content, line_start);
                        let attrs = extract_attributes(&prefix_comments);
                        if !attrs.is_empty() {
                            for n in [alias_name, tag_name] {
                                if is_valid_c_ident(&n) {
                                    if decl_kind == "struct" {
                                        merge_struct_into(
                                            &mut structs,
                                            CtStruct {
                                                name: n,
                                                fields: Vec::new(),
                                                attrs: attrs.clone(),
                                            },
                                        );
                                    } else if decl_kind == "enum" {
                                        merge_enum_into(
                                            &mut enums,
                                            CtEnum {
                                                name: n,
                                                variants: Vec::new(),
                                                attrs: attrs.clone(),
                                            },
                                        );
                                    }
                                }
                            }
                        }
                        i = next_idx;
                        continue;
                    }
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
            if i + 1 < len {
                i += 2;
            } else {
                i = len;
            }
        } else {
            break;
        }
    }
    i
}

fn try_parse_forward_typedef(content: &str, start_after_kw: usize) -> Option<(String, String, usize)> {
    let bytes = content.as_bytes();
    let len = bytes.len();
    let mut i = skip_ws_and_comments(content, start_after_kw);
    let mut idents = Vec::new();

    while i < len && bytes[i] != b';' && bytes[i] != b'{' && bytes[i] != b'(' {
        if bytes[i] == b'/' && i + 1 < len && (bytes[i + 1] == b'/' || bytes[i + 1] == b'*') {
            i = skip_ws_and_comments(content, i);
            continue;
        }
        if bytes[i].is_ascii_alphabetic() || bytes[i] == b'_' {
            let w_start = i;
            while i < len && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                i += 1;
            }
            idents.push(content[w_start..i].to_string());
        } else if bytes[i].is_ascii_whitespace() {
            i += 1;
        } else {
            return None;
        }
    }

    if i < len && bytes[i] == b';' && idents.len() == 2 {
        Some((idents[0].clone(), idents[1].clone(), i + 1))
    } else {
        None
    }
}

/// Starting right after `struct` or `enum`, parses optional tag name, `{ body }`, optional alias name, and `;`.
/// Returns `None` if this is a forward declaration (`struct Foo;`) or variable/function return type without `{ ... }`.
fn try_parse_braced_declaration(
    content: &str,
    start_after_kw: usize,
) -> Option<(String, String, String, usize, usize, usize)> {
    let bytes = content.as_bytes();
    let len = bytes.len();
    let mut i = skip_ws_and_comments(content, start_after_kw);

    // Collect tokens before `{` or `;`
    let mut tag_name = String::new();
    let mut ident_count = 0usize;

    while i < len && bytes[i] != b'{' && bytes[i] != b';' {
        if bytes[i] == b'/' && i + 1 < len && (bytes[i + 1] == b'/' || bytes[i + 1] == b'*') {
            i = skip_ws_and_comments(content, i);
            continue;
        }
        // Handle C23/C++11 [[...]] attributes before `{`
        if bytes[i] == b'[' && i + 1 < len && bytes[i + 1] == b'[' {
            let mut b_depth = 1usize;
            i += 2;
            while i < len && b_depth > 0 {
                if bytes[i] == b'[' {
                    b_depth += 1;
                } else if bytes[i] == b']' {
                    b_depth = b_depth.saturating_sub(1);
                }
                i += 1;
            }
            continue;
        }
        if bytes[i].is_ascii_alphabetic() || bytes[i] == b'_' {
            let w_start = i;
            while i < len && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                i += 1;
            }
            let w = &content[w_start..i];
            let next_non_ws = skip_ws_and_comments(content, i);
            if matches!(
                w,
                "__attribute__" | "__declspec" | "_Alignas" | "alignas"
            ) || (next_non_ws < len && bytes[next_non_ws] == b'(' && ident_count == 0)
            {
                if next_non_ws < len && bytes[next_non_ws] == b'(' {
                    i = next_non_ws + 1;
                    let mut p_depth = 1;
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
                // If we already saw a tag name and now see another identifier followed by `(`,
                // this is a function returning `struct Tag func_name(...)`, NOT a struct definition!
                if next_non_ws < len && bytes[next_non_ws] == b'(' {
                    return None;
                }
                tag_name = w.to_string();
                ident_count += 1;
            }
        } else if bytes[i].is_ascii_whitespace() {
            i += 1;
        } else {
            // Unexpected token before `{` (e.g., pointer `struct Foo* bar()`, `=`, `,`, `(`)
            return None;
        }
    }

    if i >= len || bytes[i] != b'{' {
        return None;
    }

    let open_brace_idx = i;

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
            if i + 1 < len {
                i += 2;
            } else {
                i = len;
            }
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

    let close_brace_idx = i;
    let body_end = i;
    let body_str = content[body_start..body_end].to_string();
    i += 1; // skip '}'

    // Parse optional alias name before ';' (taking the first non-attribute identifier before any ',')
    let mut alias_name = String::new();
    let mut hit_comma = false;
    while i < len && bytes[i] != b';' {
        if bytes[i] == b'/' && i + 1 < len && (bytes[i + 1] == b'/' || bytes[i + 1] == b'*') {
            i = skip_ws_and_comments(content, i);
            continue;
        }
        if bytes[i] == b',' {
            hit_comma = true;
            i += 1;
            continue;
        }
        if bytes[i].is_ascii_alphabetic() || bytes[i] == b'_' {
            let w_start = i;
            while i < len && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                i += 1;
            }
            let w = &content[w_start..i];
            if w == "__attribute__" || w == "__declspec" || w == "_Alignas" || w == "alignas" {
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
            } else if !hit_comma && alias_name.is_empty() && w != "packed" && w != "aligned" {
                alias_name = w.to_string();
            }
        } else {
            i += 1;
        }
    }

    if i < len && bytes[i] == b';' {
        i += 1;
    }

    Some((
        tag_name,
        body_str,
        alias_name,
        open_brace_idx,
        close_brace_idx,
        i,
    ))
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
            } else {
                i = len;
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

            // Check if there are trailing comments on the SAME line right after ';'
            loop {
                while i < len && (bytes[i] == b' ' || bytes[i] == b'\t' || bytes[i] == b'\r') {
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
                    break;
                } else if i + 1 < len && bytes[i] == b'/' && bytes[i + 1] == b'*' {
                    let c_start = i;
                    i += 2;
                    while i + 1 < len && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                        i += 1;
                    }
                    if i + 1 < len {
                        i += 2;
                    } else {
                        i = len;
                    }
                    pending_comments.push_str(&body[c_start..i]);
                    pending_comments.push('\n');
                } else {
                    break;
                }
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
    // Handle optional `= default_val` and bitfield `: width` on first declarator
    let (first_no_eq, first_eq_def) = match first.split_once('=') {
        Some((lhs, rhs)) => (lhs.trim(), Some(rhs.trim())),
        None => (first, None),
    };
    let first_no_bitfield = first_no_eq.split(':').next().unwrap_or(first_no_eq).trim();
    let words: Vec<&str> = first_no_bitfield.split_whitespace().collect();
    if words.len() < 2 {
        return;
    }

    let first_var = words[words.len() - 1];
    let base_type = words[..words.len() - 1].join(" ");

    let mut all_vars = vec![(first_var, first_eq_def)];
    for part in &comma_parts[1..] {
        let (no_eq, eq_def) = match part.split_once('=') {
            Some((lhs, rhs)) => (lhs.trim(), Some(rhs.trim())),
            None => (*part, None),
        };
        let no_bf = no_eq.split(':').next().unwrap_or(no_eq).trim();
        all_vars.push((no_bf, eq_def));
    }

    for (v, inline_def) in all_vars {
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

        let mut attrs = field_attrs.to_vec();
        if let Some(def_val) = inline_def {
            if !def_val.is_empty()
                && !attrs
                    .iter()
                    .any(|a| a.name == "def" || a.name == "default")
            {
                attrs.push(CtAttr {
                    name: "def".to_string(),
                    value: def_val.to_string(),
                });
            }
        }

        fields.push(CtField {
            name: var_str.to_string(),
            type_name: final_type,
            kind,
            is_pointer: is_ptr,
            array_len,
            attrs,
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
            } else {
                i = len;
            }
            pending_comments.push_str(&body[c_start..i]);
            pending_comments.push('\n');
            continue;
        }

        let is_sep = i == len || bytes[i] == b',' || bytes[i] == b';';
        if is_sep {
            let item = current_code.trim();
            if !item.is_empty() {
                let attrs = extract_attributes(&pending_comments);
                let (name, val) = if let Some((n, v)) = item.split_once('=') {
                    let clean_n = n.trim();
                    let clean_v = v.trim();
                    let parsed_v = if clean_v.starts_with("0x") || clean_v.starts_with("0X") {
                        i64::from_str_radix(&clean_v[2..], 16).unwrap_or(current_val)
                    } else {
                        clean_v.parse::<i64>().unwrap_or(current_val)
                    };
                    current_val = parsed_v + 1;
                    (clean_n.to_string(), parsed_v)
                } else {
                    let val = current_val;
                    current_val += 1;
                    (item.to_string(), val)
                };

                if is_valid_c_ident(&name) {
                    variants.push(CtVariant {
                        name,
                        value: val,
                        attrs,
                    });
                }
            }
            pending_comments.clear();
            current_code.clear();
            i += 1;
            continue;
        }

        current_code.push(bytes[i] as char);
        i += 1;
    }

    variants
}
