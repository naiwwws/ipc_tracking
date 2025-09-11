use log::{error, info, warn, debug};
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};
use tokio::fs;
use tokio::time::{sleep, interval, Duration};
use tokio::sync::mpsc;
use serde_json::json;
use tokio::signal;

use crate::config::{Config, DeviceConfig}; // Add DeviceConfig import
use crate::modbus::ModbusClient;
use crate::devices::{Device, DeviceData, FlowmeterDevice, RpmDevice}; // Add RpmDevice
use crate::output::{DataFormatter, DataSender, ConsoleFormatter, ConsoleSender};
use crate::output::raw_sender::{RawDataSender, RawDataFormat};
#[cfg(feature = "sqlite")]
use crate::services::DatabaseService;
#[cfg(feature = "sqlite")]
use crate::services::MtwsService;
use crate::utils::error::ModbusError;
use tokio::sync::Mutex as TokioMutex;
use crate::devices::gps::{GpsService, GpsData};

pub struct DataService {
    config: Config,
    devices: Vec<Box<dyn Device>>,
    device_data: Arc<Mutex<HashMap<String, Box<dyn DeviceData>>>>,
    device_data_by_address: Arc<Mutex<HashMap<u8, Box<dyn DeviceData>>>>, // Add this field
    device_address_to_uuid: HashMap<u8, String>,
    modbus_client: Arc<ModbusClient>,
    formatter: Box<dyn DataFormatter>,
    senders: Vec<Box<dyn DataSender>>,
    #[cfg(feature = "sqlite")]
    database_service: Option<DatabaseService>,
    polling_handle: Arc<TokioMutex<Option<tokio::task::JoinHandle<()>>>>,
    // NEW: GPS service
    gps_service: Option<GpsService>,
    // NEW: MTWS service
    #[cfg(feature = "sqlite")]
    mtws_service: Option<MtwsService>,
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
            devices: cloned_devices,
            device_data: self.device_data.clone(),
            device_data_by_address: self.device_data_by_address.clone(),
            device_address_to_uuid: self.device_address_to_uuid.clone(),
            modbus_client: self.modbus_client.clone(),
            formatter: Box::new(ConsoleFormatter),
            senders: Vec::new(),
            #[cfg(feature = "sqlite")]
            database_service: self.database_service.clone(),
            polling_handle: self.polling_handle.clone(),
            gps_service: self.gps_service.clone(),
            #[cfg(feature = "sqlite")]
            mtws_service: None, // Don't clone MTWS service to avoid circular references
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
        #[cfg(feature = "sqlite")]
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
        
        #[cfg(not(feature = "sqlite"))]
        let _database_service = ();

        // Create formatter
        let formatter: Box<dyn DataFormatter> = Box::new(ConsoleFormatter);

        // Initialize senders
        let mut senders: Vec<Box<dyn DataSender>> = Vec::new();
        senders.push(Box::new(ConsoleSender));

        // Initialize GPS service if enabled WITH IMMEDIATE START
        let gps_service = if config.gps.enabled {
            info!("🧭 Initializing GPS service on port {}", config.gps.port);
            let gps_service = GpsService::new(
                config.gps.port.clone(),
                config.gps.baud_rate,
            );
            
            // ALWAYS auto-start GPS for continuous reading
            info!("🧭 Starting GPS service with continuous reading...");
            if let Err(e) = gps_service.start().await {
                warn!("⚠️ Failed to auto-start GPS service: {}", e);
            } else {
                info!("✅ GPS service started with continuous reading");
                
                // Give GPS time to initialize and get first fix
                tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
                
                // Check initial status
                let status = gps_service.get_status().await;
                info!("🧭 GPS service status: {}", status);
            }
            
            Some(gps_service)
        } else {
            info!("📝 GPS service disabled in config");
            None
        };

