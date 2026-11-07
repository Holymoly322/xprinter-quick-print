use crate::{
    layout::{Preset, Scale},
    PresetForm,
};
pub trait PresetEditing {
    fn form(&self) -> PresetForm;
    fn apply_form(&self, form: &PresetForm) -> anyhow::Result<Preset>;
}
impl PresetEditing for Preset {
    fn form(&self) -> PresetForm {
        PresetForm {
            name: self.name.clone().into(),
            width_mm: self.width.to_string().into(),
            height_mm: self.height.to_string().into(),
            left_mm: self.margins[0].to_string().into(),
            top_mm: self.margins[1].to_string().into(),
            right_mm: self.margins[2].to_string().into(),
            bottom_mm: self.margins[3].to_string().into(),
            x_mm: self.offset[0].to_string().into(),
            y_mm: self.offset[1].to_string().into(),
            calibration_x: self.calibration[0].to_string().into(),
            calibration_y: self.calibration[1].to_string().into(),
            percent: self.percent.to_string().into(),
            mode: match self.mode {
                Scale::Actual => 0,
                Scale::Fit => 1,
                Scale::Fill => 2,
                Scale::Custom => 3,
            },
            rotation: (self.rotation / 90) as i32,
            centered: self.centered,
        }
    }
    fn apply_form(&self, f: &PresetForm) -> anyhow::Result<Self> {
        fn n(s: &str) -> anyhow::Result<f64> {
            Ok(s.replace(',', ".").parse()?)
        }
        let mut p = self.clone();
        p.name = f.name.to_string();
        anyhow::ensure!(!p.name.trim().is_empty(), "Введите название");
        p.width = n(&f.width_mm)?;
        p.height = n(&f.height_mm)?;
        p.margins = [
            n(&f.left_mm)?,
            n(&f.top_mm)?,
            n(&f.right_mm)?,
            n(&f.bottom_mm)?,
        ];
        p.offset = [n(&f.x_mm)?, n(&f.y_mm)?];
        p.calibration = [n(&f.calibration_x)?, n(&f.calibration_y)?];
        p.percent = n(&f.percent)?;
        p.rotation = (f.rotation * 90) as u16;
        p.mode = match f.mode {
            0 => Scale::Actual,
            1 => Scale::Fit,
            2 => Scale::Fill,
            _ => Scale::Custom,
        };
        p.centered = f.centered;
        crate::layout::calculate([100., 100.], &p)?;
        Ok(p)
    }
}
