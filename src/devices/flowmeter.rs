use async_trait::async_trait;
use chrono::{DateTime, Utc};
use log::{error, info};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::any::Any;
use std::collections::HashMap;
use std::fmt;

use super::traits::{Device, DeviceData};
use crate::modbus::client::ModbusClientTrait;  // ← Fixed import path
use crate::utils::error::ModbusError;

const FLOWMETER_REG_OFFSET_ADDR: u16 = 1;

#[derive(Debug, Clone)]
pub struct FlowmeterDevice {
    pub address: u8,
    pub name: String,
    pub start_register: u16,
    pub register_count: u16,
    pub reset_coil: u16,
}

impl FlowmeterDevice {
    pub fn new(address: u8, name: String) -> Self {
        Self {
            address,
            name,
            start_register: 245 - FLOWMETER_REG_OFFSET_ADDR,
            register_count: 22,
            reset_coil: 3 - FLOWMETER_REG_OFFSET_ADDR,
        }
    }

    // Read raw payload for debugging
    pub async fn read_raw_payload(&self, client: &dyn ModbusClientTrait) -> Result<FlowmeterRawPayload, ModbusError> {
        info!("📡 Reading raw payload from device {} ({})", self.address, self.name);
        
        let raw_bytes = client.read_holding_registers(self.address, self.start_register, self.register_count).await?;
        
        info!("📊 Received {} bytes from device {}", raw_bytes.len(), self.address);
        
        FlowmeterRawPayload::new(self.address, raw_bytes, self.start_register, self.register_count)
    }

