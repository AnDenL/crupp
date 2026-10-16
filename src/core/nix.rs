use crate::config::{Language, Manifest};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Mutex, Once, OnceLock};

static COMPILER_INCLUDES_CACHE: OnceLock<Mutex<HashMap<String, Vec<PathBuf>>>> = OnceLock::new();
static SYSTEM_LIB_DIRS_CACHE: OnceLock<Mutex<Option<Vec<PathBuf>>>> = OnceLock::new();
static NIX_SHELL_ENV_ONCE: Once = Once::new();

fn get_includes_cache() -> &'static Mutex<HashMap<String, Vec<PathBuf>>> {
    COMPILER_INCLUDES_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn get_lib_dirs_cache() -> &'static Mutex<Option<Vec<PathBuf>>> {
    SYSTEM_LIB_DIRS_CACHE.get_or_init(|| Mutex::new(None))
}

#[inline]
fn push_unique_path(dirs: &mut Vec<PathBuf>, seen: &mut HashSet<PathBuf>, p: PathBuf) {
    if p.is_dir() && seen.insert(p.clone()) {
        dirs.push(p);
    }
}

/// Detects whether we are running on NixOS or inside a Nix environment (`nix-shell` / `nix develop`).
pub fn is_nix_env() -> bool {
    std::env::var_os("IN_NIX_SHELL").is_some()
        || std::env::var_os("NIX_CFLAGS_COMPILE").is_some()
        || std::env::var_os("NIX_LDFLAGS").is_some()
        || std::env::var_os("NIX_CC").is_some()
        || Path::new("/etc/NIXOS").exists()
        || Path::new("/run/current-system/nixos-version").exists()
        || Path::new("/nix/store").is_dir()
}

/// Resolves an executable name (e.g. `"gcc"`, `"clang"`) to its full path in `PATH` if possible.
pub fn resolve_executable_path(bin: &str) -> String {
    let p = Path::new(bin);
    if p.is_absolute() || bin.contains('/') {
        if p.exists() {
            return bin.to_string();
        }
    }
    if let Some(path_var) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path_var) {
            let cand = dir.join(bin);
            if cand.is_file() {
                return cand.to_string_lossy().to_string();
            }
        }
    }
    bin.to_string()
}

