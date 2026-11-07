#![windows_subsystem = "windows"]
use xprinter_quick_print::{drivers, layout, pdf, printing, queue, storage};
mod app;
slint::include_modules!();
fn main() {
    if let Err(error) = app::run() {
        tracing::error!("{error:#}");
        let _ = rfd::MessageDialog::new()
            .set_title("Xprinter Quick Print")
            .set_description(format!("{error:#}"))
            .set_level(rfd::MessageLevel::Error)
            .show();
    }
}
mod editor;
#[cfg(test)]
mod ui_tests;
