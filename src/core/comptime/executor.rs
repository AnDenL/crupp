use colored::*;
use std::ffi::{CStr, CString, c_char, c_int, c_void};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, Once};

pub enum TCCState {}

const TCC_OUTPUT_MEMORY: c_int = 1;
const TCC_RELOCATE_AUTO: *mut c_void = 1 as *mut c_void;

static TCC_TCCDEFS: &str = include_str!("../../../vendor/tcc/include/tccdefs.h");
static TCC_STDDEF: &str = include_str!("../../../vendor/tcc/include/stddef.h");
static TCC_STDARG: &str = include_str!("../../../vendor/tcc/include/stdarg.h");
static TCC_STDBOOL: &str = include_str!("../../../vendor/tcc/include/stdbool.h");
static TCC_FLOAT: &str = include_str!("../../../vendor/tcc/include/float.h");

static INIT_RUNTIME_LIBS: Once = Once::new();
static LIBM_PRELOADED: AtomicBool = AtomicBool::new(false);
static COMPTIME_MUTEX: Mutex<()> = Mutex::new(());
static TCC_LAST_ERROR: Mutex<Option<String>> = Mutex::new(None);

fn ensure_runtime_libraries() {
    INIT_RUNTIME_LIBS.call_once(|| unsafe {
        for lib in &["libc.so.6", "libc.so"] {
            if let Ok(c_lib) = CString::new(*lib) {
                libc::dlopen(c_lib.as_ptr(), libc::RTLD_NOW | libc::RTLD_GLOBAL);
            }
        }
        for lib in &["libm.so.6", "libm.so"] {
            if let Ok(c_lib) = CString::new(*lib) {
                let handle = libc::dlopen(c_lib.as_ptr(), libc::RTLD_NOW | libc::RTLD_GLOBAL);
                if !handle.is_null() {
                    LIBM_PRELOADED.store(true, Ordering::SeqCst);
                }
            }
        }

        // Also probe discovered NixOS / system library directories for libc.so.6 and libm.so.6
        let (c_compiler, _) = crate::config::CompilerType::default().resolve_c_compiler();
        for dir in crate::core::nix::discover_system_library_dirs(&c_compiler) {
            let libc_cand = dir.join("libc.so.6");
            if libc_cand.exists() {
                if let Ok(c_path) = CString::new(libc_cand.to_string_lossy().as_bytes()) {
                    libc::dlopen(c_path.as_ptr(), libc::RTLD_NOW | libc::RTLD_GLOBAL);
                }
            }
            let libm_cand = dir.join("libm.so.6");
            if libm_cand.exists() {
                if let Ok(c_path) = CString::new(libm_cand.to_string_lossy().as_bytes()) {
                    let handle =
                        libc::dlopen(c_path.as_ptr(), libc::RTLD_NOW | libc::RTLD_GLOBAL);
                    if !handle.is_null() {
                        LIBM_PRELOADED.store(true, Ordering::SeqCst);
                    }
                }
            }
        }
    });
}

pub fn sanitize_unit_id(origin_file: &Path) -> String {
    origin_file
        .to_string_lossy()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect()
}

fn escape_c_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 4);
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            _ => out.push(c),
        }
    }
    out
}

/// RAII Guard that ensures stdout and stderr descriptors are ALWAYS restored to their original state,
/// even if a panic or error occurs during comptime code execution.
struct StdioGuard {
    saved_stdout: libc::c_int,
    saved_stderr: libc::c_int,
}

impl Drop for StdioGuard {
    fn drop(&mut self) {
        unsafe {
            libc::fflush(std::ptr::null_mut());
            if self.saved_stdout >= 0 {
                libc::dup2(self.saved_stdout, libc::STDOUT_FILENO);
                libc::close(self.saved_stdout);
            }
            if self.saved_stderr >= 0 {
                libc::dup2(self.saved_stderr, libc::STDERR_FILENO);
                libc::close(self.saved_stderr);
            }
        }
    }
}

