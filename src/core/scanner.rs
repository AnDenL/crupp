use crate::config::{Language, Package};
use regex::Regex;
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::sync::OnceLock;

static RE_EXPORT: OnceLock<Regex> = OnceLock::new();
static RE_MODULE: OnceLock<Regex> = OnceLock::new();
static RE_IMPORT: OnceLock<Regex> = OnceLock::new();
static RE_STRIP: OnceLock<Regex> = OnceLock::new();
static RE_MAIN: OnceLock<Regex> = OnceLock::new();

#[derive(Debug, Clone)]
pub struct TranslationUnit {
    pub path: PathBuf,
    pub language: Language,
    pub exported_module: Option<String>,
    pub imports: Vec<String>,
    pub base_hash: String,
    pub has_main: bool,
}

fn compute_base_hash(content: &str, lang: Language, config: &Package) -> String {
    let mut hasher = Sha256::new();
    hasher.update(content);
    match lang {
        Language::C => {
            hasher.update(config.compiler.c_compiler().as_bytes());
            hasher.update(config.c_standard.as_bytes());
            for f in &config.c_flags {
                hasher.update(f.as_bytes());
            }
        }
        Language::Cpp => {
            hasher.update(config.compiler.cpp_compiler().as_bytes());
            hasher.update(config.standard.as_bytes());
            for f in &config.cpp_flags {
                hasher.update(f.as_bytes());
            }
        }
    }
    for flag in &config.flags {
        hasher.update(flag.as_bytes());
    }
    for inc in &config.include_dirs {
        hasher.update(inc.as_bytes());
    }
    hex::encode(hasher.finalize())
}

fn strip_comments(text: &str) -> String {
    let re = RE_STRIP.get_or_init(|| Regex::new(r"//[^\n]*|(?s)/\*.*?\*/").unwrap());
    re.replace_all(text, " ").to_string()
}

pub fn scan_file(path: &Path, config: &Package) -> Option<TranslationUnit> {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    let language = match ext {
        "c" => Language::C,
        "cpp" | "cxx" | "cc" | "cppm" | "ixx" | "C" => Language::Cpp,
        _ => return None,
    };

    let raw_content = fs::read_to_string(path).ok()?;
    let content = strip_comments(&raw_content);

    let re_main = RE_MAIN.get_or_init(|| Regex::new(r"(?m)\b(?:int|void)\s+main\s*\(").unwrap());
    let has_main = re_main.is_match(&content);

    let mut exported_module = None;
    let mut imports = Vec::new();

    if language == Language::Cpp {
        let re_export = RE_EXPORT.get_or_init(|| {
            Regex::new(r"(?m)^\s*export\s+module\s+([a-zA-Z0-9_\.:]+)\s*;").unwrap()
        });
        let re_module = RE_MODULE
            .get_or_init(|| Regex::new(r"(?m)^\s*module\s+([a-zA-Z0-9_\.:]+)\s*;").unwrap());
        let re_import = RE_IMPORT
            .get_or_init(|| Regex::new(r"(?m)^\s*import\s+([a-zA-Z0-9_\.:]+)\s*;").unwrap());

        exported_module = re_export
            .captures(&content)
            .or_else(|| re_module.captures(&content))
            .and_then(|cap| cap.get(1))
            .map(|m| m.as_str().to_string());

        imports = re_import
            .captures_iter(&content)
            .filter_map(|cap| cap.get(1))
            .map(|m| m.as_str().to_string())
            .collect();
    }

    Some(TranslationUnit {
        path: path.to_path_buf(),
        language,
        exported_module,
        imports,
        base_hash: compute_base_hash(&raw_content, language, config),
        has_main,
    })
}

pub fn discover_sources(source_dir: &str, config: &Package) -> Vec<TranslationUnit> {
    let root_path = Path::new(source_dir);
    let is_root_scan = root_path == Path::new(".") || root_path == Path::new("./");
    let search_dir = if root_path.exists() {
        root_path
    } else if Path::new("src").exists() {
        Path::new("src")
    } else {
        Path::new(".")
    };

    let out_dir_name = Path::new(&config.out_dir)
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "build".to_string());

    let mut units = Vec::new();
    for entry in walkdir::WalkDir::new(search_dir)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }

        let is_in_out = path.components().any(|c| match c {
            Component::Normal(name) => name == out_dir_name.as_str(),
            _ => false,
        });
        if is_in_out {
            continue;
        }

        let is_hidden = path.components().any(|c| match c {
            Component::Normal(name) => name.to_string_lossy().starts_with('.'),
            _ => false,
        });
        if is_hidden {
            continue;
        }

        if is_root_scan {
            let is_external_suite = path.components().any(|c| match c {
                Component::Normal(name) => {
                    let n = name.to_string_lossy();
                    n == "tests"
                        || n == "fuzzing"
                        || n == "examples"
                        || n == "benchmark"
                        || n == "benchmarks"
                }
                _ => false,
            });
            if is_external_suite {
                continue;
            }
        }

        if let Some(unit) = scan_file(path, config) {
            units.push(unit);
        }
    }
    units
}

pub fn auto_discover_include_dirs(root: &Path, out_dir: &str) -> Vec<String> {
    let mut dirs = HashSet::new();
    dirs.insert(".".to_string());

    let out_dir_name = Path::new(out_dir).file_name().unwrap_or_default();

    for entry in walkdir::WalkDir::new(root)
        .max_depth(3)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }

        let is_excluded = path.components().any(|c| match c {
            Component::Normal(name) => {
                name == out_dir_name || name.to_string_lossy().starts_with('.')
            }
            _ => false,
        });
        if is_excluded {
            continue;
        }

        if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            if matches!(ext, "h" | "hpp" | "hxx" | "hh" | "inl") {
                if let Some(parent) = path.parent() {
                    dirs.insert(parent.to_string_lossy().to_string());
                }
            }
        }
    }

    let mut list: Vec<String> = dirs.into_iter().collect();
    list.sort();
    list
}
