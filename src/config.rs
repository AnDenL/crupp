use colored::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Language {
    C,
    Cpp,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BuildProfile {
    Default,
    Debug,
    Release,
    Sanitize,
    Custom(String),
}

impl BuildProfile {
    pub fn name(&self) -> &str {
        match self {
            BuildProfile::Default => "default",
            BuildProfile::Debug => "debug",
            BuildProfile::Release => "release",
            BuildProfile::Sanitize => "sanitize",
            BuildProfile::Custom(s) => s.as_str(),
        }
    }

    pub fn default_flags(&self) -> Vec<String> {
        match self {
            BuildProfile::Default => vec![],
            BuildProfile::Debug => vec!["-g".into(), "-O0".into(), "-DDEBUG=1".into()],
            BuildProfile::Release => vec!["-O3".into(), "-DNDEBUG=1".into()],
            BuildProfile::Sanitize => vec![
                "-fsanitize=address,undefined".into(),
                "-g".into(),
                "-O1".into(),
                "-fno-omit-frame-pointer".into(),
            ],
            BuildProfile::Custom(_) => vec![],
        }
    }

    pub fn _flags(&self) -> Vec<String> {
        self.default_flags()
    }
}

#[derive(Deserialize, Serialize, Debug, Clone, Default)]
pub struct ProfileConfig {
    #[serde(default)]
    pub flags: Vec<String>,
    #[serde(default)]
    pub c_flags: Vec<String>,
    #[serde(default)]
    pub cpp_flags: Vec<String>,
    #[serde(default)]
    pub compiler: Option<String>,
}

#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct Manifest {
    #[serde(default)]
    pub package: Package,
    #[serde(default)]
    pub bin: Vec<BinTarget>,
    #[serde(default)]
    pub profile: HashMap<String, ProfileConfig>,
}

impl Manifest {
    pub fn resolve_profile_flags(&self, profile: &BuildProfile) -> Vec<String> {
        let name = profile.name();
        if let Some(cfg) = self.profile.get(name) {
            if !cfg.flags.is_empty() {
                return cfg.flags.clone();
            }
        }
        profile.default_flags()
    }
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

pub fn default_cpp_standard() -> String {
    "-std=c++23".to_string()
}

pub fn default_c_standard() -> String {
    "-std=c23".to_string()
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

pub fn is_command_available(cmd: &str) -> bool {
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
            let note = "⚠️  'clang' was not found in PATH; automatically using 'gcc' as fallback."
                .yellow()
                .to_string();
            return ("gcc".to_string(), Some(note));
        } else if requested == "gcc" && is_command_available("clang") {
            let note = "⚠️  'gcc' was not found in PATH; automatically using 'clang' as fallback."
                .yellow()
                .to_string();
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
            let note =
                "⚠️  'clang++' was not found in PATH; automatically using 'g++' as fallback."
                    .yellow()
                    .to_string();
            return ("g++".to_string(), Some(note));
        } else if requested == "g++" && is_command_available("clang++") {
            let note =
                "⚠️  'g++' was not found in PATH; automatically using 'clang++' as fallback."
                    .yellow()
                    .to_string();
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
                profile: HashMap::new(),
            },
            true,
        ));
    };

    let content = fs::read_to_string(&actual_path).map_err(|e| {
        format!(
            "Could not read configuration file '{}': {}",
            actual_path.display(),
            e
        )
    })?;

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

// ============================================================================
// Manifest Mutation & Host Library Inspection APIs
// ============================================================================

#[derive(Debug, Clone)]
pub enum LibCheckStatus {
    Installed(String),
    Missing {
        apt: String,
        pacman: String,
        dnf: String,
    },
}

