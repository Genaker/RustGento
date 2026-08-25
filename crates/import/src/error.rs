use thiserror::Error;

#[derive(Debug, Error)]
pub enum ImportError {
    #[error("CSV error: {0}")]
    Csv(#[from] csv::Error),
    #[error("CSV must contain a 'sku' column")]
    MissingSkuColumn,
    #[error("CSV has no header row")]
    EmptyCsv,
    #[error("database error: {0}")]
    Db(#[from] sqlx::Error),
}
