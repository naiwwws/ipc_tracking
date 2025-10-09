use chrono::{DateTime, Utc};
use clap::ArgMatches;
use log::{debug, error, info, warn};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    // IPC Identification
    pub ipc_uuid: String,               // NEW: Unique identifier for this IPC
    pub ipc_name: String,               // NEW: Human-readable name for this IPC
    pub ipc_version: String,            // NEW: Software version
    
    // Connection settings
    pub serial_port: String,
    pub baud_rate: u32,
    pub timeout_ms: u64,
    pub parity: ParityConfig,
    
    // Monitoring settings
    pub update_interval_seconds: u64,
    pub max_retries: u32,
    pub retry_delay_ms: u64,
    
    // Device configuration
    pub devices: Vec<DeviceConfig>,
    
    // Data collection settings
    pub data_collection: DataCollectionConfig,
    
    // Output settings
    pub output: OutputConfig,
    
    // Metadata for payload merging
    pub site_info: SiteInfo,
    
    // Legacy compatibility
    pub device_addresses: Vec<u8>,

    // ADD: API server configuration
    pub api_server: ApiServerConfig,

    // NEW: GPS configuration
    pub gps: GpsConfig,

    // NEW: MTWS configuration
    pub mtws: MtwsConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceConfig {
    pub uuid: String,                   // Device UUID
    pub address: u8,                    // Modbus address
    pub device_type: String,            // Device type
    pub name: String,                   // Device name
    pub location: String,               // Physical location
    pub enabled: bool,                  // Whether device is enabled
    pub polling_interval: Option<u64>,  // Custom polling interval
    pub parameters: Vec<String>,        // Parameters to read
    pub metadata: HashMap<String, String>, // Additional metadata
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataCollectionConfig {
    pub batch_size: usize,
    pub buffer_size: usize,
    pub auto_save_interval: u64,
    pub include_raw_data: bool,
    pub include_timestamps: bool,
    pub include_metadata: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutputConfig {
    pub default_format: String,
    pub file_output: Option<FileOutputConfig>,
    pub http_output: Option<HttpOutputConfig>,
    pub mqtt_output: Option<MqttOutputConfig>,
    pub database_output: Option<DatabaseOutputConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileOutputConfig {
    pub enabled: bool,
    pub path: String,
    pub rotate: bool,
    pub max_file_size_mb: u64,
    pub compression: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpOutputConfig {
    pub enabled: bool,
    pub endpoint: String,
    pub headers: HashMap<String, String>,
    pub timeout_seconds: u64,
    pub retry_attempts: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MqttOutputConfig {
    pub enabled: bool,
    pub broker: String,
    pub topic_prefix: String,
    pub qos: u8,
    pub retain: bool,
    pub client_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatabaseOutputConfig {
    pub enabled: bool,
    pub sqlite_config: SqliteConfig,
    pub batch_size: usize,
    pub flush_interval_seconds: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SqliteConfig {
    pub database_path: String,
    pub max_connections: usize,
    pub connection_timeout_seconds: u64,
    pub enable_wal: bool,
    pub cache_size: i32,
    pub auto_vacuum: bool,
    pub batch_size: usize,
    pub busy_timeout_ms: u64,
    pub wal_mode: bool,
    pub sync_mode: String,
    pub cache_size_kb: i32,
}

impl Default for SqliteConfig {
    fn default() -> Self {
        Self {
            database_path: "data/sensor_data.db".to_string(),
            max_connections: 5,
            connection_timeout_seconds: 30,
            enable_wal: false,
            cache_size: 2000,
            auto_vacuum: true,
            batch_size: 500,
            busy_timeout_ms: 30000,
            wal_mode: false,
            sync_mode: "OFF".to_string(),
            cache_size_kb: 2000,
        }
    }
}
impl Default for FileOutputConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            path: "./data/sensor_data.json".to_string(),
            rotate: true,
            max_file_size_mb: 100,
            compression: false,
        }
    }
}

impl Default for DatabaseOutputConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            sqlite_config: SqliteConfig::default(),
            batch_size: 100,
            flush_interval_seconds: 60,
        }
    }
}

// ✅ ADD: Complete ApiServerConfig if missing
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiServerConfig {
    pub enabled: bool,
    pub port: u16,
    pub host: String,
    pub cors_enabled: bool,
    pub cors_origins: Vec<String>,
}

impl Default for ApiServerConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            port: 3000,
            host: "0.0.0.0".to_string(),
            cors_enabled: true,
            cors_origins: vec!["*".to_string()],
        }
    }
}

