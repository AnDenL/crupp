use colored::*;
use regex::Regex;
use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

static RE_UNDEF: OnceLock<Regex> = OnceLock::new();

pub fn explain_io_error(binary: &str, err: &std::io::Error) -> String {
    if err.kind() == std::io::ErrorKind::NotFound {
        format!(
            "Compiler executable '{}' was not found in PATH!\n    💡 Solution: Install it (`nix-shell -p gcc clang` on NixOS, `sudo apt install clang gcc` on Ubuntu, or `sudo pacman -S clang gcc` on Arch) or adjust 'compiler' in Crub.toml.",
            binary
        )
    } else {
        format!("Failed to execute '{}': {}", binary, err)
    }
}

pub fn explain_compiler_error(stderr: &str) {
    if stderr.contains("file not found") || stderr.contains("No such file or directory") {
        println!(
            "    {} Header file not found. If this is a project header, check its folder is in 'include_dirs'. On NixOS, run 'crupp nix' and enter 'nix-shell' so NIX_CFLAGS_COMPILE is populated.",
            "💡 Tip:".yellow().bold()
        );
    }
    if stderr.contains("concepts") || stderr.contains("requires") || stderr.contains("ranges") {
        println!(
            "    {} C++20 feature detected. Try setting standard = \"-std=c++20\" or \"-std=c++23\" in Crub.toml.",
            "💡 Tip:".yellow().bold()
        );
    }
    if stderr.contains("module") && stderr.contains("not found") {
        println!(
            "    {} C++20 module not found. Ensure the module export interface (.cppm) has 'export module <name>;' and is placed in source_dir.",
            "💡 Tip:".yellow().bold()
        );
    }
    if stderr.contains("Werror") || stderr.contains("treated as errors") {
        println!(
            "    {} Warnings were treated as errors (-Werror). Fix the warnings or adjust 'flags' in Crub.toml.",
            "💡 Tip:".yellow().bold()
        );
    }
}

struct KnownLib {
    pattern: &'static str,
    lib_name: &'static str,
    description: &'static str,
    package_hint: &'static str,
}