    // Read both raw and processed data for comparison
    pub async fn read_data_with_raw(&self, client: &dyn ModbusClientTrait) -> Result<(Box<dyn DeviceData>, FlowmeterRawPayload), ModbusError> {
        let raw_bytes = client.read_holding_registers(self.address, self.start_register, self.register_count).await?;
        
        // Create processed data
        let processed_data = self.parse_raw_data(&raw_bytes)?;
        
        // Create raw payload
        let raw_payload = FlowmeterRawPayload::new(self.address, raw_bytes, self.start_register, self.register_count)?;
        
        Ok((processed_data, raw_payload))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlowmeterData {
    pub device_address: u8, //  Ensure this field exists
    pub timestamp: DateTime<Utc>,
    pub mass_flow_rate: f32,
    pub density_flow: f32,
    pub temperature: f32,
    pub volume_flow_rate: f32,
    pub mass_total: f32,
    pub volume_total: f32,
    pub mass_inventory: f32,
    pub volume_inventory: f32,
    pub error_code: u16,
}

impl Default for FlowmeterData {
    fn default() -> Self {
        Self {
            device_address: 0,
            timestamp: Utc::now(),
            mass_flow_rate: 0.0,
            density_flow: 0.0,
            temperature: 0.0,
            volume_flow_rate: 0.0,
            mass_total: 0.0,
            volume_total: 0.0,
            mass_inventory: 0.0,
            volume_inventory: 0.0,
            error_code: 0,
        }
    }
}

#[async_trait]
impl Device for FlowmeterDevice {
    fn device_type(&self) -> &str {
        "Flowmeter"
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
    
    async fn read_data(&self, client: &dyn ModbusClientTrait) -> Result<Box<dyn DeviceData>, ModbusError> {  // ← Changed signature
        let raw_data = client.read_holding_registers(self.address, self.start_register, self.register_count).await?;
        self.parse_raw_data(&raw_data)
    }
    
    async fn reset_accumulation(&self, client: &dyn ModbusClientTrait) -> Result<(), ModbusError> {  // ← Changed signature
        client.write_single_coil(self.address, self.reset_coil, true).await?;
        info!("Reset accumulation for device {} ({})", self.address, self.name);
        Ok(())
    }
    
    fn parse_raw_data(&self, data: &[u8]) -> Result<Box<dyn DeviceData>, ModbusError> {
        if data.len() < 44 {
            error!("Insufficient data length for device {}: {}", self.address, data.len());
            return Err(ModbusError::InvalidData("Insufficient data length".to_string()));
        }

        // Convert register pairs to u32 values (big-endian)
        let error_code = ((data[0] as u32) << 24) | ((data[1] as u32) << 16) | 
                        ((data[2] as u32) << 8) | (data[3] as u32);
        
        let mass_flow_rate_raw = ((data[4] as u32) << 24) | ((data[5] as u32) << 16) | 
                                ((data[6] as u32) << 8) | (data[7] as u32);
        
        let density_flow_raw = ((data[8] as u32) << 24) | ((data[9] as u32) << 16) | 
                              ((data[10] as u32) << 8) | (data[11] as u32);
        
        let temperature_raw = ((data[12] as u32) << 24) | ((data[13] as u32) << 16) | 
                             ((data[14] as u32) << 8) | (data[15] as u32);
        
        let volume_flow_rate_raw = ((data[16] as u32) << 24) | ((data[17] as u32) << 16) | 
                                  ((data[18] as u32) << 8) | (data[19] as u32);
        
        let mass_total_raw = ((data[28] as u32) << 24) | ((data[29] as u32) << 16) | 
                            ((data[30] as u32) << 8) | (data[31] as u32);
        
        let volume_total_raw = ((data[32] as u32) << 24) | ((data[33] as u32) << 16) | 
                              ((data[34] as u32) << 8) | (data[35] as u32);
        
        let mass_inventory_raw = ((data[36] as u32) << 24) | ((data[37] as u32) << 16) | 
                                ((data[38] as u32) << 8) | (data[39] as u32);
        
        let volume_inventory_raw = ((data[40] as u32) << 24) | ((data[41] as u32) << 16) | 
                                  ((data[42] as u32) << 8) | (data[43] as u32);

        let flowmeter_data = FlowmeterData {
            device_address: self.address,
            timestamp: Utc::now(),
            mass_flow_rate: f32::from_bits(mass_flow_rate_raw),
            density_flow: f32::from_bits(density_flow_raw),
            temperature: f32::from_bits(temperature_raw),
            volume_flow_rate: f32::from_bits(volume_flow_rate_raw),
            mass_total: f32::from_bits(mass_total_raw),
            volume_total: f32::from_bits(volume_total_raw),
            mass_inventory: f32::from_bits(mass_inventory_raw),
            volume_inventory: f32::from_bits(volume_inventory_raw),
            error_code: (error_code & 0xFFFF) as u16, // Ensure error_code is u16
        };
        
        Ok(Box::new(flowmeter_data))
    }
}

impl DeviceData for FlowmeterData {
    fn as_any(&self) -> &dyn Any {
        self
    }
    
    //  Add the missing device_address method
    fn device_address(&self) -> u8 {
        self.device_address
    }
    
    fn timestamp(&self) -> chrono::DateTime<chrono::Utc> {
        self.timestamp
    }
    
    fn unix_ts(&self) -> i64 {
        self.timestamp.timestamp()
    }

    fn to_json(&self) -> Value {
        serde_json::to_value(self).unwrap_or(Value::Null)
    }
    
    fn get_parameter(&self, name: &str) -> Option<String> {
        match name {
            "MassFlowRate" => Some(format!("{:.2} kg/h", self.mass_flow_rate)),
            "DensityFlow" => Some(format!("{:.4} kg/L", self.density_flow)),
            "Temperature" => Some(format!("{:.1}°C", self.temperature)),
            "VolumeFlowRate" => Some(format!("{:.3} L/h", self.volume_flow_rate)),
            "MassTotal" => Some(format!("{:.2} kg", self.mass_total)),
            "VolumeTotal" => Some(format!("{:.3} L", self.volume_total)),
            "MassInventory" => Some(format!("{:.2} kg", self.mass_inventory)),
            "VolumeInventory" => Some(format!("{:.3} L", self.volume_inventory)),
            "ErrorCode" => Some(self.error_code.to_string()),
            _ => None,
        }
    }
    
    fn get_all_parameters(&self) -> Vec<(String, String)> {
        vec![
            ("MassFlowRate".to_string(), self.mass_flow_rate.to_string()),
            ("DensityFlow".to_string(), self.density_flow.to_string()),
            ("Temperature".to_string(), self.temperature.to_string()),
            ("VolumeFlowRate".to_string(), self.volume_flow_rate.to_string()),
            ("MassTotal".to_string(), self.mass_total.to_string()),
            ("VolumeTotal".to_string(), self.volume_total.to_string()),
            ("MassInventory".to_string(), self.mass_inventory.to_string()),
            ("VolumeInventory".to_string(), self.volume_inventory.to_string()),
            ("ErrorCode".to_string(), self.error_code.to_string()),
        ]
    }
    
    // ADD: New method to return raw float values
    fn get_parameters_as_floats(&self) -> HashMap<String, f32> {
        let mut params = HashMap::new();
        params.insert("MassFlowRate".to_string(), self.mass_flow_rate);
        params.insert("DensityFlow".to_string(), self.density_flow);
        params.insert("Temperature".to_string(), self.temperature);
        params.insert("VolumeFlowRate".to_string(), self.volume_flow_rate);
        params.insert("MassTotal".to_string(), self.mass_total);
        params.insert("VolumeTotal".to_string(), self.volume_total);
        params.insert("MassInventory".to_string(), self.mass_inventory);
        params.insert("VolumeInventory".to_string(), self.volume_inventory);
        params.insert("ErrorCode".to_string(), self.error_code as f32);
        params
    }
    
    fn device_type(&self) -> String {
        "flowmeter".to_string()
    }
    
    fn device_name(&self) -> String {
        format!("Device {}", self.device_address)
    }
    
    fn device_location(&self) -> String {
        "Unknown".to_string()
    }
    
    //  Add clone method for storage
    fn clone_box(&self) -> Box<dyn DeviceData> {
        Box::new(self.clone())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlowmeterRawPayload {
    pub device_address: u8,
    pub timestamp: DateTime<Utc>,
    pub raw_bytes: Vec<u8>,          // Original raw bytes from Modbus
    pub hex_string: String,          // Hex representation for debugging
    pub payload_size: usize,         // Size of raw data
    pub register_count: u16,         // Number of registers read
    pub start_register: u16,         // Starting register address
    
    // Parsed raw values (before conversion to engineering units)
    pub error_code_raw: u32,
    pub mass_flow_rate_raw: u32,
    pub density_flow_raw: u32,
    pub temperature_raw: u32,
    pub volume_flow_rate_raw: u32,
    pub mass_total_raw: u32,
    pub volume_total_raw: u32,
    pub mass_inventory_raw: u32,
    pub volume_inventory_raw: u32,
}

impl FlowmeterRawPayload {
    pub fn new(device_address: u8, raw_bytes: Vec<u8>, start_register: u16, register_count: u16) -> Result<Self, ModbusError> {
        if raw_bytes.len() < 44 {
            return Err(ModbusError::InvalidData(format!("Insufficient raw data: {} bytes", raw_bytes.len())));
        }

        let hex_string = hex::encode(&raw_bytes);
        let payload_size = raw_bytes.len();

        // Parse raw values from bytes
        let error_code_raw = ((raw_bytes[0] as u32) << 24) | ((raw_bytes[1] as u32) << 16) | 
                            ((raw_bytes[2] as u32) << 8) | (raw_bytes[3] as u32);
        
        let mass_flow_rate_raw = ((raw_bytes[4] as u32) << 24) | ((raw_bytes[5] as u32) << 16) | 
                                ((raw_bytes[6] as u32) << 8) | (raw_bytes[7] as u32);
        
        let density_flow_raw = ((raw_bytes[8] as u32) << 24) | ((raw_bytes[9] as u32) << 16) | 
                              ((raw_bytes[10] as u32) << 8) | (raw_bytes[11] as u32);
        
        let temperature_raw = ((raw_bytes[12] as u32) << 24) | ((raw_bytes[13] as u32) << 16) | 
                             ((raw_bytes[14] as u32) << 8) | (raw_bytes[15] as u32);
        
        let volume_flow_rate_raw = ((raw_bytes[16] as u32) << 24) | ((raw_bytes[17] as u32) << 16) | 
                                  ((raw_bytes[18] as u32) << 8) | (raw_bytes[19] as u32);
        
        let mass_total_raw = ((raw_bytes[28] as u32) << 24) | ((raw_bytes[29] as u32) << 16) | 
                            ((raw_bytes[30] as u32) << 8) | (raw_bytes[31] as u32);
        
        let volume_total_raw = ((raw_bytes[32] as u32) << 24) | ((raw_bytes[33] as u32) << 16) | 
                              ((raw_bytes[34] as u32) << 8) | (raw_bytes[35] as u32);
        
        let mass_inventory_raw = ((raw_bytes[36] as u32) << 24) | ((raw_bytes[37] as u32) << 16) | 
                                ((raw_bytes[38] as u32) << 8) | (raw_bytes[39] as u32);
        
        let volume_inventory_raw = ((raw_bytes[40] as u32) << 24) | ((raw_bytes[41] as u32) << 16) | 
                                  ((raw_bytes[42] as u32) << 8) | (raw_bytes[43] as u32);

        Ok(FlowmeterRawPayload {
            device_address,
            timestamp: Utc::now(),
            raw_bytes,
            hex_string,
            payload_size,
            register_count,
            start_register,
            error_code_raw,
            mass_flow_rate_raw,
            density_flow_raw,
            temperature_raw,
            volume_flow_rate_raw,
            mass_total_raw,
            volume_total_raw,
            mass_inventory_raw,
            volume_inventory_raw,
        })
    }

    // Convert raw values to engineering units for verification
    pub fn to_engineering_units(&self) -> FlowmeterData {
        FlowmeterData {
            device_address: self.device_address,
            timestamp: self.timestamp,
            mass_flow_rate: f32::from_bits(self.mass_flow_rate_raw),
            density_flow: f32::from_bits(self.density_flow_raw),
            temperature: f32::from_bits(self.temperature_raw),
            volume_flow_rate: f32::from_bits(self.volume_flow_rate_raw),
            mass_total: f32::from_bits(self.mass_total_raw),
            volume_total: f32::from_bits(self.volume_total_raw),
            mass_inventory: f32::from_bits(self.mass_inventory_raw),
            volume_inventory: f32::from_bits(self.volume_inventory_raw),
            error_code: (self.error_code_raw & 0xFFFF) as u16, // Ensure error_code is u16
        }
    }

    // Get detailed debug information
    pub fn debug_info(&self) -> String {
        format!(
            "FlowmeterRawPayload Debug Info:\n\
            Device Address: {}\n\
            Timestamp: {}\n\
            Payload Size: {} bytes\n\
            Register Range: {} - {}\n\
            Hex Data: {}\n\
            \n\
            Raw Values (Hex):\n\
            Error Code: 0x{:08X}\n\
            Mass Flow Rate: 0x{:08X} -> {:.2}\n\
            Density Flow: 0x{:08X} -> {:.4}\n\
            Temperature: 0x{:08X} -> {:.2}\n\
            Volume Flow Rate: 0x{:08X} -> {:.3}\n\
            Mass Total: 0x{:08X} -> {:.2}\n\
            Volume Total: 0x{:08X} -> {:.3}\n\
            Mass Inventory: 0x{:08X} -> {:.2}\n\
            Volume Inventory: 0x{:08X} -> {:.3}\n",
            self.device_address,
            self.timestamp,
            self.payload_size,
            self.start_register,
            self.start_register + self.register_count - 1,
            self.hex_string,
            self.error_code_raw,
            self.mass_flow_rate_raw, f32::from_bits(self.mass_flow_rate_raw),
            self.density_flow_raw, f32::from_bits(self.density_flow_raw),
            self.temperature_raw, f32::from_bits(self.temperature_raw),
            self.volume_flow_rate_raw, f32::from_bits(self.volume_flow_rate_raw),
            self.mass_total_raw, f32::from_bits(self.mass_total_raw),
            self.volume_total_raw, f32::from_bits(self.volume_total_raw),
            self.mass_inventory_raw, f32::from_bits(self.mass_inventory_raw),
            self.volume_inventory_raw, f32::from_bits(self.volume_inventory_raw)
        )
    }

    // Get online decoder URL
    pub fn get_decoder_url(&self) -> String {
        format!("https://www.rapidtables.com/convert/number/hex-to-decimal.html?x={}", self.hex_string)
    }

    // Export raw data in different formats
    pub fn to_base64(&self) -> String {
        base64::encode(&self.raw_bytes)
    }

    pub fn to_binary_string(&self) -> String {
        self.raw_bytes.iter()
            .map(|b| format!("{:08b}", b))
            .collect::<Vec<String>>()
            .join(" ")
    }
}

impl fmt::Display for FlowmeterRawPayload {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "FlowmeterRaw[{}]: {} bytes, Hex: {}", 
               self.device_address, self.payload_size, 
               if self.hex_string.len() > 32 { 
                   format!("{}...", &self.hex_string[..32]) 
               } else { 
                   self.hex_string.clone() 
               })
    }
}