use crate::editor::PresetEditing;
use crate::{
    layout::Preset,
    queue::{Job, Status},
    MainWindow,
};
use slint::{ComponentHandle, Model};
use std::{cell::RefCell, path::PathBuf, rc::Rc, sync::mpsc};
enum Task {
    Render {
        path: PathBuf,
        page: u16,
        preset: Box<Preset>,
        revision: u64,
        to_editor: bool,
    },
    Print(Vec<Job>),
    Printers,
    Probe(String),
    Inventory,
}
enum Event {
    Preview {
        pixels: image::RgbaImage,
        count: u16,
        revision: u64,
        to_editor: bool,
    },
    Error(String),
    Printed(Vec<Job>, String),
    Printers(Vec<String>),
    Probe(String),
}
struct State {
    dir: PathBuf,
    settings: crate::storage::Settings,
    presets: Vec<Preset>,
    path: Option<PathBuf>,
    jobs: Vec<Job>,
    history: Vec<Job>,
    queue_rows: Rc<slint::VecModel<crate::QueueRow>>,
    pending_queue: Option<usize>,
    batch: Option<crate::queue::PacedQueue>,
    clock: std::time::Instant,
    cancel: std::sync::Arc<std::sync::atomic::AtomicBool>,
    revision: u64,
    tx: mpsc::Sender<Task>,
    latest: std::sync::Arc<std::sync::atomic::AtomicU64>,
}
impl State {
    fn matching_pending_job(&self, ui: &MainWindow, job: &Job) -> Option<usize> {
        let index = ui.get_queue_index().max(0) as usize;
        self.jobs
            .get(index)
            .filter(|old| {
                !matches!(old.status, Status::Queued(_))
                    && old.path == job.path
                    && old.copies == job.copies
                    && old.printer == job.printer
                    && old.preset == job.preset
                    && old.pages == job.pages
            })
            .map(|_| index)
    }
    fn edit_queue_copies(&mut self, ui: &MainWindow, index: usize, copies: i32) {
        if ui.get_busy() || !(0..=i32::from(crate::queue::MAX_COPIES)).contains(&copies) {
            return;
        }
        if let Some(job) = self.jobs.get_mut(index) {
            if matches!(job.status, Status::Queued(_)) {
                return;
            }
            job.copies = copies as u16;
            job.status = Status::Prepared;
            if ui.get_queue_index().max(0) as usize == index
                && self.path.as_ref() == Some(&job.path)
            {
                ui.set_copies(copies);
            }
        }
        self.refresh_queue(ui);
    }
    fn complete_batch_job(&mut self, ui: &MainWindow, jobs: &[Job]) {
        if let Some(index) = self.pending_queue.take() {
            if let Some(job) = jobs.first() {
                self.jobs[index] = job.clone();
            }
            let success = jobs
                .first()
                .is_some_and(|j| matches!(j.status, Status::Queued(_)));
            if success {
                let now = self.clock.elapsed();
                if let Some(batch) = self.batch.as_mut() {
                    if let Err(error) = batch.complete(now) {
                        tracing::error!("Batch state: {error:#}");
                        self.stop_batch(ui);
                    }
                }
                if self.batch.as_ref().is_some_and(|b| b.has_remaining()) {
                    ui.set_queue_progress(format!("Партия {}/{} передана в Windows. Когда лента будет готова, оторвите её и нажмите «Продолжить».", index + 1, self.jobs.len()).into());
                } else if self.batch.take().is_some() {
                    ui.set_queue_progress("Все партии переданы в очередь Windows.".into());
                }
            } else {
                self.stop_batch(ui);
            }
        }
        ui.set_busy(self.batch.is_some());
        self.refresh_batch(ui);
    }

