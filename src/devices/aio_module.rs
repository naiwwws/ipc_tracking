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
        // Expected 78 bytes for 39 registers (39 * 2 = 78 bytes)
        if raw_data.len() < 78 {
            return Err(ModbusError::InvalidData(format!(
                "AIO module response too short: expected 78 bytes, got {}",
                raw_data.len()
            )));
        }

        info!("📊 Parsing AIO data: {} bytes received", raw_data.len());

        // Parse data exactly as the Lua code does:
        // Data starts from register 1, so raw_data[0-1] = BAUD_RATE (register 1)
        
        // BAUD_RATE: arg[1] << 8 | arg[2] -> raw_data[0] << 8 | raw_data[1]
        let baud_rate = ((raw_data[0] as u16) << 8) | (raw_data[1] as u16);
        
        // Parse 4 channels exactly as Lua does
        let mut channels = Vec::new();
        for y in 0..4 {
            // PULSE_CH: arg[3 + 2*y] << 8 | arg[4 + 2*y]
            // For y=0: arg[3] << 8 | arg[4] -> raw_data[2] << 8 | raw_data[3]
            let pulse_idx = 2 + 2 * y;
            let pulse_count = ((raw_data[pulse_idx] as u16) << 8) | (raw_data[pulse_idx + 1] as u16);
            
            // THRESHOLD_CH: arg[11 + 2*y] << 8 | arg[12 + 2*y]
            // For y=0: arg[11] << 8 | arg[12] -> raw_data[10] << 8 | raw_data[11]
            let threshold_idx = 10 + 2 * y;
            let threshold = ((raw_data[threshold_idx] as u16) << 8) | (raw_data[threshold_idx + 1] as u16);
            
            // FREQ_CH: arg[19 + 2*y] << 8 | arg[20 + 2*y]
            // For y=0: arg[19] << 8 | arg[20] -> raw_data[18] << 8 | raw_data[19]
            let freq_idx = 18 + 2 * y;
            let frequency = ((raw_data[freq_idx] as u16) << 8) | (raw_data[freq_idx + 1] as u16);
            
            // RPM_CH: arg[27 + 2*y] << 8 | arg[28 + 2*y]
            // For y=0: arg[27] << 8 | arg[28] -> raw_data[26] << 8 | raw_data[27]
            let rpm_idx = 26 + 2 * y;
            let rpm_value = ((raw_data[rpm_idx] as u16) << 8) | (raw_data[rpm_idx + 1] as u16);
            
            // AVG_CH: arg[35 + 2*y] << 8 | arg[36 + 2*y]
            // For y=0: arg[35] << 8 | arg[36] -> raw_data[34] << 8 | raw_data[35]
            let avg_idx = 34 + 2 * y;
            let average_value = ((raw_data[avg_idx] as u16) << 8) | (raw_data[avg_idx + 1] as u16);
            
            // DUR_RPM_CH: arg[47 + 4*y] << 24 | arg[48 + 4*y] << 16 | arg[49 + 4*y] << 8 | arg[50 + 4*y]
            // For y=0: arg[47-50] -> raw_data[46-49]
            let dur_rpm_idx = 46 + 4 * y;
            let duration_rpm = ((raw_data[dur_rpm_idx] as u32) << 24)
                             | ((raw_data[dur_rpm_idx + 1] as u32) << 16)
                             | ((raw_data[dur_rpm_idx + 2] as u32) << 8)
                             | (raw_data[dur_rpm_idx + 3] as u32);
            
            // DUR_AE: arg[63 + 4*y] << 24 | arg[64 + 4*y] << 16 | arg[65 + 4*y] << 8 | arg[66 + 4*y]
            // For y=0: arg[63-66] -> raw_data[62-65]
            let dur_ae_idx = 62 + 4 * y;
            let duration_ae = ((raw_data[dur_ae_idx] as u32) << 24)
                            | ((raw_data[dur_ae_idx + 1] as u32) << 16)
                            | ((raw_data[dur_ae_idx + 2] as u32) << 8)
                            | (raw_data[dur_ae_idx + 3] as u32);

            channels.push(AioChannelData {
                channel_id: (y + 1) as u8,
                pulse_count,
                threshold,
                frequency,
                rpm_value,
                average_value,
                duration_rpm,
                duration_ae,
            });

            info!("📊 Channel {}: pulse={}, threshold={}, freq={}, rpm={}, avg={}, dur_rpm={}, dur_ae={}", 
                  y + 1, pulse_count, threshold, frequency, rpm_value, average_value, duration_rpm, duration_ae);
        }

        // DIN_STATE: dinTmp = arg[43] << 8 | arg[44] -> raw_data[42] << 8 | raw_data[43]
        let din_word = ((raw_data[42] as u16) << 8) | (raw_data[43] as u16);
        
        // Parse digital inputs exactly as Lua: (dinTmp >> j) & 0x1
        let mut digital_inputs = Vec::new();
        for j in 0..16 {
            digital_inputs.push((din_word >> j) & 0x1 == 1);
        }

        info!("📊 AIO Parsed: baud_rate={}, din_word=0b{:016b}, {} channels", 
              baud_rate, din_word, channels.len());

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
        "aio"
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
        info!("🔧 Reading AIO module data from device '{}' at address {}", self.name, self.address);

        // First try a simple connectivity test
        // info!("🧪 Testing AIO connectivity with single register read...");
        // match client.read_holding_registers(self.address, 0x0000, 1).await {
        //     Ok(test_data) => {
        //         info!("✅ AIO connectivity test PASSED: {} bytes", test_data.len());
        //     }
        //     Err(e) => {
        //         warn!("❌ AIO connectivity test FAILED: {}", e);
        //         return Err(ModbusError::CommunicationError(format!(
        //             "AIO module at address {} not responding to basic connectivity test: {}", 
        //             self.address, e
        //         )));
        //     }
        // }

        // Match Lua implementation exactly: read registers 1-39 (39 registers)
        // This corresponds to the working Lua code: start_register_addr = 1, quantity = 39
        info!("📡 Reading full AIO data: registers 1-39 (39 registers) from device {}", self.address);
        let raw_data = client
            .read_holding_registers(self.address, 0x0001, 39)
            .await
            .map_err(|e| {
                error!("Failed to read AIO module data from address {}: {}", self.address, e);
                e
            })?;

        info!("✅ Successfully read {} bytes from AIO module at address {}", raw_data.len(), self.address);
        
        // Debug: Show first 20 bytes to compare with expected values
        if raw_data.len() >= 20 {
            info!("🔍 AIO Raw data (first 20 bytes): {:02X?}", &raw_data[0..20]);
        }

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
        "aio".to_string()
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