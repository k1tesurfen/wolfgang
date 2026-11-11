//! Two jobs:
//!
//! 1. Which character this build is (`WOLFGANG_CHARACTER`: `wolfgang`, `ully` or
//!    `burgunde`, default `wolfgang`; the list is `characters.json` at the repo root).
//!    Passed on to the code as `env!("WOLFGANG_CHARACTER")`, see `src/character.rs`.
//! 2. whisper.cpp's Metal code checks the macOS version at runtime (`@available`), which
//!    needs `__isPlatformVersionAtLeast` from clang's runtime library. rustc links with
//!    `-nodefaultlibs`, so that library is added here; only that one function is taken from it.

fn main() {
    println!("cargo:rerun-if-env-changed=WOLFGANG_CHARACTER");
    println!("cargo:rerun-if-changed=../../characters.json");
    let key = std::env::var("WOLFGANG_CHARACTER").unwrap_or_else(|_| "wolfgang".into());
    let list = std::fs::read_to_string("../../characters.json").expect("characters.json at the repo root");
    assert!(list.contains(&format!("\"key\": \"{key}\"")), "WOLFGANG_CHARACTER={key} isn't in characters.json");
    println!("cargo:rustc-env=WOLFGANG_CHARACTER={key}");

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("macos") {
        return;
    }
    let out = std::process::Command::new("xcrun").args(["clang", "--print-runtime-dir"]).output().expect("xcrun clang (Xcode command line tools)");
    let dir = String::from_utf8(out.stdout).expect("utf-8 path");
    println!("cargo:rustc-link-search=native={}", dir.trim());
    println!("cargo:rustc-link-lib=static=clang_rt.osx");
}
