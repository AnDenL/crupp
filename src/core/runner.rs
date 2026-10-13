use crate::config::load_manifest_or_autodetect;
use colored::*;
use std::path::PathBuf;
use std::time::Instant;
use tokio::process::Command;

pub async fn run_target(
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
                format!("⚡ Process exited successfully in {:.2}ms", elapsed.as_secs_f64() * 1000.0)
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
