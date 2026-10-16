use clap::{Parser, Subcommand};

#[derive(Parser, Debug, Clone)]
#[command(
    name = "crupp",
    version = "0.4.2",
    about = "🚀 A lightning-fast, zero-config Cargo-like build system for C and C++ (with C++20 modules, comptime, and NixOS integration)"
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
        /// Optional target name (binary target to build)
        #[arg(value_name = "TARGET")]
        file_or_target: Option<String>,

        /// Path to configuration manifest (Crub.toml or crupp.toml)
        #[arg(short, long, default_value = "Crub.toml")]
        manifest: String,

        /// Build a specific target (binary name)
        #[arg(short = 't', long = "target", alias = "bin")]
        target: Option<String>,

        /// Build with optimizations enabled (-O3, -DNDEBUG)
        #[arg(short = 'r', long = "release", conflicts_with_all = ["debug", "sanitize"])]
        release: bool,

        /// Build with debug symbols and no optimizations (-g, -O0, -DDEBUG)
        #[arg(short = 'd', long = "debug", conflicts_with_all = ["release", "sanitize"])]
        debug: bool,

        /// Build with address and undefined behavior sanitizers (-fsanitize=address,undefined -g -O1)
        #[arg(short = 's', long = "sanitize", conflicts_with_all = ["release", "debug"])]
        sanitize: bool,

        /// Build using a custom profile defined in Crub.toml ([profile.<name>])
        #[arg(long = "profile")]
        profile: Option<String>,

        /// Force recompile by invalidating cache
        #[arg(long = "force")]
        force: bool,
    },

    /// Build and run a target or standalone source file (.c / .cpp)
    Run {
        /// Optional target name or path to standalone source file (e.g., scratchpad.c)
        #[arg(value_name = "TARGET_OR_FILE")]
        file_or_target: Option<String>,

        /// Path to configuration manifest
        #[arg(short, long, default_value = "Crub.toml")]
        manifest: String,

        /// Run a specific target (defaults to first binary) or path to single source file (e.g., test.c)
        #[arg(short = 't', long = "target", alias = "bin")]
        target: Option<String>,

        /// Build with optimizations enabled
        #[arg(short = 'r', long = "release", conflicts_with_all = ["debug", "sanitize"])]
        release: bool,

        /// Build with debug symbols
        #[arg(short = 'd', long = "debug", conflicts_with_all = ["release", "sanitize"])]
        debug: bool,

        /// Build with address and undefined behavior sanitizers (-fsanitize=address,undefined -g -O1)
        #[arg(short = 's', long = "sanitize", conflicts_with_all = ["release", "debug"])]
        sanitize: bool,

        /// Build using a custom profile defined in Crub.toml ([profile.<name>])
        #[arg(long = "profile")]
        profile: Option<String>,

        /// Arguments passed to the target executable (after `--`)
        #[arg(last = true)]
        args: Vec<String>,
    },

    /// Fast syntax-only check without linking (-fsyntax-only)
    Check {
        /// Optional target to check
        #[arg(value_name = "TARGET")]
        file_or_target: Option<String>,

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

    /// Generate compile_commands.json for VS Code / clangd / Neovim / CLion (with NixOS -isystem support)
    Compdb {
        /// Path to configuration manifest
        #[arg(short, long, default_value = "Crub.toml")]
        manifest: String,

        /// Output path for compilation database
        #[arg(short = 'o', long = "output", default_value = "compile_commands.json")]
        output: String,
    },

    /// Generate NixOS environment files (shell.nix, flake.nix, .envrc) from Crub.toml
    #[command(alias = "shell")]
    Nix {
        /// Path to configuration manifest
        #[arg(short, long, default_value = "Crub.toml")]
        manifest: String,

        /// Also generate flake.nix for Nix Flakes (`nix develop`)
        #[arg(long = "flake")]
        flake: bool,

        /// Also generate .envrc for direnv
        #[arg(long = "envrc")]
        envrc: bool,
    },

    /// Add a binary target [[bin]] to Crub.toml
    AddBin {
        /// Binary target name (executable name)
        name: String,

        /// Source file path containing main() (e.g., src/tools/inspect.c)
        path: String,

        /// Path to configuration manifest
        #[arg(short, long, default_value = "Crub.toml")]
        manifest: String,
    },

    /// Add a system library dependency to Crub.toml with host verification
    AddLib {
        /// Library name or pkg-config name (e.g., glfw, cjson, sqlite3, m, zlib)
        lib: String,

        /// Path to configuration manifest
        #[arg(short, long, default_value = "Crub.toml")]
        manifest: String,

        /// Skip verifying whether the library is installed on the host system
        #[arg(long = "no-check")]
        no_check: bool,
    },

    /// Check system dependencies and libraries listed in Crub.toml
    #[command(alias = "doctor")]
    CheckLibs {
        /// Path to configuration manifest
        #[arg(short, long, default_value = "Crub.toml")]
        manifest: String,
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

        /// Also generate shell.nix for NixOS
        #[arg(long = "nix")]
        nix: bool,
    },

    /// Create a new project directory with template files
    New {
        /// Directory name for the new project
        name: String,

        /// Language template (c or cpp)
        #[arg(long, default_value = "cpp")]
        lang: String,

        /// Also generate shell.nix for NixOS
        #[arg(long = "nix")]
        nix: bool,
    },
}