/// If running on NixOS outside `nix-shell` (`IN_NIX_SHELL` is unset), and the project has a `shell.nix`
/// in the current directory, automatically imports `NIX_CFLAGS_COMPILE`, `NIX_LDFLAGS`, `PKG_CONFIG_PATH`,
/// `LIBRARY_PATH`, `C_INCLUDE_PATH`, `CPLUS_INCLUDE_PATH`, and `PATH` from `nix-shell` if `NIX_CFLAGS_COMPILE` is not yet set.
pub fn activate_nix_env_if_available() {
    NIX_SHELL_ENV_ONCE.call_once(|| {
        if std::env::var_os("IN_NIX_SHELL").is_some()
            || std::env::var_os("NIX_CFLAGS_COMPILE").is_some()
            || std::env::var_os("CRUPP_NO_AUTO_NIX_SHELL").is_some()
        {
            return;
        }

        if !Path::new("shell.nix").is_file() || !is_nix_env() {
            return;
        }

        let has_nix_shell = Command::new("nix-shell")
            .arg("--version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false);

        if !has_nix_shell {
            return;
        }

        if let Ok(output) = Command::new("nix-shell")
            .arg("--run")
            .arg("env -0")
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
        {
            if output.status.success() {
                let raw = String::from_utf8_lossy(&output.stdout);
                for entry in raw.split('\0') {
                    if let Some((k, v)) = entry.split_once('=') {
                        if matches!(
                            k,
                            "NIX_CFLAGS_COMPILE"
                                | "NIX_CFLAGS_COMPILE_BEFORE"
                                | "NIX_LDFLAGS"
                                | "NIX_LDFLAGS_BEFORE"
                                | "NIX_CC"
                                | "PKG_CONFIG_PATH"
                                | "PKG_CONFIG_PATH_FOR_TARGET"
                                | "C_INCLUDE_PATH"
                                | "CPLUS_INCLUDE_PATH"
                                | "CPATH"
                                | "LIBRARY_PATH"
                                | "LD_LIBRARY_PATH"
                                | "PATH"
                        ) {
                            unsafe {
                                std::env::set_var(k, v);
                            }
                        }
                    }
                }
            }
        }
    });
}

/// Reads a Nix cc-wrapper support file (e.g. `libc-cflags`, `cc-cflags`, `libcxx-cxxflags`, `libc-ldflags`)
/// if the compiler binary belongs to a Nix wrapper in `/nix/store/.../bin/<cc>`.
fn read_nix_wrapper_support_files(compiler_bin: &str, filenames: &[&str]) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut wrapper_roots = Vec::new();

    if let Ok(nix_cc) = std::env::var("NIX_CC") {
        let p = PathBuf::from(nix_cc.trim());
        if p.is_dir() {
            wrapper_roots.push(p);
        }
    }

    let resolved = resolve_executable_path(compiler_bin);
    let bin_path = Path::new(&resolved);
    if let Some(parent) = bin_path.parent().and_then(|p| p.parent()) {
        if parent.join("nix-support").is_dir() && !wrapper_roots.contains(&parent.to_path_buf()) {
            wrapper_roots.push(parent.to_path_buf());
        }
    }
    if let Ok(canon) = fs::canonicalize(bin_path) {
        if let Some(parent) = canon.parent().and_then(|p| p.parent()) {
            if parent.join("nix-support").is_dir()
                && !wrapper_roots.contains(&parent.to_path_buf())
            {
                wrapper_roots.push(parent.to_path_buf());
            }
        }
    }

    for root in wrapper_roots {
        let support_dir = root.join("nix-support");
        for fname in filenames {
            let fpath = support_dir.join(fname);
            if let Ok(content) = fs::read_to_string(&fpath) {
                for part in content.split_whitespace() {
                    tokens.push(part.to_string());
                }
            }
        }
    }

    tokens
}

/// Extracts include directory paths from a list of compiler flags (`-I<dir>`, `-I <dir>`, `-isystem <dir>`, `-idirafter <dir>`).
pub fn extract_include_dirs_from_flags(flags: &[String]) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    let mut seen = HashSet::new();
    let mut i = 0;

    while i < flags.len() {
        let tok = flags[i].trim();
        if matches!(tok, "-I" | "-isystem" | "-idirafter" | "-iquote") {
            if i + 1 < flags.len() {
                let p = PathBuf::from(flags[i + 1].trim());
                if p.is_dir() && seen.insert(p.clone()) {
                    dirs.push(p);
                }
                i += 2;
                continue;
            }
        } else if let Some(rest) = tok
            .strip_prefix("-isystem")
            .or_else(|| tok.strip_prefix("-idirafter"))
            .or_else(|| tok.strip_prefix("-I"))
        {
            let clean = rest.trim();
            if !clean.is_empty() {
                let p = PathBuf::from(clean);
                if p.is_dir() && seen.insert(p.clone()) {
                    dirs.push(p);
                }
            }
        }
        i += 1;
    }

    dirs
}

/// Extracts library directory paths (`-L<dir>`, `-L <dir>`, `-rpath <dir>`) from linker flags.
pub fn extract_lib_dirs_from_flags(flags: &[String]) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    let mut seen = HashSet::new();
    let mut i = 0;

    while i < flags.len() {
        let tok = flags[i].trim();
        if tok == "-L" || tok == "-rpath" {
            if i + 1 < flags.len() {
                let p = PathBuf::from(flags[i + 1].trim());
                if p.is_dir() && seen.insert(p.clone()) {
                    dirs.push(p);
                }
                i += 2;
                continue;
            }
        } else if let Some(rest) = tok.strip_prefix("-L") {
            let clean = rest.trim();
            if !clean.is_empty() {
                let p = PathBuf::from(clean);
                if p.is_dir() && seen.insert(p.clone()) {
                    dirs.push(p);
                }
            }
        } else if let Some(rest) = tok.strip_prefix("-Wl,-rpath,") {
            let clean = rest.trim();
            if !clean.is_empty() {
                let p = PathBuf::from(clean);
                if p.is_dir() && seen.insert(p.clone()) {
                    dirs.push(p);
                }
            }
        }
        i += 1;
    }

    dirs
}

