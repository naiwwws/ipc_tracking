use async_trait::async_trait;
use log::{info, warn, error};
use serde::{Deserialize, Serialize};
use std::any::Any;
use std::collections::HashMap;
use chrono::{DateTime, Utc};
use serde_json::Value;
use std::sync::Mutex;

use crate::devices::traits::{Device, DeviceData};
use crate::modbus::ModbusClientTrait;
use crate::utils::error::ModbusError;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpmChannelData {
    pub channel_id: u8,
    pub rpm_value: u16,
    pub freq_value: u16,
    pub pulse_config: u16,
    // Remove engine_duration_minutes - this will be calculated separately
    pub is_engine_running: bool,
    pub rpm_threshold: u16,
    pub status: String,
    pub error_code: u16,
    pub engine_duration_seconds: u64, // Add this for calculated duration
}

impl RpmChannelData {
    pub fn new(channel_id: u8, rpm_threshold: u16) -> Self {
        Self {
            channel_id,
            rpm_value: 0,
            freq_value: 0,
            pulse_config: 0,
            is_engine_running: false,
            rpm_threshold,
            status: "Unknown".to_string(),
            error_code: 0,
            engine_duration_seconds: 0, // Initialize to 0
        }
    }

    pub fn is_engine_running(&self) -> bool {
        self.rpm_value >= self.rpm_threshold && self.error_code == 0
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpmData {
    pub device_address: u8,
    pub channels: Vec<RpmChannelData>,
    pub total_channels: u8,
    pub timestamp: DateTime<Utc>,
    pub device_status: String,
    pub global_error_code: u16,
}

impl RpmData {
    pub fn new(address: u8, total_channels: u8, rpm_thresholds: Vec<u16>) -> Self {
        let mut channels = Vec::new();
        
        for i in 0..total_channels {
            let threshold = rpm_thresholds.get(i as usize).copied().unwrap_or(500);
            channels.push(RpmChannelData::new(i + 1, threshold));
        }

        Self {
            device_address: address,
            channels,
            total_channels,
            timestamp: Utc::now(),
            device_status: "Unknown".to_string(),
            global_error_code: 0,
        }
    }

    // Get primary RPM channel (usually channel 1)
    pub fn get_primary_channel(&self) -> Option<&RpmChannelData> {
        self.channels.first()
    }

    // Get specific channel by ID
    pub fn get_channel(&self, channel_id: u8) -> Option<&RpmChannelData> {
        self.channels.iter().find(|ch| ch.channel_id == channel_id)
    }

    // Get all running engines
    pub fn get_running_engines(&self) -> Vec<&RpmChannelData> {
        self.channels.iter().filter(|ch| ch.is_engine_running()).collect()
    }

    // Get total running time across all channels (convert seconds to minutes)
    pub fn get_total_engine_minutes(&self) -> f32 {
        self.channels.iter().map(|ch| ch.engine_duration_seconds as f32 / 3600.0).sum()
    }
}

impl DeviceData for RpmData {
    fn device_address(&self) -> u8 {
        self.device_address
    }

    fn timestamp(&self) -> DateTime<Utc> {
        self.timestamp
    }

    fn get_parameter(&self, name: &str) -> Option<String> {
        match name {
            "TotalChannels" => Some(self.total_channels.to_string()),
            "DeviceStatus" => Some(self.device_status.clone()),
            "GlobalErrorCode" => Some(self.global_error_code.to_string()),
            "RunningEngines" => Some(self.get_running_engines().len().to_string()),
            "TotalEngineMinutes" => Some(format!("{:.1}", self.get_total_engine_minutes())),
            _ => {
                // Handle channel-specific parameters like "RPM1", "RPM2", etc.
                if name.starts_with("RPM") && name.len() > 3 {
                    if let Ok(channel_id) = name[3..].parse::<u8>() {
                        return self.get_channel(channel_id)
                            .map(|ch| ch.rpm_value.to_string());
                    }
                } else if name.starts_with("EngineDurationSeconds") && name.len() > 21 {
                    if let Ok(channel_id) = name[21..].parse::<u8>() {
                        return self.get_channel(channel_id)
                            .map(|ch| ch.engine_duration_seconds.to_string());
                    }
                } else if name.starts_with("Freq") && name.len() > 4 {
                    if let Ok(channel_id) = name[4..].parse::<u8>() {
                        return self.get_channel(channel_id)
                            .map(|ch| ch.freq_value.to_string());
                    }
                } else if name.starts_with("Pulse") && name.len() > 5 {
                    if let Ok(channel_id) = name[5..].parse::<u8>() {
                        return self.get_channel(channel_id)
                            .map(|ch| ch.pulse_config.to_string());
                    }
                }

                // Legacy support for single channel (primary)
                match name {
                    "RPM" => self.get_primary_channel().map(|ch| ch.rpm_value.to_string()),
                    "Freq" => self.get_primary_channel().map(|ch| ch.freq_value.to_string()),
                    "Pulse" => self.get_primary_channel().map(|ch| ch.pulse_config.to_string()),
                    "Status" => Some(self.device_status.clone()),
                    "ErrorCode" => Some(self.global_error_code.to_string()),
                    _ => None,
                }
            }
        }
    }

    fn to_json(&self) -> Value {
        serde_json::to_value(self).unwrap_or(Value::Null)
    }

    fn get_all_parameters(&self) -> Vec<(String, String)> {
        let mut params = vec![
            ("TotalChannels".to_string(), self.total_channels.to_string()),
            ("DeviceStatus".to_string(), self.device_status.clone()),
            ("GlobalErrorCode".to_string(), self.global_error_code.to_string()),
            ("RunningEngines".to_string(), self.get_running_engines().len().to_string()),
            ("TotalEngineMinutes".to_string(), format!("{:.1}", self.get_total_engine_minutes())),
            ("Timestamp".to_string(), self.timestamp.timestamp().to_string()),
        ];

        // Add channel-specific parameters
        for channel in &self.channels {
            let ch_id = channel.channel_id;
            params.extend(vec![
                (format!("RPM{}", ch_id), channel.rpm_value.to_string()),
                (format!("Freq{}", ch_id), channel.freq_value.to_string()),
                (format!("Pulse{}", ch_id), channel.pulse_config.to_string()),
                (format!("EngineDurationSeconds{}", ch_id), channel.engine_duration_seconds.to_string()),
                (format!("Status{}", ch_id), channel.status.clone()),
                (format!("ErrorCode{}", ch_id), channel.error_code.to_string()),
                (format!("IsRunning{}", ch_id), channel.is_engine_running().to_string()),
                (format!("RPMThreshold{}", ch_id), channel.rpm_threshold.to_string()),
            ]);
        }

        params
    }

    fn get_parameters_as_floats(&self) -> HashMap<String, f32> {
        let mut params = HashMap::new();
        
        params.insert("TotalChannels".to_string(), self.total_channels as f32);
        params.insert("GlobalErrorCode".to_string(), self.global_error_code as f32);
        params.insert("RunningEngines".to_string(), self.get_running_engines().len() as f32);
        params.insert("TotalEngineMinutes".to_string(), self.get_total_engine_minutes());

        // Add channel-specific float parameters
        for channel in &self.channels {
            let ch_id = channel.channel_id;
            params.insert(format!("RPM{}", ch_id), channel.rpm_value as f32);
            params.insert(format!("Freq{}", ch_id), channel.freq_value as f32);
            params.insert(format!("Pulse{}", ch_id), channel.pulse_config as f32);
            params.insert(format!("EngineDurationSeconds{}", ch_id), channel.engine_duration_seconds as f32);
            params.insert(format!("ErrorCode{}", ch_id), channel.error_code as f32);
            params.insert(format!("RPMThreshold{}", ch_id), channel.rpm_threshold as f32);
            params.insert(format!("IsRunning{}", ch_id), if channel.is_engine_running() { 1.0 } else { 0.0 });
        }

        // Legacy support for primary channel
        if let Some(primary) = self.get_primary_channel() {
            params.insert("RPM".to_string(), primary.rpm_value as f32);
            params.insert("Freq".to_string(), primary.freq_value as f32);
            params.insert("Pulse".to_string(), primary.pulse_config as f32);
            params.insert("ErrorCode".to_string(), primary.error_code as f32);
        }

        params
    }

    fn device_type(&self) -> String {
        "rpm".to_string()
    }

    fn device_name(&self) -> String {
        format!("RPM Device {} ({} channels)", self.device_address, self.total_channels)
    }

    fn device_location(&self) -> String {
        "Unknown".to_string()
    }

    fn clone_box(&self) -> Box<dyn DeviceData> {
        Box::new(self.clone())
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn unix_ts(&self) -> i64 {
        self.timestamp.timestamp()
    }
}

pub struct RpmDevice {
    address: u8,
    name: String,
    location: String,
    total_channels: u8,
    rpm_thresholds: Vec<u16>,
    // Use Mutex for interior mutability
    last_rpm_values: Mutex<HashMap<u8, u16>>,
    outlier_confirmation_counts: Mutex<HashMap<u8, u16>>,
    outlier_detection_threshold: u16,
    outlier_confirmation_threshold: u16,
}

impl RpmDevice {
    pub fn new(address: u8, name: String) -> Self {
        Self {
            address,
            name,
            location: "Unknown".to_string(),
            total_channels: 2, // Default 2 channels
            rpm_thresholds: vec![500, 500], // Default thresholds
            last_rpm_values: Mutex::new(HashMap::new()),
            outlier_confirmation_counts: Mutex::new(HashMap::new()),
            outlier_detection_threshold: 150,
            outlier_confirmation_threshold: 15,
        }
    }

    pub fn with_config(
        address: u8,
        name: String,
        location: String,
        total_channels: u8,
        rpm_thresholds: Vec<u16>,
    ) -> Self {
        let channels = if total_channels == 0 { 2 } else { total_channels };
        let mut thresholds = rpm_thresholds;
        
        // Ensure we have thresholds for all channels
        while thresholds.len() < channels as usize {
            thresholds.push(500); // Default threshold
        }

        Self {
            address,
            name,
            location,
            total_channels: channels,
            rpm_thresholds: thresholds,
            last_rpm_values: Mutex::new(HashMap::new()),
            outlier_confirmation_counts: Mutex::new(HashMap::new()),
            outlier_detection_threshold: 150,
            outlier_confirmation_threshold: 15,
        }
    }

    // Auto-detect number of channels by reading configuration register
    async fn detect_channels(&self, client: &dyn ModbusClientTrait) -> Result<u8, ModbusError> {
        match client.read_holding_registers(self.address, 0x00FF, 1).await {
            Ok(data) => {
                if data.len() >= 2 {
                    let channel_config = u16::from_be_bytes([data[0], data[1]]);
                    let detected_channels = (channel_config & 0x00FF) as u8;
                    
                    if detected_channels > 0 && detected_channels <= 8 {
                        info!("📡 Auto-detected {} channels for RPM device {}", detected_channels, self.address);
                        return Ok(detected_channels);
                    }
                }
            }
            Err(_) => {
                info!("📡 Channel auto-detection failed for device {}, using configured channels: {}", 
                      self.address, self.total_channels);
            }
        }
        
        Ok(self.total_channels)
    }

    // Apply outlier detection for specific channel
    fn apply_outlier_detection(&self, channel_id: u8, mut value: u16) -> u16 {
        // Lock the Mutexes
        let mut last_values = self.last_rpm_values.lock().unwrap();
        let mut outlier_counts = self.outlier_confirmation_counts.lock().unwrap();
        
        if let Some(&last_value) = last_values.get(&channel_id) {
            let diff = if value > last_value { value - last_value } else { last_value - value };
            
            if diff > self.outlier_detection_threshold {
                let current_count = *outlier_counts.get(&channel_id).unwrap_or(&0);
                
                if current_count < self.outlier_confirmation_threshold {
                    // Use last value and increment counter
                    value = last_value;
                    outlier_counts.insert(channel_id, current_count + 1);
                    warn!("🚨 RPM{} outlier detected for device {}: {} -> {} (using last value, count: {}/{})", 
                          channel_id, self.address, last_value, value, current_count + 1, self.outlier_confirmation_threshold);
                } else {
                    // Reset counter and accept new value
                    outlier_counts.insert(channel_id, 0);
                    info!("✅ RPM{} outlier confirmed for device {}: accepting new value {}", 
                          channel_id, self.address, value);
                }
            } else {
                // Normal value, reset counter
                outlier_counts.insert(channel_id, 0);
            }
        }

        // Update last value
        last_values.insert(channel_id, value);
        
        value
    }
}

#[async_trait]
impl Device for RpmDevice {
    async fn read_data(&self, client: &dyn ModbusClientTrait) -> Result<Box<dyn DeviceData>, ModbusError> {
        info!("🔄 Reading RPM data from device {} ({}) - {} channels", self.address, self.name, self.total_channels);

        // Auto-detect channels if needed
        let active_channels = self.detect_channels(client).await.unwrap_or(self.total_channels);
        
        let mut rpm_data = RpmData::new(self.address, active_channels, self.rpm_thresholds.clone());

        // Calculate total registers needed based on your structure:
        // RPM_CH1, RPM_CH2, FREQ_CH1, FREQ_CH2, PULSE_CH1, PULSE_CH2, SLAVE_ADDR, BAUDRATE
        // For 2 channels: 2 RPM + 2 FREQ + 2 PULSE + 1 SLAVE + 1 BAUD = 8 registers
        let total_registers = match active_channels {
            1 => 5, // 1 RPM + 1 FREQ + 1 PULSE + 1 SLAVE + 1 BAUD
            2 => 8, // 2 RPM + 2 FREQ + 2 PULSE + 1 SLAVE + 1 BAUD
            3 => 11, // 3 RPM + 3 FREQ + 3 PULSE + 1 SLAVE + 1 BAUD
            4 => 14, // 4 RPM + 4 FREQ + 4 PULSE + 1 SLAVE + 1 BAUD
            _ => (active_channels * 3 + 2) as u16, // General formula
        };
        
        match client.read_holding_registers(self.address, 0x0000, total_registers).await {
            Ok(data) => {
                if data.len() >= (total_registers * 2) as usize { // 2 bytes per register
                    let mut global_error = false;
                    
                    // Parse RPM values first (RPM_CH1, RPM_CH2, ...)
                    for channel_idx in 0..active_channels {
                        let rpm_offset = (channel_idx as usize) * 2; // 2 bytes per register
                        
                        if rpm_offset + 1 < data.len() {
                            let raw_rpm = u16::from_be_bytes([data[rpm_offset], data[rpm_offset + 1]]);
                            let filtered_rpm = self.apply_outlier_detection(channel_idx + 1, raw_rpm);
                            
                            if let Some(channel) = rpm_data.channels.get_mut(channel_idx as usize) {
                                channel.rpm_value = filtered_rpm;
                                channel.status = "OK".to_string();
                                channel.error_code = 0;
                            }
                        } else {
                            global_error = true;
                        }
                    }
                    
                    // Parse FREQ values (FREQ_CH1, FREQ_CH2, ...)
                    let freq_start_offset = (active_channels as usize) * 2; // After all RPM values
                    for channel_idx in 0..active_channels {
                        let freq_offset = freq_start_offset + (channel_idx as usize) * 2;
                        
                        if freq_offset + 1 < data.len() {
                            let freq_value = u16::from_be_bytes([data[freq_offset], data[freq_offset + 1]]);
                            
                            if let Some(channel) = rpm_data.channels.get_mut(channel_idx as usize) {
                                channel.freq_value = freq_value;
                            }
                        } else {
                            global_error = true;
                        }
                    }
                    
                    // Parse PULSE values (PULSE_CH1, PULSE_CH2, ...)
                    let pulse_start_offset = freq_start_offset + (active_channels as usize) * 2; // After all FREQ values
                    for channel_idx in 0..active_channels {
                        let pulse_offset = pulse_start_offset + (channel_idx as usize) * 2;
                        
                        if pulse_offset + 1 < data.len() {
                            let pulse_config = u16::from_be_bytes([data[pulse_offset], data[pulse_offset + 1]]);
                            
                            if let Some(channel) = rpm_data.channels.get_mut(channel_idx as usize) {
                                channel.pulse_config = pulse_config;
                                
                                // Update engine running status
                                channel.is_engine_running = channel.is_engine_running();
                                
                                info!("📊 Channel {} - RPM: {}, Freq: {}, Pulse: {}, Running: {}", 
                                      channel.channel_id, channel.rpm_value, channel.freq_value, 
                                      channel.pulse_config, channel.is_engine_running);
                            }
                        } else {
                            global_error = true;
                        }
                    }
                    
                    // Optional: Read SLAVE_ADDR and BAUDRATE (last 2 registers) for diagnostics
                    let slave_addr_offset = pulse_start_offset + (active_channels as usize) * 2;
                    let baud_rate_offset = slave_addr_offset + 2;
                    
                    if baud_rate_offset + 1 < data.len() {
                        let slave_addr = u16::from_be_bytes([data[slave_addr_offset], data[slave_addr_offset + 1]]);
                        let baud_rate = u16::from_be_bytes([data[baud_rate_offset], data[baud_rate_offset + 1]]);
                        
                        info!("📡 Device {} config - Slave: {}, Baud: {}", self.address, slave_addr, baud_rate);
                    }
                    
                    rpm_data.device_status = if global_error { "Partial Read Error".to_string() } else { "OK".to_string() };
                    rpm_data.global_error_code = if global_error { 1 } else { 0 };
                    
                } else {
                    warn!("⚠️ Invalid RPM data length from device {} (got {} bytes, expected {})", 
                          self.address, data.len(), total_registers * 2);
                    
                    rpm_data.device_status = "Invalid Data Length".to_string();
                    rpm_data.global_error_code = 1;
                    
                    // Mark all channels as error
                    for channel in &mut rpm_data.channels {
                        channel.status = "Read Error".to_string();
                        channel.error_code = 1;
                    }
                }
            }
            Err(e) => {
                error!("❌ Failed to read RPM from device {}: {}", self.address, e);
                
                rpm_data.device_status = "Modbus Read Error".to_string();
                rpm_data.global_error_code = 1;
                
                // Reset all channel values on error
                for channel in &mut rpm_data.channels {
                    channel.rpm_value = 0;
                    channel.freq_value = 0;
                    channel.pulse_config = 0;
                    channel.status = "Communication Error".to_string();
                    channel.error_code = 1;
                    channel.is_engine_running = false;
                }
                
                // Reset outlier detection on error
                self.last_rpm_values.lock().unwrap().clear();
                self.outlier_confirmation_counts.lock().unwrap().clear();
            }
        }

        Ok(Box::new(rpm_data))
    }

    async fn reset_accumulation(&self, client: &dyn ModbusClientTrait) -> Result<(), ModbusError> {
        info!("🔄 Resetting RPM device {} ({}) - {} channels", self.address, self.name, self.total_channels);
        
        // Send reset command to MCU for all channels
        match client.write_single_coil(self.address, 0x00FF, true).await {
            Ok(_) => {
                info!("✅ RPM device {} reset successful (all channels)", self.address);
                Ok(())
            }
            Err(e) => {
                error!("❌ Failed to reset RPM device {}: {}", self.address, e);
                Err(e)
            }
        }
    }

    // Update parse_raw_data to match the new structure
    fn parse_raw_data(&self, data: &[u8]) -> Result<Box<dyn DeviceData>, ModbusError> {
        let total_registers = match self.total_channels {
            1 => 5,
            2 => 8,
            3 => 11,
            4 => 14,
            _ => (self.total_channels * 3 + 2) as usize,
        };
        
        let min_data_length = total_registers * 2; // 2 bytes per register
        
        if data.len() < min_data_length {
            return Err(ModbusError::InvalidData(
                format!("Insufficient data length for {} channel RPM parsing (need {} bytes, got {})", 
                       self.total_channels, min_data_length, data.len())
            ));
        }

        let mut rpm_data = RpmData::new(self.address, self.total_channels, self.rpm_thresholds.clone());

        // Parse using the same structure as read_data
        // Parse RPM values
        for channel_idx in 0..self.total_channels {
            let rpm_offset = (channel_idx as usize) * 2;
            if rpm_offset + 1 < data.len() {
                let rpm_value = u16::from_be_bytes([data[rpm_offset], data[rpm_offset + 1]]);
                if let Some(channel) = rpm_data.channels.get_mut(channel_idx as usize) {
                    channel.rpm_value = rpm_value;
                }
            }
        }
        
        // Parse FREQ values
        let freq_start_offset = (self.total_channels as usize) * 2;
        for channel_idx in 0..self.total_channels {
            let freq_offset = freq_start_offset + (channel_idx as usize) * 2;
            if freq_offset + 1 < data.len() {
                let freq_value = u16::from_be_bytes([data[freq_offset], data[freq_offset + 1]]);
                if let Some(channel) = rpm_data.channels.get_mut(channel_idx as usize) {
                    channel.freq_value = freq_value;
                }
            }
        }
        
        // Parse PULSE values
        let pulse_start_offset = freq_start_offset + (self.total_channels as usize) * 2;
        for channel_idx in 0..self.total_channels {
            let pulse_offset = pulse_start_offset + (channel_idx as usize) * 2;
            if pulse_offset + 1 < data.len() {
                let pulse_config = u16::from_be_bytes([data[pulse_offset], data[pulse_offset + 1]]);
                if let Some(channel) = rpm_data.channels.get_mut(channel_idx as usize) {
                    channel.pulse_config = pulse_config;
                    channel.status = "OK".to_string();
                    channel.is_engine_running = channel.is_engine_running();
                }
            }
        }

        rpm_data.device_status = "OK".to_string();

        info!("📊 Parsed RPM Device {}: {} channels processed", self.address, self.total_channels);

        Ok(Box::new(rpm_data))
    }

    fn address(&self) -> u8 {
        self.address
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn device_type(&self) -> &str {
        "rpm"
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}