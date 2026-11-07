use std::{
    os::windows::ffi::OsStrExt,
    path::{Path, PathBuf},
};
use windows::{
    core::{Interface, PCWSTR},
    Win32::{
        Foundation::HANDLE,
        System::Com::{
            CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize, IPersistFile,
            CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED,
        },
        UI::Shell::{
            FOLDERID_Desktop, FOLDERID_Programs, IShellLinkW, SHGetKnownFolderPath, ShellLink,
            KF_FLAG_DEFAULT,
        },
    },
};
fn wide(path: &Path) -> Vec<u16> {
    path.as_os_str().encode_wide().chain(Some(0)).collect()
}
struct Com;
impl Com {
    fn initialize() -> anyhow::Result<Self> {
        unsafe {
            CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok()?;
        }
        Ok(Self)
    }
}
impl Drop for Com {
    fn drop(&mut self) {
        unsafe {
            CoUninitialize();
        }
    }
}
fn folder(id: &windows::core::GUID) -> anyhow::Result<PathBuf> {
    unsafe {
        let value = SHGetKnownFolderPath(id, KF_FLAG_DEFAULT, HANDLE::default())?;
        let text = value.to_string();
        CoTaskMemFree(Some(value.0.cast()));
        Ok(text?.into())
    }
}
pub fn create_links(target: &Path, links: &[PathBuf]) -> anyhow::Result<()> {
    let _com = Com::initialize()?;
    // Shell links expect a conventional absolute path, not canonicalize()'s extended \\?\ prefix.
    let target = std::path::absolute(target)?;
    anyhow::ensure!(target.is_file(), "Файл приложения не найден");
    let directory = target
        .parent()
        .ok_or_else(|| anyhow::anyhow!("У EXE отсутствует каталог"))?;
    unsafe {
        for path in links {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)?;
            link.SetPath(PCWSTR(wide(&target).as_ptr()))?;
            link.SetWorkingDirectory(PCWSTR(wide(directory).as_ptr()))?;
            link.SetDescription(PCWSTR(
                "Xprinter Quick Print — печать PDF-этикеток\0"
                    .encode_utf16()
                    .collect::<Vec<_>>()
                    .as_ptr(),
            ))?;
            link.SetIconLocation(PCWSTR(wide(&target).as_ptr()), 0)?;
            let persist: IPersistFile = link.cast()?;
            persist.Save(PCWSTR(wide(path).as_ptr()), true)?;
        }
    }
    Ok(())
}
pub fn create_user_shortcuts() -> anyhow::Result<Vec<PathBuf>> {
    let target = std::env::current_exe()?;
    let paths = vec![
        folder(&FOLDERID_Desktop)?.join("Xprinter Quick Print.lnk"),
        folder(&FOLDERID_Programs)?
            .join("Xprinter Quick Print")
            .join("Xprinter Quick Print.lnk"),
    ];
    create_links(&target, &paths)?;
    Ok(paths)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn shortcut_targets_current_executable() {
        let dir = std::env::temp_dir().join(format!("xqp-shortcut-{}", std::process::id()));
        let path = dir.join("test.lnk");
        let exe = std::env::current_exe().unwrap();
        create_links(&exe, std::slice::from_ref(&path)).unwrap();
        let _com = Com::initialize().unwrap();
        unsafe {
            let link: IShellLinkW =
                CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).unwrap();
            let persist: IPersistFile = link.cast().unwrap();
            persist
                .Load(
                    PCWSTR(wide(&path).as_ptr()),
                    windows::Win32::System::Com::STGM_READ,
                )
                .unwrap();
            let mut target = [0u16; 32768];
            link.GetPath(&mut target, std::ptr::null_mut(), 0).unwrap();
            let len = target.iter().position(|c| *c == 0).unwrap();
            assert_eq!(
                std::fs::canonicalize(PathBuf::from(String::from_utf16(&target[..len]).unwrap()))
                    .unwrap(),
                std::fs::canonicalize(exe).unwrap()
            );
        }
        std::fs::remove_dir_all(dir).unwrap();
    }
}
