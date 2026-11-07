use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Preset {
    pub name: String,
    #[serde(default)]
    pub driver: Option<DriverSnapshot>,
    pub width: f64,
    pub height: f64,
    pub margins: [f64; 4], // left, top, right, bottom
    pub offset: [f64; 2],
    pub rotation: u16,
    pub mode: Scale,
    pub percent: f64,
    pub centered: bool,
    pub calibration: [f64; 2],
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
pub enum Scale {
    Actual,
    Fit,
    Fill,
    Custom,
}
impl Default for Preset {
    fn default() -> Self {
        Self {
            driver: None,
            name: "Этикетка 40 × 30".into(),
            width: 40.,
            height: 30.,
            margins: [0.; 4],
            offset: [0.; 2],
            rotation: 0,
            mode: Scale::Fit,
            percent: 100.,
            centered: true,
            calibration: [0.; 2],
        }
    }
}
#[derive(Clone, Debug, PartialEq)]
pub struct Layout {
    pub paper: [f64; 2],
    pub clip: [f64; 4],
    pub content: [f64; 4],
    pub rotation: u16,
}
pub fn mm_to_px(mm: f64, dpi: f64) -> f64 {
    mm * dpi / 25.4
}
pub fn calculate(source: [f64; 2], p: &Preset) -> anyhow::Result<Layout> {
    anyhow::ensure!(
        source.iter().all(|v| v.is_finite() && *v > 0.),
        "Некорректный размер PDF"
    );
    anyhow::ensure!(
        [p.width, p.height, p.percent]
            .iter()
            .all(|v| v.is_finite() && *v > 0.),
        "Размер и масштаб должны быть положительными"
    );
    anyhow::ensure!(
        p.width <= 1000. && p.height <= 1000. && p.percent <= 1000.,
        "Превышен допустимый размер или масштаб"
    );
    anyhow::ensure!(
        [0, 90, 180, 270].contains(&p.rotation),
        "Поворот должен быть 0/90/180/270"
    );
    anyhow::ensure!(
        p.margins.iter().all(|v| v.is_finite() && *v >= 0.)
            && p.offset
                .iter()
                .chain(p.calibration.iter())
                .all(|v| v.is_finite()),
        "Некорректные отступы"
    );
    let [l, t, r, b] = p.margins;
    let (w, h) = (p.width - l - r, p.height - t - b);
    anyhow::ensure!(w > 0. && h > 0., "Отступы перекрывают этикетку");
    let [sw, sh] = if p.rotation % 180 == 90 {
        [source[1], source[0]]
    } else {
        source
    };
    let scale = match p.mode {
        Scale::Actual => 1.,
        Scale::Fit => (w / sw).min(h / sh),
        Scale::Fill => (w / sw).max(h / sh),
        Scale::Custom => p.percent / 100.,
    };
    let (cw, ch) = (sw * scale, sh * scale);
    let x = l + if p.centered { (w - cw) / 2. } else { 0. } + p.offset[0] + p.calibration[0];
    let y = t + if p.centered { (h - ch) / 2. } else { 0. } + p.offset[1] + p.calibration[1];
    Ok(Layout {
        paper: [p.width, p.height],
        clip: [l, t, w, h],
        content: [x, y, cw, ch],
        rotation: p.rotation,
    })
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct DriverSnapshot {
    pub printer: String,
    #[serde(default)]
    pub host_id: String,
    pub driver: String,
    pub environment: String,
    pub version: u64,
    pub devmode_version: u16,
    pub bytes: Vec<u8>,
}
impl DriverSnapshot {
    pub fn compatible(&self, other: &Self) -> bool {
        !self.host_id.is_empty()
            && self.host_id == other.host_id
            && self.printer == other.printer
            && self.driver == other.driver
            && self.environment == other.environment
            && self.version == other.version
            && self.devmode_version == other.devmode_version
    }
    pub fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.bytes.len() >= 220 && self.bytes.len() <= 1024 * 1024,
            "Некорректный размер снимка DEVMODE"
        );
        let size = u16::from_le_bytes([self.bytes[68], self.bytes[69]]) as usize;
        let extra = u16::from_le_bytes([self.bytes[70], self.bytes[71]]) as usize;
        anyhow::ensure!(
            size >= 220 && size + extra == self.bytes.len(),
            "Неверные границы снимка DEVMODE"
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fit() {
        let l = calculate([100., 150.], &Preset::default()).unwrap();
        assert_eq!(l.content, [10., 0., 20., 30.]);
    }
    #[test]
    fn rotation() {
        let p = Preset {
            rotation: 90,
            ..Default::default()
        };
        let l = calculate([100., 150.], &p).unwrap();
        assert!((l.content[2] - 40.).abs() < 1e-8);
    }
    #[test]
    fn invalid() {
        assert!(calculate([0., 1.], &Preset::default()).is_err());
        let p = Preset {
            margins: [30.; 4],
            ..Default::default()
        };
        assert!(calculate([1., 1.], &p).is_err());
    }
    #[test]
    fn units() {
        assert_eq!(mm_to_px(25.4, 203.), 203.);
    }
    #[test]
    fn actual_offset() {
        let p = Preset {
            mode: Scale::Actual,
            centered: false,
            offset: [2., -1.],
            margins: [1.; 4],
            ..Default::default()
        };
        assert_eq!(
            calculate([10., 20.], &p).unwrap().content,
            [3., 0., 10., 20.]
        );
    }
    #[test]
    fn fill_custom() {
        let mut p = Preset {
            mode: Scale::Fill,
            ..Default::default()
        };
        assert_eq!(calculate([100., 150.], &p).unwrap().content[2], 40.);
        p.mode = Scale::Custom;
        p.percent = 50.;
        assert_eq!(calculate([100., 150.], &p).unwrap().content[3], 75.);
    }
    #[test]
    fn serde_roundtrip() {
        let p = Preset::default();
        assert_eq!(
            p,
            serde_json::from_str::<Preset>(&serde_json::to_string(&p).unwrap()).unwrap()
        );
    }
}
#[cfg(test)]
mod snapshot_tests {
    use super::*;
    #[test]
    fn compatibility() {
        let mut a = DriverSnapshot {
            printer: "a".into(),
            host_id: "test-host".into(),
            driver: "x".into(),
            environment: "Windows x64".into(),
            version: 1,
            devmode_version: 1,
            bytes: vec![0; 220],
        };
        a.bytes[68..70].copy_from_slice(&220u16.to_le_bytes());
        assert!(a.validate().is_ok());
        let mut b = a.clone();
        b.printer = "b".into();
        assert!(!a.compatible(&b));
        b = a.clone();
        b.version = 2;
        assert!(!a.compatible(&b));
        a.bytes[70] = 1;
        assert!(a.validate().is_err());
    }
}
