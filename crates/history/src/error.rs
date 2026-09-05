//! Fixed operational errors prevent connection strings and SQL values from entering logs.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("invalid history configuration")]
    Configuration,
    #[error("history record violates its field bounds")]
    Record,
    #[error("PostgreSQL connection unavailable")]
    Connection,
    #[error("PostgreSQL pool deadline expired")]
    Pool,
    #[error("history migration failed")]
    Migration,
    #[error("history query failed")]
    Query(#[from] diesel::result::Error),
    #[error("history task was interrupted")]
    Task,
    #[error("history retention capacity unavailable")]
    Capacity,
}
impl Error {
    pub fn retryable(&self) -> bool {
        !matches!(self, Self::Record | Self::Configuration)
            && !matches!(
                self,
                Self::Query(diesel::result::Error::DatabaseError(
                    diesel::result::DatabaseErrorKind::CheckViolation
                        | diesel::result::DatabaseErrorKind::NotNullViolation
                        | diesel::result::DatabaseErrorKind::ForeignKeyViolation,
                    _
                ))
            )
    }
}