pub fn check_system_library(lib_name: &str) -> LibCheckStatus {
    let clean = lib_name.trim().trim_start_matches("-l").trim();

    // Standard C library math / pthread / dl are built into standard libc on Unix
    if matches!(clean, "m" | "pthread" | "dl" | "rt") {
        return LibCheckStatus::Installed(format!("standard C library (-l{})", clean));
    }

    // 1. Try pkg-config if installed
    let pkg_candidates = [
        clean.to_string(),
        format!("lib{}", clean),
        format!("{}3", clean),
        format!("{}2", clean),
    ];
    for pkg in &pkg_candidates {
        if let Ok(output) = Command::new("pkg-config").arg("--exists").arg(pkg).output() {
            if output.status.success() {
                let ver = Command::new("pkg-config")
                    .arg("--modversion")
                    .arg(pkg)
                    .output()
                    .ok()
                    .and_then(|o| String::from_utf8(o.stdout).ok())
                    .map(|s| s.trim().to_string())
                    .unwrap_or_default();
                let detail = if ver.is_empty() {
                    format!("found via pkg-config ({})", pkg)
                } else {
                    format!("found via pkg-config ({} v{})", pkg, ver)
                };
                return LibCheckStatus::Installed(detail);
            }
        }
    }

    // 2. Direct compiler linker probe (works even without pkg-config installed!)
    let (compiler, _) = CompilerType::default().resolve_c_compiler();
    let probe_res = Command::new(&compiler)
        .arg("-x")
        .arg("c")
        .arg("-")
        .arg("-o")
        .arg("/dev/null")
        .arg(format!("-l{}", clean))
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .and_then(|mut child| {
            if let Some(mut stdin) = child.stdin.take() {
                use std::io::Write;
                let _ = stdin.write_all(b"int main(void){return 0;}\n");
            }
            child.wait()
        });

    if let Ok(status) = probe_res {
        if status.success() {
            return LibCheckStatus::Installed(format!("resolved by system linker (-l{})", clean));
        }
    }

    // 3. Fallback filesystem search across standard library locations
    let lib_patterns = [
        format!("lib{}.so", clean),
        format!("lib{}.a", clean),
        format!("lib{}.dylib", clean),
    ];
    let search_roots = [
        "/usr/lib",
        "/usr/lib64",
        "/usr/local/lib",
        "/lib",
        "/usr/lib/x86_64-linux-gnu",
        "/usr/lib/aarch64-linux-gnu",
        "/opt/homebrew/lib",
    ];

    for root in &search_roots {
        let p = Path::new(root);
        if p.is_dir() {
            for pattern in &lib_patterns {
                if p.join(pattern).exists() {
                    return LibCheckStatus::Installed(format!("found at {}/{}", root, pattern));
                }
            }
        }
    }

    // Not found — generate helpful package manager commands
    let apt_pkg = if clean.starts_with("lib") {
        format!("{}-dev", clean)
    } else {
        format!("lib{}-dev", clean)
    };
    let dnf_pkg = format!("{}-devel", clean);
    let pacman_pkg = clean.to_string();

    LibCheckStatus::Missing {
        apt: format!("sudo apt install {}", apt_pkg),
        pacman: format!("sudo pacman -S {}", pacman_pkg),
        dnf: format!("sudo dnf install {}", dnf_pkg),
    }
}

pub fn add_bin_to_manifest(
    manifest_path_str: &str,
    name: &str,
    path: &str,
) -> Result<String, String> {
    let manifest_path =
        locate_manifest(manifest_path_str).unwrap_or_else(|| PathBuf::from(manifest_path_str));

    let content = if manifest_path.exists() {
        fs::read_to_string(&manifest_path).map_err(|e| e.to_string())?
    } else {
        default_with_name("app")
    };

    if let Ok(manifest) = toml::from_str::<Manifest>(&content) {
        if manifest.bin.iter().any(|b| b.name == name) {
            return Err(format!(
                "Binary target '{}' is already defined in '{}'.",
                name,
                manifest_path.display()
            ));
        }
    }

    let mut new_content = content.trim_end().to_string();
    new_content.push_str(&format!(
        "\n\n[[bin]]\nname = \"{}\"\npath = \"{}\"\n",
        name, path
    ));

    fs::write(&manifest_path, new_content).map_err(|e| e.to_string())?;

    let file_check = if !Path::new(path).exists() {
        format!(" (note: source file '{}' does not exist yet)", path)
    } else {
        String::new()
    };

    Ok(format!(
        "Added binary target '{}' ({}) to '{}'{}",
        name,
        path,
        manifest_path.display(),
        file_check
    ))
}

