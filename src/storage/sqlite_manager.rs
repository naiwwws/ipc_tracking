use chrono::{Utc};
use log::{info, warn};
use sqlx::{SqlitePool, Row};
use std::path::Path;
use std::time::Duration;

use crate::config::settings::SqliteConfig;
use crate::storage::models::{FlowmeterReading, FlowmeterStats, RpmReading, CombinedDeviceReading};
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

    // Update schema creation with new tables
    async fn initialize_schema(&self) -> Result<(), ModbusError> {
        info!("🔧 Initializing database schema...");

        // Existing flowmeter_readings table
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
                mass_inventory REAL NOT NULL,
                volume_inventory REAL NOT NULL,
                volume_total REAL NOT NULL,
                error_code INTEGER NOT NULL DEFAULT 0
            )
        "#)
        .execute(&self.pool)
        .await?;

        // NEW: Multi-channel RPM readings table
        sqlx::query(r#"
            CREATE TABLE IF NOT EXISTS rpm_readings_v2 (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                device_address INTEGER NOT NULL,
                unix_timestamp INTEGER NOT NULL,
                total_channels INTEGER NOT NULL,
                channel_data TEXT NOT NULL, -- JSON as TEXT in SQLite
                engine_duration_seconds INTEGER DEFAULT 0,
                running_engines_count INTEGER DEFAULT 0,
                device_status TEXT DEFAULT 'Unknown',
                global_error_code INTEGER DEFAULT 0,
                created_at INTEGER DEFAULT (strftime('%s', 'now'))
            )
        "#)
        .execute(&self.pool)
        .await?;

        // NEW: Combined device readings table
        sqlx::query(r#"
            CREATE TABLE IF NOT EXISTS combined_device_readings (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                vessel_id TEXT NOT NULL,
                reading_timestamp INTEGER NOT NULL,
                
                -- GPS Data
                latitude REAL,
                longitude REAL,
                speed REAL,
                course REAL,
                altitude REAL,
                satellites INTEGER,
                
                -- Multi-device data as JSON/TEXT
                flowmeter_data TEXT, -- JSON as TEXT
                rpm_data TEXT,       -- JSON as TEXT
                engine_durations TEXT, -- JSON as TEXT
                
                -- Environmental
                wind_speed REAL,
                wind_direction REAL,
                
                -- Power
                battery_voltage REAL,
                external_power_voltage REAL,
                
                -- Status
                status_flags TEXT, -- JSON as TEXT
                
                created_at INTEGER DEFAULT (strftime('%s', 'now'))
            )
        "#)
        .execute(&self.pool)
        .await?;

        // Create engine_durations table for RPM tracking
        sqlx::query(r#"
            CREATE TABLE IF NOT EXISTS engine_durations (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                device_address INTEGER NOT NULL,
                channel_id INTEGER NOT NULL,
                engine_address INTEGER NOT NULL UNIQUE, -- device_address * 100 + channel_id
                duration_seconds INTEGER NOT NULL DEFAULT 0,
                last_rpm_value INTEGER DEFAULT 0,
                is_running BOOLEAN DEFAULT FALSE,
                engine_type TEXT DEFAULT 'engine',
                rpm_threshold INTEGER DEFAULT 500,
                last_updated INTEGER NOT NULL,
                created_at INTEGER NOT NULL,
                
                -- Composite unique constraint
                UNIQUE(device_address, channel_id)
            )
        "#)
        .execute(&self.pool)
        .await?;

        // Create engine_duration_history for tracking changes
        sqlx::query(r#"
            CREATE TABLE IF NOT EXISTS engine_duration_history (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                engine_address INTEGER NOT NULL,
                device_address INTEGER NOT NULL,
                channel_id INTEGER NOT NULL,
                duration_seconds_before INTEGER NOT NULL,
                duration_seconds_after INTEGER NOT NULL,
                rpm_value INTEGER NOT NULL,
                is_running BOOLEAN NOT NULL,
                change_reason TEXT, -- 'running', 'stopped', 'reset', 'manual'
                timestamp INTEGER NOT NULL,
                
                FOREIGN KEY(engine_address) REFERENCES engine_durations(engine_address)
            )
        "#)
        .execute(&self.pool)
        .await?;

        // NEW: Device status table
        sqlx::query(r#"
            CREATE TABLE IF NOT EXISTS device_status (
                device_uuid TEXT PRIMARY KEY,
                device_address INTEGER NOT NULL,
                last_seen INTEGER NOT NULL,
                status TEXT NOT NULL,
                error_count INTEGER DEFAULT 0,
                total_readings INTEGER DEFAULT 0,
                updated_at INTEGER NOT NULL
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

    // Enhanced migration with new table checks
    async fn migrate_schema(&self) -> Result<(), ModbusError> {
        info!("🔄 Checking for database schema migrations...");
        
        // Check if new tables exist and add any missing columns
        let tables_to_check = vec![
            ("rpm_readings_v2", "SELECT name FROM sqlite_master WHERE type='table' AND name='rpm_readings_v2'"),
            ("combined_device_readings", "SELECT name FROM sqlite_master WHERE type='table' AND name='combined_device_readings'"),
            ("engine_durations", "SELECT name FROM sqlite_master WHERE type='table' AND name='engine_durations'"),
            ("device_status", "SELECT name FROM sqlite_master WHERE type='table' AND name='device_status'"),
        ];

        for (table_name, check_query) in tables_to_check {
            let exists = sqlx::query(check_query)
                .fetch_optional(&self.pool)
                .await?
                .is_some();

            if !exists {
                info!("📋 Table '{}' doesn't exist, will be created", table_name);
            } else {
                info!("✅ Table '{}' exists", table_name);
            }
        }

        // Add any missing columns to existing tables (future migrations)
        self.add_missing_columns().await?;
        
        Ok(())
    }

    // Add missing columns for future migrations
    async fn add_missing_columns(&self) -> Result<(), ModbusError> {
        let migrations = vec![
            // Example: Add new column to existing flowmeter table if needed
            ("flowmeter_readings", "quality_indicator", "ALTER TABLE flowmeter_readings ADD COLUMN quality_indicator INTEGER DEFAULT 0"),
        ];

        for (table, column, alter_sql) in migrations {
            // Check if column exists
            let column_exists = sqlx::query(&format!(
                "SELECT name FROM pragma_table_info('{}') WHERE name = '{}'", 
                table, column
            ))
            .fetch_optional(&self.pool)
            .await?
            .is_some();

            if !column_exists {
                match sqlx::query(alter_sql).execute(&self.pool).await {
                    Ok(_) => info!("✅ Added column '{}' to table '{}'", column, table),
                    Err(e) => warn!("⚠️ Failed to add column '{}' to table '{}': {}", column, table, e),
                }
            }
        }

        Ok(())
    }

    // Enhanced index creation for all tables
    async fn create_minimal_indexes(&self) -> Result<(), ModbusError> {
        let indexes = vec![
            // Flowmeter indexes
            "CREATE INDEX IF NOT EXISTS idx_flowmeter_timestamp ON flowmeter_readings(unix_timestamp)",
            "CREATE INDEX IF NOT EXISTS idx_flowmeter_device_time ON flowmeter_readings(device_address, unix_timestamp)",
            
            // RPM indexes
            "CREATE INDEX IF NOT EXISTS idx_rpm_device_time ON rpm_readings_v2(device_address, unix_timestamp)",
            "CREATE INDEX IF NOT EXISTS idx_rpm_timestamp ON rpm_readings_v2(unix_timestamp)",
            "CREATE INDEX IF NOT EXISTS idx_rpm_running_engines ON rpm_readings_v2(running_engines_count)",
            "CREATE INDEX IF NOT EXISTS idx_rpm_status ON rpm_readings_v2(device_status)",
            
            // Combined readings indexes
            "CREATE INDEX IF NOT EXISTS idx_combined_vessel_time ON combined_device_readings(vessel_id, reading_timestamp)",
            "CREATE INDEX IF NOT EXISTS idx_combined_timestamp ON combined_device_readings(reading_timestamp)",
            
            // Engine durations indexes
            "CREATE INDEX IF NOT EXISTS idx_engine_device_channel ON engine_durations(device_address, channel_id)",
            "CREATE INDEX IF NOT EXISTS idx_engine_last_updated ON engine_durations(last_updated)",
            "CREATE INDEX IF NOT EXISTS idx_engine_address ON engine_durations(engine_address)",
            
            // Device status indexes
            "CREATE INDEX IF NOT EXISTS idx_device_status_address ON device_status(device_address)",
            "CREATE INDEX IF NOT EXISTS idx_device_status_last_seen ON device_status(last_seen)",
        ];

        for index_sql in indexes {
            match sqlx::query(index_sql).execute(&self.pool).await {
                Ok(_) => {},
                Err(e) => warn!("⚠️ Failed to create index: {} - {}", index_sql, e),
            }
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
                    temperature, volume_flow_rate, mass_total, volume_total, mass_inventory, volume_inventory, error_code
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#)
            .bind(reading.device_address)
            .bind(reading.unix_timestamp)
            .bind(reading.mass_flow_rate)
            .bind(reading.density_flow)
            .bind(reading.temperature)
            .bind(reading.volume_flow_rate)
            .bind(reading.mass_total)
            .bind(reading.volume_total)
            .bind(reading.mass_inventory)
            .bind(reading.volume_inventory)
            .bind(reading.volume_inventory)
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

    // NEW: Insert combined device readings
    pub async fn insert_combined_reading(&self, reading: &CombinedDeviceReading) -> Result<(), ModbusError> {
        sqlx::query(r#"
            INSERT INTO combined_device_readings (
                vessel_id, reading_timestamp, latitude, longitude, speed, course, altitude, satellites,
                flowmeter_data, rpm_data, engine_durations, wind_speed, wind_direction,
                battery_voltage, external_power_voltage, status_flags
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        "#)
        .bind(&reading.vessel_id)
        .bind(reading.reading_timestamp)
        .bind(reading.latitude)
        .bind(reading.longitude)
        .bind(reading.speed)
        .bind(reading.course)
        .bind(reading.altitude)
        .bind(reading.satellites)
        .bind(&reading.flowmeter_data)
        .bind(&reading.rpm_data)
        .bind(&reading.engine_durations)
        .bind(reading.wind_speed)
        .bind(reading.wind_direction)
        .bind(reading.battery_voltage)
        .bind(reading.external_power_voltage)
        .bind(&reading.status_flags)
        .execute(&self.pool)
        .await?;

        info!("💾 Inserted combined device reading for vessel {}", reading.vessel_id);
        Ok(())
    }

    pub async fn cleanup_old_data(&self, hours_to_keep: i64) -> Result<u64, ModbusError> {
        let cutoff_timestamp = (Utc::now() - chrono::Duration::hours(hours_to_keep)).timestamp();
        
        let mut total_deleted = 0u64;

        // Clean flowmeter readings
        let result = sqlx::query("DELETE FROM flowmeter_readings WHERE unix_timestamp < ?")
            .bind(cutoff_timestamp)
            .execute(&self.pool)
            .await?;
        total_deleted += result.rows_affected();

        // Clean RPM readings
        let result = sqlx::query("DELETE FROM rpm_readings_v2 WHERE unix_timestamp < ?")
            .bind(cutoff_timestamp)
            .execute(&self.pool)
            .await?;
        total_deleted += result.rows_affected();

        // Clean combined readings
        let result = sqlx::query("DELETE FROM combined_device_readings WHERE reading_timestamp < ?")
            .bind(cutoff_timestamp)
            .execute(&self.pool)
            .await?;
        total_deleted += result.rows_affected();

        // Vacuum database
        sqlx::query("VACUUM").execute(&self.pool).await?;

        info!("🧹 Cleaned up {} old records across all tables", total_deleted);
        Ok(total_deleted)
    }

    // NEW: Get database statistics
    pub async fn get_database_stats(&self) -> Result<DatabaseStats, ModbusError> {
        let flowmeter_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM flowmeter_readings")
            .fetch_one(&self.pool).await.unwrap_or(0);

            let combined_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM combined_device_readings")
            .fetch_one(&self.pool).await.unwrap_or(0);

        let active_devices: i64 = sqlx::query_scalar("SELECT COUNT(DISTINCT device_address) FROM device_status WHERE last_seen > ?")
            .bind(Utc::now().timestamp() - 3600) // Active in last hour
            .fetch_one(&self.pool).await.unwrap_or(0);

        Ok(DatabaseStats {
            total_readings: flowmeter_count + combined_count,
            flowmeter_readings: flowmeter_count,
            combined_readings: combined_count,
            active_devices,
            database_size_bytes: 0, // Would need filesystem access to calculate
            connection_count: 1, // SQLite is single connection
        })
    }


    pub async fn close(&self) {
        info!("🔒 Closing SQLite database connections");
        self.pool.close().await;
    }
}

#[derive(Debug)]
pub struct DatabaseStats {
    pub total_readings: i64,
    pub flowmeter_readings: i64,
    pub combined_readings: i64,
    pub active_devices: i64,
    pub database_size_bytes: u64,
    pub connection_count: usize,
}