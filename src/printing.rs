//! Windows ABI boundary. Every owned spooler/DC handle is guarded by Drop.
use crate::{
    layout::Preset,
    pdf::{compose, Renderer},
};
use anyhow::{bail, ensure, Result};
use std::path::Path;
use std::{ffi::c_void, mem::size_of, ptr};
type Handle = *mut c_void;
#[repr(C)]
#[derive(Clone, Copy)]
struct PrinterInfo4 {
    name: *const u16,
    server: *const u16,
    attributes: u32,
}
#[repr(C)]
struct DocInfo {
    size: i32,
    name: *const u16,
    output: *const u16,
    datatype: *const u16,
    flags: u32,
}
#[repr(C)]
struct BitmapHeader {
    size: u32,
    width: i32,
    height: i32,
    planes: u16,
    bits: u16,
    compression: u32,
    image_size: u32,
    xppm: i32,
    yppm: i32,
    used: u32,
    important: u32,
}
#[repr(C)]
struct BitmapInfo {
    header: BitmapHeader,
    colors: [u32; 1],
}
#[link(name = "winspool")]
unsafe extern "system" {
    fn EnumPrintersW(
        flags: u32,
        name: *const u16,
        level: u32,
        buffer: *mut u8,
        size: u32,
        needed: *mut u32,
        returned: *mut u32,
    ) -> i32;
    fn DeviceCapabilitiesW(
        device: *const u16,
        port: *const u16,
        cap: u16,
        output: *mut u16,
        dm: *const u8,
    ) -> i32;
    fn OpenPrinterW(name: *const u16, handle: *mut Handle, defaults: *const c_void) -> i32;
    fn ClosePrinter(handle: Handle) -> i32;
    fn DocumentPropertiesW(
        hwnd: Handle,
        printer: Handle,
        name: *const u16,
        output: *mut u8,
        input: *const u8,
        mode: u32,
    ) -> i32;
}
#[link(name = "gdi32")]
unsafe extern "system" {
    fn CreateDCW(
        driver: *const u16,
        device: *const u16,
        output: *const u16,
        devmode: *const u8,
    ) -> Handle;
    fn DeleteDC(dc: Handle) -> i32;
    fn GetDeviceCaps(dc: Handle, index: i32) -> i32;
    fn StartDocW(dc: Handle, info: *const DocInfo) -> i32;
    fn StartPage(dc: Handle) -> i32;
    fn EndPage(dc: Handle) -> i32;
    fn EndDoc(dc: Handle) -> i32;
    fn AbortDoc(dc: Handle) -> i32;
    fn StretchDIBits(
        dc: Handle,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        sx: i32,
        sy: i32,
        sw: i32,
        sh: i32,
        bits: *const c_void,
        info: *const BitmapInfo,
        usage: u32,
        rop: u32,
    ) -> i32;
}
fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}
unsafe fn string(p: *const u16) -> String {
    if p.is_null() {
        return String::new();
    }
    let mut n = 0;
    while *p.add(n) != 0 && n < 32768 {
        n += 1;
    }
    String::from_utf16_lossy(std::slice::from_raw_parts(p, n))
}
fn winerr(what: &str) -> anyhow::Error {
    anyhow::anyhow!("{}: {}", what, std::io::Error::last_os_error())
}
struct Printer(Handle);
impl Drop for Printer {
    fn drop(&mut self) {
        unsafe {
            ClosePrinter(self.0);
        }
    }
}
struct Dc(Handle);
impl Drop for Dc {
    fn drop(&mut self) {
        unsafe {
            DeleteDC(self.0);
        }
    }
}
struct Document<'a> {
    dc: &'a Dc,
    done: bool,
}
impl Drop for Document<'_> {
    fn drop(&mut self) {
        if !self.done {
            unsafe {
                AbortDoc(self.dc.0);
            }
        }
    }
}
fn open(name: &str) -> Result<Printer> {
    let mut h = ptr::null_mut();
    ensure!(
        unsafe { OpenPrinterW(wide(name).as_ptr(), &mut h, ptr::null()) } != 0,
        "{}",
        winerr("OpenPrinter")
    );
    Ok(Printer(h))
}
pub fn enumerate() -> Result<Vec<String>> {
    unsafe {
        let (mut bytes, mut count) = (0, 0);
        let initial = EnumPrintersW(
            6,
            ptr::null(),
            4,
            ptr::null_mut(),
            0,
            &mut bytes,
            &mut count,
        );
        if bytes == 0 {
            ensure!(initial != 0, "{}", winerr("EnumPrinters"));
            return Ok(vec![]);
        }
        ensure!(
            bytes < 64 * 1024 * 1024,
            "Слишком большой ответ диспетчера печати"
        );
        let mut data = vec![0u64; (bytes as usize).div_ceil(8)];
        ensure!(
            EnumPrintersW(
                6,
                ptr::null(),
                4,
                data.as_mut_ptr().cast(),
                bytes,
                &mut bytes,
                &mut count
            ) != 0,
            "{}",
            winerr("EnumPrinters")
        );
        ensure!(
            count as usize * size_of::<PrinterInfo4>() <= bytes as usize,
            "Некорректный ответ EnumPrinters"
        );
        let infos =
            std::slice::from_raw_parts(data.as_ptr().cast::<PrinterInfo4>(), count as usize);
        Ok(infos.iter().map(|p| string(p.name)).collect())
    }
}
pub fn probe(name: &str) -> Result<String> {
    let printer = open(name)?;
    let dm = devmode(&printer, name, false)?;
    let dc = create_dc(name, &dm)?;
    unsafe {
        Ok(format!("Драйвер доступен • {} × {} dpi • поля слева/сверху: {:.2}/{:.2} мм. Статус устройства не подтверждён.",GetDeviceCaps(dc.0,88),GetDeviceCaps(dc.0,90),GetDeviceCaps(dc.0,112) as f64*25.4/GetDeviceCaps(dc.0,88).max(1) as f64,GetDeviceCaps(dc.0,113) as f64*25.4/GetDeviceCaps(dc.0,90).max(1) as f64))
    }
}
fn devmode(p: &Printer, name: &str, prompt: bool) -> Result<Vec<u64>> {
    unsafe {
        let n = wide(name);
        let bytes = DocumentPropertiesW(
            ptr::null_mut(),
            p.0,
            n.as_ptr(),
            ptr::null_mut(),
            ptr::null(),
            0,
        );
        ensure!(
            (220..1024 * 1024).contains(&bytes),
            "Драйвер не предоставил корректный DEVMODE"
        );
        let mut data = vec![0u64; (bytes as usize).div_ceil(8)];
        let code = DocumentPropertiesW(
            ptr::null_mut(),
            p.0,
            n.as_ptr(),
            data.as_mut_ptr().cast(),
            ptr::null(),
            if prompt { 6 } else { 2 },
        );
        ensure!(
            code == 1,
            "DocumentProperties: отменено или ошибка ({code})"
        );
        let b = std::slice::from_raw_parts(data.as_ptr().cast::<u8>(), data.len() * 8);
        let size = u16::from_le_bytes([b[68], b[69]]) as usize;
        let extra = u16::from_le_bytes([b[70], b[71]]) as usize;
        ensure!(
            size >= 220 && size + extra <= bytes as usize,
            "Несовместимая структура DEVMODE"
        );
        Ok(data)
    }
}
fn create_dc(name: &str, dm: &[u64]) -> Result<Dc> {
    let dc = unsafe {
        CreateDCW(
            wide("WINSPOOL").as_ptr(),
            wide(name).as_ptr(),
            ptr::null(),
            dm.as_ptr().cast(),
        )
    };
    ensure!(!dc.is_null(), "{}", winerr("CreateDC"));
    Ok(Dc(dc))
}
// Use one driver copy; application repeats complete documents, avoiding double multiplication.
pub fn page_sequence(pages: u16, copies: u16) -> Result<Vec<u16>> {
    ensure!(
        pages > 0 && (1..=crate::queue::MAX_COPIES).contains(&copies),
        "Некорректное число страниц или копий"
    );
    Ok((0..copies).flat_map(|_| 0..pages).collect())
}
pub fn print(path: &Path, name: &str, preset: &Preset, copies: u16, pages: &[u16]) -> Result<i32> {
    print_control(
        path,
        name,
        preset,
        copies,
        pages,
        &std::sync::atomic::AtomicBool::new(false),
        None,
    )
}
pub fn print_cancellable(
    path: &Path,
    name: &str,
    preset: &Preset,
    copies: u16,
    pages: &[u16],
    cancel: &std::sync::atomic::AtomicBool,
) -> Result<i32> {
    print_control(path, name, preset, copies, pages, cancel, None)
}
/// Explicit virtual-printer diagnostic. Never selects a physical printer.
pub fn print_to_pdf(path: &Path, preset: &Preset, copies: u16, output: &Path) -> Result<i32> {
    print_control(
        path,
        "Microsoft Print to PDF",
        preset,
        copies,
        &[],
        &std::sync::atomic::AtomicBool::new(false),
        Some(output),
    )
}
fn print_control(
    path: &Path,
    name: &str,
    preset: &Preset,
    copies: u16,
    pages: &[u16],
    cancel: &std::sync::atomic::AtomicBool,
    output: Option<&Path>,
) -> Result<i32> {
    let printer = open(name)?;
    let mut dm = devmode(&printer, name, false)?;
    if let Some(snapshot) = &preset.driver {
        dm = restore(snapshot, &printer, name, &dm)?;
    }
    let stock = paper_id(&printer, name, &dm, preset.width, preset.height).ok();
    unsafe {
        let b = std::slice::from_raw_parts_mut(dm.as_mut_ptr().cast::<u8>(), dm.len() * 8);
        // DEVMODEW public offsets: dmFields=72; print union=76; copies=86.
        let (fields, paper) =
            crate::print_geometry::paper_request(u32::from_le_bytes(b[72..76].try_into()?), stock);
        b[72..76].copy_from_slice(&fields.to_le_bytes());
        b[102..166].fill(0);
        b[76..78].copy_from_slice(&1i16.to_le_bytes());
        b[78..80].copy_from_slice(&paper.to_le_bytes());
        b[80..82].copy_from_slice(&((preset.height * 10.).round() as i16).to_le_bytes());
        b[82..84].copy_from_slice(&((preset.width * 10.).round() as i16).to_le_bytes());
        b[84..86].copy_from_slice(&100i16.to_le_bytes());
        b[86..88].copy_from_slice(&1i16.to_le_bytes());
        let input = dm.clone();
        ensure!(
            DocumentPropertiesW(
                ptr::null_mut(),
                printer.0,
                wide(name).as_ptr(),
                dm.as_mut_ptr().cast(),
                input.as_ptr().cast(),
                10
            ) == 1,
            "Драйвер отклонил настройки"
        );
        let b = std::slice::from_raw_parts(dm.as_ptr().cast::<u8>(), dm.len() * 8);
        ensure!(
            i16::from_le_bytes([b[86], b[87]]) == 1,
            "Драйвер не подтвердил одну копию; печать остановлена"
        );
        tracing::info!(
            paper_id = i16::from_le_bytes([b[78], b[79]]),
            paper_width_tenth_mm = i16::from_le_bytes([b[82], b[83]]),
            paper_height_tenth_mm = i16::from_le_bytes([b[80], b[81]]),
            driver_copies = i16::from_le_bytes([b[86], b[87]]),
            "Нормализованный DEVMODE задания"
        );
    }
    let dc = create_dc(name, &dm)?;
    let (dx, dy, pw, ph, ox, oy) = unsafe {
        (
            GetDeviceCaps(dc.0, 88),
            GetDeviceCaps(dc.0, 90),
            GetDeviceCaps(dc.0, 110),
            GetDeviceCaps(dc.0, 111),
            GetDeviceCaps(dc.0, 112),
            GetDeviceCaps(dc.0, 113),
        )
    };
    ensure!(dx > 0 && dy > 0, "Драйвер не сообщил разрешение");
    ensure!((pw as f64*25.4/dx as f64-preset.width).abs()<1.0 && (ph as f64*25.4/dy as f64-preset.height).abs()<1.0,"Драйвер не применил размер этикетки. Настройте пользовательскую бумагу через свойства принтера");
    let renderer = Renderer::new()?;
    let document = renderer.document(path)?;
    let page_count = document.pages().len();
    let selected: Vec<u16> = if pages.is_empty() {
        (0..page_count).collect()
    } else {
        pages.to_vec()
    };
    ensure!(
        selected.iter().all(|p| *p < page_count),
        "Номер страницы вне PDF"
    );
    let sequence = (0..copies).flat_map(|_| selected.iter().copied());
    ensure!(
        !selected.is_empty() && (1..=crate::queue::MAX_COPIES).contains(&copies),
        "Некорректное число копий"
    );
    let printable = unsafe { [GetDeviceCaps(dc.0, 8), GetDeviceCaps(dc.0, 10)] };
    tracing::info!(
        requested_copies = copies,
        pdf_pages = page_count,
        selected_pages = selected.len(),
        expected_gdi_pages = u32::from(copies) * selected.len() as u32,
        dpi_x = dx,
        dpi_y = dy,
        physical_width = pw,
        physical_height = ph,
        offset_x = ox,
        offset_y = oy,
        printable_width = printable[0],
        printable_height = printable[1],
        "Параметры задания перед StartDoc; драйвер подтвердил одну копию"
    );
    let title = wide(
        path.file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .as_ref(),
    );
    let output_wide = output.map(|p| wide(p.to_string_lossy().as_ref()));
    let info = DocInfo {
        size: size_of::<DocInfo>() as i32,
        name: title.as_ptr(),
        output: output_wide.as_ref().map_or(ptr::null(), |p| p.as_ptr()),
        datatype: ptr::null(),
        flags: 0,
    };
    if cancel.load(std::sync::atomic::Ordering::Relaxed) {
        return Err(crate::errors::PrintError::Cancelled.into());
    }
    let id = unsafe { StartDocW(dc.0, &info) };
    ensure!(id > 0, "{}", winerr("StartDoc"));
    let mut guard = Document {
        dc: &dc,
        done: false,
    };
    let mut sent_pages = 0;
    for page_index in sequence {
        if cancel.load(std::sync::atomic::Ordering::Relaxed) {
            return Err(crate::errors::PrintError::Cancelled.into());
        }
        let source = Renderer::render_document(
            &document,
            page_index,
            ((preset.width.max(preset.height) / 25.4 * dx.max(dy) as f64 * 2.).ceil() as i32)
                .clamp(512, 4096),
        )?;
        let raster = compose(&source, preset, dx as f64)?;
        let target = [
            (preset.width * dx as f64 / 25.4).round() as i32,
            (preset.height * dy as f64 / 25.4).round() as i32,
        ];
        let plan = crate::print_geometry::printable_blit(
            [raster.width(), raster.height()],
            target,
            [ox, oy],
            printable,
        )
        .map_err(anyhow::Error::msg)?;
        let [sx, sy, sw, sh] = plan.source;
        let cropped = image::imageops::crop_imm(&raster, sx, sy, sw, sh).to_image();
        let mut bgra = cropped.into_raw();
        for p in bgra.as_chunks_mut::<4>().0 {
            p.swap(0, 2);
        }
        let w = sw as i32;
        let h = sh as i32;
        let header = BitmapInfo {
            header: BitmapHeader {
                size: 40,
                width: w,
                height: -h,
                planes: 1,
                bits: 32,
                compression: 0,
                image_size: 0,
                xppm: 0,
                yppm: 0,
                used: 0,
                important: 0,
            },
            colors: [0],
        };
        if cancel.load(std::sync::atomic::Ordering::Relaxed) {
            return Err(crate::errors::PrintError::Cancelled.into());
        }
        unsafe {
            ensure!(StartPage(dc.0) > 0, "{}", winerr("StartPage"));
            let result = StretchDIBits(
                dc.0,
                plan.destination[0],
                plan.destination[1],
                plan.destination[2],
                plan.destination[3],
                0,
                0,
                w,
                h,
                bgra.as_ptr().cast(),
                &header,
                0,
                0x00CC0020,
            );
            if result == 0 || result == -1 {
                bail!("{}", winerr("StretchDIBits"));
            }
            ensure!(EndPage(dc.0) > 0, "{}", winerr("EndPage"));
        }
        sent_pages += 1;
    }
    ensure!(unsafe { EndDoc(dc.0) } > 0, "{}", winerr("EndDoc"));
    guard.done = true;
    tracing::info!(
        job_id = id,
        sent_pages,
        requested_copies = copies,
        "EndDoc: страницы переданы Windows; физическое количество не подтверждено"
    );
    Ok(id)
}

