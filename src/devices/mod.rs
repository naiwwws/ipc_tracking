pub mod traits;
pub mod flowmeter;
pub mod rpm; // NEW: Add RPM module
pub mod gps;

pub use traits::{Device, DeviceData};
pub use flowmeter::{FlowmeterDevice, FlowmeterData};
pub use rpm::{RpmDevice, RpmChannelData}; // NEW: Export RPM types
pub use gps::{GpsService, GpsData};