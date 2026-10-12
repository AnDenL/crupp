use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "crupp",
    version = "0.3.0",
    about = "A lightning-fast Cargo-like build system for C and C++"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Build targets in the project (compile + link)
    Build {
        /// Path to configuration manifest
        #[arg(short, long, default_value = "Crub.toml")]
        manifest: String,

        /// Build a specific target (binary name)
        #[arg(short = 't', long = "target", alias = "bin")]
        target: Option<String>,
    },
    /// Build and run a specific target
    Run {
        /// Path to configuration manifest
        #[arg(short, long, default_value = "Crub.toml")]
        manifest: String,

        /// Run a specific target (defaults to first binary)
        #[arg(short = 't', long = "target", alias = "bin")]
        target: Option<String>,

        /// Arguments passed to the target executable
        #[arg(last = true)]
        args: Vec<String>,
    },
    /// Remove build artifacts and compiler cache
    Clean {
        /// Path to configuration manifest
        #[arg(short, long, default_value = "Crub.toml")]
        manifest: String,
    },
    /// Generate compile_commands.json for VS Code / clangd / Neovim
    Compdb {
        #[arg(short, long, default_value = "Crub.toml")]
        manifest: String,
    },
    /// Generate Crub.toml template
    Toml {
        #[arg(default_value = "my_app")]
        name: String,
    },
}
