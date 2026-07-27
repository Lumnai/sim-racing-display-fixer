use std::path::PathBuf;

fn main() {
    slint_build::compile("ui/app.slint").expect("slint build failed");

    // Embed the icon, manifest and version info. Without these rerun hints Cargo caches the build
    // script's output and keeps embedding a stale manifest, so changes to DPI awareness silently
    // never reach the executable.
    println!("cargo:rerun-if-changed=app.manifest");
    println!("cargo:rerun-if-changed=icons/icon.ico");
    println!("cargo:rerun-if-changed=build.rs");

    let dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap());

    // The resource script is generated rather than checked in and hand-edited. As a file its
    // version sat at 1.0.5 for four releases while Cargo.toml moved on, so every exe reported the
    // wrong version in its properties. Deriving it here means it cannot drift again.
    let version = std::env::var("CARGO_PKG_VERSION").unwrap();
    let mut parts: Vec<String> = version.split('.').map(str::to_string).collect();
    parts.resize(4, "0".to_string());
    let comma = parts.join(",");
    let dotted = parts.join(".");

    // A resource script resolves paths relative to itself, and this one lives in OUT_DIR, so the
    // files it pulls in have to be absolute. Backslashes are escaped for the RC compiler.
    let esc = |p: PathBuf| p.display().to_string().replace('\\', "\\\\");
    let icon = esc(dir.join("icons/icon.ico"));
    let manifest = esc(dir.join("app.manifest"));

    let rc = format!(
        r#"1 ICON "{icon}"
1 24 "{manifest}"

1 VERSIONINFO
FILEVERSION {comma}
PRODUCTVERSION {comma}
FILEOS 0x4L
FILETYPE 0x1L
BEGIN
    BLOCK "StringFileInfo"
    BEGIN
        BLOCK "040904b0"
        BEGIN
            VALUE "CompanyName", "Lunis"
            VALUE "FileDescription", "Sim Display Fixer"
            VALUE "FileVersion", "{dotted}"
            VALUE "InternalName", "Sim Display Fixer"
            VALUE "LegalCopyright", "Copyright (c) 2026 Lunis"
            VALUE "OriginalFilename", "sim-display-fixer.exe"
            VALUE "ProductName", "Sim Display Fixer"
            VALUE "ProductVersion", "{dotted}"
        END
    END
    BLOCK "VarFileInfo"
    BEGIN
        VALUE "Translation", 0x409, 1200
    END
END
"#
    );

    let rc_path = out.join("app.rc");
    std::fs::write(&rc_path, rc).expect("could not write the generated resource script");
    embed_resource::compile(&rc_path, embed_resource::NONE)
        .manifest_required()
        .expect("resource compilation failed: icon, manifest and version info would all be missing");
}
