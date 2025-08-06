use chrono::Utc;
use serde_json::Value;
use std::collections::HashMap;

use crate::devices::traits::DeviceData;
use crate::utils::error::ModbusError;

pub trait DataFormatter: Send + Sync {
    fn format_single_device(&self, addr: u8, data: &dyn DeviceData) -> String;
    fn format_header(&self) -> String;
    fn formatter_type(&self) -> &str;
}

pub struct ConsoleFormatter;

impl DataFormatter for ConsoleFormatter {
    fn format_single_device(&self, addr: u8, data: &dyn DeviceData) -> String {
        let params = data.get_parameters_as_floats();
        format!(
            "Device {}: Mass Flow: {:.3}, Temperature: {:.2}°C, Density: {:.4}, Volume Flow: {:.3}",
            addr,
            params.get("MassFlowRate").unwrap_or(&0.0),
            params.get("Temperature").unwrap_or(&0.0),
            params.get("DensityFlow").unwrap_or(&0.0),
            params.get("VolumeFlowRate").unwrap_or(&0.0)
        )
    }
    
    fn format_header(&self) -> String {
        "Device Data:".to_string()
    }
    
    fn formatter_type(&self) -> &str {
        "console"
    }
}

// ADD: Missing JsonFormatter
pub struct JsonFormatter;

impl DataFormatter for JsonFormatter {
    fn format_single_device(&self, addr: u8, data: &dyn DeviceData) -> String {
        let params = data.get_parameters_as_floats();
        serde_json::json!({
            "device_address": addr,
            "timestamp": data.unix_ts(),
            "data": params
        }).to_string()
    }
    
    fn format_header(&self) -> String {
        "".to_string()
    }
    
    fn formatter_type(&self) -> &str {
        "json"
    }
}

// ADD: Missing CsvFormatter
pub struct CsvFormatter;

impl DataFormatter for CsvFormatter {
    fn format_single_device(&self, addr: u8, data: &dyn DeviceData) -> String {
        let params = data.get_parameters_as_floats();
        format!(
            "{},{},{:.3},{:.2},{:.4},{:.3},{:.3},{:.3}",
            addr,
            data.unix_ts(),
            params.get("MassFlowRate").unwrap_or(&0.0),
            params.get("Temperature").unwrap_or(&0.0),
            params.get("DensityFlow").unwrap_or(&0.0),
            params.get("VolumeFlowRate").unwrap_or(&0.0),
            params.get("MassTotal").unwrap_or(&0.0),
            params.get("VolumeTotal").unwrap_or(&0.0)
        )
    }
    
    fn format_header(&self) -> String {
        "Device,Timestamp,MassFlow,Temperature,Density,VolumeFlow,MassTotal,VolumeTotal,ErrorCode".to_string()
    }
    
    fn formatter_type(&self) -> &str {
        "csv"
    }
}

// ADD: Missing HexFormatter
pub struct HexFormatter;

impl DataFormatter for HexFormatter {
    fn format_single_device(&self, addr: u8, data: &dyn DeviceData) -> String {
        let params = data.get_parameters_as_floats();
        let mut hex_data = Vec::new();
        
        for (key, value) in params {
            hex_data.push(format!("{}:0x{:08X}", key, value.to_bits()));
        }
        
        format!("Device {}: [{}]", addr, hex_data.join(", "))
    }
    
    fn format_header(&self) -> String {
        "Hex Device Data:".to_string()
    }
    
    fn formatter_type(&self) -> &str {
        "hex"
    }
}