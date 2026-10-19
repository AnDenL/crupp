mod cli;
mod config;
mod core;

use clap::Parser;
use cli::{Cli, Commands};
use colored::*;
use config::BuildProfile;
use std::fs;
use std::path::Path;

#[tokio::main]
async fn main() {
    core::nix::activate_nix_env_if_available();

    let cli = Cli::parse();
    let verbose = cli.verbose;

    match &cli.command {
        Commands::Build {
            file_or_target,
            manifest,
            target,
            release,
            debug,
            sanitize,
            profile,
            force,
        } => {
            let effective_target = target.as_deref().or(file_or_target.as_deref());

            let build_profile = if *release {
                BuildProfile::Release
            } else if *debug {
                BuildProfile::Debug
            } else if *sanitize {
                BuildProfile::Sanitize
            } else if let Some(p) = profile {
                BuildProfile::Custom(p.clone())
            } else {
                BuildProfile::Default
            };

            let profile_str = match &build_profile {
                BuildProfile::Release => " [release]".bright_magenta(),
                BuildProfile::Debug => " [debug]".bright_blue(),
                BuildProfile::Sanitize => " [sanitize]".bright_red(),
                BuildProfile::Custom(p) => format!(" [{}]", p).bright_yellow(),
                BuildProfile::Default => "".normal(),
            };

            println!(
                "{}{}",
                "Building project...".bright_green().bold(),
                profile_str
            );

            let opts = core::builder::BuildOptions {
                manifest_path: manifest,
                target_name: effective_target,
                profile: build_profile,
                jobs: cli.jobs,
                verbose,
                force: *force,
            };

            if let Err(e) = core::builder::build_project_with_options(opts).await {
                eprintln!("\n{} {}", "❌ Build failed:".red().bold(), e);
                std::process::exit(1);
            }
        }

        Commands::Run {
            file_or_target,
            manifest,
            target,
            release,
            debug,
            sanitize,
            profile,
            args,
        } => {
            let effective_target = target.as_deref().or(file_or_target.as_deref());

            let build_profile = if *release {
                BuildProfile::Release
            } else if *debug {
                BuildProfile::Debug
            } else if *sanitize {
                BuildProfile::Sanitize
            } else if let Some(p) = profile {
                BuildProfile::Custom(p.clone())
            } else {
                BuildProfile::Default
            };

            // Check if effective_target refers to a standalone C or C++ source file
            let is_standalone_source = effective_target.map_or(false, |t| {
                let p = Path::new(t);
                if let Some(ext) = p.extension().and_then(|e| e.to_str()) {
                    matches!(
                        ext.to_lowercase().as_str(),
                        "c" | "cpp" | "cxx" | "cc" | "c++"
                    )
                } else {
                    false
                }
            }) || effective_target
                .map_or(false, |t| Path::new(t).is_file());

            if is_standalone_source {
                let file_path = Path::new(effective_target.unwrap());
                if let Err(e) = core::runner::run_standalone_file(
                    file_path,
                    manifest,
                    build_profile,
                    args,
                    verbose,
                )
                .await
                {
                    eprintln!("\n{} {}", "❌ Run error:".red().bold(), e);
                    std::process::exit(1);
                }
                return;
            }

            let opts = core::builder::BuildOptions {
                manifest_path: manifest,
                target_name: effective_target,
                profile: build_profile,
                jobs: cli.jobs,
                verbose,
                force: false,
            };

            if let Err(e) = core::builder::build_project_with_options(opts).await {
                eprintln!("\n{} {}", "❌ Cannot run (build failed):".red().bold(), e);
                std::process::exit(1);
            }

            println!("{}", "▶️  Running target...".bright_cyan().bold());
            if let Err(e) =
                core::runner::run_target_verbose(manifest, effective_target, args, verbose).await
            {
                eprintln!("\n{} {}", "❌ Target error:".red().bold(), e);
                std::process::exit(1);
            }
        }

        Commands::AddBin {
            name,
            path,
            manifest,
        } => match config::add_bin_to_manifest(manifest, name, path) {
            Ok(msg) => println!("{:>12} {}", "Success".green().bold(), msg),
            Err(e) => {
                eprintln!("{} {}", "❌ Error:".red().bold(), e);
                std::process::exit(1);
            }
        },

        Commands::AddLib {
            lib,
            manifest,
            no_check,
        } => match config::add_library_to_manifest(manifest, lib, *no_check) {
            Ok(msg) => println!("{:>12} {}", "Success".green().bold(), msg),
            Err(e) => {
                eprintln!("{} {}", "❌ Error:".red().bold(), e);
                std::process::exit(1);
            }
        },

        Commands::CheckLibs { manifest } => {
            if let Err(e) = config::check_manifest_libraries(manifest) {
                eprintln!("{} {}", "❌ Error:".red().bold(), e);
                std::process::exit(1);
            }
        }

        Commands::Check {
            file_or_target,
            manifest,
            target,
        } => {
            let effective_target = target.as_deref().or(file_or_target.as_deref());
            if let Err(e) = core::builder::check_project(manifest, effective_target, verbose).await
            {
                eprintln!("\n{} {}", "❌ Syntax check failed:".red().bold(), e);
                std::process::exit(1);
            }
        }

        Commands::Test { manifest, filter } => {
            println!("{}", "🧪 Running automated tests...".bright_cyan().bold());
            if let Err(e) = run_tests(manifest, filter.as_deref(), verbose).await {
                eprintln!("\n{} {}", "❌ Tests failed:".red().bold(), e);
                std::process::exit(1);
            }
        }

        Commands::Clean { manifest, all } => {
            if let Err(e) = core::builder::clean_project_with_all(manifest, *all) {
                eprintln!("\n{} {}", "❌ Clean failed:".red().bold(), e);
                std::process::exit(1);
            }
        }

        Commands::Compdb { manifest, output } => {
            println!("{}", "Generating compilation database...".bright_green());
            if let Err(e) = core::builder::export_compdb(manifest).await {
                eprintln!("{} {}", "Generation failed:".red().bold(), e);
                std::process::exit(1);
            }
            if output != "compile_commands.json" {
                let _ = fs::copy("compile_commands.json", output);
            }
        }

        Commands::Nix {
            manifest,
            flake,
            envrc,
        } => {
            println!(
                "{}",
                "❄️  Generating NixOS development environment..."
                    .bright_cyan()
                    .bold()
            );
            let (loaded_manifest, _) = match config::load_manifest_or_autodetect(manifest) {
                Ok(res) => res,
                Err(e) => {
                    eprintln!("{} {}", "❌ Error:".red().bold(), e);
                    std::process::exit(1);
                }
            };

            let shell_content = core::nix::generate_shell_nix(&loaded_manifest);
            if let Err(e) = fs::write("shell.nix", shell_content) {
                eprintln!(
                    "{} Failed to write shell.nix: {}",
                    "❌ Error:".red().bold(),
                    e
                );
                std::process::exit(1);
            }
            println!("{:>12} 'shell.nix'", "Created".green().bold());

            if *flake {
                let flake_content = core::nix::generate_flake_nix(&loaded_manifest);
                if let Err(e) = fs::write("flake.nix", flake_content) {
                    eprintln!(
                        "{} Failed to write flake.nix: {}",
                        "❌ Error:".red().bold(),
                        e
                    );
                    std::process::exit(1);
                }
                println!("{:>12} 'flake.nix'", "Created".green().bold());
            }

            if *envrc {
                let envrc_content = if *flake { "use flake\n" } else { "use nix\n" };
                let _ = fs::write(".envrc", envrc_content);
                println!("{:>12} '.envrc'", "Created".green().bold());
            }

            println!(
                "\n  {} Enter your NixOS environment with: {}",
                "💡 Tip:".yellow().bold(),
                if *flake {
                    "nix develop".bright_green().bold()
                } else {
                    "nix-shell".bright_green().bold()
                }
            );
        }

        Commands::Toml { name, lang } => {
            println!("{}", "Generating Crub.toml...".bright_green());
            let content = config::default_with_name_and_lang(name, lang.as_deref());
            if let Err(e) = fs::write("Crub.toml", content) {
                eprintln!("{} {}", "Generation failed:".red().bold(), e);
                std::process::exit(1);
            }
            println!("{:>12} 'Crub.toml'", "Created".green().bold());
        }

        Commands::Init { name, lang, nix } => {
            let project_name = if name.is_empty() {
                std::env::current_dir()
                    .ok()
                    .and_then(|p| p.file_name().map(|s| s.to_string_lossy().to_string()))
                    .unwrap_or_else(|| "app".to_string())
            } else {
                name.clone()
            };

            println!(
                "{} '{}' in current directory...",
                "Initializing project".bright_green().bold(),
                project_name
            );

            let is_c = lang == "c";
            let main_file = if is_c { "src/main.c" } else { "src/main.cpp" };
            let _ = fs::create_dir_all("src");

            if !Path::new(main_file).exists() {
                let sample_code = if is_c {
                    "#include <stdio.h>\n\nint main(void) {\n    printf(\"Hello from Crupp!\\n\");\n    return 0;\n}\n"
                } else {
                    "#include <iostream>\n\nint main() {\n    std::cout << \"Hello from Crupp!\" << std::endl;\n    return 0;\n}\n"
                };
                let _ = fs::write(main_file, sample_code);
                println!("{:>12} '{}'", "Created".green().bold(), main_file);
            }

            if !Path::new("Crub.toml").exists() && !Path::new("crupp.toml").exists() {
                let toml_content = config::default_with_name_and_lang(&project_name, Some(lang));
                let _ = fs::write("Crub.toml", toml_content);
                println!("{:>12} 'Crub.toml'", "Created".green().bold());
            }

            if (*nix || core::nix::is_nix_env()) && !Path::new("shell.nix").exists() {
                if let Ok((m, _)) = config::load_manifest_or_autodetect("Crub.toml") {
                    let _ = fs::write("shell.nix", core::nix::generate_shell_nix(&m));
                    println!("{:>12} 'shell.nix'", "Created".green().bold());
                }
            }

            if !Path::new(".gitignore").exists() {
                let gitignore = "build/\ncompile_commands.json\n*.o\n*.pcm\n.direnv/\nresult\n";
                let _ = fs::write(".gitignore", gitignore);
                println!("{:>12} '.gitignore'", "Created".green().bold());
            }

            println!("{:>12} initialization!", "Finished".bright_green().bold());
        }

        Commands::New { name, lang, nix } => {
            println!(
                "{} new project in '{}'...",
                "Creating".bright_green().bold(),
                name
            );

            let project_dir = Path::new(name);
            if project_dir.exists() {
                eprintln!(
                    "{} Directory '{}' already exists!",
                    "Error:".red().bold(),
                    name
                );
                std::process::exit(1);
            }

            let src_dir = project_dir.join("src");
            if let Err(e) = fs::create_dir_all(&src_dir) {
                eprintln!("Failed to create directory: {}", e);
                std::process::exit(1);
            }

            let is_c = lang == "c";
            let main_path = if is_c {
                src_dir.join("main.c")
            } else {
                src_dir.join("main.cpp")
            };

            let sample_code = if is_c {
                "#include <stdio.h>\n\nint main(void) {\n    printf(\"Hello from Crupp!\\n\");\n    return 0;\n}\n"
            } else {
                "#include <iostream>\n\nint main() {\n    std::cout << \"Hello from Crupp!\" << std::endl;\n    return 0;\n}\n"
            };
            let _ = fs::write(&main_path, sample_code);

            let toml_path = project_dir.join("Crub.toml");
            let toml_content = config::default_with_name_and_lang(name, Some(lang));
            let _ = fs::write(&toml_path, toml_content);

            if *nix || core::nix::is_nix_env() {
                if let Ok((m, _)) = config::load_manifest_or_autodetect(&toml_path) {
                    let _ = fs::write(
                        project_dir.join("shell.nix"),
                        core::nix::generate_shell_nix(&m),
                    );
                }
            }

            let gitignore_path = project_dir.join(".gitignore");
            let gitignore = "build/\ncompile_commands.json\n*.o\n*.pcm\n.direnv/\nresult\n";
            let _ = fs::write(&gitignore_path, gitignore);

            println!(
                "{:>12} project '{}'!",
                "Created".bright_green().bold(),
                name
            );
            println!("\nTo get started:\n  cd {}\n  crupp run\n", name);
        }
        Commands::Completions { shell } => {
            use clap::CommandFactory;
            use clap_complete::generate;
            let mut cmd = Cli::command();
            let bin_name = cmd.get_name().to_string();
            generate(*shell, &mut cmd, bin_name, &mut std::io::stdout());
        }
    }
}

