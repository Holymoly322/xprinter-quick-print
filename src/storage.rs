use crate::layout::Preset;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub version: u32,
    pub printer: String,
    pub preset: usize,
    pub default_preset: Option<usize>,
    pub dark: bool,
    pub last_folder: PathBuf,
    pub width: u32,
    pub height: u32,
    pub first_run: bool,
    pub queue_pause_seconds: u32,
    pub position: Option<[i32; 2]>,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            version: 1,
            printer: String::new(),
            preset: 0,
            default_preset: None,
            dark: true,
            last_folder: PathBuf::new(),
            width: 1200,
            height: 800,
            first_run: true,
            queue_pause_seconds: 5,
            position: None,
        }
    }
}
pub fn directory() -> anyhow::Result<PathBuf> {
    let exe = std::env::current_exe()?;
    let base = exe.parent().unwrap();
    let dir = if base.join("portable.flag").exists() {
        base.join("config")
    } else {
        PathBuf::from(
            std::env::var_os("APPDATA").ok_or_else(|| anyhow::anyhow!("APPDATA отсутствует"))?,
        )
        .join("XprinterQuickPrint")
    };
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}
pub fn read<T: serde::de::DeserializeOwned>(path: &Path) -> anyhow::Result<T> {
    anyhow::ensure!(
        std::fs::metadata(path)?.len() <= 64 * 1024 * 1024,
        "JSON превышает допустимые 64 МБ"
    );
    Ok(serde_json::from_slice(&std::fs::read(path)?)?)
}
pub fn atomic<T: Serialize>(path: &Path, value: &T) -> anyhow::Result<()> {
    use std::io::Write;
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_nanos();
    let temp = path.with_extension(format!("json.{}.{stamp}.tmp", std::process::id()));
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp)?;
    f.write_all(&serde_json::to_vec_pretty(value)?)?;
    f.sync_all()?;
    drop(f);
    if path.exists() {
        std::fs::copy(path, path.with_extension("json.bak"))?;
    }
    let a: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let b: Vec<u16> = temp.as_os_str().encode_wide().chain(Some(0)).collect();
    unsafe {
        windows::Win32::Storage::FileSystem::MoveFileExW(
            windows::core::PCWSTR(b.as_ptr()),
            windows::core::PCWSTR(a.as_ptr()),
            windows::Win32::Storage::FileSystem::MOVEFILE_REPLACE_EXISTING
                | windows::Win32::Storage::FileSystem::MOVEFILE_WRITE_THROUGH,
        )?;
    }
    Ok(())
}
use std::os::windows::ffi::OsStrExt;
pub fn load(dir: &Path) -> anyhow::Result<(Settings, Vec<Preset>)> {
    let settings_path = dir.join("settings.json");
    let s: Settings = if settings_path.exists() {
        let value: serde_json::Value = read(&settings_path)?;
        let (value, migrated) = migrate(value)?;
        let s: Settings = serde_json::from_value(value)?;
        if migrated {
            atomic(&settings_path, &s)?;
        }
        s
    } else {
        Settings::default()
    };
    let p: Vec<Preset> = if dir.join("presets.json").exists() {
        read(&dir.join("presets.json"))?
    } else {
        builtin_presets()?
    };
    anyhow::ensure!(!p.is_empty(), "Список пресетов пуст");
    for x in &p {
        crate::layout::calculate([100., 100.], x)?;
    }
    Ok((s, p))
}

pub fn migrate(mut value: serde_json::Value) -> anyhow::Result<(serde_json::Value, bool)> {
    let object = value
        .as_object_mut()
        .ok_or_else(|| anyhow::anyhow!("Настройки должны быть объектом JSON"))?;
    let version = object.get("version").and_then(|v| v.as_u64()).unwrap_or(0);
    anyhow::ensure!(
        version <= 1,
        "Неизвестная версия настроек; исходный файл не изменён"
    );
    if version == 0 {
        if let Some(p) = object.remove("last_printer") {
            object.insert("printer".into(), p);
        }
        object.insert("version".into(), 1.into());
    }
    Ok((value, version == 0))
}

