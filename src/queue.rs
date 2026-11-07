pub const MAX_COPIES: u16 = 9999;
use anyhow::{ensure, Result};
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum Status {
    Prepared,
    Queued(i32),
    Failed(String),
    Cancelled,
}
#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct Job {
    pub path: std::path::PathBuf,
    pub printer: String,
    pub preset: crate::layout::Preset,
    pub copies: u16,
    #[serde(default)]
    pub pages: Vec<u16>,
    pub status: Status,
}
pub trait Backend {
    fn submit(&self, job: &Job) -> Result<i32>;
}
#[derive(Default)]
pub struct Windows {
    pub cancel: std::sync::Arc<std::sync::atomic::AtomicBool>,
}
impl Backend for Windows {
    fn submit(&self, j: &Job) -> Result<i32> {
        crate::printing::print_cancellable(
            &j.path,
            &j.printer,
            &j.preset,
            j.copies,
            &j.pages,
            &self.cancel,
        )
    }
}
pub fn submit(backend: &impl Backend, job: &mut Job) {
    job.status = match validate(job).and_then(|_| backend.submit(job)) {
        Ok(id) => Status::Queued(id),
        Err(e) => {
            if matches!(
                e.downcast_ref::<crate::errors::PrintError>(),
                Some(crate::errors::PrintError::Cancelled)
            ) {
                Status::Cancelled
            } else {
                Status::Failed(e.to_string())
            }
        }
    };
}
fn validate(j: &Job) -> Result<()> {
    ensure!(
        !j.printer.trim().is_empty(),
        crate::errors::PrintError::MissingPrinter
    );
    ensure!(
        (1..=MAX_COPIES).contains(&j.copies),
        crate::errors::PrintError::InvalidCopies
    );
    crate::layout::calculate([100., 100.], &j.preset)?;
    Ok(())
}

pub fn parse_pages(text: &str, count: u16) -> Result<Vec<u16>> {
    if text.trim().is_empty() {
        return Ok(vec![]);
    }
    let mut result = vec![];
    for group in text.split(',') {
        let parts = group
            .trim()
            .split('-')
            .map(|v| v.trim().parse::<u16>())
            .collect::<std::result::Result<Vec<_>, _>>()?;
        ensure!(
            !parts.is_empty() && parts.len() <= 2,
            "Неверный диапазон страниц"
        );
        let start = parts[0];
        let end = *parts.last().unwrap();
        ensure!(
            start > 0 && end >= start && end <= count,
            "Страницы должны быть в диапазоне 1–{count}"
        );
        for page in start..=end {
            if !result.contains(&(page - 1)) {
                result.push(page - 1);
            }
        }
    }
    Ok(result)
}

impl Status {
    pub fn label(&self) -> String {
        match self {
            Self::Prepared => "Подготовлено".into(),
            Self::Queued(id) => format!("Передано в очередь №{id}"),
            Self::Failed(e) => format!("Ошибка: {e}"),
            Self::Cancelled => "Отменено".into(),
        }
    }
}

pub fn increment_copies(current: u16, amount: u16) -> Result<u16> {
    ensure!(amount > 0, crate::errors::PrintError::InvalidCopies);
    current
        .checked_add(amount)
        .filter(|value| *value <= MAX_COPIES)
        .ok_or_else(|| crate::errors::PrintError::InvalidCopies.into())
}

