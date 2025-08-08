use log::{error, info, warn, debug};
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};
use tokio::fs;
use tokio::time::{sleep, interval, Duration};
use tokio::sync::mpsc;
use serde_json::json;

use crate::config::{Config, DeviceConfig}; // Add DeviceConfig import
use crate::modbus::ModbusClient;
use crate::devices::{Device, DeviceData, FlowmeterDevice, RpmDevice}; // Add RpmDevice
use crate::output::{DataFormatter, DataSender, ConsoleFormatter, ConsoleSender};
use crate::output::raw_sender::{RawDataSender, RawDataFormat};
use crate::services::DatabaseService;
use crate::utils::error::ModbusError;
use tokio::sync::Mutex as TokioMutex;
use crate::devices::gps::{GpsService, GpsData};

pub struct DataService {
    config: Config,
    devices: Vec<Box<dyn Device>>,
    device_data: Arc<Mutex<HashMap<String, Box<dyn DeviceData>>>>,
    device_address_to_uuid: HashMap<u8, String>,
    modbus_client: Arc<ModbusClient>,
    formatter: Box<dyn DataFormatter>,
    senders: Vec<Box<dyn DataSender>>,
    database_service: Option<DatabaseService>,
    polling_handle: Arc<TokioMutex<Option<tokio::task::JoinHandle<()>>>>,
    // NEW: GPS service
    gps_service: Option<GpsService>,
}


impl Clone for DataService {
    fn clone(&self) -> Self {
        // Create new devices vector by recreating them from config
        let mut cloned_devices: Vec<Box<dyn Device>> = Vec::new();
        
        // Only clone devices if we have the config available
        for device_config in &self.config.devices {
            if device_config.enabled {
                let device = FlowmeterDevice::new(
                    device_config.address,
                    device_config.name.clone(),
                );
                cloned_devices.push(Box::new(device));
            }
        }

        Self {
            config: self.config.clone(),
            devices: cloned_devices, // Use properly cloned devices
            device_data: self.device_data.clone(),
            device_address_to_uuid: self.device_address_to_uuid.clone(),
            modbus_client: self.modbus_client.clone(),
            formatter: Box::new(ConsoleFormatter),
            senders: Vec::new(),
            database_service: self.database_service.clone(),
            polling_handle: self.polling_handle.clone(),
            gps_service: self.gps_service.clone(), // NEW field
        }
    }
}

impl DataService {
    pub async fn new(config: Config) -> Result<Self, ModbusError> {
        info!("🚀 Initializing Data Service");
        info!("🏭 IPC: {} [{}]", config.get_ipc_name(), config.get_ipc_uuid());
        
        // Create modbus client
        let modbus_client = Arc::new(ModbusClient::new(&config.serial_port, config.baud_rate, &config.parity)?);
        
        // Create devices from config
        let mut devices: Vec<Box<dyn Device>> = Vec::new();
        let mut device_address_to_uuid = HashMap::new();

        // Initialize devices
        for device_config in &config.devices {
            if device_config.enabled {
                match device_config.device_type.to_lowercase().as_str() {
                    "flowmeter" => {
                        let device = FlowmeterDevice::new(
                            device_config.address, 
                            device_config.name.clone(),
                        );
                        devices.push(Box::new(device));
                    }
                    "rpm" => {
                        let (total_channels, rpm_thresholds) = config.get_rpm_channel_config(device_config.address);
                        
                        let rpm_device = RpmDevice::with_config(
                            device_config.address,
                            device_config.name.clone(),
                            device_config.location.clone(),
                            total_channels,
                            rpm_thresholds,
                        );
                        
                        devices.push(Box::new(rpm_device));
                        
                        info!("📋 Configured multi-channel RPM device with {} channels at address {}", 
                              total_channels, device_config.address);
                    }

                    _ => {
                        warn!("⚠️ Unknown device type: {}", device_config.device_type);
                        continue;
                    }
                }
                
                device_address_to_uuid.insert(device_config.address, device_config.uuid.clone());
                
                info!("📋 Registered {} device '{}' at address {} with UUID: {}", 
                      device_config.device_type,
                      device_config.name, 
                      device_config.address,
                      device_config.uuid);
            }
        }

        // Initialize and start database service
        let database_service = if config.output.database_output.as_ref().map(|db| db.enabled).unwrap_or(false) {
            match DatabaseService::new(config.clone()).await {
                Ok(db_service) => {
                    info!("💾 Database service initialized successfully");
                    Some(db_service)
                }
                Err(e) => {
                    error!("❌ Failed to initialize database service: {}", e);
                    None
                }
            }
        } else {
            info!("📝 Database service disabled");
            None
        };

        // Create formatter
        let formatter: Box<dyn DataFormatter> = Box::new(ConsoleFormatter);

        // Initialize senders
        let mut senders: Vec<Box<dyn DataSender>> = Vec::new();
        senders.push(Box::new(ConsoleSender));

        // NEW: Initialize GPS service if enabled
        let gps_service = if config.gps.enabled {
            info!("🧭 Initializing GPS service on port {}", config.gps.port);
            let gps_service = GpsService::new(
                config.gps.port.clone(),
                config.gps.baud_rate,
            );
            
            // Auto-start if configured
            if config.gps.auto_start {
                if let Err(e) = gps_service.start().await {
                    warn!("⚠️ Failed to auto-start GPS service: {}", e);
                } else {
                    info!("🧭 GPS service auto-started");
                }
            }
            
            Some(gps_service)
        } else {
            info!("📝 GPS service disabled in config");
            None
        };

        // Create the DataService instance with new fields
        let data_service = Self {
            config,
            devices,
            device_data: Arc::new(Mutex::new(HashMap::new())),
            device_address_to_uuid,
            modbus_client,
            formatter,
            senders,
            database_service,
           
            polling_handle: Arc::new(TokioMutex::new(None)),
            gps_service,
        };

        Ok(data_service)
    }

