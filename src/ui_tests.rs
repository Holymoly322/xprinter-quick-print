//! Tests render the real Slint component tree without creating any operating-system window.
use slint::{
    platform::{
        software_renderer::{MinimalSoftwareWindow, RepaintBufferType},
        Platform, PlatformError, WindowAdapter,
    },
    ComponentHandle,
};
use std::{
    cell::RefCell,
    rc::Rc,
    time::{Duration, Instant},
};
struct Headless {
    windows: Rc<RefCell<Vec<Rc<MinimalSoftwareWindow>>>>,
    start: Instant,
}
impl Platform for Headless {
    fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, PlatformError> {
        let window = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
        self.windows.borrow_mut().push(window.clone());
        Ok(window)
    }
    fn duration_since_start(&self) -> Duration {
        self.start.elapsed()
    }
}
fn render(window: &MinimalSoftwareWindow, width: u32, height: u32, path: &str) {
    window.set_size(slint::PhysicalSize::new(width, height));
    window.window().request_redraw();
    for _ in 0..20 {
        slint::platform::update_timers_and_animations();
        std::thread::sleep(Duration::from_millis(20));
    }
    let mut pixels = vec![slint::Rgb8Pixel::default(); (width * height) as usize];
    assert!(window.draw_if_needed(|renderer| {
        renderer.render(&mut pixels, width as usize);
    }));
    assert!(
        pixels
            .iter()
            .any(|p| p.r != pixels[0].r || p.g != pixels[0].g || p.b != pixels[0].b),
        "UI must contain visible content"
    );
    image::RgbImage::from_raw(
        width,
        height,
        pixels.iter().flat_map(|p| [p.r, p.g, p.b]).collect(),
    )
    .unwrap()
    .save(path)
    .unwrap();
}
#[test]
fn render_windows() {
    let windows = Rc::new(RefCell::new(vec![]));
    slint::platform::set_platform(Box::new(Headless {
        windows: windows.clone(),
        start: Instant::now(),
    }))
    .unwrap();
    let ui = crate::MainWindow::new().unwrap();
    crate::app::verify_paced_ui(&ui);
    let pixels = image::open("tests/fixtures/label-preview.png")
        .unwrap()
        .into_rgba8();
    let mut pixels = pixels;
    crate::pdf::decorate_preview(
        &mut pixels,
        &crate::layout::calculate([100., 150.], &Default::default()).unwrap(),
    );
    let image = slint::Image::from_rgba8(
        slint::SharedPixelBuffer::<slint::Rgba8Pixel>::clone_from_slice(
            pixels.as_raw(),
            pixels.width(),
            pixels.height(),
        ),
    );
    ui.set_preview(image.clone());
    ui.set_filename("label.pdf • 100 × 150 мм".into());
    ui.set_status("Проверка отрисовки интерфейса".into());
    ui.set_presets(
        Rc::new(slint::VecModel::from(vec![slint::SharedString::from(
            "Этикетка 40 × 30",
        )]))
        .into(),
    );
    ui.show().unwrap();
    render(&windows.borrow()[0], 1200, 800, "ui-main-dark.png");
    ui.set_dark(false);
    render(&windows.borrow()[0], 1200, 800, "ui-main-light.png");
    ui.set_dark(true);
    ui.set_queue_count(2);
    ui.set_queue_ready_count(1);
    ui.set_queue_rows(
        Rc::new(slint::VecModel::from(vec![
            crate::QueueRow {
                title: "Первая этикетка • HH".into(),
                copies: 1,
                status: "Передано в Windows".into(),
                sent: true,
            },
            crate::QueueRow {
                title: "Вторая этикетка • HH".into(),
                copies: 1,
                status: "Подготовлено".into(),
                sent: false,
            },
        ]))
        .into(),
    );
    ui.set_queue_expanded(true);
    ui.set_batch_active(true);
    ui.set_batch_paused(true);
    ui.set_busy(true);
    ui.set_queue_wait_seconds(3);
    ui.set_queue_progress("Партия 1/2 передана в Windows. Когда лента будет готова, оторвите её и нажмите «Продолжить».".into());
    render(&windows.borrow()[0], 1200, 800, "ui-queue-paused.png");
    ui.set_queue_wait_seconds(0);
    render(&windows.borrow()[0], 1200, 800, "ui-queue-continue.png");
    ui.set_queue_expanded(false);
    ui.set_batch_active(false);
    ui.set_batch_paused(false);
    ui.set_busy(false);
    ui.set_setup(true);
    render(&windows.borrow()[0], 1200, 800, "ui-printer-setup.png");
    ui.set_setup(false);
    let editor = crate::PresetEditor::new().unwrap();
    use crate::editor::PresetEditing;
    editor.set_form(crate::layout::Preset::default().form());
    editor.set_preview(image);
    editor.show().unwrap();
    render(&windows.borrow()[1], 1100, 820, "ui-preset-editor.png");
    editor.hide().unwrap();
    ui.hide().unwrap();
}
