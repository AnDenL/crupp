use super::diagnostics::{explain_compiler_error, explain_io_error};
use super::options::BuildContext;
use crate::config::{CompilerType, Language};
use crate::core::scanner::TranslationUnit;
use colored::*;
use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::process::Command;

pub async fn compile_unit(
    ctx: Arc<BuildContext>,
    unit: TranslationUnit,
    deep_hash: String,
) -> Result<PathBuf, String> {
    // Collision-free unique object and hash naming based on sanitized relative path
    let rel_path = unit.path.strip_prefix(".").unwrap_or(&unit.path);
    let safe_name = rel_path
        .to_string_lossy()
        .replace(['/', '\\'], "_")
        .replace('.', "_");

    let hash_prefix = if deep_hash.len() >= 8 {
        &deep_hash[0..8]
    } else {
        deep_hash.as_str()
    };
    let obj_name = format!("{}_{}.o", safe_name, hash_prefix);
    let obj_path = ctx.obj_dir.join(&obj_name);
    let cache_file = ctx.obj_dir.join(format!("{}.hash", safe_name));

    let is_cached = if !ctx.force {
        if let Ok(cached) = fs::read_to_string(&cache_file) {
            let mut valid = obj_path.exists();
            if let Language::Cpp = unit.language {
                let (resolved_cpp, _) = ctx.package.compiler.resolve_cpp_compiler();
                if resolved_cpp.contains("clang") {
                    if let Some(mod_name) = &unit.exported_module {
                        valid = valid && ctx.obj_dir.join(format!("{}.pcm", mod_name)).exists();
                    }
                }
            }
            cached == deep_hash && valid
        } else {
            false
        }
    } else {
        false
    };

    if is_cached {
        ctx.pb.suspend(|| {
            println!(
                "{:>12} {}",
                "Cached".bright_blue().bold(),
                unit.path.display()
            );
        });
        return Ok(obj_path);
    }

    ctx.pb.suspend(|| {
        println!("{:>12} {}", "Compiling".green().bold(), unit.path.display());
    });

    let raw_source = fs::read_to_string(&unit.path).map_err(|e| e.to_string())?;
    let file_to_compile = if crate::core::comptime::has_comptime_markers(&raw_source) {
        let transformed_code = crate::core::comptime::transform_comptime(
            &raw_source,
            &ctx.package.include_dirs,
            &ctx.obj_dir,
            &unit.path,
        )?;

        if transformed_code != raw_source {
            let gen_dir = ctx.obj_dir.join("gen");
            fs::create_dir_all(&gen_dir).map_err(|e| e.to_string())?;

            let file_name = unit
                .path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| "unit.c".to_string());
            let shadow_file = gen_dir.join(format!("{}_{}", safe_name, file_name));
            let abs_orig = fs::canonicalize(&unit.path).unwrap_or_else(|_| unit.path.clone());
            let escaped_orig = abs_orig
                .to_string_lossy()
                .replace('\\', "\\\\")
                .replace('"', "\\\"");
            let final_shadow_code = format!("#line 1 \"{}\"\n{}", escaped_orig, transformed_code);

            fs::write(&shadow_file, final_shadow_code).map_err(|e| e.to_string())?;
            shadow_file
        } else {
            unit.path.clone()
        }
    } else {
        unit.path.clone()
    };

    let (compiler_bin, standard, lang_flags, fallback_note) = match unit.language {
        Language::C => {
            let (resolved, note) = ctx.package.compiler.resolve_c_compiler();
            (resolved, &ctx.package.c_standard, &ctx.package.c_flags, note)
        }
        Language::Cpp => {
            let (resolved, note) = ctx.package.compiler.resolve_cpp_compiler();
            (resolved, &ctx.package.standard, &ctx.package.cpp_flags, note)
        }
    };

    if let Some(note) = fallback_note {
        ctx.pb.suspend(|| println!("{}", note));
    }

    let mut cmd = Command::new(&compiler_bin);
    cmd.arg(standard).arg("-c");
    for flag in &ctx.package.flags {
        for part in flag.split_whitespace() {
            cmd.arg(part);
        }
    }
    for flag in lang_flags {
        for part in flag.split_whitespace() {
            cmd.arg(part);
        }
    }
    for flag in &ctx.profile_flags {
        for part in flag.split_whitespace() {
            cmd.arg(part);
        }
    }
    for extra in crate::core::nix::extra_compiler_flags(&ctx.package.libs) {
        cmd.arg(extra);
    }

    cmd.arg("-DCOMPTIME=0");
    cmd.arg("-Dcomptime(x)=(x)");
    cmd.arg("-Dtcc(x)=(x)");
    cmd.arg(&file_to_compile).arg("-o").arg(&obj_path);

    let mut seen_includes = HashSet::new();

    for inc in &ctx.package.include_dirs {
        let clean = inc.trim().trim_start_matches("-I").trim();
        if clean.is_empty() {
            continue;
        }
        if seen_includes.insert(clean.to_string()) {
            cmd.arg(format!("-I{}", clean));
        }
    }

    if let Some(parent) = unit.path.parent() {
        let p_str = parent.to_string_lossy();
        let clean = p_str.trim().trim_start_matches("./").trim();
        if !clean.is_empty() && clean != "." {
            if seen_includes.insert(clean.to_string()) {
                cmd.arg(format!("-I{}", clean));
            }
        }
    }

    if unit.language == Language::Cpp {
        let is_export = unit.exported_module.is_some();
        let has_imports = !unit.imports.is_empty();
        let module_flags = CompilerType::get_module_flags_for_binary(
            &compiler_bin,
            &ctx.obj_dir,
            is_export,
            &unit.exported_module,
            has_imports,
        );
        cmd.args(module_flags);
    }

    if ctx.verbose {
        ctx.pb.suspend(|| {
            println!("  [Compiler Command]: {:?}", cmd);
        });
    }

    let output = cmd
        .output()
        .await
        .map_err(|e| explain_io_error(&compiler_bin, &e))?;

    if output.status.success() {
        let _ = fs::write(cache_file, &deep_hash);
        Ok(obj_path)
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        ctx.pb.suspend(|| {
            eprintln!(
                "\n{} {}",
                "  Error compiling".red().bold(),
                unit.path.display()
            );
            eprintln!("{}", stderr);
            explain_compiler_error(&stderr);
        });
        Err(format!(
            "Compilation stopped due to errors in {}",
            unit.path.display()
        ))
    }
}