/// Captures `stdout` and `stderr` written by comptime code into separate buffers,
/// restoring the original file descriptors immediately after execution.
fn capture_stdio<F: FnOnce() -> c_int>(f: F) -> (String, String, c_int) {
    unsafe {
        libc::fflush(std::ptr::null_mut());

        let saved_out = libc::dup(libc::STDOUT_FILENO);
        let saved_err = libc::dup(libc::STDERR_FILENO);
        if saved_out < 0 || saved_err < 0 {
            if saved_out >= 0 {
                libc::close(saved_out);
            }
            if saved_err >= 0 {
                libc::close(saved_err);
            }
            let code = f();
            return (String::new(), String::new(), code);
        }

        let _guard = StdioGuard {
            saved_stdout: saved_out,
            saved_stderr: saved_err,
        };

        let tmp_out = libc::tmpfile();
        let tmp_err = libc::tmpfile();
        if tmp_out.is_null() || tmp_err.is_null() {
            if !tmp_out.is_null() {
                libc::fclose(tmp_out);
            }
            if !tmp_err.is_null() {
                libc::fclose(tmp_err);
            }
            let code = f();
            return (String::new(), String::new(), code);
        }

        let out_fd = libc::fileno(tmp_out);
        let err_fd = libc::fileno(tmp_err);

        libc::dup2(out_fd, libc::STDOUT_FILENO);
        libc::dup2(err_fd, libc::STDERR_FILENO);

        let code = f();

        libc::fflush(std::ptr::null_mut());
        libc::dup2(saved_out, libc::STDOUT_FILENO);
        libc::dup2(saved_err, libc::STDERR_FILENO);

        let read_tmp = |tmp: *mut libc::FILE| -> String {
            let mut bytes = Vec::new();
            libc::rewind(tmp);
            let mut buf = [0u8; 8192];
            loop {
                let n = libc::fread(buf.as_mut_ptr() as *mut libc::c_void, 1, buf.len(), tmp);
                if n == 0 {
                    break;
                }
                bytes.extend_from_slice(&buf[..n]);
            }
            libc::fclose(tmp);
            String::from_utf8_lossy(&bytes).to_string()
        };

        let stdout_str = read_tmp(tmp_out);
        let stderr_str = read_tmp(tmp_err);
        (stdout_str, stderr_str, code)
    }
}

/// Initializes the shared TCC runtime home (`tccdefs.h`, `stddef.h`, `stdarg.h`, `stdbool.h`, `float.h`, `libtcc1.a`)
/// and writes the unit-specific `<comptime.h>` into an isolated per-unit directory to prevent parallel race conditions.
pub fn prepare_unit_headers(
    obj_dir: &Path,
    origin_file: &Path,
    comptime_h_content: &str,
) -> (PathBuf, PathBuf) {
    let _guard = COMPTIME_MUTEX.lock().unwrap_or_else(|e| e.into_inner());

    let tcc_home = obj_dir.join("tcc_home");
    let shared_inc_dir = tcc_home.join("include");
    let shared_crupp_dir = shared_inc_dir.join("crupp");
    let lib_dir = tcc_home.join("lib");
    let marker = shared_inc_dir.join("tccdefs.h");

    let _ = fs::create_dir_all(&shared_crupp_dir);
    let _ = fs::create_dir_all(&lib_dir);

    if !marker.exists() {
        let _ = fs::write(shared_inc_dir.join("tccdefs.h"), TCC_TCCDEFS);
        let _ = fs::write(shared_inc_dir.join("stddef.h"), TCC_STDDEF);
        let _ = fs::write(shared_inc_dir.join("stdarg.h"), TCC_STDARG);
        let _ = fs::write(shared_inc_dir.join("stdbool.h"), TCC_STDBOOL);
        let _ = fs::write(shared_inc_dir.join("float.h"), TCC_FLOAT);

        let empty_ar = b"!<arch>\n";
        let _ = fs::write(tcc_home.join("libtcc1.a"), empty_ar);
        let _ = fs::write(lib_dir.join("libtcc1.a"), empty_ar);
    }

    // Write unit-specific <comptime.h> into an isolated directory per source file
    let unit_id = sanitize_unit_id(origin_file);
    let unit_inc_dir = obj_dir.join("tcc_units").join(&unit_id).join("include");
    let unit_crupp_dir = unit_inc_dir.join("crupp");
    let _ = fs::create_dir_all(&unit_crupp_dir);

    let _ = fs::write(unit_inc_dir.join("comptime.h"), comptime_h_content);
    let _ = fs::write(unit_crupp_dir.join("comptime.h"), comptime_h_content);
    let _ = fs::write(unit_inc_dir.join("crupp_comptime.h"), comptime_h_content);

    // Also update shared tcc_home/include/comptime.h while holding COMPTIME_MUTEX for IDE inspection
    let _ = fs::write(shared_inc_dir.join("comptime.h"), comptime_h_content);
    let _ = fs::write(shared_crupp_dir.join("comptime.h"), comptime_h_content);
    let _ = fs::write(shared_inc_dir.join("crupp_comptime.h"), comptime_h_content);

    let abs_tcc_home = fs::canonicalize(&tcc_home).unwrap_or_else(|_| {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(&tcc_home)
    });
    let abs_unit_inc = fs::canonicalize(&unit_inc_dir).unwrap_or_else(|_| {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(&unit_inc_dir)
    });

    (abs_tcc_home, abs_unit_inc)
}

