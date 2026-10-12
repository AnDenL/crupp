use colored::*;
use std::ffi::{CStr, CString, c_char, c_int, c_void};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, Once};

pub enum TCCState {}

const TCC_OUTPUT_MEMORY: c_int = 1;
const TCC_RELOCATE_AUTO: *mut c_void = 1 as *mut c_void;

static TCC_TCCDEFS: &str = include_str!("../../vendor/tcc/include/tccdefs.h");
static TCC_STDDEF: &str = include_str!("../../vendor/tcc/include/stddef.h");
static TCC_STDARG: &str = include_str!("../../vendor/tcc/include/stdarg.h");
static TCC_STDBOOL: &str = include_str!("../../vendor/tcc/include/stdbool.h");
static TCC_FLOAT: &str = include_str!("../../vendor/tcc/include/float.h");

static INIT_RUNTIME_LIBS: Once = Once::new();

fn ensure_runtime_libraries() {
    INIT_RUNTIME_LIBS.call_once(|| unsafe {
        for lib in &["libc.so.6", "libm.so.6", "libc.so", "libm.so"] {
            let c_lib = CString::new(*lib).unwrap();
            libc::dlopen(c_lib.as_ptr(), libc::RTLD_NOW | libc::RTLD_GLOBAL);
        }
    });
}

fn prepare_embedded_headers(obj_dir: &Path) -> PathBuf {
    let tcc_home = obj_dir.join("tcc_home");
    let inc_dir = tcc_home.join("include");
    let lib_dir = tcc_home.join("lib");
    let _ = fs::create_dir_all(&inc_dir);
    let _ = fs::create_dir_all(&lib_dir);

    let _ = fs::write(inc_dir.join("tccdefs.h"), TCC_TCCDEFS);
    let _ = fs::write(inc_dir.join("stddef.h"), TCC_STDDEF);
    let _ = fs::write(inc_dir.join("stdarg.h"), TCC_STDARG);
    let _ = fs::write(inc_dir.join("stdbool.h"), TCC_STDBOOL);
    let _ = fs::write(inc_dir.join("float.h"), TCC_FLOAT);

    let empty_ar = b"!<arch>\n";
    let _ = fs::write(tcc_home.join("libtcc1.a"), empty_ar);
    let _ = fs::write(lib_dir.join("libtcc1.a"), empty_ar);

    fs::canonicalize(&tcc_home).unwrap_or_else(|_| {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(&tcc_home)
    })
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

static COMPTIME_MUTEX: Mutex<()> = Mutex::new(());
static TCC_LAST_ERROR: Mutex<Option<String>> = Mutex::new(None);

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

pub fn has_comptime_markers(content: &str) -> bool {
    content.contains("COMPTIME") || content.contains("comptime(") || content.contains("tcc(")
}

pub fn transform_comptime(
    content: &str,
    include_dirs: &[String],
    obj_dir: &Path,
    origin_file: &Path,
) -> Result<String, String> {
    if !has_comptime_markers(content) {
        return Ok(content.to_string());
    }

    ensure_runtime_libraries();
    let tcc_home = prepare_embedded_headers(obj_dir);
    let mut result = content.to_string();

    while let Some((start_idx, end_idx, comptime_code)) = find_next_comptime_block(&result) {
        let generated_c = execute_c_code(&comptime_code, include_dirs, &tcc_home, origin_file)?;
        result.replace_range(start_idx..end_idx, &generated_c);
    }

    while let Some((start_idx, end_idx, expr)) = find_next_comptime_expr(&result) {
        let wrapped_code = format!(
            r#"
            #include <stdio.h>
            #include <stdint.h>
            #include <math.h>

            int main(void) {{
                #define _crupp_print_val(x) _Generic((x), \
                    float: printf("%ff", (double)(x)), \
                    double: printf("%f", (double)(x)), \
                    char*: printf("\"%s\"", (char*)(x)), \
                    const char*: printf("\"%s\"", (const char*)(x)), \
                    default: printf("%lld", (long long)(x)) \
                )
                _crupp_print_val({expr});
                return 0;
            }}
            "#
        );

        let generated_val = execute_c_code(&wrapped_code, include_dirs, &tcc_home, origin_file)?;
        result.replace_range(start_idx..end_idx, generated_val.trim());
    }

    Ok(result)
}

