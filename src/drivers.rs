//! No vendor package is installed unless compatibility and provenance are verified.
use anyhow::{ensure, Result};
pub fn windows_setup() -> Result<()> {
    std::process::Command::new("rundll32.exe")
        .args(["printui.dll,PrintUIEntry", "/il"])
        .spawn()?;
    Ok(())
}
pub fn settings() -> Result<()> {
    std::process::Command::new("explorer.exe")
        .arg("ms-settings:printers")
        .spawn()?;
    Ok(())
}
pub fn allow_package(
    model: &str,
    hardware_match: bool,
    signature: bool,
    official: bool,
) -> Result<()> {
    ensure!(
        !model.trim().is_empty() && hardware_match && signature && official,
        "Совместимость или происхождение пакета не подтверждены. Используйте мастер Windows"
    );
    Ok(())
}

pub fn inventory() -> Result<String> {
    use windows::{
        core::PCWSTR,
        Win32::{
            Devices::DeviceAndDriverInstallation::*,
            Foundation::HWND,
            Graphics::Printing::{EnumPrinterDriversW, DRIVER_INFO_2W},
        },
    };
    let mut lines = vec![];
    unsafe {
        let (mut size, mut count) = (0, 0);
        let _ = EnumPrinterDriversW(
            PCWSTR::null(),
            PCWSTR::null(),
            2,
            None,
            &mut size,
            &mut count,
        );
        if size > 0 && size < 16 * 1024 * 1024 {
            let mut data = vec![0u64; (size as usize).div_ceil(8)];
            let bytes =
                std::slice::from_raw_parts_mut(data.as_mut_ptr().cast::<u8>(), size as usize);
            EnumPrinterDriversW(
                PCWSTR::null(),
                PCWSTR::null(),
                2,
                Some(bytes),
                &mut size,
                &mut count,
            )?;
            ensure!(
                count as usize * std::mem::size_of::<DRIVER_INFO_2W>() <= size as usize,
                "Неверный список драйверов"
            );
            for driver in
                std::slice::from_raw_parts(data.as_ptr().cast::<DRIVER_INFO_2W>(), count as usize)
            {
                let name = driver.pName.to_string().unwrap_or_default();
                if name.to_lowercase().contains("xprinter") {
                    lines.push(format!("Установленный драйвер: {name}"));
                }
            }
        }
        struct DeviceSet(HDEVINFO);
        impl Drop for DeviceSet {
            fn drop(&mut self) {
                unsafe {
                    let _ = SetupDiDestroyDeviceInfoList(self.0);
                }
            }
        }
        let devices = DeviceSet(SetupDiGetClassDevsW(
            None,
            PCWSTR::null(),
            HWND::default(),
            DIGCF_PRESENT | DIGCF_ALLCLASSES,
        )?);
        let property = |info: &SP_DEVINFO_DATA, key| -> String {
            let mut bytes = vec![0u8; 8192];
            let mut required = 0;
            if SetupDiGetDeviceRegistryPropertyW(
                devices.0,
                info,
                key,
                None,
                Some(&mut bytes),
                Some(&mut required),
            )
            .is_err()
            {
                return String::new();
            }
            let words = bytes[..(required as usize).min(bytes.len())]
                .as_chunks::<2>()
                .0
                .iter()
                .map(|b| u16::from_le_bytes([b[0], b[1]]))
                .collect::<Vec<_>>();
            String::from_utf16_lossy(&words)
                .trim_matches('\0')
                .replace('\0', "; ")
        };
        for index in 0..16384 {
            let mut info = SP_DEVINFO_DATA {
                cbSize: std::mem::size_of::<SP_DEVINFO_DATA>() as u32,
                ..Default::default()
            };
            if SetupDiEnumDeviceInfo(devices.0, index, &mut info).is_err() {
                break;
            }
            let friendly = property(&info, SPDRP_FRIENDLYNAME);
            let description = property(&info, SPDRP_DEVICEDESC);
            let ids = property(&info, SPDRP_HARDWAREID);
            if format!("{friendly} {description} {ids}")
                .to_lowercase()
                .contains("xprinter")
            {
                lines.push(format!("PnP-кандидат: {friendly} {description}; ID: {ids}"));
            }
        }
    }
    if lines.is_empty() {
        lines.push("В списке драйверов и PnP не найдено явного имени Xprinter. Это не доказывает отсутствие пакета в Driver Store или сетевого принтера.".into());
    }
    lines.push("Модель и совместимость автоматически не подтверждены. Выберите точную модель в мастере Windows либо установите её официальный пакет вручную. Драйвер, очередь и доступность устройства проверяются отдельно.".into());
    Ok(lines.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reject() {
        assert!(allow_package("", true, true, true).is_err());
        assert!(allow_package("Xprinter", false, true, true).is_err());
        assert!(allow_package("XP", true, false, true).is_err());
        assert!(allow_package("XP", true, true, false).is_err());
    }
    #[test]
    fn compatible() {
        assert!(allow_package("verified", true, true, true).is_ok());
    }
}
