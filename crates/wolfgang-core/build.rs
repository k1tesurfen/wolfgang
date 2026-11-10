//! whisper.cpp's Metal code checks the macOS version at runtime (`@available`), which
//! needs `__isPlatformVersionAtLeast` from clang's runtime library. rustc links with
//! `-nodefaultlibs`, so that library is added here; only that one function is taken from it.

fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("macos") {
        return;
    }
    let out = std::process::Command::new("xcrun").args(["clang", "--print-runtime-dir"]).output().expect("xcrun clang (Xcode command line tools)");
    let dir = String::from_utf8(out.stdout).expect("utf-8 path");
    println!("cargo:rustc-link-search=native={}", dir.trim());
    println!("cargo:rustc-link-lib=static=clang_rt.osx");
}
