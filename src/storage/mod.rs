#[cfg(feature = "sqlite")]
pub mod sqlite_manager;
#[cfg(feature = "sqlite")]
pub mod models;

#[cfg(feature = "sqlite")]
pub use models::{EngineDuration, EngineDurationHistory, FlowmeterReading, FlowmeterStats, RpmReading, RpmStats};
#[cfg(feature = "sqlite")]
pub use sqlite_manager::{DatabaseStats, SqliteManager};