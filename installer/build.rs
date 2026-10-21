//! Embeds the built ezkey.exe and gives the installer an "asInvoker"
//! manifest: without one, Windows' installer detection asks for admin
//! rights because the file name contains "setup".

use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-env-changed=EZKEY_EXE");
    let exe = std::env::var_os("EZKEY_EXE")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../target/release/ezkey.exe")
        });
    if !exe.exists() {
        panic!(
            "{} not found; build it first: cargo build --release -p ezkey-win",
            exe.display()
        );
    }
    println!("cargo:rerun-if-changed={}", exe.display());
    println!("cargo:rustc-env=EZKEY_EXE_PATH={}", exe.display());

    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        println!("cargo:rustc-link-arg-bins=/MANIFEST:EMBED");
        println!("cargo:rustc-link-arg-bins=/MANIFESTUAC:level='asInvoker' uiAccess='false'");
    }
}
