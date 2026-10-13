use super::types::{CtAttr, CtEnum, CtField, CtStruct, CtVariant, TypeKind};
use regex::Regex;
use std::sync::OnceLock;

static RE_STRUCT: OnceLock<Regex> = OnceLock::new();
static RE_ENUM: OnceLock<Regex> = OnceLock::new();
static RE_ATTR: OnceLock<Regex> = OnceLock::new();

pub fn extract_attributes(text: &str) -> Vec<CtAttr> {
    let mut attrs = Vec::new();

    let re_attr = RE_ATTR.get_or_init(|| {
        Regex::new(r#"(?:@|#\[\s*)([a-zA-Z_]\w*)(?:\s*(?:[(=:])\s*"?([^"')\]]+)"?\s*\)?)?(?:\])?"#).unwrap()
    });

    for caps in re_attr.captures_iter(text) {
        if let Some(name_match) = caps.get(1) {
            let name = name_match.as_str().to_string();
            let value = caps.get(2).map_or("", |m| m.as_str()).trim().to_string();
            attrs.push(CtAttr { name, value });
        }
    }

    attrs
}

pub fn parse_c_declarations(content: &str) -> (Vec<CtStruct>, Vec<CtEnum>) {
    let mut structs = Vec::new();
    let mut enums = Vec::new();

    let re_struct = RE_STRUCT.get_or_init(|| {
        Regex::new(
            r#"(?s)(?P<prefix>(?:/[*].*?[*]/\s*|//[^\n]*\n\s*)*)?(?:typedef\s+struct\s*(?P<tag>\w+)?\s*\{(?P<body>[^}]+)\}\s*(?P<alias>\w+)\s*;|struct\s+(?P<sname>\w+)\s*(?:[a-zA-Z_]\w*\s*)?\{(?P<sbody>[^}]+)\}\s*(?:(?P<salias>\w+)\s*)?;)"#
        ).unwrap()
    });

    for caps in re_struct.captures_iter(content) {
        let prefix = caps.name("prefix").map(|m| m.as_str()).unwrap_or("");
        let body = caps
            .name("body")
            .or_else(|| caps.name("sbody"))
            .map(|m| m.as_str())
            .unwrap_or("");

        let name = caps
            .name("alias")
            .or_else(|| caps.name("sname"))
            .or_else(|| caps.name("tag"))
            .or_else(|| caps.name("salias"))
            .map(|m| m.as_str())
            .unwrap_or("");

        if name.is_empty() || body.is_empty() {
            continue;
        }

        let attrs = extract_attributes(prefix);
        let fields = parse_struct_fields(body);

        structs.push(CtStruct {
            name: name.to_string(),
            fields,
            attrs,
        });
    }

    let re_enum = RE_ENUM.get_or_init(|| {
        Regex::new(
            r#"(?s)(?P<prefix>(?:/[*].*?[*]/\s*|//[^\n]*\n\s*)*)?(?:typedef\s+enum\s*(?P<tag>\w+)?\s*\{(?P<body>[^}]+)\}\s*(?P<alias>\w+)\s*;|enum\s+(?P<ename>\w+)\s*\{(?P<ebody>[^}]+)\}\s*(?:(?P<ealias>\w+)\s*)?;)"#
        ).unwrap()
    });

    for caps in re_enum.captures_iter(content) {
        let prefix = caps.name("prefix").map(|m| m.as_str()).unwrap_or("");
        let body = caps
            .name("body")
            .or_else(|| caps.name("ebody"))
            .map(|m| m.as_str())
            .unwrap_or("");

        let name = caps
            .name("alias")
            .or_else(|| caps.name("ename"))
            .or_else(|| caps.name("tag"))
            .or_else(|| caps.name("ealias"))
            .map(|m| m.as_str())
            .unwrap_or("");

        if name.is_empty() || body.is_empty() {
            continue;
        }

        let attrs = extract_attributes(prefix);
        let variants = parse_enum_variants(body);
        enums.push(CtEnum {
            name: name.to_string(),
            variants,
            attrs,
        });
    }

    (structs, enums)
}

fn parse_struct_fields(body: &str) -> Vec<CtField> {
    let mut fields = Vec::new();

    // Iterate statement by statement ending with ';'
    for statement in body.split(';') {
        let raw_stmt = statement.trim();
        if raw_stmt.is_empty() {
            continue;
        }

        // Extract any field-level attributes (from line comments or block comments)
        let field_attrs = extract_attributes(raw_stmt);

        // Remove comments for clean type/name parsing
        let mut clean_stmt = String::new();
        for line in raw_stmt.lines() {
            let l = line.trim();
            if let Some(idx) = l.find("//") {
                let code_part = l[..idx].trim();
                if !code_part.is_empty() {
                    clean_stmt.push_str(code_part);
                    clean_stmt.push(' ');
                }
            } else {
                clean_stmt.push_str(l);
                clean_stmt.push(' ');
            }
        }
        let stmt = clean_stmt.trim();
        if stmt.is_empty() {
            continue;
        }

        let comma_parts: Vec<&str> = stmt.split(',').map(|s| s.trim()).collect();
        if comma_parts.is_empty() {
            continue;
        }

        let first = comma_parts[0];
        let words: Vec<&str> = first.split_whitespace().collect();
        if words.len() < 2 {
            continue;
        }

        let first_var = words[words.len() - 1];
        let base_type = words[..words.len() - 1].join(" ");

        let mut all_vars = vec![first_var];
        all_vars.extend_from_slice(&comma_parts[1..]);

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

            let final_type = if is_ptr && !base_type.contains('*') {
                format!("{}*", base_type)
            } else {
                base_type.clone()
            };

            let kind = TypeKind::from_c_type(&base_type, is_ptr, array_len > 0);

            fields.push(CtField {
                name: var_str.to_string(),
                type_name: final_type,
                kind,
                is_pointer: is_ptr,
                array_len,
                attrs: field_attrs.clone(),
            });
        }
    }

    fields
}

fn parse_enum_variants(body: &str) -> Vec<CtVariant> {
    let mut variants = Vec::new();
    let mut current_val = 0i64;

    for item in body.split(',') {
        let raw = item.trim();
        if raw.is_empty() {
            continue;
        }

        let attrs = extract_attributes(raw);

        let clean = if let Some(idx) = raw.find("//") {
            raw[..idx].trim()
        } else {
            raw
        };

        if clean.is_empty() {
            continue;
        }

        if let Some(eq_idx) = clean.find('=') {
            let name = clean[..eq_idx].trim().to_string();
            let val_str = clean[eq_idx + 1..].trim();
            if let Ok(v) = val_str.parse::<i64>() {
                current_val = v;
            } else if val_str.starts_with("0x") || val_str.starts_with("0X") {
                if let Ok(v) = i64::from_str_radix(&val_str[2..], 16) {
                    current_val = v;
                }
            }
            variants.push(CtVariant {
                name,
                value: current_val,
                attrs,
            });
            current_val += 1;
        } else {
            let name = clean.split_whitespace().next().unwrap_or(clean).to_string();
            variants.push(CtVariant {
                name,
                value: current_val,
                attrs,
            });
            current_val += 1;
        }
    }

    variants
}
