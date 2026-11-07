use crate::layout::{calculate, mm_to_px, Preset};
use image::{Rgba, RgbaImage};
use pdfium_render::prelude::*;
use std::path::Path;
// PDFium owns process-global state: overlapping Init/Destroy pairs are forbidden.
static RENDERER_ACTIVE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
pub struct Renderer {
    pdfium: Option<Pdfium>,
}
pub struct Page {
    pub pixels: RgbaImage,
    pub size: [f64; 2],
    pub count: u16,
}
impl Renderer {
    pub fn new() -> anyhow::Result<Self> {
        let path = std::env::current_exe()?
            .parent()
            .unwrap()
            .join("pdfium.dll");
        use std::sync::atomic::Ordering;
        anyhow::ensure!(
            RENDERER_ACTIVE
                .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                .is_ok(),
            "PDFium уже занят другим экземпляром движка"
        );
        match Pdfium::bind_to_library(path) {
            Ok(bindings) => Ok(Self {
                pdfium: Some(Pdfium::new(bindings)),
            }),
            Err(error) => {
                RENDERER_ACTIVE.store(false, Ordering::Release);
                Err(error.into())
            }
        }
    }
    pub fn page(&self, path: &Path, index: u16, width: i32) -> anyhow::Result<Page> {
        let doc = self.document(path)?;
        Self::render_document(&doc, index, width)
    }
    pub fn document(&self, path: &Path) -> anyhow::Result<PdfDocument<'_>> {
        Ok(self
            .pdfium
            .as_ref()
            .expect("active PDFium")
            .load_pdf_from_file(path, None)?)
    }
    pub fn render_document(doc: &PdfDocument<'_>, index: u16, width: i32) -> anyhow::Result<Page> {
        let page = doc.pages().get(index)?;
        let size = [
            page.width().value as f64 * 25.4 / 72.,
            page.height().value as f64 * 25.4 / 72.,
        ];
        let config = PdfRenderConfig::new()
            .set_target_width(width)
            .set_maximum_height(4096);
        let pixels = page.render_with_config(&config)?.as_image().to_rgba8();
        Ok(Page {
            pixels,
            size,
            count: doc.pages().len(),
        })
    }
}
impl Drop for Renderer {
    fn drop(&mut self) {
        drop(self.pdfium.take());
        RENDERER_ACTIVE.store(false, std::sync::atomic::Ordering::Release);
    }
}
pub fn compose(page: &Page, p: &Preset, dpi: f64) -> anyhow::Result<RgbaImage> {
    let l = calculate(page.size, p)?;
    let w = mm_to_px(l.paper[0], dpi).round() as u32;
    let h = mm_to_px(l.paper[1], dpi).round() as u32;
    anyhow::ensure!(
        w > 0 && h > 0 && (w as u64) * (h as u64) <= 40_000_000,
        "Слишком большое растровое изображение"
    );
    let mut out = RgbaImage::from_pixel(w, h, Rgba([255, 255, 255, 255]));
    let rotated = match l.rotation {
        90 => image::imageops::rotate90(&page.pixels),
        180 => image::imageops::rotate180(&page.pixels),
        270 => image::imageops::rotate270(&page.pixels),
        _ => page.pixels.clone(),
    };
    // Inverse mapping avoids allocating huge scaled images for fill/custom modes.
    let [x, y, cw, ch] = l.content;
    let [cl, ct, cwidth, cheight] = l.clip;
    for py in 0..h {
        let my = (py as f64 + 0.5) * 25.4 / dpi;
        if my < ct || my >= ct + cheight || my < y || my >= y + ch {
            continue;
        }
        for px in 0..w {
            let mx = (px as f64 + 0.5) * 25.4 / dpi;
            if mx < cl || mx >= cl + cwidth || mx < x || mx >= x + cw {
                continue;
            }
            let sx = (((mx - x) / cw) * rotated.width() as f64).floor() as u32;
            let sy = (((my - y) / ch) * rotated.height() as f64).floor() as u32;
            if sx < rotated.width() && sy < rotated.height() {
                out.put_pixel(px, py, *rotated.get_pixel(sx, sy));
            }
        }
    }
    Ok(out)
}

pub fn preview(page: &Page, preset: &Preset, dpi: f64) -> anyhow::Result<RgbaImage> {
    let mut image = compose(page, preset, dpi)?;
    decorate_preview(&mut image, &crate::layout::calculate(page.size, preset)?);
    Ok(image)
}
pub fn decorate_preview(image: &mut RgbaImage, layout: &crate::layout::Layout) {
    let (w, h) = image.dimensions();
    if w < 2 || h < 2 {
        return;
    }
    let border = Rgba([96, 205, 255, 255]);
    let dash = Rgba([245, 168, 76, 255]);
    for x in 0..w {
        image.put_pixel(x, 0, border);
        image.put_pixel(x, h - 1, border);
        image.put_pixel(x, 1, border);
        image.put_pixel(x, h - 2, border);
    }
    for y in 0..h {
        image.put_pixel(0, y, border);
        image.put_pixel(w - 1, y, border);
        image.put_pixel(1, y, border);
        image.put_pixel(w - 2, y, border);
    }
    let [left, top, width, height] = layout.clip;
    let x0 = (left / layout.paper[0] * (w - 1) as f64).round() as u32;
    let x1 = ((left + width) / layout.paper[0] * (w - 1) as f64)
        .round()
        .min((w - 1) as f64) as u32;
    let y0 = (top / layout.paper[1] * (h - 1) as f64).round() as u32;
    let y1 = ((top + height) / layout.paper[1] * (h - 1) as f64)
        .round()
        .min((h - 1) as f64) as u32;
    for x in x0..=x1 {
        if (x / 5) % 2 == 0 {
            image.put_pixel(x, y0, dash);
            image.put_pixel(x, y1, dash);
        }
    }
    for y in y0..=y1 {
        if (y / 5) % 2 == 0 {
            image.put_pixel(x0, y, dash);
            image.put_pixel(x1, y, dash);
        }
    }
}

#[cfg(test)]
mod raster_tests {
    use super::*;
    #[test]
    fn same_geometry_clips_pixels() {
        let page = Page {
            pixels: RgbaImage::from_pixel(10, 20, Rgba([0, 0, 0, 255])),
            size: [10., 20.],
            count: 1,
        };
        let p = Preset {
            width: 20.,
            height: 20.,
            margins: [2.; 4],
            ..Default::default()
        };
        let out = compose(&page, &p, 25.4).unwrap();
        assert_eq!(out.dimensions(), (20, 20));
        assert_eq!(*out.get_pixel(0, 0), Rgba([255, 255, 255, 255]));
        assert_eq!(*out.get_pixel(10, 10), Rgba([0, 0, 0, 255]));
    }
    #[test]
    fn all_rotations() {
        let mut pixels = RgbaImage::from_pixel(2, 1, Rgba([255, 255, 255, 255]));
        pixels.put_pixel(0, 0, Rgba([0, 0, 0, 255]));
        let page = Page {
            pixels,
            size: [2., 1.],
            count: 1,
        };
        for rotation in [0, 90, 180, 270] {
            let p = Preset {
                width: 2.,
                height: 2.,
                rotation,
                ..Default::default()
            };
            let image = compose(&page, &p, 254.).unwrap();
            assert_eq!(image.dimensions(), (20, 20));
            assert!(image.pixels().any(|p| p[0] == 0));
        }
    }
}