unsafe extern "C" {
    fn tcc_new() -> *mut TCCState;
    fn tcc_delete(s: *mut TCCState);
    fn tcc_set_lib_path(s: *mut TCCState, path: *const c_char);
    fn tcc_set_output_type(s: *mut TCCState, output_type: c_int) -> c_int;
    fn tcc_add_include_path(s: *mut TCCState, pathname: *const c_char) -> c_int;
    fn tcc_add_sysinclude_path(s: *mut TCCState, pathname: *const c_char) -> c_int;
    fn tcc_add_library_path(s: *mut TCCState, pathname: *const c_char) -> c_int;
    fn tcc_define_symbol(s: *mut TCCState, sym: *const c_char, value: *const c_char);
    fn tcc_add_library(s: *mut TCCState, libraryname: *const c_char) -> c_int;
    fn tcc_compile_string(s: *mut TCCState, buf: *const c_char) -> c_int;
    fn tcc_relocate(s: *mut TCCState, ptr: *mut c_void) -> c_int;
    fn tcc_get_symbol(s: *mut TCCState, name: *const c_char) -> *mut c_void;
    fn tcc_set_error_func(
        s: *mut TCCState,
        error_opaque: *mut c_void,
        error_func: unsafe extern "C" fn(*mut c_void, *const c_char),
    );
}

extern "C" fn tcc_error_callback(_opaque: *mut c_void, msg: *const c_char) {
    if !msg.is_null() {
        let err = unsafe { CStr::from_ptr(msg).to_string_lossy().to_string() };
        // Ignore benign "library 'm' not found" if libm.so.6 is already preloaded in process memory
        if LIBM_PRELOADED.load(Ordering::SeqCst)
            && (err.contains("library 'm' not found") || err.contains("library \"m\" not found"))
        {
            return;
        }
        if let Ok(mut lock) = TCC_LAST_ERROR.lock() {
            if let Some(existing) = lock.as_mut() {
                existing.push('\n');
                existing.push_str(&err);
            } else {
                *lock = Some(err);
            }
        }
    }
}