/// Queries the compiler (`gcc`, `g++`, `clang`, `clang++`) via `-E -Wp,-v -x c/c++ /dev/null`
/// and combines with `NIX_CFLAGS_COMPILE` / `nix-support` to obtain the complete ordered list of system include paths.
pub fn query_compiler_system_includes(compiler_bin: &str, lang: Language) -> Vec<PathBuf> {
    activate_nix_env_if_available();

    let lang_key = match lang {
        Language::C => "c",
        Language::Cpp => "c++",
    };
    let env_fingerprint = std::env::var("NIX_CFLAGS_COMPILE").unwrap_or_default();
    let cache_key = format!("{}::{}::{}", compiler_bin, lang_key, env_fingerprint);

    if let Ok(cache) = get_includes_cache().lock() {
        if let Some(cached) = cache.get(&cache_key) {
            return cached.clone();
        }
    }

    let mut result = Vec::new();
    let mut seen = HashSet::new();

    // 1. Collect from NIX_CFLAGS_COMPILE_BEFORE & NIX_CFLAGS_COMPILE
    for var in &["NIX_CFLAGS_COMPILE_BEFORE", "NIX_CFLAGS_COMPILE"] {
        if let Ok(val) = std::env::var(var) {
            let toks: Vec<String> = val.split_whitespace().map(|s| s.to_string()).collect();
            for d in extract_include_dirs_from_flags(&toks) {
                push_unique_path(&mut result, &mut seen, d);
            }
        }
    }

    // 2. Collect from standard CPATH / C_INCLUDE_PATH / CPLUS_INCLUDE_PATH
    let env_vars: &[&str] = match lang {
        Language::C => &["CPATH", "C_INCLUDE_PATH", "INCLUDE"],
        Language::Cpp => &["CPATH", "CPLUS_INCLUDE_PATH", "C_INCLUDE_PATH", "INCLUDE"],
    };
    for var in env_vars {
        if let Ok(val) = std::env::var(var) {
            for p in std::env::split_paths(&val) {
                if !p.as_os_str().is_empty() {
                    push_unique_path(&mut result, &mut seen, p);
                }
            }
        }
    }

    // 3. Collect from Nix cc-wrapper nix-support files
    let support_files: &[&str] = match lang {
        Language::C => &["libc-cflags", "cc-cflags"],
        Language::Cpp => &["libcxx-cxxflags", "libc-cflags", "cc-cflags"],
    };
    let wrapper_toks = read_nix_wrapper_support_files(compiler_bin, support_files);
    for d in extract_include_dirs_from_flags(&wrapper_toks) {
        push_unique_path(&mut result, &mut seen, d);
    }

    // 4. Query compiler preprocessor search list directly (`<compiler> -E -Wp,-v -x <lang> /dev/null`)
    if let Ok(out) = Command::new(compiler_bin)
        .args(["-E", "-Wp,-v", "-x", lang_key, "/dev/null"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
    {
        let stderr = String::from_utf8_lossy(&out.stderr);
        let mut in_search_list = false;
        for line in stderr.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("#include \"...\" search starts here:")
                || trimmed.starts_with("#include <...> search starts here:")
            {
                in_search_list = true;
                continue;
            }
            if trimmed.starts_with("End of search list.") {
                in_search_list = false;
                break;
            }
            if in_search_list && line.starts_with(' ') {
                let clean = trimmed
                    .trim_end_matches("(framework directory)")
                    .trim();
                if !clean.is_empty() {
                    let p = PathBuf::from(clean);
                    let canon = fs::canonicalize(&p).unwrap_or(p);
                    push_unique_path(&mut result, &mut seen, canon);
                }
            }
        }
    }

    if let Ok(mut cache) = get_includes_cache().lock() {
        cache.insert(cache_key, result.clone());
    }

    result
}

/// Discovers C system include paths specifically ordered and filtered for TinyCC (`comptime` execution).
/// Places standard libc (`glibc-dev/include`) and Nix package headers BEFORE any compiler-internal
/// intrinsic directories (`/lib/gcc/.../include`, `/lib/clang/.../include`) so TCC's own `stddef.h` and `stdarg.h`
/// are never shadowed by GCC/Clang compiler builtins.
pub fn discover_tcc_system_includes(c_compiler: &str) -> Vec<PathBuf> {
    let raw_dirs = query_compiler_system_includes(c_compiler, Language::C);

    let mut primary_dirs = Vec::new();
    let mut intrinsic_dirs = Vec::new();
    let mut seen = HashSet::new();

    for d in raw_dirs {
        let s = d.to_string_lossy();
        let is_compiler_intrinsic = s.contains("/lib/gcc/")
            || s.contains("/lib64/gcc/")
            || s.contains("/lib/clang/")
            || s.contains("include-fixed");
        if is_compiler_intrinsic {
            push_unique_path(&mut intrinsic_dirs, &mut seen, d);
        } else {
            push_unique_path(&mut primary_dirs, &mut seen, d);
        }
    }

    // Standard FHS fallbacks if present
    for sys_path in &[
        "/usr/include",
        "/usr/local/include",
        "/usr/include/x86_64-linux-gnu",
        "/usr/include/aarch64-linux-gnu",
    ] {
        let p = PathBuf::from(sys_path);
        push_unique_path(&mut primary_dirs, &mut seen, p);
    }

    // If on NixOS and none of the primary directories contain <stdio.h>, scan /nix/store for glibc-*-dev/include
    let has_stdio = primary_dirs.iter().any(|d| d.join("stdio.h").is_file());
    if !has_stdio && Path::new("/nix/store").is_dir() {
        if let Ok(entries) = fs::read_dir("/nix/store") {
            let mut glibc_candidates = Vec::new();
            for entry in entries.filter_map(|e| e.ok()) {
                let fname = entry.file_name();
                let name = fname.to_string_lossy();
                if (name.contains("-glibc-") || name.contains("-musl-")) && name.ends_with("-dev") {
                    let inc = entry.path().join("include");
                    if inc.join("stdio.h").is_file() {
                        glibc_candidates.push(inc);
                    }
                }
            }
            glibc_candidates.sort();
            glibc_candidates.reverse();
            for inc in glibc_candidates {
                push_unique_path(&mut primary_dirs, &mut seen, inc);
            }
        }
    }

    primary_dirs.extend(intrinsic_dirs);
    primary_dirs
}

/// Discovers system and Nix store library directories (`libc.so.6`, `libm.so.6`, `NIX_LDFLAGS`, `LIBRARY_PATH`, etc.).
/// Written with zero closure captures to prevent any borrow-checker double-borrow conflicts.
pub fn discover_system_library_dirs(c_compiler: &str) -> Vec<PathBuf> {
    activate_nix_env_if_available();

    if let Ok(cache) = get_lib_dirs_cache().lock() {
        if let Some(cached) = cache.as_ref() {
            return cached.clone();
        }
    }

    let mut dirs = Vec::new();
    let mut seen = HashSet::new();

    // 1. NIX_LDFLAGS_BEFORE and NIX_LDFLAGS
    for var in &["NIX_LDFLAGS_BEFORE", "NIX_LDFLAGS"] {
        if let Ok(val) = std::env::var(var) {
            let toks: Vec<String> = val.split_whitespace().map(|s| s.to_string()).collect();
            for d in extract_lib_dirs_from_flags(&toks) {
                push_unique_path(&mut dirs, &mut seen, d);
            }
        }
    }

    // 2. LIBRARY_PATH and LD_LIBRARY_PATH
    for var in &["LIBRARY_PATH", "LD_LIBRARY_PATH"] {
        if let Ok(val) = std::env::var(var) {
            for p in std::env::split_paths(&val) {
                if !p.as_os_str().is_empty() {
                    push_unique_path(&mut dirs, &mut seen, p);
                }
            }
        }
    }

    // 3. Nix cc-wrapper nix-support/libc-ldflags
    let wrapper_ldflags =
        read_nix_wrapper_support_files(c_compiler, &["libc-ldflags", "cc-ldflags"]);
    for d in extract_lib_dirs_from_flags(&wrapper_ldflags) {
        push_unique_path(&mut dirs, &mut seen, d);
    }

    // 4. Query compiler for exact location of libc.so.6 and libm.so
    for lib_file in &["libc.so.6", "libm.so.6", "libc.so", "crt1.o"] {
        if let Ok(out) = Command::new(c_compiler)
            .arg(format!("-print-file-name={}", lib_file))
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
        {
            if out.status.success() {
                let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
                let p = Path::new(&s);
                if p.is_file() {
                    let canon = fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
                    if let Some(parent) = canon.parent() {
                        push_unique_path(&mut dirs, &mut seen, parent.to_path_buf());
                    }
                }
            }
        }
    }

    // 5. Common NixOS profile library paths & standard FHS library paths
    if let Ok(home) = std::env::var("HOME") {
        push_unique_path(&mut dirs, &mut seen, PathBuf::from(format!("{}/.nix-profile/lib", home)));
    }
    if let Ok(user) = std::env::var("USER") {
        push_unique_path(&mut dirs, &mut seen, PathBuf::from(format!("/etc/profiles/per-user/{}/lib", user)));
    }
    for root in &[
        "/run/current-system/sw/lib",
        "/usr/lib",
        "/usr/lib64",
        "/usr/local/lib",
        "/lib",
        "/lib64",
        "/usr/lib/x86_64-linux-gnu",
        "/usr/lib/aarch64-linux-gnu",
        "/opt/homebrew/lib",
    ] {
        push_unique_path(&mut dirs, &mut seen, PathBuf::from(root));
    }

    // 6. Last-resort fallback scan of /nix/store for glibc lib directory if on NixOS
    let has_libc = dirs
        .iter()
        .any(|d| d.join("libc.so.6").exists() || d.join("libc.so").exists());
    if !has_libc && Path::new("/nix/store").is_dir() {
        if let Ok(entries) = fs::read_dir("/nix/store") {
            let mut candidates = Vec::new();
            for entry in entries.filter_map(|e| e.ok()) {
                let fname = entry.file_name();
                let name = fname.to_string_lossy();
                if name.contains("-glibc-") && !name.ends_with("-dev") && !name.ends_with("-bin") {
                    let lib_dir = entry.path().join("lib");
                    if lib_dir.join("libc.so.6").exists() {
                        candidates.push(lib_dir);
                    }
                }
            }
            candidates.sort();
            candidates.reverse();
            for c in candidates {
                push_unique_path(&mut dirs, &mut seen, c);
            }
        }
    }

    if let Ok(mut cache) = get_lib_dirs_cache().lock() {
        *cache = Some(dirs.clone());
    }

    dirs
}

/// Queries `pkg-config --cflags` for all configured libraries in `libs`.
pub fn pkg_config_cflags(libs: &[String]) -> Vec<String> {
    activate_nix_env_if_available();
    let mut flags = Vec::new();
    let mut seen = HashSet::new();

    for lib in libs {
        let clean = lib.trim().trim_start_matches("-l").trim();
        if clean.is_empty() || matches!(clean, "m" | "pthread" | "dl" | "rt") {
            continue;
        }
        let candidates = [
            clean.to_string(),
            format!("lib{}", clean),
            format!("{}3", clean),
            format!("{}2", clean),
        ];
        for cand in &candidates {
            if let Ok(out) = Command::new("pkg-config")
                .args(["--cflags", cand])
                .stdin(Stdio::null())
                .stderr(Stdio::null())
                .output()
            {
                if out.status.success() {
                    let s = String::from_utf8_lossy(&out.stdout);
                    for part in s.split_whitespace() {
                        if seen.insert(part.to_string()) {
                            flags.push(part.to_string());
                        }
                    }
                    break;
                }
            }
        }
    }

    flags
}

/// Queries `pkg-config --libs` for all configured libraries in `libs`.
pub fn pkg_config_libs(libs: &[String]) -> Vec<String> {
    activate_nix_env_if_available();
    let mut flags = Vec::new();
    let mut seen = HashSet::new();

    for lib in libs {
        let clean = lib.trim().trim_start_matches("-l").trim();
        if clean.is_empty() || matches!(clean, "m" | "pthread" | "dl" | "rt") {
            continue;
        }
        let candidates = [
            clean.to_string(),
            format!("lib{}", clean),
            format!("{}3", clean),
            format!("{}2", clean),
        ];
        for cand in &candidates {
            if let Ok(out) = Command::new("pkg-config")
                .args(["--libs", cand])
                .stdin(Stdio::null())
                .stderr(Stdio::null())
                .output()
            {
                if out.status.success() {
                    let s = String::from_utf8_lossy(&out.stdout);
                    for part in s.split_whitespace() {
                        if seen.insert(part.to_string()) {
                            flags.push(part.to_string());
                        }
                    }
                    break;
                }
            }
        }
    }

    flags
}

/// Collects extra compiler flags for NixOS / `pkg-config` (`-I...`, `-isystem ...`, `-D...`).
pub fn extra_compiler_flags(libs: &[String]) -> Vec<String> {
    activate_nix_env_if_available();
    let mut args = Vec::new();
    let mut seen = HashSet::new();

    for flag in pkg_config_cflags(libs) {
        if seen.insert(flag.clone()) {
            args.push(flag);
        }
    }

    // Forward explicit -I, -isystem, and -D flags from NIX_CFLAGS_COMPILE so even unwrapped compilers succeed
    for var in &["NIX_CFLAGS_COMPILE_BEFORE", "NIX_CFLAGS_COMPILE"] {
        if let Ok(val) = std::env::var(var) {
            let toks: Vec<String> = val.split_whitespace().map(|s| s.to_string()).collect();
            for inc_dir in extract_include_dirs_from_flags(&toks) {
                let flag = format!("-isystem{}", inc_dir.to_string_lossy());
                if seen.insert(flag) {
                    args.push("-isystem".to_string());
                    args.push(inc_dir.to_string_lossy().to_string());
                }
            }
            let mut i = 0;
            while i < toks.len() {
                let t = toks[i].trim();
                if t == "-D" && i + 1 < toks.len() {
                    let def = format!("-D{}", toks[i + 1].trim());
                    if seen.insert(def.clone()) {
                        args.push(def);
                    }
                    i += 2;
                    continue;
                } else if t.starts_with("-D") && t.len() > 2 {
                    if seen.insert(t.to_string()) {
                        args.push(t.to_string());
                    }
                }
                i += 1;
            }
        }
    }

    args
}

/// Collects extra linker flags for NixOS / `pkg-config`, automatically adding `-Wl,-rpath,<dir>`
/// for any `/nix/store/.../lib` directory so built executables run without `LD_LIBRARY_PATH` issues.
pub fn extra_linker_flags(lib_dirs: &[String], libs: &[String]) -> Vec<String> {
    activate_nix_env_if_available();
    let mut args = Vec::new();
    let mut seen_l_dirs = HashSet::new();
    let mut seen_rpaths = HashSet::new();
    let mut seen_other = HashSet::new();

    let mut register_lib_dir = |dir_str: &str, out: &mut Vec<String>| {
        let clean = dir_str.trim().trim_start_matches("-L").trim();
        if clean.is_empty() {
            return;
        }
        if seen_l_dirs.insert(clean.to_string()) {
            out.push(format!("-L{}", clean));
        }
        if (clean.starts_with("/nix/store") || is_nix_env())
            && Path::new(clean).is_dir()
            && seen_rpaths.insert(clean.to_string())
        {
            out.push(format!("-Wl,-rpath,{}", clean));
        }
    };

    for d in lib_dirs {
        register_lib_dir(d, &mut args);
    }

    for flag in pkg_config_libs(libs) {
        if let Some(dir) = flag.strip_prefix("-L") {
            register_lib_dir(dir, &mut args);
        } else if seen_other.insert(flag.clone()) {
            args.push(flag);
        }
    }

    for var in &["NIX_LDFLAGS_BEFORE", "NIX_LDFLAGS"] {
        if let Ok(val) = std::env::var(var) {
            let toks: Vec<String> = val.split_whitespace().map(|s| s.to_string()).collect();
            for dir in extract_lib_dirs_from_flags(&toks) {
                register_lib_dir(&dir.to_string_lossy(), &mut args);
            }
        }
    }

    args
}

/// Maps a C/C++ library name from `Crub.toml` (`libs = [...]`) to its `nixpkgs` attribute name.
pub fn map_lib_to_nix_pkg(lib_name: &str) -> Option<&'static str> {
    let clean = lib_name.trim().trim_start_matches("-l").trim();
    match clean {
        "" | "m" | "pthread" | "dl" | "rt" | "c" | "stdc++" => None,
        "z" | "zlib" => Some("zlib"),
        "ssl" | "crypto" | "openssl" => Some("openssl"),
        "curl" | "libcurl" => Some("curl"),
        "sqlite3" | "sqlite" => Some("sqlite"),
        "cjson" | "libcjson" => Some("cjson"),
        "SDL2" | "sdl2" => Some("SDL2"),
        "SDL3" | "sdl3" => Some("sdl3"),
        "glfw" | "glfw3" => Some("glfw"),
        "raylib" => Some("raylib"),
        "GL" | "gl" | "opengl" => Some("libGL"),
        "GLEW" | "glew" => Some("glew"),
        "vulkan" => Some("vulkan-loader"),
        "X11" | "x11" => Some("xorg.libX11"),
        "wayland-client" | "wayland" => Some("wayland"),
        "uv" | "libuv" => Some("libuv"),
        "yaml" | "libyaml" => Some("libyaml"),
        "xml2" | "libxml2" => Some("libxml2"),
        "archive" | "libarchive" => Some("libarchive"),
        "pcre2" | "pcre2-8" => Some("pcre2"),
        "pcre" => Some("pcre"),
        "fmt" => Some("fmt"),
        "spdlog" => Some("spdlog"),
        "gtest" | "gmock" => Some("gtest"),
        "catch2" => Some("catch2_3"),
        "boost" => Some("boost"),
        "lua" => Some("lua"),
        "png" | "libpng" => Some("libpng"),
        "jpeg" | "libjpeg" => Some("libjpeg"),
        "freetype" | "freetype2" => Some("freetype"),
        "harfbuzz" => Some("harfbuzz"),
        "ncurses" | "curses" => Some("ncurses"),
        "readline" => Some("readline"),
        "ffi" | "libffi" => Some("libffi"),
        _ => None,
    }
}