    //  Helper method to get device config by address
    fn get_device_config_by_address(&self, address: u8) -> Option<&crate::config::DeviceConfig> {
        self.config.devices.iter().find(|d| d.address == address)
    }

    //  Database storage method
    async fn store_device_data_to_database(
        &self,
        device_address: u8,
        device_data: &dyn DeviceData,
    ) -> Result<(), ModbusError> {
        if let Some(db_service) = &self.database_service {
            if let Some(uuid) = self.get_uuid_from_address(device_address) {
                if let Some(_device_config) = self.get_device_config_by_address(device_address) {
                    db_service.store_device_data(
                        uuid,           
                        device_address,
                        device_data,
                    ).await?;
                    
                    info!("💾 Stored data for device {} to database", device_address);
                } else {
                    warn!("⚠️  Device config not found for address: {}", device_address);
                }
            } else {
                warn!("⚠️  UUID not found for device address: {}", device_address);
            }
        }
        Ok(())
    }

    // Helper method
    fn get_uuid_from_address(&self, address: u8) -> Option<&String> {
        self.device_address_to_uuid.get(&address)
    }

    //  Main continuous monitoring method
    pub async fn run(&self, debug_output: bool) -> Result<(), ModbusError> {
        info!("🚀 Starting service in endpoint-driven mode");
        info!("⚙️  Debug output: {}", if debug_output { "enabled" } else { "disabled" });
        
        // Database status
        if let Some(_) = &self.database_service {
            info!("💾 Database storage: ENABLED");
        } else {
            info!("📝 Database storage: DISABLED");
        }


        // Initial reading to verify devices are working (one-time only)
        info!("🔍 Performing initial device check...");
        self.read_all_devices_once().await?;
        
        info!("✅ Service started successfully");
        info!("⏱️  Update interval: {} seconds", self.config.update_interval_seconds);
        info!("⏳ Service ready. Waiting for WebSocket clients to start streaming...");
        
        // Keep the service running but don't poll automatically
        loop {
            tokio::time::sleep(tokio::time::Duration::from_secs(30)).await;
            
        }
    }


    //  Fixed read_all_devices_once method
    pub async fn read_all_devices_once(&self) -> Result<(), ModbusError> {
        info!("📖 Reading data from all devices once...");
        
        let mut all_data = Vec::new();
        
        for device in &self.devices {
            match device.read_data(self.modbus_client.as_ref()).await {
                Ok(data) => {
                    let device_address = device.address();
                    
                    //  Clone the UUID to avoid borrowing issues
                    let device_uuid = self.get_uuid_from_address(device_address)
                        .cloned()
                        .unwrap_or_else(|| "unknown".to_string());
                    
                    //  Store in database if database service is available
                    if let Err(e) = self.store_device_data_to_database(device_address, data.as_ref()).await {
                        error!("Failed to store device data: {}", e);
                    }
                    
                    // Store in memory for immediate access
                    {
                        let mut device_data = self.device_data.lock().unwrap();
                        device_data.insert(device_uuid, data.clone_box());
                    }
                    all_data.push(data);
                }
                Err(e) => {
                    error!("Failed to read from device {}: {}", device.address(), e);
                }
            }
        }
        
        // Send to output formatters
        if !all_data.is_empty() {
            let formatted_data = all_data.iter()
                .map(|data| {
                    let device = self.devices.iter()
                        .find(|d| self.device_address_to_uuid.get(&d.address()).is_some())
                        .unwrap_or(&self.devices[0]);
                    self.formatter.format_single_device(device.address(), data.as_ref())
                })
                .collect::<Vec<_>>()
                .join("\n");
            
            for sender in &self.senders {
                if let Err(e) = sender.send(&formatted_data).await {
                    error!("Failed to send data: {}", e);
                }
            }
        }
        
        Ok(())
    }

