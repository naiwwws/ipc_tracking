use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;

use crate::devices::{traits::DeviceData, RpmChannelData};

// MINIMAL: Essential flowmeter reading structure
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct FlowmeterReading {
    pub id: Option<i64>,
    pub device_address: u8,
    pub unix_timestamp: i64,  // Changed from unix_ts
    
    // Core measurement data
    pub mass_flow_rate: f32,
    pub density_flow: f32,
    pub temperature: f32,
    pub volume_flow_rate: f32,
    pub mass_total: f32,
    pub volume_total: f32,
    pub error_code: u16,
}

#[derive(Debug, Clone, sqlx::FromRow, Serialize, Deserialize)]
pub struct RpmReading {
    pub id: Option<i64>,
    pub device_address: u8,
    // Multi-channel RPM data
    pub channel: u8,
    
    // Engine duration tracking
    pub engine_duration_minutes: u64,
    pub channel_data: String, // JSON string for channel data
    // Status
    pub global_error_code: u16,
    
    // Metadata
    pub created_at: Option<i64>,
}

impl RpmReading {
    pub fn from_rpm_data(device_address: u8, rpm_data: &RpmChannelData, engine_duration: u64) -> Self {
        let channel_data = serde_json::to_string(&rpm_data.channel_id).unwrap_or("[]".to_string());

        Self {
            id: None,
            device_address,
            channel: rpm_data.channel_id,
            engine_duration_minutes: engine_duration,
            channel_data,
            global_error_code: rpm_data.error_code,
            created_at: Some(Utc::now().timestamp()),
        }
    }
}

// Keep minimal stats structure
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct FlowmeterStats {
    pub total_readings: i64,
    pub avg_mass_flow_rate: Option<f32>,
    pub max_mass_flow_rate: Option<f32>,
    pub min_mass_flow_rate: Option<f32>,
    pub avg_temperature: Option<f32>,
    pub latest_timestamp: Option<i64>,
    pub earliest_timestamp: Option<i64>,
}

#[derive(Debug, Clone, sqlx::FromRow, Serialize, Deserialize)]
pub struct RpmStats {
    pub total_readings: i64,
    pub avg_rpm: f64,
    pub max_rpm: u16,
    pub min_rpm: u16,
    pub total_engine_hours: f64,
    pub latest_timestamp: i64,
    pub earliest_timestamp: i64,
    pub active_engines_count: i64,
}

// MTWS Data Structures
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MtwsField {
    #[serde(rename = "Value")]
    pub value: String,
    #[serde(rename = "Name")]
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MtwsPayload {
    #[serde(rename = "SIN")]
    pub sin: u16,
    #[serde(rename = "Name")]
    pub name: String,
    #[serde(rename = "IsForward")]
    pub is_forward: bool,
    #[serde(rename = "Fields")]
    pub fields: Vec<MtwsField>,
    #[serde(rename = "MIN")]
    pub min: u16,
}

// MINIMAL: Constructor for FlowmeterReading
impl FlowmeterReading {
    pub fn from_flowmeter_data(
        device_address: u8,
        flowmeter_data: &crate::devices::flowmeter::FlowmeterData,
    ) -> Self {
        Self {
            id: None,
            device_address,
            unix_timestamp: flowmeter_data.unix_ts(), // Fixed method name
            mass_flow_rate: flowmeter_data.mass_flow_rate,
            density_flow: flowmeter_data.density_flow,
            temperature: flowmeter_data.temperature,
            volume_flow_rate: flowmeter_data.volume_flow_rate,
            mass_total: flowmeter_data.mass_total,
            volume_total: flowmeter_data.volume_total,
            error_code: flowmeter_data.error_code,
        }
    }
}

// Constructor for MtwsField
impl MtwsField {
    pub fn new(name: String, value: String) -> Self {
        Self { value, name }
    }
}

// Constructor and builder for MtwsPayload
impl MtwsPayload {
    pub fn new() -> Self {
        Self {
            sin: 128,
            name: "ReportMsg".to_string(),
            is_forward: false,
            fields: Vec::new(),
            min: 1,
        }
    }
    
    pub fn add_field(&mut self, name: String, value: String) {
        self.fields.push(MtwsField::new(name, value));
    }
}

#[derive(Debug, Clone, sqlx::FromRow, Serialize, Deserialize)]
pub struct CombinedDeviceReading {
    pub id: Option<i64>,
    pub vessel_id: String,
    pub reading_timestamp: i64,
    
    // GPS Data
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
    pub speed: Option<f32>,
    pub course: Option<f32>,
    pub altitude: Option<f32>,
    pub satellites: Option<u8>,
    
    // Flowmeter Data (JSON for multiple flowmeters)
    pub flowmeter_data: Option<String>,
    
    // RPM Data (JSON for multiple engines/channels)
    pub rpm_data: Option<String>,
    
    // Engine Durations (JSON for all engines)
    pub engine_durations: Option<String>,
    
    // Environmental Data
    pub wind_speed: Option<f32>,
    pub wind_direction: Option<f32>,
    
    // Power Data
    pub battery_voltage: Option<f32>,
    pub external_power_voltage: Option<f32>,
    
    // Status Data
    pub status_flags: Option<String>, // JSON for various status flags
    
    pub created_at: Option<i64>,
}

// Add these new models

#[derive(Debug, Clone, sqlx::FromRow, Serialize, Deserialize)]
pub struct EngineDuration {
    pub id: Option<i64>,
    pub device_address: u8,
    pub channel_id: u8,
    pub engine_address: u8, // device_address * 100 + channel_id
    pub duration_seconds: u64,
    pub last_rpm_value: u16,
    pub is_running: bool,
    pub engine_type: String,
    pub rpm_threshold: u16,
    pub last_updated: i64,
    pub created_at: i64,
}

impl EngineDuration {
    pub fn new(device_address: u8, channel_id: u8, engine_type: String, rpm_threshold: u16) -> Self {
        let now = Utc::now().timestamp();
        Self {
            id: None,
            device_address,
            channel_id,
            engine_address: (device_address as u16 * 100 + channel_id as u16) as u8,
            duration_seconds: 0,
            last_rpm_value: 0,
            is_running: false,
            engine_type,
            rpm_threshold,
            last_updated: now,
            created_at: now,
        }
    }

    pub fn duration_minutes(&self) -> u64 {
        self.duration_seconds / 60
    }

    pub fn duration_hours(&self) -> f32 {
        self.duration_seconds as f32 / 3600.0
    }
}

#[derive(Debug, Clone, sqlx::FromRow, Serialize, Deserialize)]
pub struct EngineDurationHistory {
    pub id: Option<i64>,
    pub engine_address: u8,
    pub device_address: u8,
    pub channel_id: u8,
    pub duration_seconds_before: u64,
    pub duration_seconds_after: u64,
    pub rpm_value: u16,
    pub is_running: bool,
    pub change_reason: String,
    pub timestamp: i64,
}
