use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;

use crate::devices::traits::DeviceData;

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

// Add GPS import
use crate::devices::gps::GpsData;