/// Manual batch gate. Time alone never dispatches another job.
pub struct PacedQueue {
    remaining: std::collections::VecDeque<usize>,
    current: Option<usize>,
    ready_at: std::time::Duration,
    delay: std::time::Duration,
}
impl PacedQueue {
    pub fn new(count: usize, delay: std::time::Duration) -> Result<Self> {
        Self::from_indices((0..count).collect(), delay)
    }
    pub fn from_indices(indices: Vec<usize>, delay: std::time::Duration) -> Result<Self> {
        ensure!(!indices.is_empty(), "Нет новых элементов для отправки");
        ensure!(
            delay <= std::time::Duration::from_secs(300),
            "Пауза должна быть от 0 до 300 секунд"
        );
        Ok(Self {
            remaining: indices.into(),
            current: None,
            ready_at: std::time::Duration::ZERO,
            delay,
        })
    }
    pub fn take_next(&mut self, now: std::time::Duration) -> Result<usize> {
        ensure!(self.current.is_none(), "Предыдущая партия ещё передаётся");
        ensure!(now >= self.ready_at, "Дождитесь окончания паузы");
        let index = self
            .remaining
            .pop_front()
            .ok_or_else(|| anyhow::anyhow!("Очередь завершена"))?;
        self.current = Some(index);
        Ok(index)
    }
    pub fn complete(&mut self, now: std::time::Duration) -> Result<()> {
        ensure!(self.current.take().is_some(), "Нет отправляемой партии");
        self.ready_at = now.saturating_add(self.delay);
        Ok(())
    }
    pub fn has_remaining(&self) -> bool {
        !self.remaining.is_empty()
    }
    pub fn pending_indices(&self) -> impl Iterator<Item = usize> {
        self.remaining.clone().into_iter()
    }
    pub fn wait_seconds(&self, now: std::time::Duration) -> u32 {
        self.ready_at
            .saturating_sub(now)
            .as_nanos()
            .div_ceil(1_000_000_000) as u32
    }
}
pub fn validate_batch(jobs: &[Job]) -> Result<()> {
    ensure!(!jobs.is_empty(), "Очередь пуста");
    for job in jobs {
        validate(job).map_err(|error| anyhow::anyhow!("{}: {error}", job.path.display()))?;
    }
    Ok(())
}
pub fn pending_job_indices(jobs: &[Job]) -> Vec<usize> {
    jobs.iter()
        .enumerate()
        .filter(|(_, job)| !matches!(job.status, Status::Queued(_)))
        .map(|(i, _)| i)
        .collect()
}
#[cfg(test)]
mod tests {
    use super::*;
    struct Mock;
    impl Backend for Mock {
        fn submit(&self, _: &Job) -> Result<i32> {
            Ok(42)
        }
    }
    fn job() -> Job {
        Job {
            path: "x.pdf".into(),
            printer: "mock".into(),
            preset: Default::default(),
            copies: 10,
            pages: vec![],
            status: Status::Prepared,
        }
    }
    #[test]
    fn mock() {
        let mut j = job();
        submit(&Mock, &mut j);
        assert_eq!(j.status, Status::Queued(42));
    }
    #[test]
    fn missing_printer() {
        let mut j = job();
        j.printer.clear();
        submit(&Mock, &mut j);
        assert!(matches!(j.status, Status::Failed(_)));
    }
}
#[cfg(test)]
mod page_tests {
    use super::*;
    #[test]
    fn ranges() {
        assert_eq!(parse_pages("1-3, 5, 2", 5).unwrap(), vec![0, 1, 2, 4]);
        assert!(parse_pages("0", 3).is_err());
        assert!(parse_pages("3-1", 3).is_err());
        assert!(parse_pages("4", 3).is_err());
    }
}
#[cfg(test)]
mod increments {
    use super::*;
    #[test]
    fn accumulates_each_press() {
        let mut n = 0;
        for increment in [25, 25, 50, 100] {
            n = increment_copies(n, increment).unwrap();
        }
        assert_eq!(n, 200);
        assert_eq!(increment_copies(0, 100).unwrap(), 100);
        assert!(increment_copies(MAX_COPIES, 25).is_err());
    }
}

#[cfg(test)]
mod paced_tests {
    use super::*;
    use std::time::Duration as D;
    #[test]
    fn waits_for_explicit_continue_and_cooldown() {
        let mut queue = PacedQueue::new(3, D::from_secs(5)).unwrap();
        assert_eq!(queue.take_next(D::ZERO).unwrap(), 0);
        assert!(queue.take_next(D::from_secs(100)).is_err());
        queue.complete(D::from_secs(2)).unwrap();
        assert_eq!(queue.wait_seconds(D::from_millis(6500)), 1);
        assert!(queue.take_next(D::from_secs(6)).is_err());
        assert_eq!(queue.pending_indices().collect::<Vec<_>>(), vec![1, 2]);
        assert_eq!(queue.take_next(D::from_secs(100)).unwrap(), 1);
        assert!(queue.take_next(D::from_secs(100)).is_err());
        queue.complete(D::from_secs(100)).unwrap();
        assert_eq!(queue.take_next(D::from_secs(105)).unwrap(), 2);
        queue.complete(D::from_secs(105)).unwrap();
        assert!(!queue.has_remaining());
        assert!(queue.take_next(D::from_secs(200)).is_err());
    }
    #[test]
    fn stop_keeps_submitted_job_and_skips_remaining() {
        let mut queue = PacedQueue::new(3, D::ZERO).unwrap();
        queue.take_next(D::ZERO).unwrap();
        assert_eq!(queue.pending_indices().collect::<Vec<_>>(), vec![1, 2]);
        assert!(PacedQueue::new(0, D::ZERO).is_err());
        assert!(PacedQueue::new(1, D::from_secs(301)).is_err());
        assert!(validate_batch(&[]).is_err());
    }
}

#[cfg(test)]
mod retry_tests {
    use super::*;
    #[test]
    fn already_submitted_items_are_not_replayed() {
        let make = |status| Job {
            path: "label.pdf".into(),
            printer: "Mock".into(),
            preset: Default::default(),
            copies: 1,
            pages: vec![],
            status,
        };
        let jobs = vec![make(Status::Queued(9)), make(Status::Prepared)];
        assert_eq!(pending_job_indices(&jobs), vec![1]);
        let mut plan =
            PacedQueue::from_indices(pending_job_indices(&jobs), std::time::Duration::ZERO)
                .unwrap();
        assert_eq!(plan.take_next(std::time::Duration::ZERO).unwrap(), 1);
        plan.complete(std::time::Duration::ZERO).unwrap();
        assert!(!plan.has_remaining());
        assert!(pending_job_indices(&[make(Status::Queued(9))]).is_empty());
    }
}
