use colored::*;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Language {
    C,
    Cpp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuildProfile {
    Default,
    Debug,
    Release,
}

impl BuildProfile {
    pub fn flags(&self) -> Vec<String> {
        match self {
            BuildProfile::Default => vec![],
            BuildProfile::Debug => vec!["-g".into(), "-O0".into(), "-DDEBUG=1".into()],
            BuildProfile::Release => vec!["-O3".into(), "-DNDEBUG=1".into()],
        }
    }
}

#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct Manifest {
    #[serde(default)]
    pub package: Package,
    #[serde(default)]
    pub bin: Vec<BinTarget>,
}

#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct Package {
    #[serde(default = "default_name")]
    pub name: String,
    #[serde(default)]
    pub compiler: CompilerType,
    #[serde(default = "default_cpp_standard")]
    pub standard: String,
    #[serde(default = "default_c_standard")]
    pub c_standard: String,
    #[serde(default = "default_source_dir")]
    pub source_dir: String,
    #[serde(default = "default_out_dir")]
    pub out_dir: String,
    #[serde(default)]
    pub flags: Vec<String>,
    #[serde(default)]
    pub c_flags: Vec<String>,
    #[serde(default)]
    pub cpp_flags: Vec<String>,
    #[serde(default)]
    pub include_dirs: Vec<String>,
    #[serde(default)]
    pub lib_dirs: Vec<String>,
    #[serde(default)]
    pub libs: Vec<String>,
}

fn default_name() -> String {
    std::env::current_dir()
        .ok()
        .and_then(|p| p.file_name().map(|s| s.to_string_lossy().to_string()))
        .unwrap_or_else(|| "app".to_string())
}

fn default_cpp_standard() -> String {
    "-std=c++23".to_string()
}

fn default_c_standard() -> String {
    "-std=c17".to_string()
}

fn default_source_dir() -> String {
    if Path::new("src").is_dir() {
        "src".to_string()
    } else {
        ".".to_string()
    }
}

fn default_out_dir() -> String {
    "build".to_string()
}

impl Default for Package {
    fn default() -> Self {
        Self {
            name: default_name(),
            compiler: CompilerType::default(),
            standard: default_cpp_standard(),
            c_standard: default_c_standard(),
            source_dir: default_source_dir(),
            out_dir: default_out_dir(),
            flags: vec![],
            c_flags: vec![],
            cpp_flags: vec![],
            include_dirs: vec![],
            lib_dirs: vec![],
            libs: vec![],
        }
    }
}

impl Package {
    pub fn normalize_standards(&mut self) {
        if !self.standard.is_empty() && !self.standard.starts_with("-std=") {
            self.standard = format!("-std={}", self.standard);
        }
        if !self.c_standard.is_empty() && !self.c_standard.starts_with("-std=") {
            self.c_standard = format!("-std={}", self.c_standard);
        }

        let s_lower = self.standard.to_lowercase();
        let is_cpp = s_lower.contains("c++") || s_lower.contains("gnu++");
        let is_c = s_lower.contains("c89")
            || s_lower.contains("c90")
            || s_lower.contains("c99")
            || s_lower.contains("c11")
            || s_lower.contains("c17")
            || s_lower.contains("c23")
            || s_lower.contains("c2x");

        if is_c && !is_cpp {
            self.c_standard = self.standard.clone();
        }
    }
}

#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct BinTarget {
    pub name: String,
    pub path: String,
}

#[derive(Deserialize, Serialize, Debug, Clone)]
#[serde(rename_all = "lowercase")]
pub enum CompilerType {
    Clang,
    Gcc,
    #[serde(untagged)]
    Custom(String),
}

impl Default for CompilerType {
    fn default() -> Self {
        if is_command_available("clang") {
            CompilerType::Clang
        } else if is_command_available("gcc") {
            CompilerType::Gcc
        } else {
            CompilerType::Clang
        }
    }
}

