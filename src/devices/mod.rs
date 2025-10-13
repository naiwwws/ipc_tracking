pub mod traits;
pub mod flowmeter;
pub mod rpm; // NEW: Add RPM module
pub mod aio_module; // NEW: Add AIO module
pub mod gps;

pub use traits::{Device, DeviceData};
pub use flowmeter::{FlowmeterDevice, FlowmeterData};
pub use rpm::{RpmDevice, RpmChannelData}; // NEW: Export RPM types
pub use aio_module::{AioModuleDevice, AioModuleData, AioRpmChannelData, AioRpmChannelConfig}; // Export AIO RPM sensor types
pub use gps::{GpsService, GpsData};