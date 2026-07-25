fn main() {
    slint_build::compile("ui/app.slint").expect("slint build failed");

    // Embed the icon, manifest and version info. Without these rerun hints Cargo caches the build
    // script's output and keeps embedding a stale manifest, so changes to DPI awareness silently
    // never reach the executable.
    println!("cargo:rerun-if-changed=app.rc");
    println!("cargo:rerun-if-changed=app.manifest");
    println!("cargo:rerun-if-changed=icons/icon.ico");
    let _ = embed_resource::compile("app.rc", embed_resource::NONE);
}
