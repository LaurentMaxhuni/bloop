//! Embeds the mascot icon (Explorer, Start Menu, Task Manager) and a version resource whose
//! product name and version come from Cargo.toml, so signing services that check them (see
//! `docs/code-signing.md`) see the same values in every build.

fn main() {
    println!("cargo:rerun-if-changed=installer/art/bloop.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let version = std::env::var("CARGO_PKG_VERSION").expect("cargo sets the package version");
    let mut parts = version
        .split(['.', '-', '+'])
        .map(|part| part.parse::<u16>().unwrap_or(0));
    let mut next = || parts.next().unwrap_or(0);
    let (major, minor, patch) = (next(), next(), next());
    let root = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let icon = root.join("installer/art/bloop.ico");
    let icon = icon.display().to_string().replace('\\', "\\\\");
    let rc = format!(
        r#"1 ICON "{icon}"

1 VERSIONINFO
FILEVERSION {major},{minor},{patch},0
PRODUCTVERSION {major},{minor},{patch},0
FILEOS 0x40004
FILETYPE 0x1
BEGIN
  BLOCK "StringFileInfo"
  BEGIN
    BLOCK "040904B0"
    BEGIN
      VALUE "CompanyName", "Laurent Maxhuni"
      VALUE "FileDescription", "bloop"
      VALUE "FileVersion", "{version}"
      VALUE "InternalName", "bloop"
      VALUE "LegalCopyright", "Copyright (c) 2026 bloop contributors. MIT License."
      VALUE "OriginalFilename", "bloop.exe"
      VALUE "ProductName", "bloop"
      VALUE "ProductVersion", "{version}"
    END
  END
  BLOCK "VarFileInfo"
  BEGIN
    VALUE "Translation", 0x409, 1200
  END
END
"#
    );
    // `embed-resource` is a Windows-only build dependency, so this part only exists in build
    // scripts compiled on Windows (Windows builds are never cross-compiled).
    #[cfg(windows)]
    {
        let out = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("bloop.rc");
        std::fs::write(&out, rc).expect("write the resource script");
        embed_resource::compile(&out, embed_resource::NONE)
            .manifest_optional()
            .unwrap();
    }
    #[cfg(not(windows))]
    let _ = rc;
}