fn identity(p: &Printer, name: &str, dm: &[u64]) -> Result<crate::layout::DriverSnapshot> {
    use windows::{
        core::PCWSTR,
        Win32::{
            Foundation::HANDLE,
            Graphics::Printing::{GetPrinterDriverW, DRIVER_INFO_6W},
        },
    };
    unsafe {
        let mut size = 0;
        let _ = GetPrinterDriverW(HANDLE(p.0), PCWSTR::null(), 6, None, &mut size);
        ensure!(
            size as usize >= size_of::<DRIVER_INFO_6W>() && size < 1024 * 1024,
            "Драйвер не предоставил идентификатор и версию"
        );
        let mut data = vec![0u64; (size as usize).div_ceil(8)];
        let bytes = std::slice::from_raw_parts_mut(data.as_mut_ptr().cast::<u8>(), size as usize);
        ensure!(
            GetPrinterDriverW(HANDLE(p.0), PCWSTR::null(), 6, Some(bytes), &mut size).as_bool(),
            "{}",
            winerr("GetPrinterDriver")
        );
        let info = &*data.as_ptr().cast::<DRIVER_INFO_6W>();
        let public = &*dm
            .as_ptr()
            .cast::<windows::Win32::Graphics::Gdi::DEVMODEW>();
        Ok(crate::layout::DriverSnapshot {
            printer: name.into(),
            host_id: machine_id()?,
            driver: string(info.pName.0),
            environment: string(info.pEnvironment.0),
            version: info.dwlDriverVersion,
            devmode_version: public.dmDriverVersion,
            bytes: vec![],
        })
    }
}
pub fn capture(name: &str) -> Result<crate::layout::DriverSnapshot> {
    let printer = open(name)?;
    let dm = devmode(&printer, name, true)?;
    let mut snapshot = identity(&printer, name, &dm)?;
    unsafe {
        let public = &*dm
            .as_ptr()
            .cast::<windows::Win32::Graphics::Gdi::DEVMODEW>();
        snapshot.bytes = std::slice::from_raw_parts(
            dm.as_ptr().cast::<u8>(),
            public.dmSize as usize + public.dmDriverExtra as usize,
        )
        .to_vec();
    }
    snapshot.validate()?;
    Ok(snapshot)
}
fn restore(
    snapshot: &crate::layout::DriverSnapshot,
    p: &Printer,
    name: &str,
    current: &[u64],
) -> Result<Vec<u64>> {
    snapshot.validate()?;
    ensure!(snapshot.compatible(&identity(p,name,current)?),"Снимок драйвера несовместим с выбранным принтером или версией драйвера. Сохраните новый снимок");
    let current_header = unsafe {
        &*current
            .as_ptr()
            .cast::<windows::Win32::Graphics::Gdi::DEVMODEW>()
    };
    ensure!(
        u16::from_le_bytes([snapshot.bytes[64], snapshot.bytes[65]])
            == current_header.dmSpecVersion
            && u16::from_le_bytes([snapshot.bytes[68], snapshot.bytes[69]])
                == current_header.dmSize
            && u16::from_le_bytes([snapshot.bytes[70], snapshot.bytes[71]])
                == current_header.dmDriverExtra,
        "Структура DEVMODE изменилась; сохраните снимок заново"
    );
    let mut result = vec![0u64; snapshot.bytes.len().div_ceil(8)];
    unsafe {
        std::ptr::copy_nonoverlapping(
            snapshot.bytes.as_ptr(),
            result.as_mut_ptr().cast(),
            snapshot.bytes.len(),
        );
    }
    Ok(result)
}

