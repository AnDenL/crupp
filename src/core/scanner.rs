use crate::config::{Language, Package};
use regex::Regex;
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::SystemTime;

static RE_EXPORT: OnceLock<Regex> = OnceLock::new();
static RE_MODULE: OnceLock<Regex> = OnceLock::new();
static RE_IMPORT: OnceLock<Regex> = OnceLock::new();
static RE_MAIN: OnceLock<Regex> = OnceLock::new();
static RE_INCLUDE: OnceLock<Regex> = OnceLock::new();

// In-memory cache for header file hashes to avoid re-reading the same header across multiple units
static HEADER_CACHE: OnceLock<Mutex<HashMap<PathBuf, (SystemTime, String)>>> = OnceLock::new();

fn get_header_cache() -> &'static Mutex<HashMap<PathBuf, (SystemTime, String)>> {
    HEADER_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

#[derive(Debug, Clone)]
pub struct TranslationUnit {
    pub path: PathBuf,
    pub language: Language,
    pub exported_module: Option<String>,
    pub imports: Vec<String>,
    pub base_hash: String,
    pub has_main: bool,
    pub direct_headers: Vec<PathBuf>,
}

/// Zero-intermediate-allocation state-machine comment stripper that operates directly
/// on byte slices, preserving UTF-8 multi-byte sequences and respecting string/char literals.
pub fn strip_comments_fast(text: &str) -> String {
    let bytes = text.as_bytes();
    let len = bytes.len();
    let mut out = Vec::with_capacity(len);
    let mut i = 0;

    #[derive(PartialEq, Eq)]
    enum State {
        Normal,
        StringLit,
        CharLit,
        LineComment,
        BlockComment,
    }

    let mut state = State::Normal;

    while i < len {
        let b = bytes[i];
        match state {
            State::Normal => {
                if b == b'"' {
                    state = State::StringLit;
                    out.push(b);
                } else if b == b'\'' {
                    state = State::CharLit;
                    out.push(b);
                } else if b == b'/' && i + 1 < len && bytes[i + 1] == b'/' {
                    state = State::LineComment;
                    out.push(b' ');
                    i += 1;
                } else if b == b'/' && i + 1 < len && bytes[i + 1] == b'*' {
                    state = State::BlockComment;
                    out.push(b' ');
                    i += 1;
                } else {
                    out.push(b);
                }
            }
            State::StringLit => {
                out.push(b);
                if b == b'\\' && i + 1 < len {
                    i += 1;
                    out.push(bytes[i]);
                } else if b == b'"' {
                    state = State::Normal;
                }
            }
            State::CharLit => {
                out.push(b);
                if b == b'\\' && i + 1 < len {
                    i += 1;
                    out.push(bytes[i]);
                } else if b == b'\'' {
                    state = State::Normal;
                }
            }
            State::LineComment => {
                if b == b'\n' {
                    state = State::Normal;
                    out.push(b'\n');
                } else {
                    out.push(b' ');
                }
            }
            State::BlockComment => {
                if b == b'*' && i + 1 < len && bytes[i + 1] == b'/' {
                    state = State::Normal;
                    out.push(b' ');
                    out.push(b' ');
                    i += 1;
                } else if b == b'\n' {
                    out.push(b'\n');
                } else {
                    out.push(b' ');
                }
            }
        }
        i += 1;
    }

    // Safety: we started with valid UTF-8 and only replaced ASCII slashes/stars/comments with ASCII spaces,
    // preserving newlines and all other original bytes intact.
    unsafe { String::from_utf8_unchecked(out) }
}