    pub async fn read_raw_device_data(&self, device_addr: u8, format: &str, output_file: Option<&String>) -> Result<(), ModbusError> {
        for device in &self.devices {
            if device.address() == device_addr {
                if let Some(flowmeter) = device.as_any().downcast_ref::<FlowmeterDevice>() {
                    let raw_payload = flowmeter.read_raw_payload(self.modbus_client.as_ref()).await?;
                    
                    // Print to console
                    println!("🔍 Raw Data for Device {}:", device_addr);
                    println!("{}", raw_payload.debug_info());
                    println!("📊 Payload Size: {} bytes", raw_payload.payload_size);
                    println!("🌐 Online Decoder: {}", raw_payload.get_decoder_url());
                    
                    // Save to file if requested
                    if let Some(file_path) = output_file {
                        let raw_format = match format {
                            "hex" => RawDataFormat::Hex,
                            "binary" => RawDataFormat::Binary,
                            "json" => RawDataFormat::Json,
                            _ => RawDataFormat::Debug,
                        };
                        
                        let sender = RawDataSender::new(file_path, raw_format, true);
                        sender.send_raw_payload(&raw_payload).await?;
                        
                        info!("💾 Raw data saved to: {}", file_path);
                    }
                    
                    return Ok(());
                }
            }
        }
        
        Err(ModbusError::DeviceNotFound(format!("Device {} not found or not a flowmeter", device_addr)))
    }

    pub async fn reset_accumulation(&self, device_addr: u8) -> Result<(), ModbusError> {
        if let Some(device) = self.devices.iter().find(|d| d.address() == device_addr) {
            device.reset_accumulation(self.modbus_client.as_ref()).await
        } else {
            Err(ModbusError::InvalidDevice(device_addr))
        }
    }


    // Get RPM devices configuration
    pub fn get_rpm_devices(&self) -> Vec<&DeviceConfig> {
        self.config.devices.iter()
            .filter(|d| d.enabled && d.device_type == "rpm")
            .collect()
    }

    // Get flowmeter devices configuration  
    pub fn get_flowmeter_devices(&self) -> Vec<&DeviceConfig> {
        self.config.devices.iter()
            .filter(|d| d.enabled && d.device_type == "flowmeter")
            .collect()
    }

    // Get device data by address as JSON string
    pub async fn get_device_data_by_address(&self, device_address: u8) -> Option<String> {
        if let Ok(device_data) = self.device_data.lock() {
            if let Some(uuid) = self.get_uuid_from_address(device_address) {
                if let Some(data) = device_data.get(uuid) {
                    return Some(data.to_json().to_string());
                }
            }
        }
        None
    }

    // Get channel-specific RPM data (single implementation)
    pub async fn get_rpm_channel_data(&self, device_address: u8, channel_id: u8) -> Option<String> {
        if let Some(data_str) = self.get_device_data_by_address(device_address).await {
            if let Ok(json_data) = serde_json::from_str::<serde_json::Value>(&data_str) {
                if let Some(channels) = json_data["channels"].as_array() {
                    for channel in channels {
                        if channel["channel_id"].as_u64() == Some(channel_id as u64) {
                            return Some(channel.to_string());
                        }
                    }
                }
            }
        }
        None
    }

    // Reset engine duration for a specific address
    pub async fn reset_engine_duration(&self, address: u8) -> Result<(), ModbusError> {
        Ok(())
    }

    //  CLI interface methods
    pub fn set_formatter(&mut self, formatter: Box<dyn DataFormatter>) {
        self.formatter = formatter;
        info!("🎨 Output formatter changed to: {}", self.formatter.formatter_type());
    }

    pub fn add_sender(&mut self, sender: Box<dyn DataSender>) {
        info!("📡 Adding output sender: {}", sender.sender_type());
        self.senders.push(sender);
    }

    pub fn get_database_service(&self) -> Option<&DatabaseService> {
        self.database_service.as_ref()
    }

    pub fn get_database_service_mut(&mut self) -> Option<&mut DatabaseService> {
        self.database_service.as_mut()
    }

