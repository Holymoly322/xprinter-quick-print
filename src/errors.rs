#[derive(Debug, thiserror::Error)]
pub enum PrintError {
    #[error("Принтер не выбран")]
    MissingPrinter,
    #[error("Допустимо 1–9999 копий")]
    InvalidCopies,
    #[error("Передача отменена")]
    Cancelled,
}
