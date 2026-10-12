#[path = "src/win/icon_art.rs"]
mod icon_art;

use std::path::Path;

// 256 px is left out: an uncompressed 256 px frame alone would add 256 KiB to the exe.
const SIZES: [u32; 6] = [16, 20, 24, 32, 40, 48];

fn ico() -> Vec<u8> {
    let frames: Vec<Vec<u8>> = SIZES
        .iter()
        .map(|&n| {
            let px = icon_art::render_bgra(n as usize, false);
            let mut f = Vec::new();
            for v in [40u32, n, n * 2] {
                f.extend(v.to_le_bytes());
            }
            f.extend(1u16.to_le_bytes());
            f.extend(32u16.to_le_bytes());
            f.extend([0u8; 24]);
            let row = n as usize * 4;
            for y in (0..n as usize).rev() {
                f.extend(&px[y * row..(y + 1) * row]);
            }
            let mask_row = (n as usize).div_ceil(32) * 4;
            f.extend(vec![0u8; mask_row * n as usize]);
            f
        })
        .collect();
    let mut out = Vec::new();
    out.extend(0u16.to_le_bytes());
    out.extend(1u16.to_le_bytes());
    out.extend((frames.len() as u16).to_le_bytes());
    let mut offset = 6 + 16 * frames.len() as u32;
    for (n, f) in SIZES.iter().zip(&frames) {
        out.extend([*n as u8, *n as u8, 0, 0]);
        out.extend(1u16.to_le_bytes());
        out.extend(32u16.to_le_bytes());
        out.extend((f.len() as u32).to_le_bytes());
        out.extend(offset.to_le_bytes());
        offset += f.len() as u32;
    }
    for f in frames {
        out.extend(f);
    }
    out
}

fn main() {
    println!("cargo:rerun-if-changed=src/win/icon_art.rs");
    println!("cargo:rerun-if-changed=build.rs");
    if std::env::var("CARGO_CFG_WINDOWS").is_err() {
        return;
    }
    let out = std::env::var("OUT_DIR").unwrap();
    let ico_path = Path::new(&out).join("skedaddle.ico");
    std::fs::write(&ico_path, ico()).unwrap();

    let version = std::env::var("CARGO_PKG_VERSION").unwrap();
    let mut parts: Vec<u16> = version.split('.').map(|p| p.parse().unwrap_or(0)).collect();
    parts.resize(4, 0);
    let commas = parts.iter().map(|p| p.to_string()).collect::<Vec<_>>().join(",");
    let rc = format!(
        r#"1 ICON "{ico}"
1 VERSIONINFO
FILEVERSION {commas}
PRODUCTVERSION {commas}
BEGIN
  BLOCK "StringFileInfo"
  BEGIN
    BLOCK "040904B0"
    BEGIN
      VALUE "CompanyName", "Skedaddle"
      VALUE "FileDescription", "Skedaddle"
      VALUE "FileVersion", "{version}"
      VALUE "InternalName", "skedaddle"
      VALUE "LegalCopyright", "MIT License"
      VALUE "OriginalFilename", "skedaddle.exe"
      VALUE "ProductName", "Skedaddle"
      VALUE "ProductVersion", "{version}"
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
    let rc_path = Path::new(&out).join("skedaddle.rc");
    std::fs::write(&rc_path, rc).unwrap();
    embed_resource::compile(&rc_path, embed_resource::NONE).manifest_optional().unwrap();
}
