use std::{fs, path::PathBuf};
fn main() {
    println!("cargo:rerun-if-env-changed=DIFFY_PACKAGE_KIND");
    println!(
        "cargo:rustc-env=DIFFY_PACKAGE_KIND={}",
        std::env::var("DIFFY_PACKAGE_KIND").unwrap_or_default()
    );
    println!("cargo:rerun-if-changed=build.rs");
    for size in [16, 32, 128, 256] {
        println!("cargo:rerun-if-changed=assets/icon/icon.iconset/icon_{size}x{size}.png");
    }
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        // GPUI startup overflows MSVC's default stack in debug builds. Zed
        // uses the same 8 MiB reserve for its Windows GPUI executable.
        println!("cargo:rustc-link-arg=/STACK:8388608");
    }
    let out = PathBuf::from(std::env::var_os("OUT_DIR").unwrap());
    let sizes = [16u32, 32, 128, 256];
    let images: Vec<_> = sizes
        .iter()
        .map(|size| fs::read(format!("assets/icon/icon.iconset/icon_{size}x{size}.png")).unwrap())
        .collect();
    let mut icon = Vec::new();
    icon.extend_from_slice(&[0, 0, 1, 0, 4, 0]);
    let mut offset = 6 + sizes.len() as u32 * 16;
    for (size, image) in sizes.iter().zip(&images) {
        icon.extend_from_slice(&[*size as u8, *size as u8, 0, 0]);
        icon.extend_from_slice(&1u16.to_le_bytes());
        icon.extend_from_slice(&32u16.to_le_bytes());
        icon.extend_from_slice(&(image.len() as u32).to_le_bytes());
        icon.extend_from_slice(&offset.to_le_bytes());
        offset += image.len() as u32;
    }
    for image in images {
        icon.extend_from_slice(&image);
    }
    let icon_path = out.join("Diffy.ico");
    fs::write(&icon_path, icon).unwrap();
    let rc = out.join("diffy.rc");
    fs::write(
        &rc,
        format!(
            "1 ICON \"{}\"\n",
            icon_path.display().to_string().replace('\\', "/")
        ),
    )
    .unwrap();
    // GPUI supplies the platform manifest; this resource adds only our icon.
    embed_resource::compile_for(rc, ["diffy"], embed_resource::NONE)
        .manifest_required()
        .unwrap();
}
