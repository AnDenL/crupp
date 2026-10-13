use clap::{Parser, Subcommand};

#[derive(Parser, Debug, Clone)]
#[command(
    name = "crupp",
    version = "0.4.0",
    about = "🚀 A lightning-fast, zero-config Cargo-like build system for C and C++ (with C++20 modules, comptime, and auto-detection)"
)]
pub struct Cli {
    /// Verbose output (shows full compiler invocations, timings, and diagnostics)
    #[arg(short = 'v', long, global = true)]
    pub verbose: bool,

    /// Suppress non-essential output
    #[arg(short = 'q', long, global = true)]
    pub quiet: bool,

    /// Maximum number of parallel compilation jobs (defaults to CPU cores)
    #[arg(short = 'j', long, global = true)]
    pub jobs: Option<usize>,

    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand, Debug, Clone)]
pub enum Commands {
    /// Build targets in the project (compile + link)
    Build {
        /// Path to configuration manifest (Crub.toml or crupp.toml)
        #[arg(short, long, default_value = "Crub.toml")]
        manifest: String,

        /// Build a specific target (binary name)
        #[arg(short = 't', long = "target", alias = "bin")]
        target: Option<String>,

        /// Build with optimizations enabled (-O3, -DNDEBUG)
        #[arg(short = 'r', long = "release", conflicts_with = "debug")]
        release: bool,

        /// Build with debug symbols and no optimizations (-g, -O0, -DDEBUG)
        #[arg(short = 'd', long = "debug", conflicts_with = "release")]
        debug: bool,

        /// Force recompile by invalidating cache
        #[arg(long = "force")]
        force: bool,
    },

    /// Build and run a specific target
    Run {
        /// Path to configuration manifest
        #[arg(short, long, default_value = "Crub.toml")]
        manifest: String,

        /// Run a specific target (defaults to first binary)
        #[arg(short = 't', long = "target", alias = "bin")]
        target: Option<String>,

        /// Build with optimizations enabled
        #[arg(short = 'r', long = "release", conflicts_with = "debug")]
        release: bool,

        /// Build with debug symbols
        #[arg(short = 'd', long = "debug", conflicts_with = "release")]
        debug: bool,

        /// Arguments passed to the target executable
        #[arg(last = true)]
        args: Vec<String>,
    },

    /// Fast syntax-only check without linking (-fsyntax-only)
    Check {
        /// Path to configuration manifest
        #[arg(short, long, default_value = "Crub.toml")]
        manifest: String,

        /// Target to check
        #[arg(short = 't', long = "target", alias = "bin")]
        target: Option<String>,
    },

    /// Discover and run test binaries in tests/
    Test {
        /// Path to configuration manifest
        #[arg(short, long, default_value = "Crub.toml")]
        manifest: String,

        /// Filter test names by substring
        #[arg(short = 'f', long = "filter")]
        filter: Option<String>,
    },

    /// Remove build artifacts and compiler cache
    Clean {
        /// Path to configuration manifest
        #[arg(short, long, default_value = "Crub.toml")]
        manifest: String,

        /// Also delete compile_commands.json and temporary files
        #[arg(long = "all")]
        all: bool,
    },

    /// Generate compile_commands.json for VS Code / clangd / Neovim / CLion
    Compdb {
        /// Path to configuration manifest
        #[arg(short, long, default_value = "Crub.toml")]
        manifest: String,

        /// Output path for compilation database
        #[arg(short = 'o', long = "output", default_value = "compile_commands.json")]
        output: String,
    },

    /// Generate Crub.toml template
    Toml {
        /// Project name
        #[arg(default_value = "my_app")]
        name: String,

        /// Language template: c or cpp (defaults to auto-detecting existing files)
        #[arg(long)]
        lang: Option<String>,
    },

    /// Initialize a new project in the current directory
    Init {
        /// Project name (defaults to current directory name)
        #[arg(default_value = "")]
        name: String,

        /// Language template (c or cpp)
        #[arg(long, default_value = "cpp")]
        lang: String,
    },

    /// Create a new project directory with template files
    New {
        /// Directory name for the new project
        name: String,

        /// Language template (c or cpp)
        #[arg(long, default_value = "cpp")]
        lang: String,
    },
}