fn is_command_available(cmd: &str) -> bool {
    Command::new(cmd)
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

impl CompilerType {
    pub fn c_compiler(&self) -> String {
        match self {
            CompilerType::Clang => "clang".to_string(),
            CompilerType::Gcc => "gcc".to_string(),
            CompilerType::Custom(cmd) => match cmd.as_str() {
                "g++" => "gcc".to_string(),
                "clang++" => "clang".to_string(),
                _ => cmd.clone(),
            },
        }
    }

    pub fn cpp_compiler(&self) -> String {
        match self {
            CompilerType::Clang => "clang++".to_string(),
            CompilerType::Gcc => "g++".to_string(),
            CompilerType::Custom(cmd) => match cmd.as_str() {
                "gcc" | "cc" => "g++".to_string(),
                "clang" => "clang++".to_string(),
                _ => cmd.clone(),
            },
        }
    }

    /// Resolves the actual C compiler binary to run, with intelligent fallback
    /// if the configured compiler is missing from PATH.
    pub fn resolve_c_compiler(&self) -> (String, Option<String>) {
        let requested = self.c_compiler();
        if is_command_available(&requested) {
            return (requested, None);
        }

        // Fallback resolution
        if requested == "clang" && is_command_available("gcc") {
            let note = "⚠️  'clang' was not found in PATH; automatically using 'gcc' as fallback.".yellow().to_string();
            return ("gcc".to_string(), Some(note));
        } else if requested == "gcc" && is_command_available("clang") {
            let note = "⚠️  'gcc' was not found in PATH; automatically using 'clang' as fallback.".yellow().to_string();
            return ("clang".to_string(), Some(note));
        }

        (requested, None)
    }

    /// Resolves the actual C++ compiler binary to run, with intelligent fallback
    /// if the configured compiler is missing from PATH.
    pub fn resolve_cpp_compiler(&self) -> (String, Option<String>) {
        let requested = self.cpp_compiler();
        if is_command_available(&requested) {
            return (requested, None);
        }

        // Fallback resolution
        if requested == "clang++" && is_command_available("g++") {
            let note = "⚠️  'clang++' was not found in PATH; automatically using 'g++' as fallback.".yellow().to_string();
            return ("g++".to_string(), Some(note));
        } else if requested == "g++" && is_command_available("clang++") {
            let note = "⚠️  'g++' was not found in PATH; automatically using 'clang++' as fallback.".yellow().to_string();
            return ("clang++".to_string(), Some(note));
        }

        (requested, None)
    }

    pub fn get_module_flags(
        &self,
        obj_dir: &Path,
        is_export: bool,
        mod_name: &Option<String>,
    ) -> Vec<String> {
        let is_clang = match self {
            CompilerType::Clang => true,
            CompilerType::Custom(cmd) => cmd.contains("clang"),
            _ => false,
        };

        if is_clang {
            let mut args = vec![format!(
                "-fprebuilt-module-path={}",
                obj_dir.to_string_lossy()
            )];
            if is_export {
                if let Some(name) = mod_name {
                    let pcm_path = obj_dir.join(format!("{}.pcm", name));
                    args.push(format!("-fmodule-output={}", pcm_path.to_string_lossy()));
                }
            }
            args
        } else {
            // GCC C++20 modules support
            let is_gcc = match self {
                CompilerType::Gcc => true,
                CompilerType::Custom(cmd) => cmd.contains("gcc") || cmd.contains("g++"),
                _ => false,
            };
            if is_gcc {
                vec!["-fmodules-ts".to_string()]
            } else {
                vec![]
            }
        }
    }
}

/// Finds the manifest file, checking common variants (Crub.toml, crupp.toml, Crupp.toml)
pub fn locate_manifest<P: AsRef<Path>>(requested: P) -> Option<PathBuf> {
    let req = requested.as_ref();
    if req.exists() {
        return Some(req.to_path_buf());
    }

    // Common fallback names
    let candidates = ["Crub.toml", "crupp.toml", "Crupp.toml", "crub.toml"];
    for name in &candidates {
        let p = Path::new(name);
        if p.exists() {
            return Some(p.to_path_buf());
        }
    }

    None
}

pub fn load_manifest_or_autodetect<P: AsRef<Path>>(path: P) -> Result<(Manifest, bool), String> {
    let manifest_path = locate_manifest(path);

    let Some(actual_path) = manifest_path else {
        // Auto-detect mode
        let mut pkg = Package::default();
        pkg.normalize_standards();
        return Ok((
            Manifest {
                package: pkg,
                bin: vec![],
            },
            true,
        ));
    };

    let content = fs::read_to_string(&actual_path)
        .map_err(|e| format!("Could not read configuration file '{}': {}", actual_path.display(), e))?;

    let mut manifest: Manifest = toml::from_str(&content).map_err(|e| {
        format!(
            "Syntax error in '{}':\n  {}\n  💡 Tip: Check for missing commas, unclosed quotes, or brackets in your TOML config.",
            actual_path.display(),
            e
        )
    })?;

    manifest.package.normalize_standards();

    Ok((manifest, false))
}

pub fn default_with_name(name: &str) -> String {
    default_with_name_and_lang(name, None)
}

pub fn default_with_name_and_lang(name: &str, lang_pref: Option<&str>) -> String {
    let preferred_compiler = if is_command_available("clang") {
        "clang"
    } else {
        "gcc"
    };

    // Auto-detect if user has C or C++ sources
    let is_c = if let Some(l) = lang_pref {
        l == "c"
    } else {
        Path::new("src/main.c").exists() || Path::new("main.c").exists()
    };

    let (standard_c, standard_cpp, main_path) = if is_c {
        ("-std=c17", "-std=c++23", "src/main.c")
    } else {
        ("-std=c17", "-std=c++23", "src/main.cpp")
    };

    format!(
        r#"[package]
name = "{name}"
compiler = "{preferred_compiler}"
standard = "{standard_cpp}"
c_standard = "{standard_c}"
source_dir = "./src"
out_dir = "./build"
flags = ["-Wall", "-Wextra"]

[[bin]]
name = "{name}"
path = "{main_path}"
"#
    )
}
