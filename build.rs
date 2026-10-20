fn main() {
    println!("cargo:rerun-if-changed=assets/lumen.rc");
    println!("cargo:rerun-if-changed=assets/lumen.ico");
    println!("cargo:rerun-if-changed=assets/lumen-paused.ico");
    embed_resource::compile("assets/lumen.rc", embed_resource::NONE)
        .manifest_required()
        .expect("compile assets/lumen.rc (needs the Windows SDK resource compiler)");
}
