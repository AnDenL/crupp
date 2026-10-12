use crate::config::load_manifest_or_autodetect;
use std::path::PathBuf;
use tokio::process::Command;

pub async fn run_target(
    manifest_path: &str,
    target_name: Option<&str>,
    args: &[String],
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
            "Binary '{}' not found at '{}'. Run build first or check if compilation produced an output.",
            target_bin_name,
            bin_path.display()
        ));
    }

    let mut cmd = Command::new(&bin_path);
    cmd.args(args);

    let mut child = cmd
        .spawn()
        .map_err(|e| format!("Failed to spawn executable '{}': {}", bin_path.display(), e))?;

    let status = child
        .wait()
        .await
        .map_err(|e| format!("Process wait error: {}", e))?;

    if status.success() {
        Ok(())
    } else {
        Err(format!(
            "Process '{}' exited with status code {}",
            target_bin_name,
            status.code().unwrap_or(-1)
        ))
    }
}
