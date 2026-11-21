// Build information and, on Windows, the program icon and version resource.
// - YAGUI_COMMIT / YAGUI_COMMIT_DATE: the Git commit the program was built from (shown in About
//   and by `yagui --version`); "unknown" outside a Git checkout.
// - Windows: the icon (same picture as the window, src/appicon.rs) and a VERSIONINFO block, so
//   Explorer's file properties and "Apps & features" show name, version and publisher.

include!("src/appicon.rs");

fn git(args: &[&str]) -> Option<String> {
    let o = std::process::Command::new("git").args(args).output().ok()?;
    if !o.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&o.stdout).trim().to_string();
    (!s.is_empty()).then_some(s)
}

fn main() {
    println!("cargo:rerun-if-changed=src/appicon.rs");
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=.git/HEAD");
    println!("cargo:rerun-if-changed=.git/refs/heads");
    let commit = std::env::var("YAGUI_COMMIT").ok().or_else(|| git(&["rev-parse", "--short=8", "HEAD"])).unwrap_or_else(|| "unknown".into());
    let date = git(&["log", "-1", "--format=%cs"]).unwrap_or_else(|| "unknown".into());
    println!("cargo:rustc-env=YAGUI_COMMIT={commit}");
    println!("cargo:rustc-env=YAGUI_COMMIT_DATE={date}");

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let out = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let sizes = [16u32, 24, 32, 48, 64, 128, 256];
    let mut pngs: Vec<Vec<u8>> = Vec::new();
    for &s in &sizes {
        let mut buf = Vec::new();
        let img = image::RgbaImage::from_raw(s, s, icon_pixels(s)).unwrap();
        img.write_to(&mut std::io::Cursor::new(&mut buf), image::ImageFormat::Png).unwrap();
        pngs.push(buf);
    }
    // ICO container with PNG-compressed images.
    let mut ico = Vec::new();
    ico.extend_from_slice(&[0, 0, 1, 0]);
    ico.extend_from_slice(&(sizes.len() as u16).to_le_bytes());
    let mut offset = 6 + 16 * sizes.len() as u32;
    for (i, &s) in sizes.iter().enumerate() {
        let b = if s >= 256 { 0u8 } else { s as u8 };
        ico.extend_from_slice(&[b, b, 0, 0]);
        ico.extend_from_slice(&1u16.to_le_bytes());
        ico.extend_from_slice(&32u16.to_le_bytes());
        ico.extend_from_slice(&(pngs[i].len() as u32).to_le_bytes());
        ico.extend_from_slice(&offset.to_le_bytes());
        offset += pngs[i].len() as u32;
    }
    for p in &pngs {
        ico.extend_from_slice(p);
    }
    let ico_path = out.join("yagui.ico");
    std::fs::write(&ico_path, ico).unwrap();

    let v = |k: &str| std::env::var(k).unwrap_or_else(|_| "0".into());
    let (ma, mi, pa) = (v("CARGO_PKG_VERSION_MAJOR"), v("CARGO_PKG_VERSION_MINOR"), v("CARGO_PKG_VERSION_PATCH"));
    let ver = std::env::var("CARGO_PKG_VERSION").unwrap_or_default();
    let rc = out.join("yagui.rc");
    let text = format!(
        r#"1 ICON "{ico}"
1 VERSIONINFO
FILEVERSION {ma},{mi},{pa},0
PRODUCTVERSION {ma},{mi},{pa},0
FILEOS 0x40004
FILETYPE 0x1
BEGIN
  BLOCK "StringFileInfo"
  BEGIN
    BLOCK "040904B0"
    BEGIN
      VALUE "CompanyName", "YAGUI"
      VALUE "FileDescription", "YAGUI - Yet Another Git User Interface"
      VALUE "FileVersion", "{ver}"
      VALUE "InternalName", "yagui"
      VALUE "LegalCopyright", "YAGUI contributors"
      VALUE "OriginalFilename", "yagui.exe"
      VALUE "ProductName", "YAGUI"
      VALUE "ProductVersion", "{ver} ({commit})"
    END
  END
  BLOCK "VarFileInfo"
  BEGIN
    VALUE "Translation", 0x409, 1200
  END
END
"#,
        ico = ico_path.display().to_string().replace('\\', "\\\\"),
    );
    std::fs::write(&rc, text).unwrap();
    // Without a resource compiler the program still builds; only icon and file details are missing.
    let _ = embed_resource::compile(&rc, embed_resource::NONE);
}
