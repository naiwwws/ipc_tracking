use async_trait::async_trait;
use chrono::{DateTime, Utc};
use log::{error, info, warn};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::any::Any;
use std::collections::HashMap;

use crate::devices::traits::{Device, DeviceData};
use crate::modbus::client::ModbusClientTrait;
use crate::utils::error::ModbusError;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AioModuleDevice {
    pub address: u8,
    pub name: String,
    pub location: String,
    pub update_interval_seconds: u64,
    pub timeout_ms: u64,
    pub channels: Vec<AioChannelConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AioChannelConfig {
    pub channel_id: u8,
    pub channel_name: String,
    pub channel_type: String, // "rpm", "pulse", "frequency"
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AioModuleData {
    pub device_address: u8,
    pub device_name: String,
    pub device_location: String,
    pub timestamp: DateTime<Utc>,
    pub baud_rate: u16,
    pub channels: Vec<AioChannelData>,
    pub digital_inputs: Vec<bool>, // DIN1-DIN16
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AioChannelData {
    pub channel_id: u8,
    pub pulse_count: u16,
    pub threshold: u16,
    pub frequency: u16,
    pub rpm_value: u16,
    pub average_value: u16,
    pub duration_rpm: u32,  // Engine running duration in RPM mode
    pub duration_ae: u32,   // Auxiliary engine duration
}

impl AioModuleDevice {
    pub fn new(
        address: u8,
        name: String,
        location: String,
        update_interval_seconds: u64,
        timeout_ms: u64,
    ) -> Self {
        // Default 4-channel configuration
        let mut channels = Vec::new();
        for i in 1..=4 {
            channels.push(AioChannelConfig {
                channel_id: i,
                channel_name: format!("Channel {}", i),
                channel_type: "rpm".to_string(),
                enabled: true,
            });
        }

        Self {
            address,
            name,
            location,
            update_interval_seconds,
            timeout_ms,
            channels,
        }
    }

    pub fn with_channels(mut self, channels: Vec<AioChannelConfig>) -> Self {
        self.channels = channels;
        self
    }

    fn parse_modbus_response(&self, raw_data: &[u8]) -> Result<AioModuleData, ModbusError> {
        if raw_data.len() < 78 {
            return Err(ModbusError::InvalidData(format!(
                "AIO module response too short: expected at least 78 bytes, got {}",
                raw_data.len()
            )));
        }

        // Parse baud rate from first 2 bytes
        let baud_rate = ((raw_data[0] as u16) << 8) | (raw_data[1] as u16);

        // Parse 4 channels of data
        let mut channels = Vec::new();
        for ch in 0..4 {
            let base_offset = ch * 2;
            
            // Pulse count (bytes 2-9)
            let pulse_count = ((raw_data[2 + base_offset] as u16) << 8) 
                            | (raw_data[3 + base_offset] as u16);
            
            // Threshold (bytes 10-17)
            let threshold = ((raw_data[10 + base_offset] as u16) << 8) 
                          | (raw_data[11 + base_offset] as u16);
            
            // Frequency (bytes 18-25)
            let frequency = ((raw_data[18 + base_offset] as u16) << 8) 
                          | (raw_data[19 + base_offset] as u16);
            
            // RPM value (bytes 26-33)
            let rpm_value = ((raw_data[26 + base_offset] as u16) << 8) 
                          | (raw_data[27 + base_offset] as u16);
            
            // Average value (bytes 34-41)
            let average_value = ((raw_data[34 + base_offset] as u16) << 8) 
                              | (raw_data[35 + base_offset] as u16);
            
            // Duration RPM (bytes 46-61, 4 bytes per channel)
            let dur_base = 46 + ch * 4;
            let duration_rpm = ((raw_data[dur_base] as u32) << 24)
                             | ((raw_data[dur_base + 1] as u32) << 16)
                             | ((raw_data[dur_base + 2] as u32) << 8)
                             | (raw_data[dur_base + 3] as u32);
            
            // Duration AE (bytes 62-77, 4 bytes per channel)
            let ae_base = 62 + ch * 4;
            let duration_ae = ((raw_data[ae_base] as u32) << 24)
                            | ((raw_data[ae_base + 1] as u32) << 16)
                            | ((raw_data[ae_base + 2] as u32) << 8)
                            | (raw_data[ae_base + 3] as u32);

            channels.push(AioChannelData {
                channel_id: (ch + 1) as u8,
                pulse_count,
                threshold,
                frequency,
                rpm_value,
                average_value,
                duration_rpm,
                duration_ae,
            });
        }

        // Parse digital inputs from bytes 42-43 (16 bits)
        let din_word = ((raw_data[42] as u16) << 8) | (raw_data[43] as u16);
        let mut digital_inputs = Vec::new();
        for i in 0..16 {
            digital_inputs.push((din_word >> i) & 0x1 == 1);
        }

        Ok(AioModuleData {
            device_address: self.address,
            device_name: self.name.clone(),
            device_location: self.location.clone(),
            timestamp: Utc::now(),
            baud_rate,
            channels,
            digital_inputs,
        })
    }
}

#[async_trait]
impl Device for AioModuleDevice {
    fn device_type(&self) -> &str {
        "aio_module"
    }

    fn address(&self) -> u8 {
        self.address
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    async fn read_data(&self, client: &dyn ModbusClientTrait) -> Result<Box<dyn DeviceData>, ModbusError> {
        info!("Reading AIO module data from device {} at address {}", self.name, self.address);

        // Read holding registers 1-39 (39 registers = 78 bytes)
        // This matches the Lua implementation: start_register_addr = 1, quantity = 39
        let raw_data = client
            .read_holding_registers(self.address, 1, 39)
            .await
            .map_err(|e| {
                error!("Failed to read AIO module data from address {}: {}", self.address, e);
                e
            })?;

        let aio_data = self.parse_modbus_response(&raw_data)?;
        
        info!("Successfully read AIO module data: {} channels, baud_rate={}", 
              aio_data.channels.len(), aio_data.baud_rate);

        Ok(Box::new(aio_data))
    }

    async fn reset_accumulation(&self, _client: &dyn ModbusClientTrait) -> Result<(), ModbusError> {
        warn!("Reset accumulation not implemented for AIO module device");
        Ok(())
    }

    fn parse_raw_data(&self, data: &[u8]) -> Result<Box<dyn DeviceData>, ModbusError> {
        let aio_data = self.parse_modbus_response(data)?;
        Ok(Box::new(aio_data))
    }
}

impl DeviceData for AioModuleData {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn device_address(&self) -> u8 {
        self.device_address
    }

    fn timestamp(&self) -> DateTime<Utc> {
        self.timestamp
    }

    fn to_json(&self) -> Value {
        json!({
            "device_address": self.device_address,
            "device_name": self.device_name,
            "unix_ts": self.timestamp.timestamp(),
            "baud_rate": self.baud_rate,
            "channels": self.channels.iter().map(|ch| json!({
                "channel_id": ch.channel_id,
                "pulse_count": ch.pulse_count,
                "threshold": ch.threshold,
                "frequency": ch.frequency,
                "rpm_value": ch.rpm_value,
                "average_value": ch.average_value,
                "duration_rpm": ch.duration_rpm,
                "duration_ae": ch.duration_ae
            })).collect::<Vec<_>>(),
            "digital_inputs": self.digital_inputs.iter().enumerate().map(|(i, &din)| {
                json!({
                    "din_number": i + 1,
                    "state": din
                })
            }).collect::<Vec<_>>()
        })
    }

    fn get_parameter(&self, name: &str) -> Option<String> {
        match name {
            "baud_rate" => Some(self.baud_rate.to_string()),
            "device_name" => Some(self.device_name.clone()),
            "device_location" => Some(self.device_location.clone()),
            "channel_count" => Some(self.channels.len().to_string()),
            "digital_input_count" => Some(self.digital_inputs.len().to_string()),
            _ => {
                // Check for channel-specific parameters
                if name.starts_with("ch") {
                    if let Some(parts) = name.strip_prefix("ch").and_then(|s| s.split_once("_")) {
                        if let Ok(ch_num) = parts.0.parse::<u8>() {
                            if let Some(channel) = self.channels.iter().find(|ch| ch.channel_id == ch_num) {
                                return match parts.1 {
                                    "pulse" => Some(channel.pulse_count.to_string()),
                                    "threshold" => Some(channel.threshold.to_string()),
                                    "frequency" => Some(channel.frequency.to_string()),
                                    "rpm" => Some(channel.rpm_value.to_string()),
                                    "average" => Some(channel.average_value.to_string()),
                                    "duration_rpm" => Some(channel.duration_rpm.to_string()),
                                    "duration_ae" => Some(channel.duration_ae.to_string()),
                                    _ => None,
                                };
                            }
                        }
                    }
                }
                
                // Check for digital input parameters
                if name.starts_with("din") {
                    if let Some(din_str) = name.strip_prefix("din") {
                        if let Ok(din_num) = din_str.parse::<usize>() {
                            if din_num > 0 && din_num <= self.digital_inputs.len() {
                                return Some(self.digital_inputs[din_num - 1].to_string());
                            }
                        }
                    }
                }
                
                None
            }
        }
    }

    fn get_all_parameters(&self) -> Vec<(String, String)> {
        let mut params = vec![
            ("baud_rate".to_string(), self.baud_rate.to_string()),
            ("device_name".to_string(), self.device_name.clone()),
            ("device_location".to_string(), self.device_location.clone()),
            ("channel_count".to_string(), self.channels.len().to_string()),
            ("digital_input_count".to_string(), self.digital_inputs.len().to_string()),
        ];

        // Add channel parameters
        for channel in &self.channels {
            let ch_prefix = format!("ch{}", channel.channel_id);
            params.push((format!("{}_pulse", ch_prefix), channel.pulse_count.to_string()));
            params.push((format!("{}_threshold", ch_prefix), channel.threshold.to_string()));
            params.push((format!("{}_frequency", ch_prefix), channel.frequency.to_string()));
            params.push((format!("{}_rpm", ch_prefix), channel.rpm_value.to_string()));
            params.push((format!("{}_average", ch_prefix), channel.average_value.to_string()));
            params.push((format!("{}_duration_rpm", ch_prefix), channel.duration_rpm.to_string()));
            params.push((format!("{}_duration_ae", ch_prefix), channel.duration_ae.to_string()));
        }

        // Add digital input parameters
        for (i, &din_state) in self.digital_inputs.iter().enumerate() {
            params.push((format!("din{}", i + 1), din_state.to_string()));
        }

        params
    }

    fn get_parameters_as_floats(&self) -> HashMap<String, f32> {
        let mut params = HashMap::new();
        
        params.insert("baud_rate".to_string(), self.baud_rate as f32);

        // Add channel parameters as floats
        for channel in &self.channels {
            let ch_prefix = format!("ch{}", channel.channel_id);
            params.insert(format!("{}_pulse", ch_prefix), channel.pulse_count as f32);
            params.insert(format!("{}_threshold", ch_prefix), channel.threshold as f32);
            params.insert(format!("{}_frequency", ch_prefix), channel.frequency as f32);
            params.insert(format!("{}_rpm", ch_prefix), channel.rpm_value as f32);
            params.insert(format!("{}_average", ch_prefix), channel.average_value as f32);
            params.insert(format!("{}_duration_rpm", ch_prefix), channel.duration_rpm as f32);
            params.insert(format!("{}_duration_ae", ch_prefix), channel.duration_ae as f32);
        }

        // Add digital inputs as floats (0.0 or 1.0)
        for (i, &din_state) in self.digital_inputs.iter().enumerate() {
            params.insert(format!("din{}", i + 1), if din_state { 1.0 } else { 0.0 });
        }

        params
    }

    fn device_type(&self) -> String {
        "aio_module".to_string()
    }

    fn device_name(&self) -> String {
        self.device_name.clone()
    }

    fn device_location(&self) -> String {
        self.device_location.clone()
    }

    fn clone_box(&self) -> Box<dyn DeviceData> {
        Box::new(self.clone())
    }
}