/// Generates a `shell.nix` tailored to the project's `Crub.toml` configuration.
pub fn generate_shell_nix(manifest: &Manifest) -> String {
    let mut nix_pkgs = Vec::new();
    let mut seen = HashSet::new();

    let compiler_pkg = match &manifest.package.compiler {
        crate::config::CompilerType::Clang => "clang",
        crate::config::CompilerType::Gcc => "gcc",
        crate::config::CompilerType::Custom(c) if c.contains("clang") => "clang",
        _ => "gcc",
    };

    for base in &[compiler_pkg, "pkg-config", "clang-tools"] {
        if seen.insert(base.to_string()) {
            nix_pkgs.push(base.to_string());
        }
    }

    for lib in &manifest.package.libs {
        let clean = lib.trim().trim_start_matches("-l").trim();
        let mapped = map_lib_to_nix_pkg(clean)
            .map(|s| s.to_string())
            .unwrap_or_else(|| {
                if matches!(clean, "" | "m" | "pthread" | "dl" | "rt") {
                    String::new()
                } else {
                    clean.to_string()
                }
            });
        if !mapped.is_empty() && seen.insert(mapped.clone()) {
            nix_pkgs.push(mapped);
        }
    }

    let pkgs_list = nix_pkgs
        .iter()
        .map(|p| format!("    {}", p))
        .collect::<Vec<_>>()
        .join("\n");

    format!(
        r#"{{ pkgs ? import <nixpkgs> {{}} }}:

pkgs.mkShell {{
  name = "{name}-dev";

  nativeBuildInputs = with pkgs; [
    pkg-config
    clang-tools
  ];

  buildInputs = with pkgs; [
{pkgs_list}
  ];

  shellHook = ''
    export LD_LIBRARY_PATH="${{pkgs.lib.makeLibraryPath (with pkgs; [ {inline_pkgs} ])}}:$LD_LIBRARY_PATH"
    echo "❄️  NixOS environment ready for '{name}' (crupp)"
  '';
}}
"#,
        name = manifest.package.name,
        pkgs_list = pkgs_list,
        inline_pkgs = nix_pkgs.join(" ")
    )
}

