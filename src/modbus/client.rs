use async_trait::async_trait;
use log::{error, info};
use serialport::SerialPort;
use std::io::{Read, Write};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use super::crc::crc16_modbus;
use crate::config::settings::ParityConfig;
use crate::utils::error::ModbusError;

#[async_trait]
pub trait ModbusClientTrait: Send + Sync {
    async fn read_holding_registers(
        &self,
        slave_id: u8,
        start_addr: u16,
        count: u16,
    ) -> Result<Vec<u8>, ModbusError>;
    
    // NEW: Add input registers for RPM sensors
    async fn read_input_registers(
        &self,
        slave_id: u8,
        start_addr: u16,
        count: u16,
    ) -> Result<Vec<u8>, ModbusError>;
    
    async fn write_single_coil(
        &self,
        slave_id: u8,
        coil_addr: u16,
        value: bool,
    ) -> Result<(), ModbusError>;
    
    // NEW: Read discrete inputs for digital status
    async fn read_discrete_inputs(
        &self,
        slave_id: u8,
        start_addr: u16,
        count: u16,
    ) -> Result<Vec<bool>, ModbusError>;
}

pub struct ModbusClient {
    port: Arc<Mutex<Box<dyn SerialPort>>>,  // ← Wrapped in Arc<Mutex<>>
    last_communication: Arc<Mutex<Instant>>, // Add this for timing
}

impl ModbusClient {
    pub fn new(
        port_name: &str,
        baud_rate: u32,
        parity: &ParityConfig,
    ) -> Result<Self, ModbusError> {
        info!("🔌 Connecting to Modbus RTU port: {}", port_name);
        info!("⚙️  Configuration: {} baud, 8 data bits, 1 stop bit", baud_rate);

        let serial_parity = match parity {
            ParityConfig::None => serialport::Parity::None,
            ParityConfig::Even => serialport::Parity::Even,
            ParityConfig::Odd => serialport::Parity::Odd,
        };

        let port = serialport::new(port_name, baud_rate)
            .timeout(Duration::from_millis(1000))
            .data_bits(serialport::DataBits::Eight)
            .stop_bits(serialport::StopBits::One)
            .parity(serial_parity)
            .open()
            .map_err(|e| {
                error!("❌ Failed to open serial port {}: {}", port_name, e);
                ModbusError::ConnectionError(format!("Failed to open port: {}", e))
            })?;

        info!("✅ Modbus RTU connection established successfully");
        Ok(Self {
            port: Arc::new(Mutex::new(port)),
            last_communication: Arc::new(Mutex::new(Instant::now())), // Initialize timing
        })
    }

    // Add method to ensure proper timing between communications
    fn ensure_communication_gap(&self) -> Result<(), ModbusError> {
        let min_gap = Duration::from_millis(100); // Minimum gap between communications
        
        if let Ok(mut last_comm) = self.last_communication.lock() {
            let elapsed = last_comm.elapsed();
            if elapsed < min_gap {
                let wait_time = min_gap - elapsed;
                std::thread::sleep(wait_time);
            }
            *last_comm = Instant::now();
        }
        Ok(())
    }
}

#[async_trait]
impl ModbusClientTrait for ModbusClient {
    async fn read_holding_registers(
        &self,
        slave_id: u8,
        start_addr: u16,
        count: u16,
    ) -> Result<Vec<u8>, ModbusError> {
        // Ensure proper timing
        self.ensure_communication_gap()?;
        
        info!("📊 Reading {} registers from device {} starting at address {}", count, slave_id, start_addr);

        let mut request = vec![slave_id, 0x03];
        request.extend_from_slice(&start_addr.to_be_bytes());
        request.extend_from_slice(&count.to_be_bytes());

        let crc = crc16_modbus(&request);
        request.extend_from_slice(&crc.to_le_bytes());

        let mut port = self.port.lock().map_err(|_| ModbusError::LockError)?;

        // Clear any existing data in buffer
        port.clear(serialport::ClearBuffer::All)
            .map_err(|e| ModbusError::CommunicationError(format!("Buffer clear failed: {}", e)))?;

        port.write_all(&request)
            .map_err(|e| ModbusError::CommunicationError(format!("Write failed: {}", e)))?;
        port.flush()
            .map_err(|e| ModbusError::CommunicationError(format!("Flush failed: {}", e)))?;

        // Increased wait time for RS485 communication
        std::thread::sleep(Duration::from_millis(200));

        let expected_len = 5 + (count * 2) as usize;
        let mut response = vec![0u8; expected_len];

        port.read_exact(&mut response)
            .map_err(|e| ModbusError::CommunicationError(format!("Read failed: {}", e)))?;

        // Verify CRC
        let data_len = response.len() - 2;
        let received_crc = u16::from_le_bytes([response[data_len], response[data_len + 1]]);
        let calculated_crc = crc16_modbus(&response[..data_len]);

        if received_crc != calculated_crc {
            return Err(ModbusError::CrcError);
        }

        if response[0] != slave_id || response[1] != 0x03 {
            return Err(ModbusError::InvalidResponse);
        }

        // Return only the data bytes
        Ok(response[3..data_len].to_vec())
    }