/// Discovers header dependencies for a source file and computes an aggregate hash of
/// all local included headers transitively. This guarantees cache invalidation when a header changes.
fn collect_header_dependencies(
    source_path: &Path,
    content: &str,
    include_dirs: &[String],
) -> (Vec<PathBuf>, String) {
    let re_inc =
        RE_INCLUDE.get_or_init(|| Regex::new(r#"^\s*#\s*include\s*["<]([^">]+)[">]"#).unwrap());

    let mut direct_headers = Vec::new();
    let mut direct_set = HashSet::new();
    let mut all_visited = HashSet::new();
    let mut header_hasher = Sha256::new();

    // Fast-path scan: only check lines starting with '#' and containing "include"
    for line in content.lines() {
        let trimmed = line.trim_start();
        if !trimmed.starts_with('#') || !trimmed.contains("include") {
            continue;
        }

        if let Some(caps) = re_inc.captures(trimmed) {
            let inc_name = caps.get(1).unwrap().as_str();
            if let Some(resolved) = resolve_include_path(inc_name, source_path, include_dirs) {
                if direct_set.insert(resolved.clone()) {
                    direct_headers.push(resolved);
                }
            }
        }
    }

    // Recursively hash headers
    let mut queue = direct_headers.clone();
    while let Some(hdr) = queue.pop() {
        if all_visited.insert(hdr.clone()) {
            if let Some(hash) = get_or_compute_header_hash(&hdr) {
                header_hasher.update(hdr.to_string_lossy().as_bytes());
                header_hasher.update(hash.as_bytes());

                // Inspect this header for nested includes
                if let Ok(hdr_content) = fs::read_to_string(&hdr) {
                    let clean = strip_comments_fast(&hdr_content);
                    for line in clean.lines() {
                        let trimmed = line.trim_start();
                        if !trimmed.starts_with('#') || !trimmed.contains("include") {
                            continue;
                        }

                        if let Some(caps) = re_inc.captures(trimmed) {
                            let nested_inc = caps.get(1).unwrap().as_str();
                            if let Some(res) = resolve_include_path(nested_inc, &hdr, include_dirs)
                            {
                                if !all_visited.contains(&res) {
                                    queue.push(res);
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    let headers_hash = hex::encode(header_hasher.finalize());
    (direct_headers, headers_hash)
}

fn resolve_include_path(
    inc: &str,
    current_file: &Path,
    include_dirs: &[String],
) -> Option<PathBuf> {
    // 1. Check relative to current file and its parent directories
    let mut anc = current_file.parent();
    while let Some(parent) = anc {
        let candidate = parent.join(inc);
        if candidate.is_file() {
            return fs::canonicalize(&candidate).ok().or(Some(candidate));
        }
        if parent == Path::new("") || parent == Path::new(".") {
            break;
        }
        anc = parent.parent();
    }

    // 2. Check standard project directories (., include/, src/)
    for base in &[".", "include", "src"] {
        let candidate = Path::new(base).join(inc);
        if candidate.is_file() {
            return fs::canonicalize(&candidate).ok().or(Some(candidate));
        }
    }

    // 3. Check each configured include directory
    for dir in include_dirs {
        let clean = dir.trim().trim_start_matches("-I").trim();
        if clean.is_empty() {
            continue;
        }
        let candidate = Path::new(clean).join(inc);
        if candidate.is_file() {
            return fs::canonicalize(&candidate).ok().or(Some(candidate));
        }
    }

    None
}

fn get_or_compute_header_hash(path: &Path) -> Option<String> {
    let metadata = fs::metadata(path).ok()?;
    let mtime = metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH);

    if let Ok(cache) = get_header_cache().lock() {
        if let Some((cached_mtime, cached_hash)) = cache.get(path) {
            if *cached_mtime == mtime {
                return Some(cached_hash.clone());
            }
        }
    }

    let content = fs::read_to_string(path).ok()?;
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    let hash = hex::encode(hasher.finalize());

    if let Ok(mut cache) = get_header_cache().lock() {
        cache.insert(path.to_path_buf(), (mtime, hash.clone()));
    }

    Some(hash)
}

fn compute_base_hash(
    content: &str,
    lang: Language,
    config: &Package,
    headers_hash: &str,
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"crupp-cache-v5");
    hasher.update(content.as_bytes());
    hasher.update(headers_hash.as_bytes());

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

pub fn scan_file(path: &Path, config: &Package) -> Option<TranslationUnit> {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    let language = match ext {
        "c" => Language::C,
        "cpp" | "cxx" | "cc" | "cppm" | "ixx" | "C" => Language::Cpp,
        _ => return None,
    };

    let raw_content = fs::read_to_string(path).ok()?;
    let content = strip_comments_fast(&raw_content);

    // Fast-path: only invoke regex if "main" is present in stripped content
    let has_main = if content.contains("main") {
        let re_main =
            RE_MAIN.get_or_init(|| Regex::new(r"(?m)\b(?:int|void|auto)\s+main\s*\(").unwrap());
        re_main.is_match(&content)
    } else {
        false
    };

    let mut exported_module = None;
    let mut imports = Vec::new();

    if language == Language::Cpp && (content.contains("module") || content.contains("import")) {
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

    let (direct_headers, headers_hash) =
        collect_header_dependencies(path, &content, &config.include_dirs);

    Some(TranslationUnit {
        path: path.to_path_buf(),
        language,
        exported_module,
        imports,
        base_hash: compute_base_hash(&raw_content, language, config, &headers_hash),
        has_main,
        direct_headers,
    })
}

pub fn discover_sources(source_dir: &str, config: &Package) -> Vec<TranslationUnit> {
    let root_path = Path::new(source_dir);
    let is_tests_dir = root_path.file_name().map_or(false, |n| n == "tests");
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
        .filter_entry(|e| {
            if e.depth() == 0 {
                return true;
            }
            let name = e.file_name();
            let name_str = name.to_string_lossy();
            if name_str.starts_with('.') {
                return false;
            }
            if name == out_dir_name.as_str() || name == "target" {
                return false;
            }
            if !is_tests_dir
                && (name == "tests"
                    || name == "fuzzing"
                    || name == "examples"
                    || name == "benchmark"
                    || name == "benchmarks")
            {
                return false;
            }
            true
        })
        .filter_map(|e| e.ok())
    {
        let path = entry.path();
        if !path.is_file() {
            continue;
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
        .max_depth(4)
        .into_iter()
        .filter_entry(|e| {
            if e.depth() == 0 {
                return true;
            }
            let name = e.file_name();
            let name_str = name.to_string_lossy();
            if name_str.starts_with('.') {
                return false;
            }
            if name == out_dir_name || name == "target" {
                return false;
            }
            true
        })
        .filter_map(|e| e.ok())
    {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }

        if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            if matches!(ext, "h" | "hpp" | "hxx" | "hh" | "inl") {
                if let Some(parent) = path.parent() {
                    let p_str = parent.to_string_lossy();
                    let clean = p_str.trim().trim_start_matches("./").trim();
                    if !clean.is_empty() && clean != "." {
                        dirs.insert(clean.to_string());
                    }
                }
            }
        }
    }

    let mut list: Vec<String> = dirs.into_iter().filter(|d| !d.trim().is_empty()).collect();
    list.sort();
    list
}