fn detect_cpp_in_comptime(code: &str, raw_err: &str) -> Option<&'static str> {
    let clean = crate::core::scanner::strip_comments_fast(code);
    for cpp_hdr in &[
        "<iostream>",
        "<vector>",
        "<string>",
        "<map>",
        "<unordered_map>",
        "<sstream>",
        "<algorithm>",
        "<memory>",
        "<format>",
        "'iostream' not found",
        "'vector' not found",
        "'string' not found",
    ] {
        if clean.contains(cpp_hdr) || raw_err.contains(cpp_hdr) {
            return Some(cpp_hdr);
        }
    }
    for cpp_tok in &[
        "std::",
        "namespace ",
        "template<",
        "template <",
        "constexpr ",
        "nullptr",
        "cout <<",
        "cerr <<",
    ] {
        if clean.contains(cpp_tok) {
            return Some(cpp_tok);
        }
    }
    None
}

fn format_comptime_diagnostic(raw_err: &str, code: &str, origin_file: &Path) -> String {
    let mut formatted = String::new();
    let mut hints = Vec::new();

    if let Some(cpp_feature) = detect_cpp_in_comptime(code, raw_err) {
        hints.push(format!(
            "  {} Виявлено C++ конструкцію ({}) всередині '#if comptime'!\n      {}\n      {}",
            "💡 Причина помилки (C++ у блоці #if comptime):".yellow().bold(),
            cpp_feature.bright_red().bold(),
            "• Блок '#if comptime' виконується вбудованим JIT-компілятором TinyCC (чистий C99/C11), який НЕ підтримує синтаксис C++.".cyan(),
            "• Рішення: пишіть сам генератор на чистому C (<stdio.h>, <string.h>, <comptime.h>, emit(), emitf()). Всередину emitf(\"...\") ви можете передавати будь-який C або C++ код!".bright_green()
        ));
    }

    for line in raw_err.lines() {
        formatted.push_str("  ");
        formatted.push_str(&line.bright_red().to_string());
        formatted.push('\n');

        if line.contains("library 'm' not found")
            || line.contains("library \"m\" not found")
            || line.contains("-lm")
        {
            hints.push(format!(
                "  {} {}\n      {}\n      {}",
                "💡 Що це за помилка:".yellow().bold(),
                "Бібліотека 'm' — це стандартна математична бібліотека C (libm.so, де містяться sin, cos, sqrt, pow).".bold(),
                "• На NixOS: увійдіть у 'nix-shell' або 'nix develop' (де підключено glibc/glibc.dev), або запустіть 'crupp nix'.".cyan(),
                "• На Ubuntu/Debian: встановіть 'build-essential'.".dimmed()
            ));
        } else if line.contains("library '") && line.contains("not found") {
            if let Some(start) = line.find("library '") {
                let rest = &line[start + 9..];
                if let Some(end) = rest.find('\'') {
                    let lib_name = &rest[..end];
                    let nix_hint =
                        crate::core::nix::map_lib_to_nix_pkg(lib_name).unwrap_or(lib_name);
                    hints.push(format!(
                        "  {} Відсутня бібліотека '{}' для comptime виконання.\n      Додайте її в shell.nix або запустіть: nix-shell -p {}",
                        "💡 Підказка:".yellow().bold(),
                        lib_name.green().bold(),
                        nix_hint.cyan().bold()
                    ));
                }
            }
        } else if line.contains("include file") && line.contains("not found") {
            if let Some(start) = line.find('\'') {
                let rest = &line[start + 1..];
                if let Some(end) = rest.find('\'') {
                    let hdr_name = &rest[..end];
                    hints.push(format!(
                        "  {} Заголовочний файл '{}' не знайдено під час виконання '#if comptime' у {}.\n      • Якщо це локальний заголовок проєкту — перевірте 'include_dirs' у Crub.toml.\n      • Якщо цей заголовок потрібен лише для фінального коду (а не для генератора) — перемістіть '#include \"{}\"' за межі '#if comptime' або емітуйте його через emit(\"#include ...\").",
                        "💡 Підказка:".yellow().bold(),
                        hdr_name.green().bold(),
                        origin_file.display(),
                        hdr_name
                    ));
                }
            }
        } else if line.contains("undefined symbol") {
            if let Some(start) = line.find('\'') {
                let rest = &line[start + 1..];
                if let Some(end) = rest.find('\'') {
                    let sym_name = &rest[..end];
                    hints.push(format!(
                        "  {} Невідомий символ '{}' під час виконання '#if comptime' у {}.\n      • Блок '#if comptime' виконується ПІД ЧАС компіляції (до того, як ваш проєкт скомпільовано).\n      • Якщо '{}' — це функція вашого рушія/проєкту і ви хотіли згенерувати її виклик у фінальний C-код, обгорніть її у emitf(\"{}(...);\\n\");",
                        "💡 Підказка:".yellow().bold(),
                        sym_name.green().bold(),
                        origin_file.display(),
                        sym_name,
                        sym_name
                    ));
                }
            }
        }

        // Extract line number if the error points to origin_file and show surrounding context lines
        let file_str = origin_file.to_string_lossy();
        if let Some(rest) = line.strip_prefix(file_str.as_ref()) {
            if let Some(after_colon) = rest.strip_prefix(':') {
                let digits: String = after_colon
                    .chars()
                    .take_while(|c| c.is_ascii_digit())
                    .collect();
                if let Ok(line_num) = digits.parse::<usize>() {
                    if let Ok(src) = fs::read_to_string(origin_file) {
                        let all_lines: Vec<&str> = src.lines().collect();
                        let start_ctx = line_num.saturating_sub(2);
                        let end_ctx = (line_num + 1).min(all_lines.len());
                        for idx in start_ctx..end_ctx {
                            let num = idx + 1;
                            if num == line_num {
                                formatted.push_str(&format!(
                                    "  {} {:>4} | {}\n",
                                    "→".bright_red().bold(),
                                    num.to_string().bright_red().bold(),
                                    all_lines[idx].bright_yellow().bold()
                                ));
                            } else {
                                formatted.push_str(&format!(
                                    "    {:>4} | {}\n",
                                    num.to_string().dimmed(),
                                    all_lines[idx].dimmed()
                                ));
                            }
                        }
                    }
                }
            }
        }
    }

    if !hints.is_empty() {
        formatted.push('\n');
        for h in hints {
            formatted.push_str(&h);
            formatted.push('\n');
        }
    }

    formatted.trim_end().to_string()
}