static KNOWN_LIBS: &[KnownLib] = &[
    KnownLib {
        pattern: "cJSON_",
        lib_name: "cjson",
        description: "cJSON parser library",
        package_hint: "libcjson-dev (Debian/Ubuntu), cjson (Arch / NixOS)",
    },
    KnownLib {
        pattern: "sqlite3_",
        lib_name: "sqlite3",
        description: "SQLite database engine",
        package_hint: "libsqlite3-dev (Debian/Ubuntu), sqlite (Arch / NixOS)",
    },
    KnownLib {
        pattern: "curl_",
        lib_name: "curl",
        description: "libcurl HTTP transfer library",
        package_hint: "libcurl4-openssl-dev (Debian/Ubuntu), curl (Arch / NixOS)",
    },
    KnownLib {
        pattern: "SSL_",
        lib_name: "ssl",
        description: "OpenSSL SSL/TLS library",
        package_hint: "libssl-dev (Debian/Ubuntu), openssl (Arch / NixOS)",
    },
    KnownLib {
        pattern: "EVP_",
        lib_name: "crypto",
        description: "OpenSSL Cryptography library",
        package_hint: "libssl-dev (Debian/Ubuntu), openssl (Arch / NixOS)",
    },
    KnownLib {
        pattern: "BIO_",
        lib_name: "crypto",
        description: "OpenSSL Cryptography library",
        package_hint: "libssl-dev (Debian/Ubuntu), openssl (Arch / NixOS)",
    },
    KnownLib {
        pattern: "CRYPTO_",
        lib_name: "crypto",
        description: "OpenSSL Cryptography library",
        package_hint: "libssl-dev (Debian/Ubuntu), openssl (Arch / NixOS)",
    },
    KnownLib {
        pattern: "deflate",
        lib_name: "z",
        description: "zlib compression library",
        package_hint: "zlib1g-dev (Debian/Ubuntu), zlib (Arch / NixOS)",
    },
    KnownLib {
        pattern: "inflate",
        lib_name: "z",
        description: "zlib compression library",
        package_hint: "zlib1g-dev (Debian/Ubuntu), zlib (Arch / NixOS)",
    },
    KnownLib {
        pattern: "SDL_",
        lib_name: "SDL2",
        description: "SDL2 multimedia library",
        package_hint: "libsdl2-dev (Debian/Ubuntu), sdl2 (Arch), SDL2 (NixOS)",
    },
    KnownLib {
        pattern: "sin",
        lib_name: "m",
        description: "Standard C Math library",
        package_hint: "built-in (link with -lm)",
    },
    KnownLib {
        pattern: "cos",
        lib_name: "m",
        description: "Standard C Math library",
        package_hint: "built-in (link with -lm)",
    },
    KnownLib {
        pattern: "sqrt",
        lib_name: "m",
        description: "Standard C Math library",
        package_hint: "built-in (link with -lm)",
    },
    KnownLib {
        pattern: "pow",
        lib_name: "m",
        description: "Standard C Math library",
        package_hint: "built-in (link with -lm)",
    },
    KnownLib {
        pattern: "pthread_",
        lib_name: "pthread",
        description: "POSIX Threads library",
        package_hint: "built-in (link with -lpthread or flags = [\"-pthread\"])",
    },
    KnownLib {
        pattern: "dlopen",
        lib_name: "dl",
        description: "Dynamic Linker loader library",
        package_hint: "built-in (link with -ldl)",
    },
    KnownLib {
        pattern: "dlsym",
        lib_name: "dl",
        description: "Dynamic Linker loader library",
        package_hint: "built-in (link with -ldl)",
    },
    KnownLib {
        pattern: "InitWindow",
        lib_name: "raylib",
        description: "Raylib game library",
        package_hint: "raylib",
    },
    KnownLib {
        pattern: "DrawText",
        lib_name: "raylib",
        description: "Raylib game library",
        package_hint: "raylib",
    },
    KnownLib {
        pattern: "yaml_",
        lib_name: "yaml",
        description: "LibYAML parser library",
        package_hint: "libyaml-dev (Debian/Ubuntu), libyaml (Arch / NixOS)",
    },
    KnownLib {
        pattern: "xmlParse",
        lib_name: "xml2",
        description: "LibXML2 parser library",
        package_hint: "libxml2-dev (Debian/Ubuntu), libxml2 (Arch / NixOS)",
    },
    KnownLib {
        pattern: "archive_",
        lib_name: "archive",
        description: "Libarchive library",
        package_hint: "libarchive-dev (Debian/Ubuntu), libarchive (Arch / NixOS)",
    },
    KnownLib {
        pattern: "uv_",
        lib_name: "uv",
        description: "Libuv asynchronous I/O library",
        package_hint: "libuv1-dev (Debian/Ubuntu), libuv (Arch / NixOS)",
    },
    KnownLib {
        pattern: "pcre2_",
        lib_name: "pcre2-8",
        description: "PCRE2 regular expressions library",
        package_hint: "libpcre2-dev (Debian/Ubuntu), pcre2 (Arch / NixOS)",
    },
    KnownLib {
        pattern: "pcre_",
        lib_name: "pcre",
        description: "PCRE regular expressions library",
        package_hint: "libpcre3-dev (Debian/Ubuntu), pcre (Arch / NixOS)",
    },
    KnownLib {
        pattern: "XOpenDisplay",
        lib_name: "X11",
        description: "X11 windowing library",
        package_hint: "libx11-dev (Debian/Ubuntu), libx11 (Arch), xorg.libX11 (NixOS)",
    },
    KnownLib {
        pattern: "glBegin",
        lib_name: "GL",
        description: "OpenGL graphics library",
        package_hint: "libgl1-mesa-dev (Debian/Ubuntu), mesa (Arch), libGL (NixOS)",
    },
    KnownLib {
        pattern: "vkCreate",
        lib_name: "vulkan",
        description: "Vulkan graphics loader",
        package_hint: "libvulkan-dev (Debian/Ubuntu), vulkan-icd-loader (Arch), vulkan-loader (NixOS)",
    },
];