pub fn add_library_to_manifest(
    manifest_path_str: &str,
    lib_name: &str,
    no_check: bool,
) -> Result<String, String> {
    let clean_lib = lib_name.trim().trim_start_matches("-l").trim();
    if clean_lib.is_empty() {
        return Err("Library name cannot be empty.".to_string());
    }

    let manifest_path =
        locate_manifest(manifest_path_str).unwrap_or_else(|| PathBuf::from(manifest_path_str));

    let content = if manifest_path.exists() {
        fs::read_to_string(&manifest_path).map_err(|e| e.to_string())?
    } else {
        default_with_name("app")
    };

    let mut manifest: Manifest = toml::from_str(&content).map_err(|e| {
        format!(
            "Error parsing manifest '{}': {}",
            manifest_path.display(),
            e
        )
    })?;

    if manifest
        .package
        .libs
        .iter()
        .any(|l| l.trim_start_matches("-l") == clean_lib)
    {
        return Ok(format!(
            "Library '{}' is already present in 'libs' in '{}'.",
            clean_lib,
            manifest_path.display()
        ));
    }

    let mut check_note = String::new();
    if !no_check {
        match check_system_library(clean_lib) {
            LibCheckStatus::Installed(detail) => {
                check_note = format!("  {} Host check: {}", "✓".green().bold(), detail.dimmed());
            }
            LibCheckStatus::Missing { apt, pacman, dnf } => {
                check_note = format!(
                    "  {} {}\n      {}\n        • Ubuntu/Debian: {}\n        • Arch Linux:    {}\n        • Fedora/RHEL:   {}",
                    "⚠️ ".yellow(),
                    format!("Library '{}' was NOT detected on this system!", clean_lib)
                        .yellow()
                        .bold(),
                    "Install command:".dimmed(),
                    apt.cyan(),
                    pacman.cyan(),
                    dnf.cyan()
                );
            }
        }
    }

    manifest.package.libs.push(clean_lib.to_string());

    let updated_toml = update_libs_in_toml(&content, &manifest.package.libs);
    fs::write(&manifest_path, updated_toml).map_err(|e| e.to_string())?;

    let summary = format!(
        "Added library '{}' to '{}'.",
        clean_lib.bold(),
        manifest_path.display()
    );

    if check_note.is_empty() {
        Ok(summary)
    } else {
        Ok(format!("{}\n{}", summary, check_note))
    }
}

fn update_libs_in_toml(original: &str, libs: &[String]) -> String {
    let libs_formatted = format!(
        "libs = [{}]",
        libs.iter()
            .map(|l| format!("\"{}\"", l))
            .collect::<Vec<_>>()
            .join(", ")
    );

    let re_libs = regex::Regex::new(r#"(?m)^\s*libs\s*=\s*\[[^\]]*\]"#).unwrap();
    if re_libs.is_match(original) {
        return re_libs
            .replace(original, libs_formatted.as_str())
            .to_string();
    }

    let re_pkg = regex::Regex::new(r#"(?m)^\[package\]\s*$"#).unwrap();
    if let Some(m) = re_pkg.find(original) {
        let insert_pos = m.end();
        let mut out = String::new();
        out.push_str(&original[..insert_pos]);
        out.push('\n');
        out.push_str(&libs_formatted);
        out.push_str(&original[insert_pos..]);
        return out;
    }

    format!("{}\n{}\n", original.trim_end(), libs_formatted)
}

pub fn check_manifest_libraries(manifest_path_str: &str) -> Result<(), String> {
    let (manifest, is_auto) = load_manifest_or_autodetect(manifest_path_str)?;
    if is_auto {
        return Err("No Crub.toml manifest found to check libraries for.".to_string());
    }

    let libs = &manifest.package.libs;
    if libs.is_empty() {
        println!(
            "{}",
            "ℹ️  No external libraries listed in 'libs = [...]' in Crub.toml.".yellow()
        );
        return Ok(());
    }

    println!(
        "{}",
        format!(
            "🔍 Checking {} system librar(ies) configured in Crub.toml...",
            libs.len()
        )
        .bold()
    );

    let mut installed_count = 0;
    let mut missing_count = 0;

    for lib in libs {
        let clean = lib.trim().trim_start_matches("-l").trim();
        match check_system_library(clean) {
            LibCheckStatus::Installed(detail) => {
                println!(
                    "  {} {:<16} {}",
                    "✓".green().bold(),
                    clean.bold(),
                    detail.dimmed()
                );
                installed_count += 1;
            }
            LibCheckStatus::Missing { apt, pacman, dnf } => {
                println!(
                    "  {} {:<16} {}",
                    "✗".red().bold(),
                    clean.bold(),
                    "NOT FOUND".red().bold()
                );
                println!("      Suggested install command:");
                println!("        • Ubuntu/Debian: {}", apt.cyan());
                println!("        • Arch Linux:    {}", pacman.cyan());
                println!("        • Fedora/RHEL:   {}", dnf.cyan());
                missing_count += 1;
            }
        }
    }

    println!();
    if missing_count == 0 {
        println!(
            "{}",
            format!(
                "All {} librar(ies) are installed and ready to build!",
                installed_count
            )
            .green()
            .bold()
        );
    } else {
        println!(
            "{}",
            format!(
                "Dependency status: {} installed, {} missing. Install missing packages before building.",
                installed_count, missing_count
            )
            .yellow()
            .bold()
        );
    }

    Ok(())
}