        // Create the DataService instance first
        let mut data_service = Self {
            config: config.clone(),
            devices,
            device_data: Arc::new(Mutex::new(HashMap::new())),
            device_data_by_address: Arc::new(Mutex::new(HashMap::new())),
            device_address_to_uuid,
            modbus_client,
            formatter,
            senders,
            #[cfg(feature = "sqlite")]
            database_service,
            polling_handle: Arc::new(TokioMutex::new(None)),
            gps_service,
            #[cfg(feature = "sqlite")]
            mtws_service: None, // Initialize as None first
        };

        // Now initialize MTWS service if enabled
        #[cfg(feature = "sqlite")]
        info!("Mtws Config - Data Service: {:?}", config.mtws);
        if config.mtws.enabled {
            if let Err(e) = data_service.initialize_mtws_service() {
                warn!("⚠️ Failed to initialize MTWS service: {}", e);
            } else {
                info!("🛰️ MTWS service initialized successfully");
            }
        }

        Ok(data_service)
    }

    #[cfg(feature = "sqlite")]
    pub fn initialize_mtws_service(&mut self) -> Result<(), ModbusError> {
        if !self.config.mtws.enabled {
            self.mtws_service = None;
            return Ok(());
        }

        info!("🛰️ Initializing MTWS service...");
        
        // Create MTWS service with proper Arc<DataService> reference
        let data_service_arc = Arc::new(self.clone());
        
        let mtws_service = crate::services::mtws_service::MtwsService::new(
            data_service_arc,
            self.config.clone()
        );
        
        self.mtws_service = Some(mtws_service);
        info!("✅ MTWS service initialized");
        
        Ok(())
    }

    // Helper method to get device config by address
    fn get_device_config_by_address(&self, address: u8) -> Option<&crate::config::DeviceConfig> {
        self.config.devices.iter().find(|d| d.address == address)
    }

    // Database storage method
    #[cfg(feature = "sqlite")]
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

    // SINGLE run method with all enhancements
    pub async fn run(&mut self, debug_output: bool) -> Result<(), ModbusError> {
        info!("🚀 Starting service in endpoint-driven mode");
        info!("⚙️  Debug output: {}", if debug_output { "enabled" } else { "disabled" });
        
        // Database status
        #[cfg(feature = "sqlite")]
        if let Some(_) = &self.database_service {
            info!("💾 Database storage: ENABLED");
        } else {
            info!("📝 Database storage: DISABLED");
        }
        #[cfg(not(feature = "sqlite"))]
        info!("📝 Database storage: DISABLED (sqlite feature not enabled)");

        // Start GPS service with continuous reading if enabled
        if self.gps_service.is_some() {
            match self.ensure_gps_service_running().await {
                Ok(_) => {
                    info!("🧭 GPS service started with continuous reading");
                    // Wait a bit more for GPS to stabilize
                    tokio::time::sleep(tokio::time::Duration::from_secs(3)).await;
                    
                    // Check initial GPS status
                    if let Ok(status) = self.get_gps_status().await {
                        info!("🧭 GPS Initial Status: {}", status);
                    }
                }
                Err(e) => warn!("⚠️ GPS service issue: {}", e),
            }
        }

        // Auto-start MTWS service if enabled and configured for auto-start
        #[cfg(feature = "sqlite")]
        if let Some(mtws_service) = &self.mtws_service {
            if self.config.mtws.auto_start {
                info!("🛰️ Auto-starting MTWS transmission service");
                match mtws_service.start_transmission().await {
                    Ok(_) => {
                        info!("✅ MTWS transmission service started automatically");
                        info!("📡 Endpoint: {}", self.config.get_mtws_endpoint_url());
                        info!("⏱️ Interval: {} seconds", self.config.mtws.transmission_interval_seconds);
                    }
                    Err(e) => {
                        error!("❌ Failed to auto-start MTWS transmission: {}", e);
                    }
                }
            } else {
                info!("🛰️ MTWS service available but auto-start disabled");
            }
        }

        // Initial reading to verify devices are working (one-time only)
        info!("🔍 Performing initial device check...");
        self.read_all_devices_once().await?;
        
        info!("✅ Service started successfully");
        info!("⏱️  Update interval: {} seconds", self.config.update_interval_seconds);
        info!("🛑 Press Ctrl+C to stop the service");
        
        // Keep the service running with signal handling and GPS monitoring
        let mut gps_check_counter = 0;
        let update_interval = tokio::time::Duration::from_secs(self.config.update_interval_seconds);
        
        loop {
            tokio::select! {
                // Handle Ctrl+C signal
                _ = signal::ctrl_c() => {
                    info!("🛑 Received Ctrl+C signal, shutting down gracefully...");
                    
                    // Stop MTWS service if running
                    #[cfg(feature = "sqlite")]
                    if let Some(mtws_service) = &self.mtws_service {
                        if let Err(e) = mtws_service.stop_transmission().await {
                            warn!("⚠️ Failed to stop MTWS service: {}", e);
                        } else {
                            info!("🛰️ MTWS service stopped");
                        }
                    }
                    
                    // Stop GPS service if running
                    if let Err(e) = self.stop_gps_service().await {
                        warn!("⚠️ Failed to stop GPS service: {}", e);
                    } else {
                        info!("🧭 GPS service stopped");
                    }
                    
                    info!("✅ Service shutdown completed");
                    break;
                }
                
                // Regular device reading cycle
                _ = tokio::time::sleep(update_interval) => {
                    // Periodic device reading for MTWS and other services
                    if let Err(e) = self.read_all_devices_once().await {
                        error!("❌ Failed to read devices: {}", e);
                    }
                    
                    // Check GPS health more frequently (every 3 cycles instead of 10)
                    gps_check_counter += 1;
                    if gps_check_counter >= 3 {
                        gps_check_counter = 0;
                        
                        if let Some(gps_service) = &self.gps_service {
                            // Check if GPS service is still running
                            if !gps_service.is_running().await {
                                warn!("⚠️ GPS service stopped running, restarting...");
                                if let Err(e) = self.ensure_gps_service_running().await {
                                    error!("❌ Failed to restart GPS service: {}", e);
                                }
                            } else {
                                // Periodic status check
                                if let Ok(status) = self.get_gps_status().await {
                                    debug!("🧭 GPS Status Check: {}", status);
                                }
                            }
                        }
                    }
                }
            }
        }
        
        Ok(())
    }

    // Fixed read_all_devices_once method
    pub async fn read_all_devices_once(&mut self) -> Result<(), ModbusError> {
        info!("🔄 Starting sequential device reading cycle...");
        
        let config = self.config.clone();
        let enabled_devices = config.get_enabled_devices();
        
        info!("📋 Found {} enabled devices to read", enabled_devices.len());
        
        // Read devices SEQUENTIALLY
        for device_config in enabled_devices {
            match device_config.device_type.as_str() {
                "flowmeter" => {
                    info!("🌊 Reading flowmeter device {} at address {}", device_config.name, device_config.address);
                    
                    let device = FlowmeterDevice::new(device_config.address, device_config.name.clone());
                    
                    match device.read_data(self.modbus_client.as_ref()).await {
                        Ok(device_data) => {
                            self.store_device_data(&device_config.uuid, device_data).await?;
                            info!("✅ Successfully read and stored flowmeter data from device {}", device_config.address);
                        }
                        Err(e) => {
                            error!("❌ Failed to read flowmeter {}: {}", device_config.address, e);
                        }
                    }
                    
                    // Add delay between devices for RS485
                    tokio::time::sleep(Duration::from_millis(500)).await;
                }
                "rpm" => {
                    info!("⚙️ Reading RPM device {} at address {}", device_config.name, device_config.address);

                    // Get RPM configuration from config
                    let (total_channels, thresholds) = self.config.get_rpm_channel_config(device_config.address);

                    let device = RpmDevice::new(
                        device_config.address,
                        device_config.name.clone(),
                    );

                    match device.read_data(self.modbus_client.as_ref()).await {
                        Ok(device_data) => {
                            self.store_device_data(&device_config.uuid, device_data).await?;
                            info!("✅ Successfully read and stored RPM data from device {}", device_config.address);
                        }
                        Err(e) => {
                            error!("❌ Failed to read RPM {}: {}", device_config.address, e);
                        }
                    }

                    // Add delay between devices for RS485
                    tokio::time::sleep(Duration::from_millis(500)).await;
                }
                "gps" => {
                    info!("🧭 Reading GPS device {} at address {}", device_config.name, device_config.address);
                    // GPS reading logic would go here
                }
                _ => {
                    warn!("⚠️ Unknown device type: {}", device_config.device_type);
                }
            }
        }
        
        info!("✅ Device reading cycle completed");
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

    // Device data retrieval method
    pub async fn get_device_data_by_address(&self, device_address: u8) -> Option<String> {
        if let Ok(address_map) = self.device_data_by_address.lock() {
            if let Some(device_data) = address_map.get(&device_address) {
                let json_value = device_data.to_json();
                let json_string = serde_json::to_string(&json_value).ok()?;
                info!("📊 Retrieved device data for address {}: length data {}", device_address, json_string.len());
                return Some(json_string);
            }
        }
        
        warn!("⚠️ No device data found for address {}", device_address);
        None
    }

    // Method to get current flowmeter data directly
    pub async fn get_current_flowmeter_data(&self, device_address: u8) -> Option<crate::devices::flowmeter::FlowmeterData> {
        if let Ok(address_map) = self.device_data_by_address.lock() {
            if let Some(device_data) = address_map.get(&device_address) {
                if let Some(flowmeter_data) = device_data.as_any().downcast_ref::<crate::devices::flowmeter::FlowmeterData>() {
                    return Some(flowmeter_data.clone());
                }
            }
        }
        None
    }

    // Engine duration tracking
    pub async fn get_engine_durations(&self) -> HashMap<u8, i32> {
        // TODO: Implement database storage and retrieval for engine durations
        HashMap::new()
    }

    pub async fn reset_engine_duration(&self, engine_address: u8) -> Result<(), ModbusError> {
        // TODO: Implement engine duration reset in database
        info!("🔄 Engine duration reset requested for address: {}", engine_address);
        Ok(())
    }

    // CLI interface methods
    pub fn set_formatter(&mut self, formatter: Box<dyn DataFormatter>) {
        self.formatter = formatter;
        info!("🎨 Output formatter changed to: {}", self.formatter.formatter_type());
    }

    pub fn add_sender(&mut self, sender: Box<dyn DataSender>) {
        info!("📡 Adding output sender: {}", sender.sender_type());
        self.senders.push(sender);
    }

    #[cfg(feature = "sqlite")]
    pub fn get_database_service(&self) -> Option<&DatabaseService> {
        self.database_service.as_ref()
    }

    #[cfg(feature = "sqlite")]
    pub fn get_database_service_mut(&mut self) -> Option<&mut DatabaseService> {
        self.database_service.as_mut()
    }

    #[cfg(feature = "sqlite")]
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

    // Query method
    #[cfg(feature = "sqlite")]
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

    // Stats method
    #[cfg(feature = "sqlite")]
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

    // Helper method to get device address from UUID
    fn get_address_from_uuid(&self, uuid: &str) -> Option<u8> {
        for (address, device_uuid) in &self.device_address_to_uuid {
            if device_uuid == uuid {
                return Some(*address);
            }
        }
        None
    }

    // SINGLE get_current_gps_data method with enhanced error handling
    pub async fn get_current_gps_data(&self) -> Option<GpsData> {
        if let Some(gps_service) = &self.gps_service {
            // Ensure GPS service is running
            if let Err(e) = self.ensure_gps_service_running().await {
                warn!("⚠️ Failed to ensure GPS service running: {}", e);
                return None;
            }

            // Try to get current GPS data (from continuous reading)
            match gps_service.get_current_gps_fix().await {
                Ok(Some(data)) => {
                    debug!("🧭 Got GPS data from continuous reading: lat={:?}, lon={:?}, age={}s", 
                          data.latitude, data.longitude, 
                          chrono::Utc::now().timestamp() - data.timestamp.unwrap_or(0));
                    return Some(data);
                }
                Ok(None) => {
                    debug!("🧭 No current GPS fix, trying force read...");
                    
                    // Try force read as fallback
                    match gps_service.force_read().await {
                        Ok(Some(data)) => {
                            info!("🧭 Got GPS data from force read: lat={:?}, lon={:?}", 
                                  data.latitude, data.longitude);
                            return Some(data);
                        }
                        Ok(None) => {
                            warn!("⚠️ Force read returned no GPS data");
                        }
                        Err(e) => {
                            warn!("⚠️ Force read failed: {}", e);
                        }
                    }
                }
                Err(e) => {
                    warn!("⚠️ Failed to get GPS data: {}", e);
                }
            }
        } else {
            warn!("⚠️ GPS service not initialized");
        }
        None
    }

    // Ensure GPS service is running without reinitializing
    pub async fn ensure_gps_service_running(&self) -> Result<(), ModbusError> {
        if let Some(gps_service) = &self.gps_service {
            let is_running = gps_service.is_running().await;
            
            if !is_running {
                info!("🧭 GPS service not running, starting continuous GPS reading...");
                match gps_service.start().await {
                    Ok(_) => {
                        info!("✅ GPS service started with continuous reading");
                        // Give GPS service time to establish connection and get first fix
                        tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
                    }
                    Err(e) => {
                        warn!("⚠️ Failed to start GPS service: {}", e);
                        return Err(ModbusError::CommunicationError(format!("GPS start failed: {}", e)));
                    }
                }
            } else {
                let status = gps_service.get_status().await;
                debug!("🧭 GPS service running: {}", status);
            }
            
            Ok(())
        } else {
            Err(ModbusError::ServiceNotAvailable("GPS service not enabled".to_string()))
        }
    }

    // SINGLE get_gps_status method with enhanced status
    pub async fn get_gps_status(&self) -> Result<String, ModbusError> {
        if let Some(gps_service) = &self.gps_service {
            let status = gps_service.get_status().await;
            let current_data = gps_service.get_current_data().await;
            let is_running = gps_service.is_running().await;
            
            let detailed_status = format!(
                "GPS Service: {} | Running: {} | Valid Fix: {} | Coordinates: ({:.6}, {:.6}) | Satellites: {} | Data Age: {}s",
                status,
                is_running,
                current_data.has_valid_fix(),
                current_data.latitude.unwrap_or(0.0),
                current_data.longitude.unwrap_or(0.0),
                current_data.satellites.unwrap_or(0),
                chrono::Utc::now().timestamp() - current_data.timestamp.unwrap_or(0)
            );
            
            Ok(detailed_status)
        } else {
            Ok("GPS service not available".to_string())
        }
    }

    // Method to manually refresh GPS data
    pub async fn refresh_gps_data(&self) -> Result<Option<GpsData>, ModbusError> {
        if let Some(gps_service) = &self.gps_service {
            self.ensure_gps_service_running().await?;
            
            match gps_service.get_current_gps_fix().await {
                Ok(data) => Ok(data),
                Err(e) => {
                    warn!("⚠️ Failed to refresh GPS data: {}", e);
                    Ok(None)
                }
            }
        } else {
            Err(ModbusError::ServiceNotAvailable("GPS service not enabled".to_string()))
        }
    }

    // GPS control methods
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

    // API service access
    #[cfg(feature = "api")]
    pub fn get_api_service(&self) -> Option<&crate::services::api_service::ApiService> {
        None
    }

    // Store device data method
    pub async fn store_device_data(&mut self, device_uuid: &str, device_data: Box<dyn DeviceData>) -> Result<(), ModbusError> {
        let device_address = device_data.device_address();
        
        // Store in in-memory cache for MTWS access
        if let Ok(mut data_map) = self.device_data.lock() {
            data_map.insert(device_uuid.to_string(), device_data.clone_box());
            info!("📊 Stored device data for UUID: {} (address: {})", device_uuid, device_address);
        }
        
        // Also store by address for quick lookup
        if let Ok(mut address_map) = self.device_data_by_address.lock() {
            address_map.insert(device_address, device_data.clone_box());
            info!("📊 Stored device data for address: {}", device_address);
        }

        // Store to database
        #[cfg(feature = "sqlite")]
        self.store_device_data_to_database(device_address, device_data.as_ref()).await?;

        Ok(())
    }

    // Get RPM channel data
    pub async fn get_rpm_channel_data(&self, device_address: u8, channel_id: u8) -> Option<String> {
        if let Ok(address_map) = self.device_data_by_address.lock() {
            if let Some(device_data) = address_map.get(&device_address) {
                if let Some(rpm_data) = device_data.as_any().downcast_ref::<crate::devices::rpm::RpmData>() {
                    if let Some(channel) = rpm_data.channels.iter().find(|ch| ch.channel_id == channel_id) {
                        let channel_json = serde_json::json!({
                            "channel_id": channel.channel_id,
                            "rpm_value": channel.rpm_value,
                            "freq_value": channel.freq_value,
                            "pulse_config": channel.pulse_config,
                            "rpm_threshold": channel.rpm_threshold,
                            "is_engine_running": channel.is_engine_running,
                            "status": channel.status,
                            "error_code": channel.error_code,
                            "timestamp": rpm_data.timestamp.to_rfc3339()
                        });
                        
                        return serde_json::to_string(&channel_json).ok();
                    }
                }
            }
        }
        None
    }

    // MTWS service access
    #[cfg(feature = "sqlite")]
    pub fn get_mtws_service(&self) -> Option<&MtwsService> {
        self.mtws_service.as_ref()
    }

    #[cfg(feature = "sqlite")]
    pub fn get_mtws_service_mut(&mut self) -> Option<&mut MtwsService> {
        self.mtws_service.as_mut()
    }
    

    
    // Configuration methods
    pub fn get_config(&self) -> &Config {
        &self.config
    }

    pub fn update_config(&mut self, new_config: Config) {
        self.config = new_config;
        info!("📝 Configuration updated");
        
        #[cfg(feature = "sqlite")]
        if let Some(mtws) = &self.mtws_service {
            warn!("🔄 MTWS service restart required for configuration changes");
        }
    }
        
    #[cfg(feature = "sqlite")]
    pub fn has_mtws_service(&self) -> bool {
        self.mtws_service.is_some()
    }
    
    #[cfg(not(feature = "sqlite"))]
    pub fn has_mtws_service(&self) -> bool {
        false
    }

    // Get current device data for MTWS
    pub async fn get_current_device_data(&mut self) -> HashMap<String, String> {
        // Read fresh data from all devices
        if let Err(e) = self.read_all_devices_once().await {
            warn!("⚠️ Failed to read fresh device data: {}", e);
        }
        
        // Return current data as JSON strings
        let mut result = HashMap::new();
        if let Ok(data) = self.device_data.lock() {
            for (uuid, device_data) in data.iter() {
                let json_value = device_data.to_json();
                if let Ok(json_string) = serde_json::to_string(&json_value) {
                    result.insert(uuid.clone(), json_string);
                }
            }
        }
        
        result
    }
}