fn execute_c_code(
    code: &str,
    include_dirs: &[String],
    tcc_home: &Path,
    origin_file: &Path,
) -> Result<String, String> {
    let _guard = COMPTIME_MUTEX.lock().unwrap();

    if let Ok(mut lock) = TCC_LAST_ERROR.lock() {
        *lock = None;
    }

    unsafe {
        let s = tcc_new();
        if s.is_null() {
            return Err("Failed to create TCC context in memory".to_string());
        }

        tcc_set_error_func(s, std::ptr::null_mut(), tcc_error_callback);

        let c_tcc_home = CString::new(tcc_home.to_string_lossy().as_bytes()).unwrap();
        tcc_set_lib_path(s, c_tcc_home.as_ptr());

        let inc_dir = tcc_home.join("include");
        let c_inc = CString::new(inc_dir.to_string_lossy().as_bytes()).unwrap();
        tcc_add_sysinclude_path(s, c_inc.as_ptr());

        tcc_set_output_type(s, TCC_OUTPUT_MEMORY);

        let sym_name = CString::new("COMPTIME").unwrap();
        let sym_val = CString::new("1").unwrap();
        tcc_define_symbol(s, sym_name.as_ptr(), sym_val.as_ptr());

        for sys_path in &[
            "/usr/include",
            "/usr/local/include",
            "/usr/include/x86_64-linux-gnu",
        ] {
            if Path::new(sys_path).exists() {
                let c_sys = CString::new(*sys_path).unwrap();
                tcc_add_sysinclude_path(s, c_sys.as_ptr());
            }
        }

        // Додаємо системні шляхи для лінкування бібліотек
        for lib_path in &[
            "/usr/lib",
            "/usr/lib64",
            "/lib",
            "/lib64",
            "/usr/lib/x86_64-linux-gnu",
        ] {
            if Path::new(lib_path).exists() {
                let c_lib = CString::new(*lib_path).unwrap();
                tcc_add_library_path(s, c_lib.as_ptr());
            }
        }

        for inc in include_dirs {
            let c_inc = CString::new(inc.as_str()).unwrap_or_default();
            tcc_add_include_path(s, c_inc.as_ptr());
        }
        if let Some(parent) = origin_file.parent() {
            let c_parent = CString::new(parent.to_string_lossy().as_bytes()).unwrap_or_default();
            tcc_add_include_path(s, c_parent.as_ptr());
        }

        let lib_m = CString::new("m").unwrap();
        tcc_add_library(s, lib_m.as_ptr());

        let c_code = CString::new(code).map_err(|e| e.to_string())?;

        if tcc_compile_string(s, c_code.as_ptr()) != 0 {
            let err_msg = TCC_LAST_ERROR
                .lock()
                .ok()
                .and_then(|mut l| l.take())
                .unwrap_or_else(|| "Unknown TCC syntax error".to_string());
            tcc_delete(s);
            return Err(format!(
                "Comptime syntax error in {}:\n  {}",
                origin_file.display().to_string().cyan(),
                err_msg.bright_red()
            ));
        }

        if tcc_relocate(s, TCC_RELOCATE_AUTO) < 0 {
            let err_msg = TCC_LAST_ERROR
                .lock()
                .ok()
                .and_then(|mut l| l.take())
                .unwrap_or_else(|| "Undefined symbol or memory relocation error".to_string());
            tcc_delete(s);
            return Err(format!(
                "Comptime relocation error in {}:\n  {}",
                origin_file.display().to_string().cyan(),
                err_msg.bright_red()
            ));
        }

        let main_sym_name = CString::new("main").unwrap();
        let main_sym = tcc_get_symbol(s, main_sym_name.as_ptr());

        if main_sym.is_null() {
            tcc_delete(s);
            return Err(format!(
                "No 'int main(void)' found in comptime block of {}",
                origin_file.display()
            ));
        }

        let entry_fn: extern "C" fn() -> c_int = std::mem::transmute(main_sym);

        let output = capture_stdout(|| {
            let code = entry_fn();
            if code != 0 {
                Err(format!(
                    "Comptime main() returned non-zero exit code: {}",
                    code
                ))
            } else {
                Ok(())
            }
        });

        tcc_delete(s);
        output
    }
}

