pub mod client;
pub mod protocol;
pub mod crc;

pub use client::{ModbusClient, ModbusClientTrait};  // ← Export the trait
pub use protocol::{ModbusRequest, ModbusResponse};
pub use crc::crc16_modbus;