/// Generates a `flake.nix` tailored to the project's `Crub.toml` configuration.
pub fn generate_flake_nix(manifest: &Manifest) -> String {
    let mut nix_pkgs = Vec::new();
    let mut seen = HashSet::new();

    let compiler_pkg = match &manifest.package.compiler {
        crate::config::CompilerType::Clang => "clang",
        crate::config::CompilerType::Gcc => "gcc",
        crate::config::CompilerType::Custom(c) if c.contains("clang") => "clang",
        _ => "gcc",
    };

    for base in &[compiler_pkg, "pkg-config", "clang-tools"] {
        if seen.insert(base.to_string()) {
            nix_pkgs.push(base.to_string());
        }
    }

    for lib in &manifest.package.libs {
        let clean = lib.trim().trim_start_matches("-l").trim();
        let mapped = map_lib_to_nix_pkg(clean)
            .map(|s| s.to_string())
            .unwrap_or_else(|| {
                if matches!(clean, "" | "m" | "pthread" | "dl" | "rt") {
                    String::new()
                } else {
                    clean.to_string()
                }
            });
        if !mapped.is_empty() && seen.insert(mapped.clone()) {
            nix_pkgs.push(mapped);
        }
    }

    let pkgs_list = nix_pkgs
        .iter()
        .map(|p| format!("            {}", p))
        .collect::<Vec<_>>()
        .join("\n");

    format!(
        r#"{{
  description = "NixOS development environment for {name} (Crupp)";

  inputs = {{
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
  }};

  outputs = {{ self, nixpkgs }}:
    let
      systems = [ "x86_64-linux" "aarch64-linux" "x86_64-darwin" "aarch64-darwin" ];
      forAllSystems = f: nixpkgs.lib.genAttrs systems (system: f (import nixpkgs {{ inherit system; }}));
    in {{
      devShells = forAllSystems (pkgs: {{
        default = pkgs.mkShell {{
          name = "{name}-dev";
          buildInputs = with pkgs; [
{pkgs_list}
          ];
          shellHook = ''
            export LD_LIBRARY_PATH="${{pkgs.lib.makeLibraryPath (with pkgs; [ {inline_pkgs} ])}}:$LD_LIBRARY_PATH"
          '';
        }};
      }});
    }};
}}
"#,
        name = manifest.package.name,
        pkgs_list = pkgs_list,
        inline_pkgs = nix_pkgs.join(" ")
    )
}