    async fn write_single_coil(
        &self,
        slave_id: u8,
        coil_addr: u16,
        value: bool,
    ) -> Result<(), ModbusError> {
        self.ensure_communication_gap()?;
        
        let mut request = vec![slave_id, 0x05];
        request.extend_from_slice(&coil_addr.to_be_bytes());
        request.extend_from_slice(&(if value { 0xFF00u16 } else { 0x0000u16 }).to_be_bytes());

        let crc = crc16_modbus(&request);
        request.extend_from_slice(&crc.to_le_bytes());

        let mut port = self.port.lock().map_err(|_| ModbusError::LockError)?;

        port.write_all(&request)
            .map_err(|e| ModbusError::CommunicationError(format!("Write failed: {}", e)))?;
        port.flush()
            .map_err(|e| ModbusError::CommunicationError(format!("Flush failed: {}", e)))?;

        // Wait for and verify response
        thread::sleep(Duration::from_millis(50));

        let mut response = vec![0u8; 8];
        port.read_exact(&mut response)
            .map_err(|e| ModbusError::CommunicationError(format!("Read failed: {}", e)))?;

        let data_len = response.len() - 2;
        let received_crc = u16::from_le_bytes([response[data_len], response[data_len + 1]]);
        let calculated_crs = crc16_modbus(&response[..data_len]);

        if received_crc != calculated_crs || response[0] != slave_id || response[1] != 0x05 {
            return Err(ModbusError::InvalidResponse);
        }

        Ok(())
    }

    // NEW: Implement input registers reading (Function Code 0x04)
    async fn read_input_registers(
        &self,
        slave_id: u8,
        start_addr: u16,
        count: u16,
    ) -> Result<Vec<u8>, ModbusError> {
        info!("🔍 Reading {} input registers from device {} starting at address {}", count, slave_id, start_addr);

        let mut request = vec![slave_id, 0x04]; // Function code 0x04 for input registers
        request.extend_from_slice(&start_addr.to_be_bytes());
        request.extend_from_slice(&count.to_be_bytes());

        let crc = crc16_modbus(&request);
        request.extend_from_slice(&crc.to_le_bytes());

        let mut port = self.port.lock().map_err(|_| ModbusError::LockError)?;

        port.write_all(&request)
            .map_err(|e| ModbusError::CommunicationError(format!("Write failed: {}", e)))?;
        port.flush()
            .map_err(|e| ModbusError::CommunicationError(format!("Flush failed: {}", e)))?;

        // Wait for response
        thread::sleep(Duration::from_millis(50));

        let expected_len = 5 + (count * 2) as usize;
        let mut response = vec![0u8; expected_len];

        port.read_exact(&mut response)
            .map_err(|e| ModbusError::CommunicationError(format!("Read failed: {}", e)))?;

        // Verify CRC
        let data_len = response.len() - 2;
        let received_crc = u16::from_le_bytes([response[data_len], response[data_len + 1]]);
        let calculated_crc = crc16_modbus(&response[..data_len]);

        if received_crc != calculated_crc {
            return Err(ModbusError::CrcError);
        }

        if response[0] != slave_id || response[1] != 0x04 {
            return Err(ModbusError::InvalidResponse);
        }

        // Return only the data bytes
        Ok(response[3..data_len].to_vec())
    }

    // NEW: Implement discrete inputs reading (Function Code 0x02)
    async fn read_discrete_inputs(
        &self,
        slave_id: u8,
        start_addr: u16,
        count: u16,
    ) -> Result<Vec<bool>, ModbusError> {
        info!("🔍 Reading {} discrete inputs from device {} starting at address {}", count, slave_id, start_addr);

        let mut request = vec![slave_id, 0x02]; // Function code 0x02 for discrete inputs
        request.extend_from_slice(&start_addr.to_be_bytes());
        request.extend_from_slice(&count.to_be_bytes());

        let crc = crc16_modbus(&request);
        request.extend_from_slice(&crc.to_le_bytes());

        let mut port = self.port.lock().map_err(|_| ModbusError::LockError)?;

        port.write_all(&request)
            .map_err(|e| ModbusError::CommunicationError(format!("Write failed: {}", e)))?;
        port.flush()
            .map_err(|e| ModbusError::CommunicationError(format!("Flush failed: {}", e)))?;

        // Wait for response
        thread::sleep(Duration::from_millis(50));

        let byte_count = (count + 7) / 8; // Calculate number of bytes needed
        let expected_len = 5 + byte_count as usize;
        let mut response = vec![0u8; expected_len];

        port.read_exact(&mut response)
            .map_err(|e| ModbusError::CommunicationError(format!("Read failed: {}", e)))?;

        // Verify CRC
        let data_len = response.len() - 2;
        let received_crc = u16::from_le_bytes([response[data_len], response[data_len + 1]]);
        let calculated_crc = crc16_modbus(&response[..data_len]);

        if received_crc != calculated_crc {
            return Err(ModbusError::CrcError);
        }

        if response[0] != slave_id || response[1] != 0x02 {
            return Err(ModbusError::InvalidResponse);
        }

        // Convert bytes to boolean array
        let mut bits = Vec::new();
        let data_bytes = &response[3..data_len];
        
        for (byte_idx, &byte_val) in data_bytes.iter().enumerate() {
            for bit_idx in 0..8 {
                if byte_idx * 8 + bit_idx >= count as usize {
                    break;
                }
                bits.push((byte_val & (1 << bit_idx)) != 0);
            }
        }

        Ok(bits)
    }
}