use crate::config::{BuildProfile, Package};
use indicatif::ProgressBar;
use std::path::PathBuf;

pub struct BuildOptions<'a> {
    pub manifest_path: &'a str,
    pub target_name: Option<&'a str>,
    pub profile: BuildProfile,
    pub jobs: Option<usize>,
    pub verbose: bool,
    pub force: bool,
}

impl<'a> Default for BuildOptions<'a> {
    fn default() -> Self {
        Self {
            manifest_path: "Crub.toml",
            target_name: None,
            profile: BuildProfile::Default,
            jobs: None,
            verbose: false,
            force: false,
        }
    }
}

pub struct BuildContext {
    pub package: Package,
    pub obj_dir: PathBuf,
    pub pb: ProgressBar,
    pub profile: BuildProfile,
    pub profile_flags: Vec<String>,
    pub verbose: bool,
    pub force: bool,
}