pub fn execute_c_code(
    code: &str,
    start_line: usize,
    include_dirs: &[String],
    tcc_home: &Path,
    unit_inc_dir: &Path,
    origin_file: &Path,
) -> Result<String, String> {
    ensure_runtime_libraries();
    let _guard = COMPTIME_MUTEX.lock().unwrap_or_else(|e| e.into_inner());

    let (c_compiler, _) = crate::config::CompilerType::default().resolve_c_compiler();
    let sys_include_dirs = crate::core::nix::discover_tcc_system_includes(&c_compiler);
    let sys_lib_dirs = crate::core::nix::discover_system_library_dirs(&c_compiler);

    if let Ok(mut lock) = TCC_LAST_ERROR.lock() {
        *lock = None;
    }

    unsafe {
        let s = tcc_new();
        if s.is_null() {
            return Err("Failed to initialize TCC context in memory".to_string());
        }

        tcc_set_error_func(s, std::ptr::null_mut(), tcc_error_callback);

        let c_tcc_home = CString::new(tcc_home.to_string_lossy().as_bytes()).unwrap();
        tcc_set_lib_path(s, c_tcc_home.as_ptr());

        // 1. Register isolated per-unit include dir FIRST so <comptime.h> is never contended
        let c_unit_inc = CString::new(unit_inc_dir.to_string_lossy().as_bytes()).unwrap();
        tcc_add_sysinclude_path(s, c_unit_inc.as_ptr());
        tcc_add_include_path(s, c_unit_inc.as_ptr());

        // 2. Register shared TCC runtime include dir (tccdefs.h, stddef.h, stdarg.h, etc.)
        let shared_inc = tcc_home.join("include");
        let c_shared_inc = CString::new(shared_inc.to_string_lossy().as_bytes()).unwrap();
        tcc_add_sysinclude_path(s, c_shared_inc.as_ptr());

        tcc_set_output_type(s, TCC_OUTPUT_MEMORY);

        for (k, v) in &[("COMPTIME", "1"), ("comptime", "1")] {
            let sym_name = CString::new(*k).unwrap();
            let sym_val = CString::new(*v).unwrap();
            tcc_define_symbol(s, sym_name.as_ptr(), sym_val.as_ptr());
        }

        // 3. Register discovered NixOS / system C header paths (glibc-dev, NIX_CFLAGS_COMPILE, /usr/include)
        for sys_path in &sys_include_dirs {
            if let Ok(c_sys) = CString::new(sys_path.to_string_lossy().as_bytes()) {
                tcc_add_sysinclude_path(s, c_sys.as_ptr());
            }
        }

        // 4. Register discovered NixOS / system library paths
        for lib_path in &sys_lib_dirs {
            if let Ok(c_lib) = CString::new(lib_path.to_string_lossy().as_bytes()) {
                tcc_add_library_path(s, c_lib.as_ptr());
            }
        }

        let c_dot = CString::new(".").unwrap();
        tcc_add_include_path(s, c_dot.as_ptr());

        let current_dir = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let c_cwd = CString::new(current_dir.to_string_lossy().as_bytes()).unwrap_or_default();
        tcc_add_include_path(s, c_cwd.as_ptr());

        // Add origin_file's parent and ancestor directories
        let mut anc = origin_file.parent();
        while let Some(parent) = anc {
            let p_str = parent.to_string_lossy();
            if p_str.is_empty() || p_str == "." {
                break;
            }
            if let Ok(c_parent_rel) = CString::new(p_str.as_bytes()) {
                tcc_add_include_path(s, c_parent_rel.as_ptr());
            }
            let abs_parent = if parent.is_relative() {
                current_dir.join(parent)
            } else {
                parent.to_path_buf()
            };
            if let Ok(c_parent_abs) = CString::new(abs_parent.to_string_lossy().as_bytes()) {
                tcc_add_include_path(s, c_parent_abs.as_ptr());
            }
            anc = parent.parent();
        }

        for extra_dir in &["include", "src"] {
            let abs_extra = current_dir.join(extra_dir);
            if abs_extra.is_dir() {
                if let Ok(c_ex) = CString::new(abs_extra.to_string_lossy().as_bytes()) {
                    tcc_add_include_path(s, c_ex.as_ptr());
                }
            }
        }

        for inc in include_dirs {
            let clean = inc.trim().trim_start_matches("-I").trim();
            if !clean.is_empty() {
                if let Ok(c_inc) = CString::new(clean) {
                    tcc_add_include_path(s, c_inc.as_ptr());
                }
                let abs_inc = current_dir.join(clean);
                if abs_inc.is_dir() {
                    if let Ok(c_abs) = CString::new(abs_inc.to_string_lossy().as_bytes()) {
                        tcc_add_include_path(s, c_abs.as_ptr());
                    }
                }
            }
        }

        // Only call tcc_add_library("m") if libm.so.6 was not already preloaded into global process memory via dlopen!
        // This completely prevents TCC from failing when /usr/lib/libm.so is a GNU ld script or in a non-standard path.
        if !LIBM_PRELOADED.load(Ordering::SeqCst) {
            let lib_m = CString::new("m").unwrap();
            let _ = tcc_add_library(s, lib_m.as_ptr());
        }

        // Explicitly include this unit's isolated comptime.h by absolute path first so #ifndef COMPTIME_H
        // locks in the unit-specific reflection tables without any possibility of cross-unit contention.
        let unit_comptime_h = unit_inc_dir.join("comptime.h");
        let full_source = format!(
            "#include \"{}\"\n#line {} \"{}\"\n{}",
            escape_c_string(&unit_comptime_h.to_string_lossy()),
            start_line.max(1),
            escape_c_string(&origin_file.to_string_lossy()),
            code
        );

        let c_code = CString::new(full_source).map_err(|e| e.to_string())?;

        if tcc_compile_string(s, c_code.as_ptr()) != 0 {
            let err_msg = TCC_LAST_ERROR
                .lock()
                .ok()
                .and_then(|mut l| l.take())
                .unwrap_or_else(|| "Unknown TCC syntax error".to_string());
            tcc_delete(s);
            return Err(format!(
                "Comptime compilation error in {}:\n{}",
                origin_file.display().to_string().cyan().bold(),
                format_comptime_diagnostic(&err_msg, code, origin_file)
            ));
        }

        if tcc_relocate(s, TCC_RELOCATE_AUTO) < 0 {
            let err_msg = TCC_LAST_ERROR
                .lock()
                .ok()
                .and_then(|mut l| l.take())
                .unwrap_or_else(|| "Undefined symbol or relocation failure".to_string());
            tcc_delete(s);
            return Err(format!(
                "Comptime linker error in {}:\n{}",
                origin_file.display().to_string().cyan().bold(),
                format_comptime_diagnostic(&err_msg, code, origin_file)
            ));
        }

        // Support either 'proc', 'main', or 'comptime_main' as the entrypoint function
        let proc_sym_name = CString::new("proc").unwrap();
        let main_sym_name = CString::new("main").unwrap();
        let ct_main_name = CString::new("comptime_main").unwrap();

        let proc_sym = tcc_get_symbol(s, proc_sym_name.as_ptr());
        let main_sym = if !proc_sym.is_null() {
            proc_sym
        } else {
            let m = tcc_get_symbol(s, main_sym_name.as_ptr());
            if !m.is_null() {
                m
            } else {
                tcc_get_symbol(s, ct_main_name.as_ptr())
            }
        };

        if main_sym.is_null() {
            tcc_delete(s);
            return Err(format!(
                "No 'int proc(void)' or 'int main(void)' entry point found in '#if comptime' block of {}",
                origin_file.display().to_string().cyan()
            ));
        }

        let get_emit_name = CString::new("__crupp_get_emitted_code").unwrap();
        let get_emit_sym = tcc_get_symbol(s, get_emit_name.as_ptr());

        let was_emit_name = CString::new("__crupp_was_emit_used").unwrap();
        let was_emit_sym = tcc_get_symbol(s, was_emit_name.as_ptr());

        let unmatched_attr_name = CString::new("__crupp_get_unmatched_struct_attr").unwrap();
        let unmatched_attr_sym = tcc_get_symbol(s, unmatched_attr_name.as_ptr());

        let struct_cnt_name = CString::new("__crupp_get_struct_count").unwrap();
        let struct_cnt_sym = tcc_get_symbol(s, struct_cnt_name.as_ptr());

        let free_emit_name = CString::new("__crupp_free_emitted_code").unwrap();
        let free_emit_sym = tcc_get_symbol(s, free_emit_name.as_ptr());

        let entry_fn: extern "C" fn() -> c_int = std::mem::transmute(main_sym);

        // Capture both stdout and stderr during entry_fn() execution
        let (captured_stdout, captured_stderr, exit_code) = capture_stdio(|| entry_fn());

        let mut emitted = String::new();
        if !get_emit_sym.is_null() {
            let get_emit_fn: extern "C" fn() -> *const c_char = std::mem::transmute(get_emit_sym);
            let ptr = get_emit_fn();
            if !ptr.is_null() {
                emitted = CStr::from_ptr(ptr).to_string_lossy().to_string();
            }
        }

        let mut was_emit_used = !emitted.is_empty();
        if !was_emit_sym.is_null() {
            let was_emit_fn: extern "C" fn() -> c_int = std::mem::transmute(was_emit_sym);
            if was_emit_fn() != 0 {
                was_emit_used = true;
            }
        }

        let mut unmatched_attr: Option<String> = None;
        if !unmatched_attr_sym.is_null() {
            let get_unmatched_fn: extern "C" fn() -> *const c_char =
                std::mem::transmute(unmatched_attr_sym);
            let ptr = get_unmatched_fn();
            if !ptr.is_null() {
                let s_attr = CStr::from_ptr(ptr).to_string_lossy().to_string();
                if !s_attr.is_empty() {
                    unmatched_attr = Some(s_attr);
                }
            }
        }

        let mut total_structs = 0;
        if !struct_cnt_sym.is_null() {
            let get_cnt_fn: extern "C" fn() -> c_int = std::mem::transmute(struct_cnt_sym);
            total_structs = get_cnt_fn();
        }

        if !free_emit_sym.is_null() {
            let free_emit_fn: extern "C" fn() = std::mem::transmute(free_emit_sym);
            free_emit_fn();
        }

        tcc_delete(s);

        // Check if the comptime code (with comments stripped!) explicitly calls emit(...) or emitf(...)
        let clean_code = crate::core::scanner::strip_comments_fast(code);
        let clean_code_calls_emit = clean_code.contains("emit(") || clean_code.contains("emitf(");

        // Determine final emitted C code vs console debug logs:
        // 1. If `emit()` or `emitf()` was called (`was_emit_used || clean_code_calls_emit`), `emitted` is the
        //    generated C code, and `captured_stdout` (from `printf`) is debug logging printed to stderr.
        // 2. If `emit()` / `emitf()` was NOT used (e.g. `tests/examples/comptime/src/main.c` using `printf`),
        //    `captured_stdout` is the generated C code! And if `captured_stdout` is empty while `captured_stderr`
        //    contains C declaration lines (e.g. `static const ...`), extract those C lines automatically!
        let total_output = if was_emit_used || clean_code_calls_emit {
            if !captured_stderr.is_empty() {
                eprint!("{}", captured_stderr);
            }
            if !captured_stdout.trim().is_empty() {
                for line in captured_stdout.lines() {
                    eprintln!("   {} {}", "[comptime]".bright_cyan().bold(), line);
                }
            }
            emitted
        } else if !captured_stdout.trim().is_empty() {
            if !captured_stderr.is_empty() {
                eprint!("{}", captured_stderr);
            }
            captured_stdout
        } else {
            // Fallback: if both `emitted` and `captured_stdout` are empty, check if `captured_stderr`
            // has C code lines mixed with progress banners (e.g. `static const float SINS[3] = ...`)
            let mut extracted_c = String::new();
            for line in captured_stderr.lines() {
                let t = line.trim();
                let looks_like_c = t.starts_with("static ")
                    || t.starts_with("const ")
                    || t.starts_with("int ")
                    || t.starts_with("float ")
                    || t.starts_with("double ")
                    || t.starts_with("void ")
                    || t.starts_with("struct ")
                    || t.starts_with("typedef ")
                    || t.starts_with("#");
                if looks_like_c {
                    extracted_c.push_str(line);
                    extracted_c.push('\n');
                } else {
                    eprintln!("{}", line);
                }
            }
            extracted_c
        };

        if let Some(attr) = unmatched_attr {
            eprintln!(
                "   {} У блоці comptime ({}) перевірявся атрибут '@{}', але жодна структура з '// @{}' не була знайдена (всього проскановано структур: {}).",
                "⚠️  [comptime]".yellow().bold(),
                origin_file.display(),
                attr,
                attr,
                total_structs
            );
        }

        if exit_code != 0 {
            return Err(format!(
                "Comptime execution in {} failed with non-zero exit code: {}{}",
                origin_file.display().to_string().cyan().bold(),
                exit_code.to_string().bright_red().bold(),
                if total_output.trim().is_empty() {
                    String::new()
                } else {
                    format!("\n  Emitted output before exit:\n    {}", total_output.trim())
                }
            ));
        }

        Ok(total_output)
    }
}
