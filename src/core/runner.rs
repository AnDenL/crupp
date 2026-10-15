use crate::config::{
    BuildProfile, Language, default_c_standard, default_cpp_standard, load_manifest_or_autodetect,
};
use crate::core::scanner::auto_discover_include_dirs;
use colored::*;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;
use tokio::process::Command;

pub async fn _run_target(
    manifest_path: &str,
    target_name: Option<&str>,
    args: &[String],
) -> Result<(), String> {
    run_target_verbose(manifest_path, target_name, args, false).await
}

pub async fn run_target_verbose(
    manifest_path: &str,
    target_name: Option<&str>,
    args: &[String],
    verbose: bool,
) -> Result<(), String> {
    crate::core::nix::activate_nix_env_if_available();

    let (manifest, _) = load_manifest_or_autodetect(manifest_path)?;
    let build_dir = PathBuf::from(&manifest.package.out_dir);

    let target_bin_name = if let Some(name) = target_name {
        name.to_string()
    } else if let Some(first_bin) = manifest.bin.first() {
        first_bin.name.clone()
    } else {
        manifest.package.name.clone()
    };

    let bin_path = build_dir.join(&target_bin_name);
    if !bin_path.exists() {
        return Err(format!(
            "Binary '{}' not found at '{}'. Run 'crupp build' first.",
            target_bin_name.bright_red(),
            bin_path.display()
        ));
    }

    // Ensure executable permissions on Unix systems
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(metadata) = std::fs::metadata(&bin_path) {
            let mut perms = metadata.permissions();
            if perms.mode() & 0o111 == 0 {
                perms.set_mode(perms.mode() | 0o755);
                let _ = std::fs::set_permissions(&bin_path, perms);
            }
        }
    }

    if verbose {
        println!(
            "{} {} with args: {:?}",
            "Executing:".bright_blue(),
            bin_path.display(),
            args
        );
    }

    let start_time = Instant::now();
    let mut cmd = Command::new(&bin_path);
    cmd.args(args);

    let mut child = cmd
        .spawn()
        .map_err(|e| format!("Failed to spawn executable '{}': {}", bin_path.display(), e))?;

    let status = child
        .wait()
        .await
        .map_err(|e| format!("Process wait error: {}", e))?;

    let elapsed = start_time.elapsed();

    if status.success() {
        if verbose {
            println!(
                "{}",
                format!(
                    "⚡ Process exited successfully in {:.2}ms",
                    elapsed.as_secs_f64() * 1000.0
                )
                .dimmed()
            );
        }
        Ok(())
    } else {
        Err(format!(
            "Process '{}' exited with status code {}",
            target_bin_name,
            status.code().unwrap_or(-1)
        ))
    }
}

