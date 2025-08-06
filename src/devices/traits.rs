use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde_json::Value;
use std::any::Any;
use std::collections::HashMap;

use crate::modbus::client::ModbusClientTrait;
use crate::utils::error::ModbusError;

#[async_trait]
pub trait Device: Send + Sync {
    fn device_type(&self) -> &str;
    fn address(&self) -> u8;
    fn name(&self) -> &str;
    fn as_any(&self) -> &dyn Any;

    async fn read_data(&self, client: &dyn ModbusClientTrait) -> Result<Box<dyn DeviceData>, ModbusError>;
    async fn reset_accumulation(&self, client: &dyn ModbusClientTrait) -> Result<(), ModbusError>;
    fn parse_raw_data(&self, data: &[u8]) -> Result<Box<dyn DeviceData>, ModbusError>;
}

pub trait DeviceData: Send + Sync {
    fn as_any(&self) -> &dyn Any;
    fn device_address(&self) -> u8;
    fn timestamp(&self) -> DateTime<Utc>;
    
    // Add Unix timestamp method
    fn unix_ts(&self) -> i64 {
        self.timestamp().timestamp()
    }
    
    fn to_json(&self) -> Value;
    fn get_parameter(&self, name: &str) -> Option<String>;
    fn get_all_parameters(&self) -> Vec<(String, String)>;
    
    // Add this method for raw float values
    fn get_parameters_as_floats(&self) -> HashMap<String, f32>;
    
    fn device_type(&self) -> String;
    fn device_name(&self) -> String;
    fn device_location(&self) -> String;
    fn clone_box(&self) -> Box<dyn DeviceData>;
}