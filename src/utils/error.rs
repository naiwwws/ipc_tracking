use std::fmt;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ModbusError {
    #[error("Connection error: {0}")]
    ConnectionError(String),
    
    #[error("Communication error: {0}")]
    CommunicationError(String),
    
    #[error("CRC checksum mismatch")]
    CrcError,
    
    #[error("Invalid response from device")]
    InvalidResponse,
    
    #[error("Invalid data: {0}")]
    InvalidData(String),
    
    #[error("Device not found: address {0}")]
    InvalidDevice(u8),
    
    #[error("Lock acquisition failed")]
    LockError,
    
    #[error("Timeout occurred")]
    Timeout,

    #[error("Device not found: {0}")]
    DeviceNotFound(String),
    
    #[error("Configuration error: {0}")]
    ConfigError(String),
    
    #[error("Serialization error: {0}")]
    SerializationError(String),
    
    #[error("Service not available: {0}")]
    ServiceNotAvailable(String),
}

impl From<sqlx::Error> for ModbusError {
    fn from(err: sqlx::Error) -> Self {
        ModbusError::CommunicationError(format!("Database error: {}", err))
    }
}

impl From<serde_json::Error> for ModbusError {
    fn from(err: serde_json::Error) -> Self {
        ModbusError::SerializationError(format!("JSON error: {}", err))
    }
}

impl From<std::io::Error> for ModbusError {
    fn from(err: std::io::Error) -> Self {
        ModbusError::CommunicationError(format!("IO error: {}", err))
    }
}

impl From<tokio::time::error::Elapsed> for ModbusError {
    fn from(_: tokio::time::error::Elapsed) -> Self {
        ModbusError::Timeout
    }
}