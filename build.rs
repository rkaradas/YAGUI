// Gives the Windows program its icon (Explorer, taskbar pin, Alt+Tab). The picture is the
// same one the window uses at run time (src/appicon.rs).

include!("src/appicon.rs");

fn main() {
    println!("cargo:rerun-if-changed=src/appicon.rs");
    println!("cargo:rerun-if-changed=build.rs");
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
    let rc = out.join("yagui.rc");
    std::fs::write(&rc, format!("1 ICON \"{}\"\n", ico_path.display().to_string().replace('\\', "\\\\"))).unwrap();
    // Without a resource compiler the program still builds; only the file icon is missing.
    let _ = embed_resource::compile(&rc, embed_resource::NONE);
}