async fn run_tests(manifest_path: &str, filter: Option<&str>, verbose: bool) -> Result<(), String> {
    let tests_dir = Path::new("tests");
    if !tests_dir.exists() {
        return Err("Directory 'tests/' not found in current project.".into());
    }

    let mut test_files = Vec::new();
    for entry in fs::read_dir(tests_dir).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();
        if path.is_file() {
            if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                if matches!(ext, "c" | "cpp" | "cxx" | "cc") {
                    if let Some(f) = filter {
                        if !path.to_string_lossy().contains(f) {
                            continue;
                        }
                    }
                    test_files.push(path);
                }
            }
        }
    }

    test_files.sort();

    if test_files.is_empty() {
        println!("No test sources found in 'tests/'.");
        return Ok(());
    }

    println!("Found {} test suite(s).", test_files.len());
    let mut passed = 0;
    let mut failed = 0;

    for test_path in &test_files {
        let test_name = test_path.file_stem().unwrap().to_string_lossy();
        print!("  Running test '{}'... ", test_name);

        let opts = core::builder::BuildOptions {
            manifest_path,
            target_name: Some(&test_name),
            profile: BuildProfile::Debug,
            jobs: None,
            verbose,
            force: false,
        };

        if let Err(e) = core::builder::build_project_with_options(opts).await {
            println!("{}", "FAILED (build)".red().bold());
            eprintln!("    {}", e);
            failed += 1;
            continue;
        }

        match core::runner::run_target_verbose(manifest_path, Some(&test_name), &[], verbose).await
        {
            Ok(()) => {
                println!("{}", "PASSED".green().bold());
                passed += 1;
            }
            Err(e) => {
                println!("{}", "FAILED (run)".red().bold());
                eprintln!("    {}", e);
                failed += 1;
            }
        }
    }

    println!(
        "\nTest summary: {} passed, {} failed",
        passed.to_string().green(),
        if failed > 0 {
            failed.to_string().red()
        } else {
            failed.to_string().normal()
        }
    );

    if failed > 0 {
        Err(format!("{} test(s) failed", failed))
    } else {
        Ok(())
    }
}
