use std::{
    io::Write,
    path::{Path, PathBuf},
};
fn make_icon(path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let tree = resvg::usvg::Tree::from_data(
        include_bytes!("assets/icons/app.svg"),
        &resvg::usvg::Options::default(),
    )?;
    let sizes = [16u32, 32, 48, 64, 128, 256];
    let mut images = Vec::new();
    for size in sizes {
        let mut pixmap = resvg::tiny_skia::Pixmap::new(size, size).ok_or("icon allocation")?;
        let scale = size as f32 / tree.size().width();
        resvg::render(
            &tree,
            resvg::tiny_skia::Transform::from_scale(scale, scale),
            &mut pixmap.as_mut(),
        );
        images.push(pixmap.encode_png()?);
    }
    let mut file = std::fs::File::create(path)?;
    file.write_all(&[0, 0, 1, 0])?;
    file.write_all(&(sizes.len() as u16).to_le_bytes())?;
    let mut offset = (6 + 16 * sizes.len()) as u32;
    for (size, png) in sizes.iter().zip(&images) {
        let dimension = if *size == 256 { 0 } else { *size as u8 };
        file.write_all(&[dimension, dimension, 0, 0])?;
        file.write_all(&1u16.to_le_bytes())?;
        file.write_all(&32u16.to_le_bytes())?;
        file.write_all(&(png.len() as u32).to_le_bytes())?;
        file.write_all(&offset.to_le_bytes())?;
        offset += png.len() as u32;
    }
    for png in images {
        file.write_all(&png)?;
    }
    Ok(())
}
fn main() {
    slint_build::compile("ui/main.slint").expect("compile Slint UI");
    println!("cargo:rerun-if-changed=assets/icons/app.svg");
    println!("cargo:rerun-if-changed=assets/app.manifest");
    println!("cargo:rerun-if-changed=assets/default-presets.json");
    let out = PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR"));
    let icon = out.join("app.ico");
    make_icon(&icon).expect("generate multi-size Windows icon");
    let mut resource = winresource::WindowsResource::new();
    resource
        .set_icon(icon.to_str().expect("icon path"))
        .set_manifest_file("assets/app.manifest")
        .set("ProductName", "Xprinter Quick Print")
        .set(
            "FileDescription",
            "Xprinter Quick Print - PDF label printing",
        )
        .set("OriginalFilename", "XprinterQuickPrint.exe");
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("gnu") {
        if let Some(linker) = std::env::var_os("CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER") {
            if let Some(bin) = Path::new(&linker).parent() {
                resource
                    .set_toolkit_path(bin.to_str().expect("toolkit path"))
                    .set_windres_path(
                        bin.join("x86_64-w64-mingw32-windres.exe")
                            .to_str()
                            .expect("windres path"),
                    )
                    .set_ar_path(bin.join("llvm-ar.exe").to_str().expect("ar path"));
            }
        }
    }
    resource
        .compile()
        .expect("compile Windows icon/version/manifest resources");
}
