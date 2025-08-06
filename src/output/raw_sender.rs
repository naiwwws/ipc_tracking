use async_trait::async_trait;
use log::{info, error};
use std::fs::OpenOptions;
use std::io::Write;
use tokio::fs;

use crate::devices::flowmeter::FlowmeterRawPayload;
use crate::utils::error::ModbusError;
use super::senders::DataSender;

pub struct RawDataSender {
    file_path: String,
    format: RawDataFormat,
    append: bool,
}

#[derive(Debug, Clone)]
pub enum RawDataFormat {
    Hex,        // Hexadecimal string
    Binary,     // Binary file
    Json,       // JSON with metadata
    Debug,      // Debug format with detailed info
}

impl RawDataSender {
    pub fn new(file_path: &str, format: RawDataFormat, append: bool) -> Self {
        Self {
            file_path: file_path.to_string(),
            format,
            append,
        }
    }

    pub async fn send_raw_payload(&self, payload: &FlowmeterRawPayload) -> Result<(), ModbusError> {
        match self.format {
            RawDataFormat::Hex => self.send_hex_format(payload).await,
            RawDataFormat::Binary => self.send_binary_format(payload).await,
            RawDataFormat::Json => self.send_json_format(payload).await,
            RawDataFormat::Debug => self.send_debug_format(payload).await,
        }
    }

    async fn send_hex_format(&self, payload: &FlowmeterRawPayload) -> Result<(), ModbusError> {
        let content = format!(
            "{} | Device {} | Size: {} bytes | Hex: {}\n",
            payload.timestamp.format("%Y-%m-%d %H:%M:%S%.3f"),
            payload.device_address,
            payload.payload_size,
            payload.hex_string
        );

        self.write_to_file(&content).await
    }

    async fn send_binary_format(&self, payload: &FlowmeterRawPayload) -> Result<(), ModbusError> {
        if self.append {
            let mut file = OpenOptions::new()
                .create(true)
                .append(true)
                .open(&self.file_path)
                .map_err(|e| ModbusError::CommunicationError(format!("File open error: {}", e)))?;
            
            file.write_all(&payload.raw_bytes)
                .map_err(|e| ModbusError::CommunicationError(format!("File write error: {}", e)))?;
        } else {
            fs::write(&self.file_path, &payload.raw_bytes).await
                .map_err(|e| ModbusError::CommunicationError(format!("File write error: {}", e)))?;
        }

        info!("📝 Written {} bytes to binary file: {}", payload.payload_size, self.file_path);
        Ok(())
    }

    async fn send_json_format(&self, payload: &FlowmeterRawPayload) -> Result<(), ModbusError> {
        let json_data = serde_json::to_string_pretty(payload)
            .map_err(|e| ModbusError::InvalidData(format!("JSON serialization error: {}", e)))?;

        let content = format!("{}\n", json_data);
        self.write_to_file(&content).await
    }

    async fn send_debug_format(&self, payload: &FlowmeterRawPayload) -> Result<(), ModbusError> {
        let debug_content = format!(
            "=====================================\n\
            {}\n\
            Online Decoder URL: {}\n\
            Base64: {}\n\
            Binary: {}\n\
            =====================================\n\n",
            payload.debug_info(),
            payload.get_decoder_url(),
            payload.to_base64(),
            payload.to_binary_string()
        );

        self.write_to_file(&debug_content).await
    }

    async fn write_to_file(&self, content: &str) -> Result<(), ModbusError> {
        if self.append {
            let mut file = OpenOptions::new()
                .create(true)
                .append(true)
                .open(&self.file_path)
                .map_err(|e| ModbusError::CommunicationError(format!("File open error: {}", e)))?;
            
            write!(file, "{}", content)
                .map_err(|e| ModbusError::CommunicationError(format!("File write error: {}", e)))?;
        } else {
            fs::write(&self.file_path, content).await
                .map_err(|e| ModbusError::CommunicationError(format!("File write error: {}", e)))?;
        }

        info!("📝 Raw data written to: {}", self.file_path);
        Ok(())
    }
}

#[async_trait]
impl DataSender for RawDataSender {
    async fn send(&self, data: &str) -> Result<(), ModbusError> {
        // This is for compatibility with the DataSender trait
        // The actual raw data sending should use send_raw_payload
        self.write_to_file(data).await
    }

    fn sender_type(&self) -> &str {
        "raw_data"
    }

    fn destination(&self) -> &str {
        &self.file_path
    }
}