fn paper_id(printer: &Printer, name: &str, dm: &[u64], width: f64, height: f64) -> Result<i16> {
    use windows::Win32::{
        Foundation::HANDLE,
        Graphics::Printing::{GetPrinterW, PRINTER_INFO_2W},
    };
    unsafe {
        let mut size = 0;
        let _ = GetPrinterW(HANDLE(printer.0), 2, None, &mut size);
        ensure!(
            size as usize >= size_of::<PRINTER_INFO_2W>() && size < 1024 * 1024,
            "GetPrinter: некорректный размер"
        );
        let mut data = vec![0u64; (size as usize).div_ceil(8)];
        let bytes = std::slice::from_raw_parts_mut(data.as_mut_ptr().cast::<u8>(), size as usize);
        GetPrinterW(HANDLE(printer.0), 2, Some(bytes), &mut size)?;
        let info = &*data.as_ptr().cast::<PRINTER_INFO_2W>();
        let port = info.pPortName.0;
        let device = wide(name);
        let count = DeviceCapabilitiesW(
            device.as_ptr(),
            port,
            2,
            ptr::null_mut(),
            dm.as_ptr().cast(),
        );
        ensure!(count > 0 && count < 4096, "Форматы бумаги не перечислены");
        let mut ids = vec![0u16; count as usize];
        let mut sizes = vec![windows::Win32::Foundation::POINT::default(); count as usize];
        ensure!(
            DeviceCapabilitiesW(
                device.as_ptr(),
                port,
                2,
                ids.as_mut_ptr(),
                dm.as_ptr().cast()
            ) == count,
            "Список форматов изменился"
        );
        ensure!(
            DeviceCapabilitiesW(
                device.as_ptr(),
                port,
                3,
                sizes.as_mut_ptr().cast(),
                dm.as_ptr().cast()
            ) == count,
            "Список размеров изменился"
        );
        ids.into_iter()
            .zip(sizes)
            .find(|(_, s)| {
                (s.x as f64 / 10. - width).abs() < 0.2 && (s.y as f64 / 10. - height).abs() < 0.2
            })
            .map(|(id, _)| id as i16)
            .ok_or_else(|| anyhow::anyhow!("Нужен пользовательский формат бумаги"))
    }
}

