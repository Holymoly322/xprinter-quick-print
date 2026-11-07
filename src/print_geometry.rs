//! Translate a physical-page raster to a printable DC using nonnegative rectangles.
/// DEVMODEW: explicit custom dimensions use dmPaperSize=0, not DMPAPER_USER.
pub fn paper_request(existing: u32, stock: Option<i16>) -> (u32, i16) {
    // IDs >= DMPAPER_USER are valid when enumerated by DeviceCapabilities.
    // Only an absent match needs explicit custom dimensions with paper ID zero.
    let stock = stock.filter(|id| *id > 0);
    let mut fields = (existing & !(0x10000 | 0x2 | 0x4 | 0x8)) | 0x1 | 0x2 | 0x10 | 0x100;
    if stock.is_none() {
        fields |= 0x4 | 0x8;
    }
    (fields, stock.unwrap_or(0))
}
#[derive(Debug, PartialEq)]
pub struct BlitPlan {
    pub source: [u32; 4],
    pub destination: [i32; 4],
}
pub fn printable_blit(
    raster: [u32; 2],
    page: [i32; 2],
    origin: [i32; 2],
    printable: [i32; 2],
) -> Result<BlitPlan, &'static str> {
    if raster.contains(&0) || page.iter().chain(printable.iter()).any(|v| *v <= 0) {
        return Err("Драйвер не сообщил корректную область печати");
    }
    let mut src = [0; 4];
    let mut dst = [0; 4];
    for axis in 0..2 {
        let start = i64::from(origin[axis]).max(0);
        let end = (i64::from(origin[axis]) + i64::from(printable[axis])).min(i64::from(page[axis]));
        if end <= start {
            return Err("Область печати не пересекает этикетку");
        }
        let first = start as u64 * u64::from(raster[axis]) / page[axis] as u64;
        let last = (end as u64 * u64::from(raster[axis])).div_ceil(page[axis] as u64);
        src[axis] = first as u32;
        src[axis + 2] = (last - first) as u32;
        dst[axis] = (start - i64::from(origin[axis])) as i32;
        dst[axis + 2] = (end - start) as i32;
    }
    Ok(BlitPlan {
        source: src,
        destination: dst,
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn custom_paper_is_explicit_and_stock_does_not_keep_stale_dimensions() {
        let (fields, id) = paper_request(0x10000, None);
        assert_eq!(id, 0);
        assert_eq!(fields & 0xc, 0xc);
        assert_eq!(fields & 0x10000, 0);
        let (fields, id) = paper_request(0xc | 0x10000, Some(9));
        assert_eq!(id, 9);
        assert_eq!(fields & 0xc, 0);
        assert_ne!(fields & 0x100, 0);
        // A driver-defined saved 44x23 mm format can have paper ID 260.
        let (fields, id) = paper_request(0xc | 0x10000, Some(260));
        assert_eq!(id, 260);
        assert_eq!(fields & 0xc, 0);
        assert_eq!(fields & 0x10000, 0);
        assert_eq!(paper_request(0xc, Some(256)).1, 256);
    }
    #[test]
    fn xp365b_printable_pixels_preserve_physical_alignment() {
        let plan = printable_blit([320, 192], [320, 192], [10, 0], [300, 192]).unwrap();
        assert_eq!(plan.source, [10, 0, 300, 192]);
        assert_eq!(plan.destination, [0, 0, 300, 192]);
        let original_bar_x = 16;
        assert_eq!(
            original_bar_x - plan.source[0] as i32 + plan.destination[0] + 10,
            original_bar_x
        );
    }
    #[test]
    fn anisotropic_dpi_and_negative_origin() {
        let p = printable_blit([320, 192], [320, 384], [10, 20], [300, 344]).unwrap();
        assert_eq!(p.source, [10, 10, 300, 172]);
        assert_eq!(p.destination, [0, 0, 300, 344]);
        let p = printable_blit([320, 192], [320, 192], [-10, -5], [340, 202]).unwrap();
        assert_eq!(p.source, [0, 0, 320, 192]);
        assert_eq!(p.destination, [10, 5, 320, 192]);
    }
    #[test]
    fn invalid_caps_never_produce_a_blit() {
        assert!(printable_blit([320, 192], [320, 192], [400, 0], [300, 192]).is_err());
        assert!(printable_blit([320, 192], [320, 192], [0, 0], [0, 192]).is_err());
    }
}