pub async fn run_standalone_file(
    file_path: &Path,
    manifest_path: &str,
    profile: BuildProfile,
    args: &[String],
    verbose: bool,
) -> Result<(), String> {
    crate::core::nix::activate_nix_env_if_available();

    if !file_path.exists() {
        return Err(format!("Source file '{}' not found.", file_path.display()));
    }

    let (mut manifest, is_auto) = load_manifest_or_autodetect(manifest_path)?;

    if is_auto && manifest.package.include_dirs.is_empty() {
        manifest.package.include_dirs =
            auto_discover_include_dirs(Path::new("."), &manifest.package.out_dir);
    }

    let ext = file_path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("c")
        .to_lowercase();
    let is_cpp = matches!(ext.as_str(), "cpp" | "cxx" | "cc" | "c++");
    let lang = if is_cpp { Language::Cpp } else { Language::C };

    let file_stem = file_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("standalone_app");

    let standalone_dir = PathBuf::from(&manifest.package.out_dir).join("standalone");
    let obj_dir = standalone_dir.join("obj");
    fs::create_dir_all(&obj_dir).map_err(|e| e.to_string())?;

    let obj_path = obj_dir.join(format!("{}.o", file_stem));
    let bin_path = standalone_dir.join(file_stem);

    let profile_flags = manifest.resolve_profile_flags(&profile);

    println!(
        "{} standalone file {} (profile: {})...",
        "Compiling".green().bold(),
        file_path.display().to_string().cyan(),
        profile.name().magenta()
    );

    // Comptime handling
    let raw_source = fs::read_to_string(file_path).map_err(|e| e.to_string())?;
    let file_to_compile = if crate::core::comptime::has_comptime_markers(&raw_source) {
        let gen_dir = obj_dir.join("gen");
        fs::create_dir_all(&gen_dir).map_err(|e| e.to_string())?;

        let transformed = crate::core::comptime::transform_comptime(
            &raw_source,
            &manifest.package.include_dirs,
            &obj_dir,
            file_path,
        )?;

        let shadow = gen_dir.join(file_path.file_name().unwrap());
        let abs_orig = fs::canonicalize(file_path).unwrap_or_else(|_| file_path.to_path_buf());
        let shadow_content = format!(
            "#line 1 \"{}\"\n{}",
            abs_orig.to_string_lossy(),
            transformed
        );
        fs::write(&shadow, shadow_content).map_err(|e| e.to_string())?;
        shadow
    } else {
        file_path.to_path_buf()
    };

    // 1. Compilation step
    let (compiler_bin, fallback_note) = match lang {
        Language::C => manifest.package.compiler.resolve_c_compiler(),
        Language::Cpp => manifest.package.compiler.resolve_cpp_compiler(),
    };
    if let Some(note) = fallback_note {
        println!("{}", note);
    }

    let standard = match lang {
        Language::C => {
            if manifest.package.c_standard.is_empty() {
                default_c_standard()
            } else {
                manifest.package.c_standard.clone()
            }
        }
        Language::Cpp => {
            if manifest.package.standard.is_empty() {
                default_cpp_standard()
            } else {
                manifest.package.standard.clone()
            }
        }
    };

    let mut comp_cmd = Command::new(&compiler_bin);
    comp_cmd.arg(&standard).arg("-c");

    for flag in &manifest.package.flags {
        for part in flag.split_whitespace() {
            comp_cmd.arg(part);
        }
    }
    match lang {
        Language::C => {
            for flag in &manifest.package.c_flags {
                for part in flag.split_whitespace() {
                    comp_cmd.arg(part);
                }
            }
        }
        Language::Cpp => {
            for flag in &manifest.package.cpp_flags {
                for part in flag.split_whitespace() {
                    comp_cmd.arg(part);
                }
            }
        }
    }
    for flag in &profile_flags {
        for part in flag.split_whitespace() {
            comp_cmd.arg(part);
        }
    }
    for extra in crate::core::nix::extra_compiler_flags(&manifest.package.libs) {
        comp_cmd.arg(extra);
    }

    comp_cmd.arg("-DCOMPTIME=0");
    comp_cmd.arg("-Dcomptime(x)=(x)");
    comp_cmd.arg("-Dtcc(x)=(x)");
    comp_cmd.arg(&file_to_compile).arg("-o").arg(&obj_path);

    let mut seen_includes = HashSet::new();
    for inc in &manifest.package.include_dirs {
        let clean = inc.trim().trim_start_matches("-I").trim();
        if !clean.is_empty() && seen_includes.insert(clean.to_string()) {
            comp_cmd.arg(format!("-I{}", clean));
        }
    }
    if let Some(parent) = file_path.parent() {
        let p_str = parent.to_string_lossy();
        if !p_str.is_empty() && seen_includes.insert(p_str.to_string()) {
            comp_cmd.arg(format!("-I{}", p_str));
        }
    }

    if verbose {
        println!("  [Compiler Command]: {:?}", comp_cmd);
    }

    let comp_out = comp_cmd
        .output()
        .await
        .map_err(|e| format!("Failed to invoke compiler: {}", e))?;

    if !comp_out.status.success() {
        let err_msg = String::from_utf8_lossy(&comp_out.stderr);
        return Err(format!(
            "Compilation of standalone file '{}' failed:\n{}",
            file_path.display(),
            err_msg
        ));
    }

    // 2. Linking step
    let (linker_bin, _) = match lang {
        Language::C => manifest.package.compiler.resolve_c_compiler(),
        Language::Cpp => manifest.package.compiler.resolve_cpp_compiler(),
    };

    let mut link_cmd = Command::new(&linker_bin);
    for flag in &profile_flags {
        for part in flag.split_whitespace() {
            link_cmd.arg(part);
        }
    }
    link_cmd.arg(&obj_path).arg("-o").arg(&bin_path);

    let nix_link_flags =
        crate::core::nix::extra_linker_flags(&manifest.package.lib_dirs, &manifest.package.libs);
    for flag in &nix_link_flags {
        link_cmd.arg(flag);
    }

    let mut effective_libs = manifest.package.libs.clone();
    if cfg!(target_family = "unix") && !effective_libs.iter().any(|l| l == "m") {
        effective_libs.push("m".to_string());
    }
    for lib in &effective_libs {
        let clean = lib.trim().trim_start_matches("-l").trim();
        if !clean.is_empty() {
            let l_flag = format!("-l{}", clean);
            if !nix_link_flags.contains(&l_flag) {
                link_cmd.arg(l_flag);
            }
        }
    }

    if verbose {
        println!("  [Linker Command]: {:?}", link_cmd);
    }

    let link_out = link_cmd
        .output()
        .await
        .map_err(|e| format!("Failed to invoke linker: {}", e))?;

    if !link_out.status.success() {
        let err_msg = String::from_utf8_lossy(&link_out.stderr);
        return Err(format!(
            "Linking of standalone file '{}' failed:\n{}",
            file_path.display(),
            err_msg
        ));
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(metadata) = fs::metadata(&bin_path) {
            let mut perms = metadata.permissions();
            if perms.mode() & 0o111 == 0 {
                perms.set_mode(perms.mode() | 0o755);
                let _ = fs::set_permissions(&bin_path, perms);
            }
        }
    }

    println!(
        "{} {}...\n",
        "▶️  Running".cyan().bold(),
        bin_path.display().to_string().bold()
    );

    let start_time = Instant::now();
    let mut run_cmd = Command::new(&bin_path);
    run_cmd.args(args);

    let mut child = run_cmd
        .spawn()
        .map_err(|e| format!("Failed to spawn executable '{}': {}", bin_path.display(), e))?;

    let status = child
        .wait()
        .await
        .map_err(|e| format!("Execution wait error: {}", e))?;

    let elapsed = start_time.elapsed();

    if status.success() {
        if verbose {
            println!(
                "\n{}",
                format!(
                    "⚡ Exited successfully in {:.2}ms",
                    elapsed.as_secs_f64() * 1000.0
                )
                .dimmed()
            );
        }
        Ok(())
    } else {
        Err(format!(
            "Standalone executable '{}' exited with status code {}",
            file_stem,
            status.code().unwrap_or(-1)
        ))
    }
}
