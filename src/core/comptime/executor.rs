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

fn ensure_runtime_libraries() {
    INIT_RUNTIME_LIBS.call_once(|| unsafe {
        for lib in &["libc.so.6", "libm.so.6", "libc.so", "libm.so"] {
            let c_lib = CString::new(*lib).unwrap();
            libc::dlopen(c_lib.as_ptr(), libc::RTLD_NOW | libc::RTLD_GLOBAL);
        }
    });
}

pub fn prepare_embedded_headers(
    obj_dir: &Path,
    custom_comptime_h: Option<&str>,
) -> PathBuf {
    let tcc_home = obj_dir.join("tcc_home");
    let inc_dir = tcc_home.join("include");
    let crupp_inc_dir = inc_dir.join("crupp");
    let lib_dir = tcc_home.join("lib");
    let marker = inc_dir.join("tccdefs.h");

    let _ = fs::create_dir_all(&crupp_inc_dir);
    let _ = fs::create_dir_all(&lib_dir);

    // Provide clean <comptime.h> as well as <crupp/comptime.h>
    if let Some(content) = custom_comptime_h {
        let _ = fs::write(inc_dir.join("comptime.h"), content);
        let _ = fs::write(crupp_inc_dir.join("comptime.h"), content);
        let _ = fs::write(inc_dir.join("crupp_comptime.h"), content);
    }

    if !marker.exists() {
        let _ = fs::write(inc_dir.join("tccdefs.h"), TCC_TCCDEFS);
        let _ = fs::write(inc_dir.join("stddef.h"), TCC_STDDEF);
        let _ = fs::write(inc_dir.join("stdarg.h"), TCC_STDARG);
        let _ = fs::write(inc_dir.join("stdbool.h"), TCC_STDBOOL);
        let _ = fs::write(inc_dir.join("float.h"), TCC_FLOAT);

        let empty_ar = b"!<arch>\n";
        let _ = fs::write(tcc_home.join("libtcc1.a"), empty_ar);
        let _ = fs::write(lib_dir.join("libtcc1.a"), empty_ar);
    }

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

pub fn execute_c_code(
    code: &str,
    include_dirs: &[String],
    tcc_home: &Path,
    origin_file: &Path,
) -> Result<String, String> {
    ensure_runtime_libraries();
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
            "/usr/include/aarch64-linux-gnu",
        ] {
            if Path::new(sys_path).exists() {
                let c_sys = CString::new(*sys_path).unwrap();
                tcc_add_sysinclude_path(s, c_sys.as_ptr());
            }
        }

        for lib_path in &[
            "/usr/lib",
            "/usr/lib64",
            "/lib",
            "/lib64",
            "/usr/lib/x86_64-linux-gnu",
            "/usr/lib/aarch64-linux-gnu",
        ] {
            if Path::new(lib_path).exists() {
                let c_lib = CString::new(*lib_path).unwrap();
                tcc_add_library_path(s, c_lib.as_ptr());
            }
        }

        // Add project root and CWD to TCC includes
        let c_dot = CString::new(".").unwrap();
        tcc_add_include_path(s, c_dot.as_ptr());

        let current_dir = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let c_cwd = CString::new(current_dir.to_string_lossy().as_bytes()).unwrap_or_default();
        tcc_add_include_path(s, c_cwd.as_ptr());

        for inc in include_dirs {
            let clean = inc.trim().trim_start_matches("-I").trim();
            if !clean.is_empty() {
                let c_inc = CString::new(clean).unwrap_or_default();
                tcc_add_include_path(s, c_inc.as_ptr());

                let abs_inc = current_dir.join(clean);
                if abs_inc.is_dir() {
                    let c_abs = CString::new(abs_inc.to_string_lossy().as_bytes()).unwrap_or_default();
                    tcc_add_include_path(s, c_abs.as_ptr());
                }
            }
        }

        if let Some(parent) = origin_file.parent() {
            let c_parent_rel = CString::new(parent.to_string_lossy().as_bytes()).unwrap_or_default();
            tcc_add_include_path(s, c_parent_rel.as_ptr());

            let abs_parent = if parent.is_relative() {
                current_dir.join(parent)
            } else {
                parent.to_path_buf()
            };
            let c_parent_abs = CString::new(abs_parent.to_string_lossy().as_bytes()).unwrap_or_default();
            tcc_add_include_path(s, c_parent_abs.as_ptr());
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

struct StdoutGuard {
    saved_stdout: libc::c_int,
}

impl Drop for StdoutGuard {
    fn drop(&mut self) {
        unsafe {
            libc::fflush(std::ptr::null_mut());
            if self.saved_stdout >= 0 {
                libc::dup2(self.saved_stdout, libc::STDOUT_FILENO);
                libc::close(self.saved_stdout);
            }
        }
    }
}

fn capture_stdout<F: FnOnce() -> Result<(), String>>(f: F) -> Result<String, String> {
    unsafe {
        libc::fflush(std::ptr::null_mut());

        let saved = libc::dup(libc::STDOUT_FILENO);
        if saved < 0 {
            return Err("Failed to duplicate stdout descriptor".to_string());
        }

        let _guard = StdoutGuard { saved_stdout: saved };

        let tmp = libc::tmpfile();
        if tmp.is_null() {
            return Err("Failed to create temporary buffer for comptime stdout".to_string());
        }
        let tmp_fd = libc::fileno(tmp);

        libc::dup2(tmp_fd, libc::STDOUT_FILENO);

        let execution_result = f();

        libc::fflush(std::ptr::null_mut());
        libc::dup2(saved, libc::STDOUT_FILENO);

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
