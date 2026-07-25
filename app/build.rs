fn main() {
    slint_build::compile("ui/app.slint").expect("slint build failed");
    // Embed the app icon into the exe (taskbar / title bar / explorer).
    embed_resource::compile("app.rc", embed_resource::NONE);
}