    fn refresh_batch(&self, ui: &MainWindow) {
        ui.set_batch_active(self.batch.is_some());
        ui.set_batch_paused(self.batch.is_some() && self.pending_queue.is_none());
        ui.set_queue_wait_seconds(
            self.batch
                .as_ref()
                .map_or(0, |b| b.wait_seconds(self.clock.elapsed())) as i32,
        );
    }
    fn stop_batch(&mut self, ui: &MainWindow) {
        if let Some(batch) = self.batch.take() {
            for index in batch.pending_indices() {
                self.jobs[index].status = Status::Cancelled;
            }
        }
        ui.set_busy(self.pending_queue.is_some());
        ui.set_queue_progress(
            "Очередь остановлена. Уже переданные задания Windows не отменяются.".into(),
        );
        self.refresh_queue(ui);
        self.refresh_batch(ui);
    }
    fn dispatch_next(&mut self, ui: &MainWindow) -> anyhow::Result<()> {
        let index = self
            .batch
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("Нет активной очереди"))?
            .take_next(self.clock.elapsed())?;
        self.pending_queue = Some(index);
        if let Err(error) = self.tx.send(Task::Print(vec![self.jobs[index].clone()])) {
            self.pending_queue = None;
            self.jobs[index].status = Status::Failed(error.to_string());
            self.stop_batch(ui);
            return Err(error.into());
        }
        ui.set_busy(true);
        ui.set_queue_index(index as i32);
        ui.set_queue_progress(
            format!(
                "Передача партии {}/{}: {} ×{}",
                index + 1,
                self.jobs.len(),
                self.jobs[index]
                    .path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy(),
                self.jobs[index].copies
            )
            .into(),
        );
        self.refresh_batch(ui);
        Ok(())
    }
    fn persist(&self) -> anyhow::Result<()> {
        crate::storage::atomic(&self.dir.join("presets.json"), &self.presets)?;
        crate::storage::atomic(&self.dir.join("settings.json"), &self.settings)
    }
    fn render(&mut self, ui: &MainWindow, override_preset: Option<Preset>) {
        if let Some(path) = self.path.clone() {
            self.revision += 1;
            self.latest
                .store(self.revision, std::sync::atomic::Ordering::Relaxed);
            let to_editor = override_preset.is_some();
            let preset =
                override_preset.unwrap_or_else(|| self.presets[self.settings.preset].clone());
            let _ = self.tx.send(Task::Render {
                path,
                page: (ui.get_page() - 1).max(0) as u16,
                preset: Box::new(preset),
                revision: self.revision,
                to_editor,
            });
            ui.set_status("Подготовка превью…".into());
        }
    }
    fn load_paths(&mut self, ui: &MainWindow, paths: Vec<PathBuf>, add_all: bool) {
        let Some(first) = paths.first() else {
            return;
        };
        self.path = Some(first.clone());
        self.settings.last_folder = first
            .parent()
            .unwrap_or_else(|| std::path::Path::new("."))
            .into();
        ui.set_preview(slint::Image::default());
        ui.set_page(1);
        ui.set_pages(1);
        ui.set_page_range("".into());
        ui.set_pan_x(0.);
        ui.set_pan_y(0.);
        ui.set_zoom(1.);
        ui.set_filename(
            first
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .as_ref()
                .into(),
        );
        if add_all {
            ui.set_queue_index(self.jobs.len() as i32);
            for path in paths {
                self.jobs.push(Job {
                    path,
                    printer: self.settings.printer.clone(),
                    preset: self.presets[self.settings.preset].clone(),
                    copies: ui.get_copies() as u16,
                    pages: vec![],
                    status: Status::Prepared,
                });
            }
        }
        self.refresh_queue(ui);
        self.render(ui, None);
    }
    fn refresh_presets(&self, ui: &MainWindow) {
        ui.set_presets(
            Rc::new(slint::VecModel::from(
                self.presets
                    .iter()
                    .map(|p| p.name.clone().into())
                    .collect::<Vec<slint::SharedString>>(),
            ))
            .into(),
        );
        ui.set_preset_index(self.settings.preset as i32);
    }
    fn refresh_queue(&self, ui: &MainWindow) {
        ui.set_queue_count(self.jobs.len() as i32);
        let pending = crate::queue::pending_job_indices(&self.jobs);
        ui.set_queue_ready_count(pending.len() as i32);
        let rows: Vec<crate::QueueRow> = self
            .jobs
            .iter()
            .map(|j| crate::QueueRow {
                title: format!(
                    "{} • {}",
                    j.path.file_name().unwrap_or_default().to_string_lossy(),
                    j.preset.name
                )
                .into(),
                copies: j.copies as i32,
                status: j.status.label().into(),
                sent: matches!(j.status, Status::Queued(_)),
            })
            .collect();
        if self.queue_rows.row_count() != rows.len() {
            self.queue_rows.set_vec(rows);
        } else {
            for (index, row) in rows.into_iter().enumerate() {
                self.queue_rows.set_row_data(index, row);
            }
        }
        ui.set_queue_rows(self.queue_rows.clone().into());
        ui.set_history_items(
            Rc::new(slint::VecModel::from(
                self.history
                    .iter()
                    .rev()
                    .map(|j| {
                        slint::SharedString::from(format!(
                            "{} • {} • {} ×{} • {}",
                            j.path.display(),
                            j.printer,
                            j.preset.name,
                            j.copies,
                            j.status.label()
                        ))
                    })
                    .collect::<Vec<_>>(),
            ))
            .into(),
        );
        ui.set_queue_items(
            Rc::new(slint::VecModel::from(
                self.jobs
                    .iter()
                    .map(|j| {
                        slint::SharedString::from(format!(
                            "{} ×{} • {} • {}",
                            j.path.file_name().unwrap_or_default().to_string_lossy(),
                            j.copies,
                            j.preset.name,
                            j.status.label()
                        ))
                    })
                    .collect::<Vec<_>>(),
            ))
            .into(),
        );
        ui.set_queue_text(
            format!(
                "К отправке: {} • копий: {}. Уже переданные элементы повторно не отправляются.",
                pending.len(),
                pending
                    .iter()
                    .map(|i| u64::from(self.jobs[*i].copies))
                    .sum::<u64>()
            )
            .into(),
        );
    }
    fn make_job(&self, ui: &MainWindow) -> anyhow::Result<Job> {
        anyhow::ensure!(!self.settings.printer.is_empty(), "Выберите принтер");
        Ok(Job {
            path: self
                .path
                .clone()
                .ok_or_else(|| anyhow::anyhow!("Откройте PDF"))?,
            printer: self.settings.printer.clone(),
            preset: self.presets[self.settings.preset].clone(),
            copies: ui.get_copies() as u16,
            pages: crate::queue::parse_pages(&ui.get_page_range(), ui.get_pages() as u16)?,
            status: Status::Prepared,
        })
    }
}
fn report(ui: &MainWindow, result: anyhow::Result<()>) {
    if let Err(e) = result {
        tracing::error!("{e:#}");
        ui.set_status(format!("Ошибка: {e:#}").into());
    }
}
fn worker(
    rx: mpsc::Receiver<Task>,
    tx: mpsc::Sender<Event>,
    latest: std::sync::Arc<std::sync::atomic::AtomicU64>,
    cancel: std::sync::Arc<std::sync::atomic::AtomicBool>,
) {
    let mut renderer = None;
    let mut cache: Option<(
        PathBuf,
        u16,
        Option<std::time::SystemTime>,
        u64,
        crate::pdf::Page,
    )> = None;
    for task in rx {
        match task {
            Task::Render {
                path,
                page,
                preset,
                revision,
                to_editor,
            } => {
                if revision != latest.load(std::sync::atomic::Ordering::Relaxed) {
                    continue;
                }
                let result = (|| -> anyhow::Result<_> {
                    if renderer.is_none() {
                        renderer = Some(crate::pdf::Renderer::new()?);
                    }
                    let metadata = std::fs::metadata(&path)?;
                    let modified = metadata.modified().ok();
                    let length = metadata.len();
                    if cache.as_ref().is_none_or(|(p, i, m, l, _)| {
                        p != &path || *i != page || *m != modified || *l != length
                    }) {
                        cache = Some((
                            path.clone(),
                            page,
                            modified,
                            length,
                            renderer.as_ref().unwrap().page(&path, page, 1800)?,
                        ));
                    }
                    let source = &cache.as_ref().unwrap().4;
                    Ok((
                        crate::pdf::preview(
                            source,
                            &preset,
                            (1000. / preset.width * 25.4).min(600.),
                        )?,
                        source.count,
                    ))
                })();
                match result {
                    Ok((pixels, count)) => {
                        let _ = tx.send(Event::Preview {
                            pixels,
                            count,
                            revision,
                            to_editor,
                        });
                    }
                    Err(e) => {
                        let _ = tx.send(Event::Error(format!("{e:#}")));
                    }
                }
            }
            Task::Print(mut jobs) => {
                // PDFium Init/Destroy is global. Release viewer instance before print renderer.
                cache = None;
                renderer = None;
                tracing::info!(count = jobs.len(), "Начало передачи заданий");
                let mut ids = vec![];
                let mut failure = None;
                for job in &mut jobs {
                    if failure.is_some() {
                        job.status = Status::Cancelled;
                        continue;
                    }
                    crate::queue::submit(
                        &crate::queue::Windows {
                            cancel: cancel.clone(),
                        },
                        job,
                    );
                    match &job.status {
                        Status::Queued(id) => ids.push(*id),
                        Status::Failed(e) => failure = Some(e.clone()),
                        Status::Cancelled => failure = Some("Отменено пользователем".into()),
                        _ => {}
                    }
                }
                let text = if let Some(e) = failure {
                    format!(
                        "Ошибка передачи: {e:#}. Уже передано заданий: {}",
                        ids.len()
                    )
                } else {
                    format!(
                        "Передано в очередь Windows: {ids:?}. Физическая печать не подтверждена."
                    )
                };
                tracing::info!(status=%text,"Передача завершена");
                let _ = tx.send(Event::Printed(jobs, text));
            }
            Task::Printers => match crate::printing::enumerate() {
                Ok(p) => {
                    let _ = tx.send(Event::Printers(p));
                }
                Err(e) => {
                    let _ = tx.send(Event::Error(format!("{e:#}")));
                }
            },
            Task::Inventory => {
                let text = crate::drivers::inventory()
                    .unwrap_or_else(|e| format!("Ошибка проверки: {e:#}"));
                let _ = tx.send(Event::Probe(text));
            }
            Task::Probe(name) => {
                let text = crate::printing::probe(&name)
                    .unwrap_or_else(|e| format!("Ошибка доступа: {e:#}"));
                let _ = tx.send(Event::Probe(text));
            }
        }
    }
}
fn open_editor(
    main: &MainWindow,
    state: Rc<RefCell<State>>,
    holder: Rc<RefCell<Option<crate::PresetEditor>>>,
) -> anyhow::Result<()> {
    let e = crate::PresetEditor::new()?;
    anyhow::ensure!(
        !main.get_busy(),
        "Завершите или остановите очередь перед редактированием"
    );
    let index = state.borrow().settings.preset;
    let original = state.borrow().presets[index].clone();
    let snapshot = Rc::new(RefCell::new(original.driver.clone()));
    e.set_dark(main.get_dark());
    e.set_form(original.form());
    e.set_preview(main.get_preview());
    {
        let state = state.clone();
        let main = main.as_weak();
        let ew = e.as_weak();
        let original = original.clone();
        e.on_changed(move || {
            if let (Some(u), Some(e)) = (main.upgrade(), ew.upgrade()) {
                match original.apply_form(&e.get_form()) {
                    Ok(p) => {
                        state.borrow_mut().render(&u, Some(p));
                        e.set_status("Обновление превью…".into());
                    }
                    Err(err) => e.set_status(err.to_string().into()),
                }
            }
        });
    }
    {
        let state = state.clone();
        let main = main.as_weak();
        let ew = e.as_weak();
        let original = original.clone();
        let snapshot = snapshot.clone();
        e.on_save(move |as_new| {
            if let (Some(u), Some(e)) = (main.upgrade(), ew.upgrade()) {
                let result = (|| -> anyhow::Result<()> {
                    let mut p = original.apply_form(&e.get_form())?;
                    p.driver = snapshot.borrow().clone();
                    let mut s = state.borrow_mut();
                    if as_new {
                        s.presets.push(p);
                        s.settings.preset = s.presets.len() - 1;
                    } else {
                        s.presets[index] = p;
                    }
                    s.persist()?;
                    s.refresh_presets(&u);
                    s.render(&u, None);
                    u.set_editor(false);
                    e.hide()?;
                    Ok(())
                })();
                if let Err(err) = result {
                    e.set_status(format!("{err:#}").into());
                }
            }
        });
    }
    {
        let ew = e.as_weak();
        let original = original.clone();
        let snapshot = snapshot.clone();
        e.on_reset(move || {
            if let Some(e) = ew.upgrade() {
                *snapshot.borrow_mut() = original.driver.clone();
                e.set_form(original.form());
                e.invoke_changed();
            }
        });
    }
    {
        let ew = e.as_weak();
        let main = main.as_weak();
        let state = state.clone();
        e.on_cancel(move || {
            if let Some(u) = main.upgrade() {
                u.set_editor(false);
                state.borrow_mut().render(&u, None);
            }
            if let Some(e) = ew.upgrade() {
                let _ = e.hide();
            }
        });
    }
    {
        let main = main.as_weak();
        let state = state.clone();
        e.window().on_close_requested(move || {
            if let Some(u) = main.upgrade() {
                u.set_editor(false);
                state.borrow_mut().render(&u, None);
            }
            slint::CloseRequestResponse::HideWindow
        });
    }
    {
        let ew = e.as_weak();
        let state = state.clone();
        let main = main.as_weak();
        let original = original.clone();
        let snapshot = snapshot.clone();
        e.on_test(move || {
            if let (Some(e), Some(u)) = (ew.upgrade(), main.upgrade()) {
                if u.get_busy() {
                    e.set_status("Дождитесь окончания текущего задания".into());
                    return;
                }
                let result = (|| -> anyhow::Result<()> {
                    let mut job = state.borrow().make_job(&u)?;
                    job.preset = original.apply_form(&e.get_form())?;
                    job.preset.driver = snapshot.borrow().clone();
                    job.copies = 1;
                    job.pages = vec![(u.get_page() - 1).max(0) as u16];
                    state
                        .borrow()
                        .cancel
                        .store(false, std::sync::atomic::Ordering::Relaxed);
                    state.borrow().tx.send(Task::Print(vec![job]))?;
                    u.set_busy(true);
                    Ok(())
                })();
                if let Err(err) = result {
                    e.set_status(format!("{err:#}").into());
                }
            }
        });
    }
    {
        let ew = e.as_weak();
        let state = state.clone();
        let snapshot = snapshot.clone();
        e.on_capture(move || {
            if let Some(e) = ew.upgrade() {
                let name = state.borrow().settings.printer.clone();
                match crate::printing::capture(&name) {
                    Ok(value) => {
                        *snapshot.borrow_mut() = Some(value);
                        e.set_status(
                            "Снимок получен. Нажмите «Сохранить», чтобы сохранить его в пресете."
                                .into(),
                        );
                    }
                    Err(err) => e.set_status(format!("{err:#}").into()),
                }
            }
        });
    }
    {
        let ew = e.as_weak();
        let snapshot = snapshot.clone();
        e.on_clear_driver(move || {
            *snapshot.borrow_mut() = None;
            if let Some(e) = ew.upgrade() {
                e.set_status("Снимок удалён из черновика. Нажмите «Сохранить».".into());
            }
        });
    }
    e.show()?;
    main.set_editor(true);
    e.invoke_changed();
    *holder.borrow_mut() = Some(e);
    Ok(())
}
pub fn run() -> anyhow::Result<()> {
    let dir = crate::storage::directory()?;
    let file = tracing_appender::rolling::RollingFileAppender::builder()
        .rotation(tracing_appender::rolling::Rotation::DAILY)
        .filename_prefix("xprinter.log")
        .build(dir.join("logs"))?;
    let (writer, _guard) = tracing_appender::non_blocking(file);
    tracing_subscriber::fmt()
        .with_writer(writer)
        .with_ansi(false)
        .init();
    let (settings, presets) = crate::storage::load(&dir)?;
    let (tx, rx) = mpsc::channel();
    let (events, event_rx) = mpsc::channel();
    let latest = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));
    let worker_latest = latest.clone();
    let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let worker_cancel = cancel.clone();
    std::thread::spawn(move || worker(rx, events, worker_latest, worker_cancel));
    let mut settings = settings;
    settings.queue_pause_seconds = settings.queue_pause_seconds.min(300);
    settings.preset = settings
        .default_preset
        .unwrap_or(settings.preset)
        .min(presets.len() - 1);
    let state = Rc::new(RefCell::new(State {
        dir,
        settings,
        presets,
        path: None,
        jobs: vec![],
        history: vec![],
        queue_rows: Rc::new(slint::VecModel::default()),
        pending_queue: None,
        batch: None,
        clock: std::time::Instant::now(),
        cancel,
        revision: 0,
        tx,
        latest,
    }));
    let editor: Rc<RefCell<Option<crate::PresetEditor>>> = Rc::new(RefCell::new(None));
    let ui = MainWindow::new()?;
    ui.set_app_version(env!("CARGO_PKG_VERSION").into());
    {
        let state = state.clone();
        let weak = ui.as_weak();
        ui.on_queue_copies_edited(move |index, copies| {
            if let Some(u) = weak.upgrade() {
                state
                    .borrow_mut()
                    .edit_queue_copies(&u, index.max(0) as usize, copies);
            }
        });
    }
    {
        let state = state.clone();
        let weak = ui.as_weak();
        ui.on_copies_edited(move |copies| {
            if let Some(u) = weak.upgrade() {
                let mut s = state.borrow_mut();
                let index = u.get_queue_index().max(0) as usize;
                if s.jobs
                    .get(index)
                    .is_some_and(|j| Some(&j.path) == s.path.as_ref())
                {
                    s.edit_queue_copies(&u, index, copies);
                }
            }
        });
    }
    {
        let mut s = state.borrow_mut();
        if let Ok(history) = crate::storage::read::<Vec<Job>>(&s.dir.join("history.json")) {
            s.history = history;
        }
        s.refresh_queue(&ui);
    }
    ui.set_queue_pause_seconds(state.borrow().settings.queue_pause_seconds.min(300) as i32);
    {
        let state = state.clone();
        let weak = ui.as_weak();
        ui.on_queue_pause_changed(move |seconds| {
            if let Some(u) = weak.upgrade() {
                if u.get_busy() {
                    return;
                }
                let mut s = state.borrow_mut();
                s.settings.queue_pause_seconds = seconds.clamp(0, 300) as u32;
                report(&u, s.persist());
            }
        });
    }
    ui.set_dark(state.borrow().settings.dark);
    ui.set_setup(state.borrow().settings.first_run || std::env::args().any(|a| a == "--setup"));
    state.borrow().refresh_presets(&ui);
    ui.set_printer(state.borrow().settings.printer.clone().into());
    macro_rules! callback {
        ($method:ident, $s:ident, $u:ident, $body:expr) => {{
            let state = state.clone();
            let weak = ui.as_weak();
            ui.$method(move || {
                if let Some($u) = weak.upgrade() {
                    #[allow(unused_mut)]
                    // Some callbacks only read; others update through this binding.
                    let mut $s = state.borrow_mut();
                    $body;
                }
            });
        }};
    }
    {
        let state = state.clone();
        let weak = ui.as_weak();
        ui.on_open(move || {
            let Some(u) = weak.upgrade() else {
                return;
            };
            if u.get_busy() || u.get_editor() {
                u.set_status("Завершите текущую операцию".into());
                return;
            }
            let folder = state.borrow().settings.last_folder.clone();
            if let Some(paths) = rfd::FileDialog::new()
                .add_filter("PDF", &["pdf"])
                .set_directory(folder)
                .pick_files()
            {
                let add_all = paths.len() > 1;
                state.borrow_mut().load_paths(&u, paths, add_all);
            }
        });
    }
    {
        let weak = ui.as_weak();
        ui.on_add_copies(move |amount| {
            if let Some(u) = weak.upgrade() {
                let result = (|| -> anyhow::Result<()> {
                    let n = crate::queue::increment_copies(
                        u16::try_from(u.get_copies())?,
                        u16::try_from(amount)?,
                    )?;
                    u.set_copies(n as i32);
                    u.invoke_copies_edited(n as i32);
                    Ok(())
                })();
                report(&u, result);
            }
        });
    }
    {
        let weak = ui.as_weak();
        ui.on_create_shortcuts(move || {
            if let Some(u) = weak.upgrade() {
                match xprinter_quick_print::shell::create_user_shortcuts() {
                    Ok(_) => u.set_status("Ярлыки созданы на рабочем столе и в меню «Пуск»".into()),
                    Err(error) => report(&u, Err(error)),
                }
            }
        });
    }
    callback!(on_refresh, s, _u, {
        let _ = s.tx.send(Task::Printers);
    });
    callback!(on_render, s, u, {
        s.render(&u, None);
    });
    {
        let state = state.clone();
        let holder = editor.clone();
        let weak = ui.as_weak();
        ui.on_edit(move || {
            if let Some(u) = weak.upgrade() {
                report(&u, open_editor(&u, state.clone(), holder.clone()));
            }
        });
    }
    callback!(on_default_preset, s, u, {
        s.settings.default_preset = Some(s.settings.preset);
        report(&u, s.persist());
        u.set_status("Пресет будет выбран при следующем запуске".into());
    });
    {
        let state = state.clone();
        let weak = ui.as_weak();
        ui.on_new_preset(move || {
            if let Some(u) = weak.upgrade() {
                {
                    let mut s = state.borrow_mut();
                    let mut preset = crate::storage::builtin_presets()
                        .expect("validated builtin presets")
                        .remove(0);
                    preset.name = "Новый пресет".into();
                    s.presets.push(preset);
                    s.settings.preset = s.presets.len() - 1;
                    s.refresh_presets(&u);
                    report(&u, s.persist());
                }
                u.invoke_edit();
            }
        });
    }
    callback!(on_duplicate, s, u, {
        let mut p = s.presets[s.settings.preset].clone();
        p.name.push_str(" — копия");
        s.presets.push(p);
        s.settings.preset = s.presets.len() - 1;
        s.refresh_presets(&u);
        report(&u, s.persist());
    });
    callback!(on_remove_preset, s, u, {
        if s.presets.len() > 1 {
            let i = s.settings.preset;
            s.presets.remove(i);
            s.settings.default_preset = s.settings.default_preset.and_then(|d| {
                if d == i {
                    None
                } else {
                    Some(if d > i { d - 1 } else { d })
                }
            });
            s.settings.preset = 0;
            s.refresh_presets(&u);
            s.render(&u, None);
            report(&u, s.persist());
        } else {
            u.set_status("Нельзя удалить единственный пресет".into());
        }
    });
    callback!(on_print, s, u, {
        if u.get_busy() || u.get_editor() {
            return;
        }
        if u.get_copies() <= 0 {
            u.set_status("Укажите количество копий".into());
            return;
        }
        match s.make_job(&u) {
            Ok(job) => {
                s.cancel.store(false, std::sync::atomic::Ordering::Relaxed);
                u.set_busy(true);
                s.pending_queue = s.matching_pending_job(&u, &job);
                if let Err(error) = s.tx.send(Task::Print(vec![job])) {
                    s.pending_queue = None;
                    u.set_busy(false);
                    report(&u, Err(error.into()));
                }
            }
            Err(e) => report(&u, Err(e)),
        }
    });
    callback!(on_theme, s, u, {
        s.settings.dark = !s.settings.dark;
        u.set_dark(s.settings.dark);
        report(&u, s.persist());
    });
    {
        let state = state.clone();
        let weak = ui.as_weak();
        ui.on_export_presets(move || {
            if let Some(u) = weak.upgrade() {
                let p = {
                    let s = state.borrow();
                    s.presets[s.settings.preset].clone()
                };
                let filename: String = p
                    .name
                    .chars()
                    .map(|c| if "<>:\"/\\|?*".contains(c) { '_' } else { c })
                    .take(80)
                    .collect();
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter("Пресет JSON", &["json"])
                    .set_file_name(format!("{}.json", filename.trim_end_matches(['.', ' '])))
                    .save_file()
                {
                    report(&u, crate::storage::export_preset(&path, &p));
                }
            }
        });
    }
    {
        let state = state.clone();
        let weak = ui.as_weak();
        ui.on_import_presets(move || {
            if let Some(u) = weak.upgrade() {
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter("Пресет JSON", &["json"])
                    .pick_file()
                {
                    let result = (|| -> anyhow::Result<()> {
                        let presets = crate::storage::import_presets(&path)?;
                        let mut s = state.borrow_mut();
                        s.presets.extend(presets);
                        s.refresh_presets(&u);
                        s.persist()?;
                        u.set_status("Пресет импортирован".into());
                        Ok(())
                    })();
                    report(&u, result);
                }
            }
        });
    }
    {
        let state = state.clone();
        let weak = ui.as_weak();
        ui.on_select_printer(move |name| {
            if let Some(u) = weak.upgrade() {
                let mut s = state.borrow_mut();
                s.settings.printer = name.to_string();
                let _ = s.tx.send(Task::Probe(name.to_string()));
                report(&u, s.persist());
            }
        });
    }
    {
        let state = state.clone();
        let weak = ui.as_weak();
        ui.on_select_preset(move |index| {
            if let Some(u) = weak.upgrade() {
                let mut s = state.borrow_mut();
                if index >= 0 && (index as usize) < s.presets.len() {
                    s.settings.preset = index as usize;
                    s.render(&u, None);
                    report(&u, s.persist());
                }
            }
        });
    }
    {
        let state = state.clone();
        let weak = ui.as_weak();
        ui.on_save(move |json, preview| {
            if let Some(u) = weak.upgrade() {
                let mut s = state.borrow_mut();
                let result = (|| -> anyhow::Result<()> {
                    let p: Preset = serde_json::from_str(&json)?;
                    crate::layout::calculate([100., 100.], &p)?;
                    if preview {
                        s.render(&u, Some(p));
                    } else {
                        let i = s.settings.preset;
                        s.presets[i] = p;
                        s.persist()?;
                        s.refresh_presets(&u);
                        s.render(&u, None);
                        u.set_editor(false);
                    }
                    Ok(())
                })();
                report(&u, result);
            }
        });
    }
    {
        let state = state.clone();
        let weak = ui.as_weak();
        ui.on_setup_action(move |action| {
            if let Some(u) = weak.upgrade() {
                let mut s = state.borrow_mut();
                let result = match action {
                    0 => crate::drivers::windows_setup(),
                    4 => {
                        let _ = s.tx.send(Task::Inventory);
                        Ok(())
                    }
                    1 => crate::drivers::settings(),
                    2 => {
                        let _ = s.tx.send(Task::Probe(s.settings.printer.clone()));
                        Ok(())
                    }
                    _ => {
                        s.settings.first_run = false;
                        s.persist()
                    }
                };
                report(&u, result);
            }
        });
    }
    {
        let state = state.clone();
        let weak = ui.as_weak();
        ui.on_queue_action(move |action| {
            if let Some(u) = weak.upgrade() {
                if action == 7 {
                    let mut s = state.borrow_mut();
                    s.cancel.store(true, std::sync::atomic::Ordering::Relaxed);
                    if s.batch.is_some() {
                        s.stop_batch(&u);
                    }
                    return;
                }
                if action == 8 {
                    let mut s = state.borrow_mut();
                    if s.batch.is_some() && s.pending_queue.is_none() {
                        let result = s.dispatch_next(&u);
                        report(&u, result);
                    }
                    return;
                }
                if u.get_busy() {
                    return;
                }
                let mut s = state.borrow_mut();
                let index = u.get_queue_index().max(0) as usize;
                match action {
                    0 => match s.make_job(&u) {
                        Ok(j) => { s.jobs.push(j); u.set_queue_index((s.jobs.len()-1) as i32); },
                        Err(e) => report(&u, Err(e)),
                    },
                    1 => {
                        if index < s.jobs.len() {
                            s.jobs.remove(index);
                        }
                    }
                    2 => {
                        let result = (|| -> anyhow::Result<()> {
                            let indices = crate::queue::pending_job_indices(&s.jobs);
                            let pending: Vec<_> = indices.iter().map(|i| s.jobs[*i].clone()).collect();
                            crate::queue::validate_batch(&pending)?;
                            for i in &indices { s.jobs[*i].status = Status::Prepared; }
                            s.batch = Some(crate::queue::PacedQueue::from_indices(indices,
                                std::time::Duration::from_secs(u.get_queue_pause_seconds().clamp(0,300) as u64))?);
                            s.cancel.store(false, std::sync::atomic::Ordering::Relaxed);
                            u.set_queue_expanded(true);
                            s.dispatch_next(&u)
                        })();
                        report(&u, result);
                    }
                    3 => {
                        if index > 0 && index < s.jobs.len() {
                            s.jobs.swap(index, index - 1);
                            u.set_queue_index((index - 1) as i32);
                        }
                    }
                    4 => {
                        if index + 1 < s.jobs.len() {
                            s.jobs.swap(index, index + 1);
                            u.set_queue_index((index + 1) as i32);
                        }
                    }
                    5 => {
                        if index < s.jobs.len() {
                            if matches!(s.jobs[index].status, Status::Queued(_)) {
                                u.set_status("Этот элемент уже передан. Для повтора выберите его и добавьте как новый.".into());
                                return;
                            }
                            match crate::queue::parse_pages(
                                &u.get_page_range(),
                                u.get_pages() as u16,
                            ) {
                                Ok(pages) => {
                                    let p = s.presets[s.settings.preset].clone();
                                    let printer = s.settings.printer.clone();
                                    s.jobs[index].preset = p;
                                    s.jobs[index].printer = printer;
                                    s.jobs[index].copies = u.get_copies() as u16;
                                    s.jobs[index].pages = pages;
                                    s.jobs[index].status = Status::Prepared;
                                }
                                Err(e) => report(&u, Err(e)),
                            }
                        }
                    }
                    6 => {
                        for j in &mut s.jobs {
                            if j.status == Status::Prepared {
                                j.status = Status::Cancelled;
                            }
                        }
                    }
                    9 => {
                        let copies = u.get_copies();
                        if copies > 0 {
                            for job in &mut s.jobs {
                                if !matches!(job.status, Status::Queued(_)) { job.copies = copies as u16; job.status = Status::Prepared; }
                            }
                        }
                    }
                    _ => {}
                }
                s.refresh_queue(&u);
            }
        });
    }
    {
        let state = state.clone();
        let weak = ui.as_weak();
        ui.on_select_job(move |index| {
            if let Some(u) = weak.upgrade() {
                if u.get_busy() {
                    return;
                }
                let mut s = state.borrow_mut();
                if let Some(job) = s.jobs.get(index.max(0) as usize).cloned() {
                    let preset_index =
                        if let Some(i) = s.presets.iter().position(|p| p == &job.preset) {
                            i
                        } else {
                            s.presets.push(job.preset.clone());
                            s.presets.len() - 1
                        };
                    s.settings.preset = preset_index;
                    s.settings.printer = job.printer.clone();
                    s.path = Some(job.path.clone());
                    s.refresh_presets(&u);
                    u.set_printer(job.printer.into());
                    u.set_copies(job.copies as i32);
                    u.set_page_range(
                        job.pages
                            .iter()
                            .map(|p| (p + 1).to_string())
                            .collect::<Vec<_>>()
                            .join(",")
                            .into(),
                    );
                    u.set_filename(
                        job.path
                            .file_name()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .as_ref()
                            .into(),
                    );
                    u.set_page(1);
                    u.set_preview(slint::Image::default());
                    s.render(&u, None);
                }
            }
        });
    }
    let timer = slint::Timer::default();
    {
        let editor = editor.clone();
        let state = state.clone();
        let weak = ui.as_weak();
        timer.start(
            slint::TimerMode::Repeated,
            std::time::Duration::from_millis(40),
            move || {
                let Some(u) = weak.upgrade() else {
                    return;
                };
                for event in event_rx.try_iter() {
                    match event {
                        Event::Preview {
                            pixels,
                            count,
                            revision,
                            to_editor,
                        } => {
                            if revision != state.borrow().revision {
                                continue;
                            }
                            let buffer =
                                slint::SharedPixelBuffer::<slint::Rgba8Pixel>::clone_from_slice(
                                    pixels.as_raw(),
                                    pixels.width(),
                                    pixels.height(),
                                );
                            let image = slint::Image::from_rgba8(buffer);
                            if to_editor {
                                if let Some(e) = editor.borrow().as_ref() {
                                    e.set_preview(image);
                                    e.set_status("Превью готово".into());
                                }
                            } else {
                                u.set_preview(image);
                                u.set_pages(count as i32);
                                u.set_status("Превью готово".into());
                            }
                        }
                        Event::Error(text) => u.set_status(format!("Ошибка: {text}").into()),
                        Event::Printed(jobs, text) => {
                            let mut s = state.borrow_mut();
                            s.complete_batch_job(&u, &jobs);
                            s.history.extend(jobs);
                            let count = s.history.len();
                            if count > 200 {
                                s.history.drain(0..count - 200);
                            }
                            if let Err(error) =
                                crate::storage::atomic(&s.dir.join("history.json"), &s.history)
                            {
                                tracing::error!("History: {error:#}");
                            }
                            s.refresh_queue(&u);
                            u.set_busy(s.batch.is_some());
                            s.refresh_batch(&u);
                            if let Some(e) = editor.borrow().as_ref() {
                                e.set_status(text.clone().into());
                            }
                            u.set_status(text.into());
                        }
                        Event::Printers(printers) => u.set_printers(
                            Rc::new(slint::VecModel::from(
                                printers
                                    .into_iter()
                                    .map(slint::SharedString::from)
                                    .collect::<Vec<_>>(),
                            ))
                            .into(),
                        ),
                        Event::Probe(text) => u.set_printer_info(text.into()),
                    }
                }
                state.borrow().refresh_batch(&u);
            },
        );
    }
    {
        use slint::winit_030::{winit, WinitWindowAccessor};
        let state = state.clone();
        let weak = ui.as_weak();
        ui.window().on_winit_window_event(move |_, event| {
            if let Some(u) = weak.upgrade() {
                if let winit::event::WindowEvent::DroppedFile(path) = event {
                    if path
                        .extension()
                        .is_some_and(|e| e.eq_ignore_ascii_case("pdf"))
                    {
                        if u.get_busy() || u.get_editor() {
                            u.set_status(
                                "Завершите текущую операцию и добавьте файл повторно".into(),
                            );
                        } else {
                            state.borrow_mut().load_paths(&u, vec![path.clone()], true);
                        }
                    }
                }
            }
            slint::winit_030::EventResult::Propagate
        });
    }
    if let Some([x, y]) = state.borrow().settings.position {
        ui.window().set_position(slint::PhysicalPosition::new(x, y));
    }
    ui.window().set_size(slint::PhysicalSize::new(
        state.borrow().settings.width,
        state.borrow().settings.height,
    ));
    {
        let state = state.clone();
        let weak = ui.as_weak();
        ui.window().on_close_requested(move || {
            if let Some(u) = weak.upgrade() {
                if u.get_busy() {
                    u.set_status(
                        "Завершите передачу или остановите очередь перед закрытием".into(),
                    );
                    return slint::CloseRequestResponse::KeepWindowShown;
                }
                let size = u.window().size();
                let mut s = state.borrow_mut();
                let position = u.window().position();
                s.settings.position = Some([position.x, position.y]);
                s.settings.width = size.width;
                s.settings.height = size.height;
                report(&u, s.persist());
            }
            slint::CloseRequestResponse::HideWindow
        });
    }
    state.borrow().tx.send(Task::Printers)?;
    if !state.borrow().settings.printer.is_empty() {
        state
            .borrow()
            .tx
            .send(Task::Probe(state.borrow().settings.printer.clone()))?;
    }
    ui.run()?;
    state.borrow().persist()?;
    Ok(())
}