pub fn extract_undefined_symbols(stderr: &str) -> Vec<String> {
    let re = RE_UNDEF.get_or_init(|| {
        Regex::new(r"undefined reference to [`']([^'`]+)['`]").unwrap()
    });

    let mut seen = HashSet::new();
    let mut symbols = Vec::new();

    for caps in re.captures_iter(stderr) {
        if let Some(sym) = caps.get(1) {
            let s = sym.as_str().to_string();
            if seen.insert(s.clone()) {
                symbols.push(s);
            }
        }
    }

    symbols
}

pub fn detect_missing_libs_from_stderr(stderr: &str) -> Vec<String> {
    let symbols = extract_undefined_symbols(stderr);
    let mut missing_libs = Vec::new();
    let mut seen = HashSet::new();

    for sym in &symbols {
        for entry in KNOWN_LIBS {
            if sym.starts_with(entry.pattern) || sym == entry.pattern {
                let lib = entry.lib_name.to_string();
                if seen.insert(lib.clone()) {
                    missing_libs.push(lib);
                }
            }
        }
    }

    missing_libs
}

pub fn explain_linker_error(stderr: &str) {
    let symbols = extract_undefined_symbols(stderr);

    if stderr.contains("multiple definition of `main'") || stderr.contains("multiple definition of 'main'") {
        println!(
            "    {} Multiple 'main' entrypoints detected. Declare separate [[bin]] sections in Crub.toml.",
            "💡 Tip:".yellow().bold()
        );
        return;
    }

    if stderr.contains("undefined reference to `main'") || stderr.contains("undefined reference to 'main'") {
        println!(
            "    {} No 'main' function found. Ensure at least one source file defines 'int main(void)' or 'int main(int argc, char** argv)'.",
            "💡 Tip:".yellow().bold()
        );
        return;
    }

    if symbols.is_empty() {
        return;
    }

    // Match identified libraries
    let mut matched_libs: HashMap<&'static str, (&'static str, &'static str, Vec<String>)> = HashMap::new();
    let mut unmatched_symbols = Vec::new();

    for sym in &symbols {
        let mut found = false;
        for entry in KNOWN_LIBS {
            if sym.starts_with(entry.pattern) || sym == entry.pattern {
                let e = matched_libs
                    .entry(entry.lib_name)
                    .or_insert_with(|| (entry.description, entry.package_hint, Vec::new()));
                if e.2.len() < 4 {
                    e.2.push(sym.clone());
                }
                found = true;
                break;
            }
        }
        if !found && unmatched_symbols.len() < 5 {
            unmatched_symbols.push(sym.clone());
        }
    }

    if !matched_libs.is_empty() {
        println!("\n{}", "  === Linker Diagnostics ===".yellow().bold());
        for (lib_name, (desc, pkg_hint, sample_syms)) in &matched_libs {
            println!(
                "  {} Missing library {}: {} ({})",
                "•".cyan().bold(),
                format!("'-l{}'", lib_name).bright_green().bold(),
                desc.dimmed(),
                format!("e.g. {}", sample_syms.join(", ")).italic()
            );
            println!(
                "    {} Add to your {}:",
                "👉 Solution:".green().bold(),
                "Crub.toml".bright_white().bold()
            );
            println!("       [package]");
            println!("       libs = [\"{}\"]", lib_name.bright_green());
            if !pkg_hint.starts_with("built-in") {
                println!(
                    "    {} Package hint: {} (or run 'crupp add-lib {} && crupp nix' on NixOS)",
                    "📦 Package:".dimmed(),
                    pkg_hint.cyan(),
                    lib_name
                );
            }
            println!();
        }
    }

    if !unmatched_symbols.is_empty() && matched_libs.is_empty() {
        println!(
            "\n  {} Undefined symbols: {}",
            "💡 Diagnostic:".yellow().bold(),
            unmatched_symbols.join(", ").red()
        );
        println!(
            "     Check that all .c source files are in source_dir or configure 'libs = [\"...\"]' in Crub.toml."
        );
    }
}
