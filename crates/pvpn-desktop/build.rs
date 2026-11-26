use std::path::PathBuf;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=../../assets/brand/desktop/pvpn.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows")
        || std::env::var("CARGO_CFG_TARGET_ENV").as_deref() != Ok("gnu")
    {
        return;
    }

    // winres links its .rsrc through a static archive, which GNU ld can drop
    // because no symbol from that object is referenced. Compile the resource
    // and pass the object file to the linker directly instead.
    let root = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let icon = root.join("../../assets/brand/desktop/pvpn.ico");
    let icon = std::fs::canonicalize(&icon).expect("canonicalize pvpn.ico");
    let mut icon_text = icon.to_string_lossy().into_owned();
    if let Some(stripped) = icon_text.strip_prefix(r"\\?\") {
        icon_text = stripped.to_string();
    }
    let escaped = icon_text.replace('\\', "\\\\");

    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let rc = out.join("pvpn-icon.rc");
    let obj = out.join("pvpn-icon.o");
    std::fs::write(&rc, format!("1 ICON \"{escaped}\"\n")).expect("write icon.rc");

    let status = Command::new("windres")
        .arg("--input")
        .arg(&rc)
        .arg("--output")
        .arg(&obj)
        .arg("--output-format=coff")
        .status()
        .expect("run windres (MinGW bin must be on PATH)");
    assert!(status.success(), "windres failed");
    println!("cargo:rustc-link-arg={}", obj.display());
}