#[cfg(test)]
pub(crate) fn verify_paced_ui(ui: &MainWindow) {
    let (tx, rx) = mpsc::channel();
    let job = |name: &str, copies| Job {
        path: name.into(),
        printer: "Mock printer".into(),
        preset: Preset::default(),
        copies,
        pages: vec![],
        status: Status::Prepared,
    };
    let mut s = State {
        dir: PathBuf::new(),
        settings: Default::default(),
        presets: vec![Preset::default()],
        path: None,
        jobs: vec![job("first.pdf", 25), job("second.pdf", 50)],
        history: vec![],
        queue_rows: Rc::new(slint::VecModel::default()),
        pending_queue: None,
        batch: Some(crate::queue::PacedQueue::new(2, std::time::Duration::ZERO).unwrap()),
        clock: std::time::Instant::now(),
        cancel: Default::default(),
        revision: 0,
        tx,
        latest: Default::default(),
    };
    s.dispatch_next(ui).unwrap();
    let Task::Print(mut jobs) = rx.try_recv().unwrap() else {
        panic!("Expected print task")
    };
    assert_eq!(jobs.len(), 1);
    assert_eq!(jobs[0].copies, 25);
    jobs[0].status = Status::Queued(1);
    s.complete_batch_job(ui, &jobs);
    assert!(ui.get_batch_paused());
    assert!(ui.get_busy());
    assert!(rx.try_recv().is_err(), "No automatic dispatch during pause");
    assert_eq!(s.jobs[1].status, Status::Prepared);
    s.dispatch_next(ui).unwrap();
    assert!(!ui.get_batch_paused());
    let Task::Print(mut jobs) = rx.try_recv().unwrap() else {
        panic!("Expected next print task")
    };
    assert_eq!(jobs.len(), 1);
    assert_eq!(jobs[0].path, PathBuf::from("second.pdf"));
    assert_eq!(jobs[0].copies, 50);
    jobs[0].status = Status::Queued(2);
    s.complete_batch_job(ui, &jobs);
    assert!(!ui.get_batch_active());
    assert!(!ui.get_busy());
    assert_eq!(s.jobs[0].status, Status::Queued(1));
    s.batch = Some(crate::queue::PacedQueue::new(2, std::time::Duration::ZERO).unwrap());
    s.dispatch_next(ui).unwrap();
    let Task::Print(mut jobs) = rx.try_recv().unwrap() else {
        panic!("Expected print task")
    };
    jobs[0].status = Status::Failed("Mock error".into());
    s.complete_batch_job(ui, &jobs);
    assert!(!ui.get_batch_active());
    assert_eq!(s.jobs[1].status, Status::Cancelled);
    assert!(rx.try_recv().is_err());
    s.batch = Some(crate::queue::PacedQueue::new(2, std::time::Duration::ZERO).unwrap());
    s.dispatch_next(ui).unwrap();
    let Task::Print(mut jobs) = rx.try_recv().unwrap() else {
        panic!("Expected print task")
    };
    jobs[0].status = Status::Queued(3);
    s.complete_batch_job(ui, &jobs);
    s.stop_batch(ui);
    assert!(!ui.get_busy());
    assert!(!ui.get_batch_active());
    assert_eq!(s.jobs[1].status, Status::Cancelled);
    assert!(rx.try_recv().is_err());
    s.jobs = vec![job("one.pdf", 1), job("two.pdf", 1)];
    s.batch = Some(
        crate::queue::PacedQueue::from_indices(
            crate::queue::pending_job_indices(&s.jobs),
            std::time::Duration::ZERO,
        )
        .unwrap(),
    );
    for id in [10, 11] {
        s.dispatch_next(ui).unwrap();
        let Task::Print(mut sent) = rx.try_recv().unwrap() else {
            panic!("Expected one party")
        };
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0].copies, 1);
        assert!(rx.try_recv().is_err());
        sent[0].status = Status::Queued(id);
        s.complete_batch_job(ui, &sent);
    }
    assert!(
        crate::queue::pending_job_indices(&s.jobs).is_empty(),
        "Completed jobs cannot be resent by queue start"
    );
    s.edit_queue_copies(ui, 0, 50);
    assert_eq!(s.jobs[0].copies, 1, "Sent rows are immutable");
    s.jobs = vec![job("edit.pdf", 50)];
    s.path = Some("edit.pdf".into());
    ui.set_queue_index(0);
    s.edit_queue_copies(ui, 0, 1);
    assert_eq!(s.jobs[0].copies, 1);
    let mut single = s.jobs[0].clone();
    s.pending_queue = s.matching_pending_job(ui, &single);
    assert_eq!(s.pending_queue, Some(0));
    single.status = Status::Queued(12);
    s.complete_batch_job(ui, &[single]);
    assert!(
        crate::queue::pending_job_indices(&s.jobs).is_empty(),
        "Single print consumes the matching queued entry"
    );
    ui.set_copies(0);
    ui.set_queue_progress("".into());
    ui.set_queue_count(0);
}
