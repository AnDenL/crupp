use std::env;
use std::fs;
use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-changed=vendor/tcc");

    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());

    let config_h = out_dir.join("config.h");
    let config_content = r#"
#ifndef CONFIG_H
#define CONFIG_H
#define TCC_VERSION "0.9.27"
#endif
"#;
    fs::write(&config_h, config_content).expect("Failed to write config.h for TCC");

    cc::Build::new()
        .file("vendor/tcc/libtcc.c")
        .include(&out_dir)
        .define("ONE_SOURCE", "1")
        .define("TCC_TARGET_X86_64", "1")
        .define("CONFIG_TCCDIR", "\"\"")
        .warnings(false)
        .compile("tcc");

    println!("cargo:rustc-link-lib=static=tcc");
    println!("cargo:rustc-link-lib=dl");
    println!("cargo:rustc-link-lib=m");

    println!("cargo:rustc-link-arg=-rdynamic");
}