pub fn builtin_presets() -> anyhow::Result<Vec<Preset>> {
    let presets: Vec<Preset> =
        serde_json::from_str(include_str!("../assets/default-presets.json"))?;
    anyhow::ensure!(!presets.is_empty(), "Встроенный набор пресетов пуст");
    for p in &presets {
        crate::layout::calculate([100., 100.], p)?;
        anyhow::ensure!(
            p.driver.is_none(),
            "Встроенный пресет не должен содержать частные данные драйвера"
        );
    }
    Ok(presets)
}
#[derive(Serialize, Deserialize)]
pub struct PresetExport {
    pub version: u32,
    pub presets: Vec<Preset>,
}
#[derive(Deserialize)]
#[serde(untagged)]
enum PresetInput {
    Set(PresetExport),
    List(Vec<Preset>),
    One(Box<Preset>),
}
pub fn import_presets(path: &Path) -> anyhow::Result<Vec<Preset>> {
    let data: PresetInput = read(path)?;
    let mut presets = match data {
        PresetInput::Set(set) => {
            anyhow::ensure!(set.version == 1, "Неизвестная версия экспорта пресетов");
            set.presets
        }
        PresetInput::List(list) => list,
        PresetInput::One(p) => vec![*p],
    };
    anyhow::ensure!(
        !presets.is_empty() && presets.len() <= 1000,
        "Импорт должен содержать 1–1000 пресетов"
    );
    for p in &mut presets {
        p.driver = None;
        anyhow::ensure!(!p.name.trim().is_empty(), "Пресет должен иметь название");
        crate::layout::calculate([100., 100.], p)?;
    }
    Ok(presets)
}
pub fn export_preset(path: &Path, preset: &Preset) -> anyhow::Result<()> {
    let mut preset = preset.clone();
    preset.driver = None;
    crate::layout::calculate([100., 100.], &preset)?;
    atomic(
        path,
        &PresetExport {
            version: 1,
            presets: vec![preset],
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn migration() {
        let (v, changed) = migrate(serde_json::json!({"version":0,"last_printer":"test"})).unwrap();
        assert!(changed);
        assert_eq!(v["printer"], "test");
        assert_eq!(v["version"], 1);
        assert!(migrate(serde_json::json!({"version":999})).is_err());
    }
    #[test]
    fn atomic_backup() {
        let dir = std::env::temp_dir().join(format!("xqp-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("settings.json");
        atomic(&file, &serde_json::json!({"version":0})).unwrap();
        atomic(&file, &Settings::default()).unwrap();
        let old: serde_json::Value = read(&file.with_extension("json.bak")).unwrap();
        assert_eq!(old["version"], 0);
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn export_import() {
        let presets = vec![Preset::default()];
        let bytes = serde_json::to_vec(&presets).unwrap();
        let decoded: Vec<Preset> = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(presets, decoded);
    }
}
#[cfg(test)]
mod factory_tests {
    use super::*;
    #[test]
    fn factory_and_existing_presets() {
        let dir = std::env::temp_dir().join(format!("xqp-factory-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (_, defaults) = load(&dir).unwrap();
        assert_eq!(defaults[0].name, "JOPA");
        assert_eq!([defaults[0].width, defaults[0].height], [44., 23.]);
        assert_eq!(defaults[0].margins, [0.; 4]);
        assert_eq!(defaults[0].mode, crate::layout::Scale::Fit);
        assert_eq!(defaults[0].offset, [0.; 2]);
        assert_eq!(defaults[0].calibration, [0.; 2]);
        assert_eq!(defaults[1].name, "HH");
        assert_eq!([defaults[1].width, defaults[1].height], [40., 24.]);
        assert_eq!(defaults[1].margins, [1., 0., 1., 0.]);
        // Existing single-preset configurations must survive a factory update.
        let mut custom = vec![defaults[1].clone()];
        custom[0].name = "Мой пресет".into();
        atomic(&dir.join("presets.json"), &custom).unwrap();
        assert_eq!(load(&dir).unwrap().1, custom);
        let export = dir.join("export.json");
        export_preset(&export, &custom[0]).unwrap();
        assert_eq!(import_presets(&export).unwrap(), custom);
        atomic(
            &export,
            &serde_json::json!({"version":999,"presets":custom}),
        )
        .unwrap();
        assert!(import_presets(&export).is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