fn machine_id() -> Result<String> {
    #[link(name = "advapi32")]
    unsafe extern "system" {
        fn RegGetValueW(
            key: Handle,
            subkey: *const u16,
            value: *const u16,
            flags: u32,
            kind: *mut u32,
            data: *mut c_void,
            size: *mut u32,
        ) -> u32;
    }
    let mut data = vec![0u16; 256];
    let mut size = (data.len() * 2) as u32;
    let result = unsafe {
        RegGetValueW(
            -2147483646isize as Handle,
            wide("SOFTWARE\\Microsoft\\Cryptography").as_ptr(),
            wide("MachineGuid").as_ptr(),
            2,
            ptr::null_mut(),
            data.as_mut_ptr().cast(),
            &mut size,
        )
    };
    ensure!(
        result == 0,
        "Не удалось получить идентификатор компьютера для безопасного снимка драйвера: {result}"
    );
    Ok(
        String::from_utf16_lossy(&data[..(size as usize / 2).min(data.len())])
            .trim_matches('\0')
            .to_owned(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn copies() {
        assert_eq!(page_sequence(2, 3).unwrap(), vec![0, 1, 0, 1, 0, 1]);
        assert!(page_sequence(0, 1).is_err());
        assert!(page_sequence(1, 0).is_err());
    }
}
