use colored::*;
use std::ffi::{CStr, CString, c_char, c_int, c_void};
use std::fs;
use std::path::{Path, PathBuf};
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
static COMPTIME_MUTEX: Mutex<()> = Mutex::new(());
static TCC_LAST_ERROR: Mutex<Option<String>> = Mutex::new(None);

fn ensure_runtime_libraries() {
    INIT_RUNTIME_LIBS.call_once(|| unsafe {
        for lib in &["libc.so.6", "libm.so.6", "libc.so", "libm.so"] {
            if let Ok(c_lib) = CString::new(*lib) {
                libc::dlopen(c_lib.as_ptr(), libc::RTLD_NOW | libc::RTLD_GLOBAL);
            }
        }

        // Also probe discovered NixOS / system library directories for libc.so.6 and libm.so.6
        let (c_compiler, _) = crate::config::CompilerType::default().resolve_c_compiler();
        for dir in crate::core::nix::discover_system_library_dirs(&c_compiler) {
            for lib in &["libc.so.6", "libm.so.6"] {
                let candidate = dir.join(lib);
                if candidate.exists() {
                    if let Ok(c_path) = CString::new(candidate.to_string_lossy().as_bytes()) {
                        libc::dlopen(c_path.as_ptr(), libc::RTLD_NOW | libc::RTLD_GLOBAL);
                    }
                }
            }
        }
    });
}

fn sanitize_unit_id(origin_file: &Path) -> String {
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

fn format_comptime_diagnostic(raw_err: &str, origin_file: &Path) -> String {
    let mut formatted = String::new();
    for line in raw_err.lines() {
        formatted.push_str("  ");
        formatted.push_str(&line.bright_red().to_string());
        formatted.push('\n');

        // Try to extract line number if the error points to origin_file
        let file_str = origin_file.to_string_lossy();
        if let Some(rest) = line.strip_prefix(file_str.as_ref()) {
            if let Some(after_colon) = rest.strip_prefix(':') {
                let digits: String = after_colon
                    .chars()
                    .take_while(|c| c.is_ascii_digit())
                    .collect();
                if let Ok(line_num) = digits.parse::<usize>() {
                    if let Ok(src) = fs::read_to_string(origin_file) {
                        if let Some(src_line) = src.lines().nth(line_num.saturating_sub(1)) {
                            formatted.push_str(&format!(
                                "    {} | {}\n",
                                line_num.to_string().cyan(),
                                src_line.yellow()
                            ));
                        }
                    }
                }
            }
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

        if let Some(parent) = origin_file.parent() {
            if let Ok(c_parent_rel) = CString::new(parent.to_string_lossy().as_bytes()) {
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
        }

        let lib_m = CString::new("m").unwrap();
        let _ = tcc_add_library(s, lib_m.as_ptr());
        if let Ok(mut lock) = TCC_LAST_ERROR.lock() {
            *lock = None;
        }

        // Automatically include <comptime.h> and synchronize line numbers to origin_file
        let full_source = format!(
            "#include <comptime.h>\n#line {} \"{}\"\n{}",
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
                format_comptime_diagnostic(&err_msg, origin_file)
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
                "Comptime linker error in {}:\n  {}\n  💡 Tip: Functions called inside '#if comptime' must be defined in <comptime.h>, standard C headers, or inline within the comptime block.",
                origin_file.display().to_string().cyan().bold(),
                err_msg.bright_red()
            ));
        }

        let main_sym_name = CString::new("proc").unwrap();
        let main_sym = tcc_get_symbol(s, main_sym_name.as_ptr());

        if main_sym.is_null() {
            tcc_delete(s);
            return Err(format!(
                "No 'int main(void)' entry point found in '#if comptime' block of {}",
                origin_file.display().to_string().cyan()
            ));
        }

        let get_emit_name = CString::new("__crupp_get_emitted_code").unwrap();
        let get_emit_sym = tcc_get_symbol(s, get_emit_name.as_ptr());

        let free_emit_name = CString::new("__crupp_free_emitted_code").unwrap();
        let free_emit_sym = tcc_get_symbol(s, free_emit_name.as_ptr());

        let entry_fn: extern "C" fn() -> c_int = std::mem::transmute(main_sym);
        let exit_code = entry_fn();

        // Flush any user debug printf() calls to console
        libc::fflush(std::ptr::null_mut());

        let mut emitted = String::new();
        if !get_emit_sym.is_null() {
            let get_emit_fn: extern "C" fn() -> *const c_char = std::mem::transmute(get_emit_sym);
            let ptr = get_emit_fn();
            if !ptr.is_null() {
                emitted = CStr::from_ptr(ptr).to_string_lossy().to_string();
            }
        }

        if !free_emit_sym.is_null() {
            let free_emit_fn: extern "C" fn() = std::mem::transmute(free_emit_sym);
            free_emit_fn();
        }

        tcc_delete(s);

        if exit_code != 0 {
            return Err(format!(
                "Comptime 'main()' in {} failed with non-zero exit code: {}{}",
                origin_file.display().to_string().cyan().bold(),
                exit_code.to_string().bright_red().bold(),
                if emitted.trim().is_empty() {
                    String::new()
                } else {
                    format!("\n  Emitted output before exit:\n    {}", emitted.trim())
                }
            ));
        }

        Ok(emitted)
    }
}
