fn main() {
    println!("cargo:rerun-if-changed=assets/lumen.rc");
    println!("cargo:rerun-if-changed=assets/lumen.ico");
    println!("cargo:rerun-if-changed=assets/lumen-paused.ico");
    // The icons are Win32 resources. The Linux tray embeds the .ico files
    // directly (`include_bytes!`), so no resource compiler is needed there.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    embed_resource::compile("assets/lumen.rc", embed_resource::NONE)
        .manifest_required()
        .expect("compile assets/lumen.rc (needs the Windows SDK resource compiler)");
}
