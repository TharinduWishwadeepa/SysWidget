fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        // Icon + manifest (run as admin, per-monitor DPI, segment heap) + version info
        embed_resource::compile("syswidget.rc", embed_resource::NONE)
            .manifest_required()
            .unwrap();
    }
    println!("cargo:rerun-if-changed=syswidget.rc");
    println!("cargo:rerun-if-changed=assets/syswidget.ico");
    println!("cargo:rerun-if-changed=assets/app.manifest");
}