    pub async fn check_database_health(&self) -> Result<Option<bool>, ModbusError> {
        if let Some(db_service) = &self.database_service {
            match db_service.get_flowmeter_stats().await {
                Ok(_) => Ok(Some(true)),
                Err(_) => Ok(Some(false)),
            }
        } else {
            Ok(None)
        }
    }

    // MINIMAL: Updated query method
    pub async fn query_flowmeter_data(&self, device_address: u8, limit: i64) -> Result<(), ModbusError> {
        if let Some(db_service) = &self.database_service {
            let readings = db_service.get_device_flowmeter_readings(device_address, None, Some(limit)).await?;
            
            println!("📋 Recent flowmeter readings for device {}:", device_address);
            println!("{:<15} {:<15} {:<15} {:<15} {:<8} {:<25}", 
                "Mass Flow", "Temperature", "Density", "Vol Flow", "Error", "Unix Timestamp");
            println!("{}", "-".repeat(110));
            
            for reading in readings {
                println!("{:<15.2} {:<15.2} {:<15.4} {:<15.3} {:<8} {:<25}", 
                    reading.mass_flow_rate,
                    reading.temperature,
                    reading.density_flow,
                    reading.volume_flow_rate,
                    reading.error_code,
                    reading.unix_timestamp
                );
            }
        } else {
            println!("❌ Database service not enabled");
        }
        Ok(())
    }

    // MINIMAL: Updated stats method
    pub async fn get_flowmeter_stats(&self) -> Result<(), ModbusError> {
        if let Some(db_service) = &self.database_service {
            let stats = db_service.get_flowmeter_stats().await?;
            
            println!("📊 Flowmeter Statistics:");
            println!("Total Readings: {}", stats.total_readings);
            if let Some(avg_flow) = stats.avg_mass_flow_rate {
                println!("Average Mass Flow Rate: {:.2}", avg_flow);
            }
            if let Some(max_flow) = stats.max_mass_flow_rate {
                println!("Maximum Mass Flow Rate: {:.2}", max_flow);
            }
            if let Some(min_flow) = stats.min_mass_flow_rate {
                println!("Minimum Mass Flow Rate: {:.2}", min_flow);
            }
            if let Some(avg_temp) = stats.avg_temperature {
                println!("Average Temperature: {:.2}", avg_temp);
            }
            if let Some(latest) = stats.latest_timestamp {
                println!("Latest Reading: {}", latest);
            }
        } else {
            println!("❌ Database service not enabled");
        }
        Ok(())
    }

    // Helper method to get device address from UUID (if still needed)
    fn get_address_from_uuid(&self, uuid: &str) -> Option<u8> {
        for (address, device_uuid) in &self.device_address_to_uuid {
            if device_uuid == uuid {
                return Some(*address);
            }
        }
        None
    }

    // NEW: GPS control methods
    pub async fn get_current_gps_data(&self) -> Option<GpsData> {
        if let Some(gps_service) = &self.gps_service {
            // Try to get fresh GPS fix
            match gps_service.get_current_gps_fix().await {
                Ok(Some(data)) => Some(data),
                Ok(None) => {
                    // Fallback to last known data
                    let last_data = gps_service.get_current_data().await;
                    if last_data.has_valid_fix() {
                        Some(last_data)
                    } else {
                        None
                    }
                }
                Err(_) => None,
            }
        } else {
            None
        }
    }
    
    // Remove the continuous GPS monitoring methods or make them no-op
    pub async fn start_gps_service(&self) -> Result<(), ModbusError> {
        if let Some(gps_service) = &self.gps_service {
            gps_service.start().await?;
            info!("🧭 GPS service ready for on-demand requests");
            Ok(())
        } else {
            Err(ModbusError::ServiceNotAvailable("GPS service not enabled in config".to_string()))
        }
    }
    
    pub async fn stop_gps_service(&self) -> Result<(), ModbusError> {
        if let Some(gps_service) = &self.gps_service {
            gps_service.stop().await?;
            info!("🧭 GPS service stopped");
            Ok(())
        } else {
            Err(ModbusError::ServiceNotAvailable("GPS service not enabled in config".to_string()))
        }
    }
    
    pub async fn get_gps_status(&self) -> Result<String, ModbusError> {
        if let Some(gps_service) = &self.gps_service {
            Ok(gps_service.get_status().await)
        } else {
            Ok("GPS not available".to_string())
        }
    }

    // Add method to access API service if available
    pub fn get_api_service(&self) -> Option<&crate::services::api_service::ApiService> {
        // This would need to be implemented if you want direct access
        // For now, we'll use HTTP requests to the API
        None
    }
}