// Add GPS configuration struct
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GpsConfig {
    pub enabled: bool,
    pub port: String,
    pub baud_rate: u32,
    pub auto_start: bool,
}

impl Default for GpsConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            port: "/dev/ttyUSB2".to_string(),
            baud_rate: 9600,
            auto_start: true,
        }
    }
}

// Add MTWS configuration to the Config struct
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MtwsConfig {
    pub enabled: bool,
    pub imei: String,
    pub base_endpoint_url: String,
    pub transmission_interval_seconds: u64,
    pub timeout_seconds: u64,
    pub retry_attempts: u32,
    pub retry_delay_seconds: u64,
    pub auto_start: bool,
}

impl Default for MtwsConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            imei: "123456789012345".to_string(),
            base_endpoint_url: "http://mtws.masihplayground.my.id:80/SubmitForm".to_string(),
            transmission_interval_seconds: 300,
            timeout_seconds: 30,
            retry_attempts: 3,
            retry_delay_seconds: 60,
            auto_start: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SiteInfo {
    pub site_id: String,
    pub site_name: String,
    pub location: String,
    pub timezone: String,
    pub operator: String,
    pub department: String,
    pub contact_email: String,
    pub metadata: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ParityConfig {
    None,
    Even,
    Odd,
}

// Keep legacy RegisterConfig for backward compatibility
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisterConfig {
    pub address: u16,
    pub count: u16,
    pub register_type: String,
    pub description: String,
}

impl Default for Config {
    fn default() -> Self {
        let ipc_uuid = Uuid::new_v4().to_string();
        let mut default_devices = Vec::new();
        
        // Device 1: Inlet Flowmeter
        default_devices.push(DeviceConfig {
            uuid: Uuid::new_v4().to_string(),
            address: 2,
            device_type: "flowmeter".to_string(),
            name: "Inlet Flowmeter".to_string(),
            location: "Building A - Line 1".to_string(),
            enabled: true,
            polling_interval: None,
            parameters: vec![
                "MassFlowRate".to_string(),
                "Temperature".to_string(),
                "DensityFlow".to_string(),
                "VolumeFlowRate".to_string(),
            ],
            metadata: {
                let mut map = HashMap::new();
                map.insert("calibration_date".to_string(), "2024-01-15".to_string());
                map.insert("serial_number".to_string(), "TT_SB001-2024".to_string());
                map.insert("manufacturer".to_string(), "Sealand".to_string());
                map.insert("model".to_string(), "Coriolosis".to_string());
                map
            },
        });
        
        // Device 2: Outlet Flowmeter
        default_devices.push(DeviceConfig {
            uuid: Uuid::new_v4().to_string(),
            address: 3,
            device_type: "flowmeter".to_string(),
            name: "Outlet Flowmeter".to_string(),
            location: "Building A - Line 2".to_string(),
            enabled: true,
            polling_interval: None,
            parameters: vec![
                "MassFlowRate".to_string(),
                "Temperature".to_string(),
                "DensityFlow".to_string(),
            ],
            metadata: {
                let mut map = HashMap::new();
                map.insert("calibration_date".to_string(), "2024-02-10".to_string());
                map.insert("serial_number".to_string(), "TT_SB002-2024".to_string());
                map.insert("manufacturer".to_string(), "Sealand".to_string());
                map.insert("model".to_string(), "Coriolosis".to_string());
                map
            },
        });

        Self {
            // IPC Identification
            ipc_uuid,
            ipc_name: "Industrial Data Collector".to_string(),
            ipc_version: crate::VERSION.to_string(),
            
            // Connection settings
            serial_port: "/dev/ttyS0".to_string(),
            baud_rate: 9600,
            timeout_ms: 1000,
            parity: ParityConfig::None,
            
            // Monitoring settings
            update_interval_seconds: 10,
            max_retries: 3,
            retry_delay_ms: 500,
            
            // Device configuration
            devices: default_devices.clone(),
            
            // Data collection settings
            data_collection: DataCollectionConfig {
                batch_size: 10,
                buffer_size: 100,
                auto_save_interval: 300,
                include_raw_data: false,
                include_timestamps: true,
                include_metadata: true,
            },
            
            // Output settings
            output: OutputConfig {
                default_format: "json".to_string(),
                file_output: Some(FileOutputConfig {
                    enabled: false,
                    path: "./data/sensor_data.json".to_string(),
                    rotate: true,
                    max_file_size_mb: 100,
                    compression: false,
                }),
                http_output: None,
                mqtt_output: None,
                database_output: Some(DatabaseOutputConfig {
                    enabled: true,
                    sqlite_config: SqliteConfig::default(),
                    batch_size: 100,
                    flush_interval_seconds: 60,
                }),
            },
            
            // Site information
            site_info: SiteInfo {
                site_id: "SITE_001".to_string(),
                site_name: "Industrial Plant A".to_string(),
                location: "Factory District, Industrial Zone".to_string(),
                timezone: "UTC+07:00".to_string(),
                operator: "Plant Operations Team".to_string(),
                department: "Production".to_string(),
                contact_email: "operations@plant.com".to_string(),
                metadata: {
                    let mut map = HashMap::new();
                    map.insert("region".to_string(), "Asia Pacific".to_string());
                    map.insert("facility_code".to_string(), "FAC_001".to_string());
                    map
                },
            },
            
            // Legacy compatibility
            device_addresses: default_devices.iter().map(|d| d.address).collect(),
            
            // API server configuration
            api_server: ApiServerConfig::default(),

            // GPS configuration
            gps: GpsConfig::default(),

            // MTWS configuration
            mtws: MtwsConfig::default(),
        }
    }
}

fn get_default_parameters_for_type(device_type: &str) -> Vec<String> {
    match device_type {
        "flowmeter" => vec![
            "MassFlowRate".to_string(),
            "Temperature".to_string(),
            "DensityFlow".to_string(),
            "VolumeTotal".to_string(),
        ],
        "rpm" => vec![
            "RPM".to_string(),
            "EngineDuration".to_string(),
            "IsRunning".to_string(),
        ],
        "gps" => vec![
            "Latitude".to_string(),
            "Longitude".to_string(),
            "Speed".to_string(),
            "Course".to_string(),
        ],
        "aio" => vec![
            "BaudRate".to_string(),
            "ChannelCount".to_string(),
            "AIN1".to_string(), "AIN2".to_string(), "AIN3".to_string(), "AIN4".to_string(),
            "AIN5".to_string(), "AIN6".to_string(), "AIN7".to_string(), "AIN8".to_string(),
            "DIN1".to_string(), "DIN2".to_string(), "DIN3".to_string(), "DIN4".to_string(),
            "DIN5".to_string(), "DIN6".to_string(), "DIN7".to_string(), "DIN8".to_string(),
            "DIN9".to_string(), "DIN10".to_string(), "DIN11".to_string(), "DIN12".to_string(),
            "DIN13".to_string(), "DIN14".to_string(), "DIN15".to_string(), "DIN16".to_string(),
        ],
        _ => vec!["Status".to_string()],
    }
}

impl Config {
    pub fn from_matches(matches: &ArgMatches) -> Result<Self, Box<dyn std::error::Error>> {
        let mut config = Self::default();
        
        // Override with command line arguments
        config.serial_port = matches.get_one::<String>("port").unwrap().clone();
        config.baud_rate = matches.get_one::<String>("baud").unwrap().parse()?;
        config.update_interval_seconds = matches.get_one::<String>("interval").unwrap().parse()?;
        
        // Parse devices if provided
        if let Some(devices_str) = matches.get_one::<String>("devices") {
            let addresses: Vec<u8> = devices_str
                .split(',')
                .map(|s| s.trim().parse::<u8>())
                .collect::<Result<Vec<_>, _>>()?;
            
            // Update legacy device_addresses
            config.device_addresses = addresses.clone();
            
            // Update or create device configs
            for addr in addresses.clone() {
                if !config.devices.iter().any(|d| d.address == addr) {
                    config.devices.push(DeviceConfig {
                        uuid: Uuid::new_v4().to_string(),
                        address: addr,
                        device_type: "flowmeter".to_string(),
                        name: format!("Device {}", addr),
                        location: "Unknown".to_string(),
                        enabled: true,
                        polling_interval: None,
                        parameters: vec!["MassFlowRate".to_string(), "Temperature".to_string()],
                        metadata: HashMap::new(),
                    });
                }
            }
            
            // Remove devices not in the list
            config.devices.retain(|d| addresses.clone().contains(&d.address));
        }
        
        Ok(config)
    }

    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Self, Box<dyn std::error::Error>> {
        let path_ref = path.as_ref();
        
        if !path_ref.exists() {
            return Err(format!("Config file does not exist: {}", path_ref.display()).into());
        }
        
        info!("📖 Reading config file: {}", path_ref.display());
        
        let content = std::fs::read_to_string(path_ref)
            .map_err(|e| format!("Failed to read config file {}: {}", path_ref.display(), e))?;
        
        debug!("📝 Config file content length: {} bytes", content.len());
        
        let mut config: Config = toml::from_str(&content)
            .map_err(|e| {
                error!("❌ TOML parsing error in {}: {}", path_ref.display(), e);
                error!("📝 Error details: {}", e);
                format!("Invalid TOML syntax in {}: {}", path_ref.display(), e)
            })?;
        
        // ✅ ENHANCED: Backward compatibility and validation
        if config.ipc_uuid.is_empty() || config.ipc_uuid == "auto-generated" {
            config.ipc_uuid = Uuid::new_v4().to_string();
            info!("🔧 Generated new IPC UUID: {}", config.ipc_uuid);
        }
        
        if config.ipc_name.is_empty() {
            config.ipc_name = "Industrial Data Collector".to_string();
            info!("🔧 Set default IPC name: {}", config.ipc_name);
        }
        
        if config.ipc_version.is_empty() {
            config.ipc_version = crate::VERSION.to_string();
            info!("🔧 Set IPC version: {}", config.ipc_version);
        }
        
        // ✅ ENSURE: device_addresses sync
        config.device_addresses = config.devices.iter().map(|d| d.address).collect();
 
        
        // ✅ AUTO-GENERATE: Device UUIDs if needed
        for device in &mut config.devices {
            if device.uuid.is_empty() || device.uuid == "auto-generated" {
                device.uuid = Uuid::new_v4().to_string();
                info!("🔧 Generated UUID for device '{}': {}", device.name, device.uuid);
            }
        }
        
        info!("✅ Successfully loaded config from: {}", path_ref.display());
        info!("   - Devices: {}", config.devices.len());

        info!("   - API Server: {} (port: {})", 
              config.api_server.enabled, config.api_server.port);
        let db_enabled = config.output.database_output.as_ref().map(|db| db.enabled).unwrap_or(false);
        info!("   - Database: {}", if db_enabled { "enabled" } else { "disabled" });
        
        Ok(config)
    }

    pub fn save_to_file<P: AsRef<Path>>(&self, path: P) -> Result<(), Box<dyn std::error::Error>> {
        let path_ref = path.as_ref();
        
        // ✅ ENSURE: Create directory
        if let Some(parent) = path_ref.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create directory {}: {}", parent.display(), e))?;
        }
        
        // ✅ ENHANCE: Pretty TOML output
        let content = toml::to_string_pretty(self)
            .map_err(|e| format!("Failed to serialize config to TOML: {}", e))?;
        
        std::fs::write(path_ref, content)
            .map_err(|e| format!("Failed to write config file {}: {}", path_ref.display(), e))?;
        
        info!("💾 Config saved to: {}", path_ref.display());
        Ok(())
    }

    // NEW: IPC identification methods
    pub fn get_ipc_uuid(&self) -> &str {
        &self.ipc_uuid
    }

    pub fn get_ipc_name(&self) -> &str {
        &self.ipc_name
    }

    pub fn get_ipc_version(&self) -> &str {
        &self.ipc_version
    }

    pub fn set_ipc_name(&mut self, name: String) {
        self.ipc_name = name;
    }

    pub fn regenerate_ipc_uuid(&mut self) {
        self.ipc_uuid = Uuid::new_v4().to_string();
    }

    // Get list of enabled devices
    pub fn get_enabled_devices(&self) -> Vec<&DeviceConfig> {
        self.devices.iter().filter(|d| d.enabled).collect()
    }

    // Get device by address
    pub fn get_device_by_address(&self, address: u8) -> Option<&DeviceConfig> {
        self.devices.iter().find(|d| d.address == address)
    }

    // Get device by UUID
    pub fn get_device_by_uuid(&self, uuid: &str) -> Option<&DeviceConfig> {
        self.devices.iter().find(|d| d.uuid == uuid)
    }

    // Get device by name
    pub fn get_device_by_name(&self, name: &str) -> Option<&DeviceConfig> {
        self.devices.iter().find(|d| d.name == name)
    }

    // Legacy compatibility - get device addresses
    pub fn device_addresses(&self) -> Vec<u8> {
        self.get_enabled_devices().iter().map(|d| d.address).collect()
    }

    // Sync device_addresses with devices (call after modifying devices)
    pub fn sync_device_addresses(&mut self) {
        self.device_addresses = self.devices.iter().map(|d| d.address).collect();
    }

    // Create a new device with UUID
    pub fn create_new_device(&self, address: u8, device_id: String, device_type: String, name: String, location: String) -> DeviceConfig {
        let mut metadata = HashMap::new();
        
        // Add default metadata based on device type
        match device_type.as_str() {
            "rpm" => {
                metadata.insert("total_channels".to_string(), "2".to_string());
                metadata.insert("rpm_threshold".to_string(), "500".to_string());
                metadata.insert("outlier_detection_threshold".to_string(), "150".to_string());
                metadata.insert("outlier_confirmation_threshold".to_string(), "15".to_string());
                metadata.insert("engine_types".to_string(), "main,aux".to_string());
                metadata.insert("auto_detect_channels".to_string(), "true".to_string());
            }
            "flowmeter" => {
                metadata.insert("flow_threshold".to_string(), "0.1".to_string());
                metadata.insert("calibration_factor".to_string(), "1.0".to_string());
                metadata.insert("density_correction".to_string(), "true".to_string());
            }
            "gps" => {
                metadata.insert("update_rate".to_string(), "1".to_string());
                metadata.insert("precision".to_string(), "high".to_string());
                metadata.insert("altitude_enabled".to_string(), "true".to_string());
            }
            "aio" => {
                metadata.insert("total_analog_channels".to_string(), "8".to_string());
                metadata.insert("digital_inputs_count".to_string(), "16".to_string());
                metadata.insert("channel_types".to_string(), "rpm,rpm,pulse,frequency,rpm,pulse,frequency,pulse".to_string());
                metadata.insert("rpm_thresholds".to_string(), "500,500,0,0,500,0,0,0".to_string());
                metadata.insert("auto_detect_channels".to_string(), "true".to_string());
                metadata.insert("baud_rate".to_string(), "9600".to_string());
                
                // Set individual channel configurations
                let channel_types = ["rpm", "rpm", "pulse", "frequency", "rpm", "pulse", "frequency", "pulse"];
                let thresholds = [500, 500, 0, 0, 500, 0, 0, 0];
                
                for (i, (&channel_type, &threshold)) in channel_types.iter().zip(thresholds.iter()).enumerate() {
                    metadata.insert(format!("channel_{}_type", i + 1), channel_type.to_string());
                    metadata.insert(format!("channel_{}_threshold", i + 1), threshold.to_string());
                }
            }
            _ => {}
        }

        let device_type_clone = device_type.clone(); // Clone before moving

        DeviceConfig {
            uuid: uuid::Uuid::new_v4().to_string(),
            address,
            device_type,
            name,
            location,
            enabled: true,
            polling_interval: None,
            parameters: get_default_parameters_for_type(&device_type_clone), // Use the clone
            metadata,
        }
    }

    // Get multi-channel RPM configuration
    pub fn get_rpm_channel_config(&self, address: u8) -> (u8, Vec<u16>) {
        if let Some(device) = self.get_device_by_address(address) {
            if device.device_type == "rpm" {
                let total_channels = device.metadata.get("total_channels")
                    .and_then(|s| s.parse::<u8>().ok())
                    .unwrap_or(2);
                
                let mut thresholds = Vec::new();
                for i in 1..=total_channels {
                    let threshold_key = format!("rpm_threshold_ch{}", i);
                    let threshold = device.metadata.get(&threshold_key)
                        .and_then(|s| s.parse::<u16>().ok())
                        .or_else(|| device.metadata.get("rpm_threshold")
                            .and_then(|s| s.parse::<u16>().ok()))
                        .unwrap_or(500);
                    thresholds.push(threshold);
                }
                
                return (total_channels, thresholds);
            }
        }
        (2, vec![500, 500]) // Default configuration
    }

    // Get outlier detection settings for RPM device
    pub fn get_rpm_outlier_settings(&self, address: u8) -> (u16, u16) {
        if let Some(device) = self.get_device_by_address(address) {
            if device.device_type == "rpm" {
                let detection_threshold = device.metadata.get("outlier_detection_threshold")
                    .and_then(|s| s.parse::<u16>().ok())
                    .unwrap_or(150);
                
                let confirmation_threshold = device.metadata.get("outlier_confirmation_threshold")
                    .and_then(|s| s.parse::<u16>().ok())
                    .unwrap_or(15);
                
                return (detection_threshold, confirmation_threshold);
            }
        }
        (150, 15) // Default values
    }

    // Check if auto-detection is enabled
    pub fn is_rpm_auto_detect_enabled(&self, address: u8) -> bool {
        if let Some(device) = self.get_device_by_address(address) {
            return device.metadata.get("auto_detect_channels")
                .and_then(|s| s.parse::<bool>().ok())
                .unwrap_or(true);
        }
        true
    }

    // AIO Module Configuration Methods
    
    // Get AIO channel configuration
    pub fn get_aio_channel_config(&self, address: u8) -> (u8, u8, Vec<String>) {
        if let Some(device) = self.get_device_by_address(address) {
            if device.device_type == "aio" {
                let analog_channels = device.metadata.get("total_analog_channels")
                    .and_then(|s| s.parse::<u8>().ok())
                    .unwrap_or(8);
                
                let digital_inputs = device.metadata.get("digital_inputs_count")
                    .and_then(|s| s.parse::<u8>().ok())
                    .unwrap_or(16);
                
                let channel_types = device.metadata.get("channel_types")
                    .map(|s| s.split(',').map(String::from).collect())
                    .unwrap_or_else(|| vec!["rpm".to_string(); analog_channels as usize]);
                
                return (analog_channels, digital_inputs, channel_types);
            }
        }
        (8, 16, vec!["rpm".to_string(); 8]) // Default configuration
    }

    // Get AIO channel thresholds for RPM channels
    pub fn get_aio_channel_thresholds(&self, address: u8) -> Vec<u16> {
        if let Some(device) = self.get_device_by_address(address) {
            if device.device_type == "aio" {
                let (analog_channels, _, channel_types) = self.get_aio_channel_config(address);
                let mut thresholds = Vec::new();
                
                for i in 0..analog_channels {
                    let threshold_key = format!("channel_{}_threshold", i + 1);
                    let threshold = device.metadata.get(&threshold_key)
                        .and_then(|s| s.parse::<u16>().ok())
                        .unwrap_or(if channel_types.get(i as usize).unwrap_or(&"pulse".to_string()) == "rpm" { 500 } else { 0 });
                    thresholds.push(threshold);
                }
                
                return thresholds;
            }
        }
        vec![500, 500, 0, 0, 500, 0, 0, 0] // Default thresholds
    }

    // Check if AIO auto-detection is enabled
    pub fn is_aio_auto_detect_enabled(&self, address: u8) -> bool {
        if let Some(device) = self.get_device_by_address(address) {
            return device.metadata.get("auto_detect_channels")
                .and_then(|s| s.parse::<bool>().ok())
                .unwrap_or(true);
        }
        true
    }

    // Get AIO baud rate
    pub fn get_aio_baud_rate(&self, address: u8) -> u16 {
        if let Some(device) = self.get_device_by_address(address) {
            return device.metadata.get("baud_rate")
                .and_then(|s| s.parse::<u16>().ok())
                .unwrap_or(9600);
        }
        9600
    }

    // Add method to get enabled flowmeter devices
    pub fn get_enabled_flowmeter_devices(&self) -> Vec<DeviceConfig> {
        self.get_enabled_devices()
            .into_iter()
            .filter(|device| device.device_type == "flowmeter")
            .cloned()
            .collect()
    }

    // Add method to validate flowmeter configuration
    pub fn validate_flowmeter_config(&self) -> Result<(), String> {
        let flowmeter_devices = self.get_enabled_flowmeter_devices();
        
        if flowmeter_devices.is_empty() {
            return Err("No flowmeter devices configured".to_string());
        }

        // Check for duplicate addresses
        let mut addresses = std::collections::HashSet::new();
        for device in &flowmeter_devices {
            if !addresses.insert(device.address) {
                return Err(format!("Duplicate flowmeter address: {}", device.address));
            }
        }

        info!("✅ Flowmeter configuration valid: {} devices", flowmeter_devices.len());
        Ok(())
    }

    // Add method to get full MTWS endpoint with IMEI
    pub fn get_mtws_endpoint_url(&self) -> String {
        format!("{}/{}", self.mtws.base_endpoint_url, self.mtws.imei)
    }

    pub fn get_device_imei(&self) -> &str {
        &self.mtws.imei
    }

    // Add method to validate MTWS configuration
    pub fn validate_mtws_config(&self) -> Result<(), String> {
        if !self.mtws.enabled {
            return Ok(());
        }

        if self.mtws.imei.len() != 15 || !self.mtws.imei.chars().all(|c| c.is_ascii_digit()) {
            return Err("IMEI must be exactly 15 digits".to_string());
        }

        if self.mtws.base_endpoint_url.is_empty() {
            return Err("Base endpoint URL cannot be empty".to_string());
        }

        if self.mtws.transmission_interval_seconds < 60 {
            return Err("Transmission interval must be at least 60 seconds".to_string());
        }

        Ok(())
    }
}