use chrono::{Utc};
use log::{info};
use sqlx::{SqlitePool, Row};
use std::path::Path;
use std::time::Duration;

use crate::config::settings::SqliteConfig;
use crate::storage::models::{FlowmeterReading, FlowmeterStats};
use crate::utils::error::ModbusError;

#[derive(Clone)]
pub struct SqliteManager {
    pool: SqlitePool,
    config: SqliteConfig,
}

impl SqliteManager {
    pub async fn new(config: SqliteConfig) -> Result<Self, ModbusError> {
        // Create database directory if it doesn't exist
        if let Some(parent) = Path::new(&config.database_path).parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                ModbusError::CommunicationError(format!("Failed to create database directory: {}", e))
            })?;
        }
        info!("🗄️  Initializing SQLite database: {}", config.database_path);

        // Create connection pool with optimized settings
        let pool = SqlitePool::connect_with(
            sqlx::sqlite::SqliteConnectOptions::new()
                .filename(&config.database_path)
                .create_if_missing(true)
                .busy_timeout(Duration::from_millis(config.busy_timeout_ms))
                .journal_mode(if config.enable_wal {
                    sqlx::sqlite::SqliteJournalMode::Wal
                } else {
                    sqlx::sqlite::SqliteJournalMode::Delete
                })
                .synchronous(match config.sync_mode.as_str() {
                    "OFF" => sqlx::sqlite::SqliteSynchronous::Off,
                    "NORMAL" => sqlx::sqlite::SqliteSynchronous::Normal,
                    "FULL" => sqlx::sqlite::SqliteSynchronous::Full,
                    _ => sqlx::sqlite::SqliteSynchronous::Normal,
                })
        ).await.map_err(|e| {
            ModbusError::CommunicationError(format!("Failed to connect to SQLite: {}", e))
        })?;

        // Apply performance optimizations
        sqlx::query(&format!("PRAGMA cache_size = -{}", config.cache_size))
            .execute(&pool)
            .await
            .map_err(|e| ModbusError::CommunicationError(format!("Failed to set cache size: {}", e)))?;

        if config.auto_vacuum {
            sqlx::query("PRAGMA auto_vacuum = INCREMENTAL")
                .execute(&pool)
                .await
                .map_err(|e| ModbusError::CommunicationError(format!("Failed to set auto_vacuum: {}", e)))?;
        }

        let manager = Self {
            pool,
            config,
        };

        // Initialize database schema
        manager.initialize_schema().await?;

        info!("✅ SQLite database initialized successfully");
        Ok(manager)
    }

    // Update schema creation with Unix timestamp comments
    async fn initialize_schema(&self) -> Result<(), ModbusError> {
        info!("🔧 Initializing database schema...");

        sqlx::query(r#"
            CREATE TABLE IF NOT EXISTS flowmeter_readings (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                device_address INTEGER NOT NULL,
                unix_timestamp INTEGER NOT NULL,
                
                -- Core measurements
                mass_flow_rate REAL NOT NULL,
                density_flow REAL NOT NULL,
                temperature REAL NOT NULL,
                volume_flow_rate REAL NOT NULL,
                mass_total REAL NOT NULL,
                volume_total REAL NOT NULL,
                error_code INTEGER NOT NULL DEFAULT 0,
                
            )
        "#)
        .execute(&self.pool)
        .await?;

        // Run migration for existing databases
        self.migrate_schema().await?;
        self.create_minimal_indexes().await?;
        info!("✅ Database schema initialized");
        Ok(())
    }

    // Add migration for GPS columns
    async fn migrate_schema(&self) -> Result<(), ModbusError> {
        info!("🔄 Checking for database schema migrations...");
        
        
        Ok(())
    }

    async fn create_minimal_indexes(&self) -> Result<(), ModbusError> {
        let indexes = vec![
            // Flowmeter indexes
            "CREATE INDEX IF NOT EXISTS idx_timestamp ON flowmeter_readings(unix_timestamp)",
            "CREATE INDEX IF NOT EXISTS idx_device_time ON flowmeter_readings(device_address, unix_timestamp)",
        ];

        for index_sql in indexes {
            sqlx::query(index_sql)
                .execute(&self.pool)
                .await?;
        }

        info!("✅ Database indexes created");
        Ok(())
    }


    // Update device status efficiently
    pub async fn update_device_status(&self, device_uuid: &str, device_address: u8, status: &str) -> Result<(), ModbusError> {
        let now = Utc::now().timestamp();
        
        sqlx::query(r#"
            INSERT OR REPLACE INTO device_status (
                device_uuid, device_address, last_seen, status, 
                error_count, total_readings, updated_at
            ) VALUES (
                ?, ?, ?, ?,
                COALESCE((SELECT error_count FROM device_status WHERE device_uuid = ?), 0),
                COALESCE((SELECT total_readings FROM device_status WHERE device_uuid = ?), 0) + 1,
                ?
            )
        "#)
        .bind(device_uuid)
        .bind(device_address)
        .bind(now)
        .bind(status)
        .bind(device_uuid)
        .bind(device_uuid)
        .bind(now)
        .execute(&self.pool)
        .await
        .map_err(|e| ModbusError::CommunicationError(format!("Failed to update device status: {}", e)))?;

        Ok(())
    }

    // OPTIMIZED: Minimal batch insert
    pub async fn batch_insert_flowmeter_readings(&self, readings: Vec<FlowmeterReading>) -> Result<usize, ModbusError> {
        if readings.is_empty() {
            return Ok(0);
        }

        let batch_size = self.config.batch_size.max(500);
        let mut total_inserted = 0;

        for chunk in readings.chunks(batch_size) {
            let inserted = self.insert_minimal_chunk(chunk).await?;
            total_inserted += inserted;
        }

        info!("💾 Inserted {} flowmeter readings", total_inserted);
        Ok(total_inserted)
    }

    async fn insert_minimal_chunk(&self, readings: &[FlowmeterReading]) -> Result<usize, ModbusError> {
        let mut tx = self.pool.begin().await?;
        let mut inserted_count = 0;

        for reading in readings {
            let result = sqlx::query(r#"
                INSERT INTO flowmeter_readings (
                    device_address, unix_timestamp, mass_flow_rate, density_flow, 
                    temperature, volume_flow_rate, mass_total, volume_total, error_code
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#)
            .bind(reading.device_address)
            .bind(reading.unix_timestamp)
            .bind(reading.mass_flow_rate)
            .bind(reading.density_flow)
            .bind(reading.temperature)
            .bind(reading.volume_flow_rate)
            .bind(reading.mass_total)
            .bind(reading.volume_total)
            .bind(reading.error_code)
            .execute(&mut *tx)
            .await;

            if result.is_ok() {
                inserted_count += 1;
            }
        }

        tx.commit().await?;
        Ok(inserted_count)
    }

    pub async fn get_recent_flowmeter_readings(&self, limit: i64, offset: i64) -> Result<Vec<FlowmeterReading>, ModbusError> {
        let readings = sqlx::query_as::<_, FlowmeterReading>(r#"
            SELECT * FROM flowmeter_readings 
            ORDER BY unix_timestamp DESC 
            LIMIT ? OFFSET ?
        "#)
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.pool)
        .await?;

        Ok(readings)
    }

    pub async fn get_device_flowmeter_readings(
        &self, 
        device_address: u8, 
        start_time: Option<i64>, 
        end_time: Option<i64>,
        limit: Option<i64>
    ) -> Result<Vec<FlowmeterReading>, ModbusError> {
        let mut query = "SELECT * FROM flowmeter_readings WHERE device_address = ?".to_string();
        let mut bind_values: Vec<String> = vec![device_address.to_string()];

        if let Some(start) = start_time {
            query.push_str(" AND unix_timestamp >= ?");
            bind_values.push(start.to_string());
        }

        if let Some(end) = end_time {
            query.push_str(" AND unix_timestamp <= ?");
            bind_values.push(end.to_string());
        }

        query.push_str(" ORDER BY unix_timestamp DESC");

        if let Some(limit_val) = limit {
            query.push_str(" LIMIT ?");
            bind_values.push(limit_val.to_string());
        }

        let mut query_builder = sqlx::query_as::<_, FlowmeterReading>(&query);
        for value in bind_values {
            query_builder = query_builder.bind(value);
        }

        let readings = query_builder
            .fetch_all(&self.pool)
            .await?;

        Ok(readings)
    }

    pub async fn get_flowmeter_stats(&self) -> Result<FlowmeterStats, ModbusError> {
        let stats = sqlx::query_as::<_, FlowmeterStats>(r#"
            SELECT 
                COUNT(*) as total_readings,
                AVG(mass_flow_rate) as avg_mass_flow_rate,
                MAX(mass_flow_rate) as max_mass_flow_rate,
                MIN(mass_flow_rate) as min_mass_flow_rate,
                AVG(temperature) as avg_temperature,
                MAX(unix_timestamp) as latest_timestamp,
                MIN(unix_timestamp) as earliest_timestamp
            FROM flowmeter_readings
        "#)
        .fetch_one(&self.pool)
        .await?;

        Ok(stats)
    }

    pub async fn cleanup_old_data(&self, hours_to_keep: i64) -> Result<u64, ModbusError> {
        let cutoff_timestamp = (Utc::now() - chrono::Duration::hours(hours_to_keep)).timestamp();
        
        let result = sqlx::query("DELETE FROM flowmeter_readings WHERE unix_timestamp < ?")
            .bind(cutoff_timestamp)
            .execute(&self.pool)
            .await?;

        sqlx::query("VACUUM").execute(&self.pool).await?;

        info!("🧹 Cleaned up {} old records", result.rows_affected());
        Ok(result.rows_affected())
    }

    pub async fn close(&self) {
        info!("🔒 Closing SQLite database connections");
        self.pool.close().await;
    }
}

#[derive(Debug)]
pub struct DatabaseStats {
    pub total_readings: i64,
    pub active_devices: i64,
    pub database_size_bytes: u64,
    pub connection_count: usize,
}