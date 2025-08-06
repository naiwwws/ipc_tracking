//! Industrial Modbus Communication Library
//! 
//! This library provides a modular framework for communicating with industrial devices
//! over Modbus RTU protocol, with specific support for flowmeters and expandable
//! architecture for other device types.

pub mod config;
pub mod modbus;
pub mod devices;
pub mod services;
pub mod output;
pub mod utils;
pub mod cli;
pub mod storage;

pub use config::Config;
pub use services::DataService;

pub use devices::{Device, DeviceData, FlowmeterDevice, FlowmeterData};
pub use modbus::ModbusClient;
pub use output::{DataFormatter, DataSender, ConsoleFormatter, JsonFormatter, CsvFormatter, HexFormatter};
pub use utils::error::ModbusError;
pub use storage::{SqliteManager, FlowmeterReading, FlowmeterStats};

pub const VERSION: &str = "1.0.0";