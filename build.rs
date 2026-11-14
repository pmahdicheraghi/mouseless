//! Embeds the icons, the version information and the manifest into the executable.

use std::path::Path;

fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let root = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let out = std::env::var("OUT_DIR").unwrap();
    let asset = |name: &str| Path::new(&root).join("assets").join(name).display().to_string().replace('\\', "/");
    for f in ["mouseless.ico", "mouseless-active.ico", "mouseless-paused.ico", "mouseless.manifest"] {
        println!("cargo:rerun-if-changed=assets/{f}");
    }

    let version = std::env::var("CARGO_PKG_VERSION").unwrap();
    let mut parts: Vec<u32> = version.split(['.', '-']).filter_map(|p| p.parse().ok()).collect();
    parts.resize(4, 0);
    let numeric = parts[..4].iter().map(|p| p.to_string()).collect::<Vec<_>>().join(",");

    // icon 1 is the icon of the executable, 2 and 3 are used for the tray icon
    let rc = format!(
        r#"1 ICON "{normal}"
2 ICON "{active}"
3 ICON "{paused}"
1 24 "{manifest}"

1 VERSIONINFO
FILEVERSION {numeric}
PRODUCTVERSION {numeric}
FILEOS 0x40004
FILETYPE 0x1
BEGIN
  BLOCK "StringFileInfo"
  BEGIN
    BLOCK "040904b0"
    BEGIN
      VALUE "CompanyName", "mouseless"
      VALUE "FileDescription", "mouseless - control the mouse with the keyboard"
      VALUE "FileVersion", "{version}"
      VALUE "InternalName", "mouseless"
      VALUE "LegalCopyright", "MIT License"
      VALUE "OriginalFilename", "mouseless.exe"
      VALUE "ProductName", "mouseless"
      VALUE "ProductVersion", "{version}"
    END
  END
  BLOCK "VarFileInfo"
  BEGIN
    VALUE "Translation", 0x409, 1200
  END
END
"#,
        normal = asset("mouseless.ico"),
        active = asset("mouseless-active.ico"),
        paused = asset("mouseless-paused.ico"),
        manifest = asset("mouseless.manifest"),
    );
    let rc_path = Path::new(&out).join("resources.rc");
    std::fs::write(&rc_path, rc).unwrap();
    embed_resource::compile(&rc_path, embed_resource::NONE).manifest_required().unwrap();
}
