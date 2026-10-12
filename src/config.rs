use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Language {
    C,
    Cpp,
}

#[derive(Deserialize, Debug, Clone)]
pub struct Manifest {
    #[serde(default)]
    pub package: Package,
    #[serde(default)]
    pub bin: Vec<BinTarget>,
}

#[derive(Deserialize, Debug, Clone)]
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

#[derive(Deserialize, Debug, Clone)]
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
        if std::process::Command::new("clang")
            .arg("--version")
            .output()
            .is_ok()
        {
            CompilerType::Clang
        } else {
            CompilerType::Gcc
        }
    }
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
            vec![]
        }
    }
}

pub fn load_manifest_or_autodetect<P: AsRef<Path>>(path: P) -> Result<(Manifest, bool), String> {
    let path_ref = path.as_ref();
    if !path_ref.exists() {
        return Ok((
            Manifest {
                package: Package::default(),
                bin: vec![],
            },
            true,
        ));
    }

    let content = fs::read_to_string(path_ref)
        .map_err(|e| format!("Could not read configuration file: {}", e))?;

    let mut manifest: Manifest = toml::from_str(&content).map_err(|e| {
            format!(
                "Syntax error in Crub.toml:\n  {}\n  💡 Tip: Check for missing commas, unclosed quotes, or brackets.",
                e
            )
        })?;

    manifest.package.normalize_standards();

    Ok((manifest, false))
}

pub fn default_with_name(name: &str) -> String {
    format!(
        r#"[package]
name = "{name}"
compiler = "clang"
standard = "-std=c++23"
c_standard = "-std=c17"
source_dir = "./src"
out_dir = "./build"
flags = ["-Wall", "-Wextra"]

[[bin]]
name = "{name}"
path = "src/main.c"
"#
    )
}