fn capture_stdout<F: FnOnce() -> Result<(), String>>(f: F) -> Result<String, String> {
    unsafe {
        libc::fflush(std::ptr::null_mut());

        let saved_stdout = libc::dup(libc::STDOUT_FILENO);
        if saved_stdout < 0 {
            return Err("Failed to duplicate stdout descriptor".to_string());
        }

        let tmp = libc::tmpfile();
        if tmp.is_null() {
            libc::close(saved_stdout);
            return Err("Failed to create temporary buffer for comptime stdout".to_string());
        }
        let tmp_fd = libc::fileno(tmp);

        libc::dup2(tmp_fd, libc::STDOUT_FILENO);

        let execution_result = f();

        libc::fflush(std::ptr::null_mut());
        libc::dup2(saved_stdout, libc::STDOUT_FILENO);
        libc::close(saved_stdout);

        libc::rewind(tmp);
        let mut captured_bytes = Vec::new();
        let mut buf = [0u8; 8192];
        loop {
            let n = libc::fread(buf.as_mut_ptr() as *mut libc::c_void, 1, buf.len(), tmp);
            if n == 0 {
                break;
            }
            captured_bytes.extend_from_slice(&buf[..n]);
        }
        libc::fclose(tmp);

        execution_result?;

        String::from_utf8(captured_bytes)
            .map_err(|e| format!("Comptime generated output is not valid UTF-8: {}", e))
    }
}

fn find_next_comptime_block(source: &str) -> Option<(usize, usize, String)> {
    let lower = source.to_lowercase();
    let marker_candidates = ["#if comptime", "#ifdef comptime", "#if defined(comptime)"];

    let mut found_start = None;
    let mut marker_len = 0;

    for marker in &marker_candidates {
        if let Some(idx) = lower.find(marker) {
            if found_start.map_or(true, |curr| idx < curr) {
                found_start = Some(idx);
                marker_len = marker.len();
            }
        }
    }

    let start_idx = found_start?;
    let body_start = source[start_idx + marker_len..]
        .find('\n')
        .map(|nl| start_idx + marker_len + nl + 1)?;

    let mut depth = 1;
    let mut current_idx = body_start;
    let mut else_pos = None;

    while depth > 0 && current_idx < source.len() {
        let slice = &source[current_idx..];
        if let Some(nl_rel) = slice.find('\n') {
            let line = slice[..nl_rel].trim();
            if line.starts_with("#if") {
                depth += 1;
            } else if depth == 1 && (line.starts_with("#else") || line.starts_with("#elif")) {
                else_pos = Some(current_idx);
            } else if line.starts_with("#endif") {
                depth -= 1;
                if depth == 0 {
                    let end_idx = current_idx + nl_rel + 1;
                    let comptime_src = if let Some(e_idx) = else_pos {
                        source[body_start..e_idx].to_string()
                    } else {
                        source[body_start..current_idx].to_string()
                    };
                    return Some((start_idx, end_idx, comptime_src));
                }
            }
            current_idx += nl_rel + 1;
        } else {
            break;
        }
    }

    None
}

fn find_next_comptime_expr(source: &str) -> Option<(usize, usize, String)> {
    let tags = ["comptime(", "tcc("];
    let mut search_from = 0;

    while search_from < source.len() {
        let mut found = None;

        for tag in &tags {
            if let Some(idx) = source[search_from..].find(tag) {
                let abs_idx = search_from + idx;
                if found.map_or(true, |(curr, _)| abs_idx < curr) {
                    found = Some((abs_idx, tag.len()));
                }
            }
        }

        let (start_pos, tag_len) = found?;
        let expr_start = start_pos + tag_len;

        let line_start = source[..start_pos].rfind('\n').map(|i| i + 1).unwrap_or(0);
        let line_prefix = source[line_start..start_pos].trim();
        if line_prefix.starts_with('#') {
            search_from = expr_start;
            continue;
        }

        let mut depth = 1;
        let mut end_pos = expr_start;

        for (i, c) in source[expr_start..].char_indices() {
            match c {
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 {
                        end_pos = expr_start + i;
                        break;
                    }
                }
                _ => {}
            }
        }

        if depth == 0 {
            let expr = source[expr_start..end_pos].trim().to_string();
            return Some((start_pos, end_pos + 1, expr));
        } else {
            search_from = expr_start;
        }
    }

    None
}
