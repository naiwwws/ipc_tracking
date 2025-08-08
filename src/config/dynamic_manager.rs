use chrono::{DateTime, Utc};
use log::{info, warn, error};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use tokio::sync::{Mutex, broadcast};
use std::sync::Arc;
use std::fmt;

use super::settings::{Config, DeviceConfig, ParityConfig};
use crate::utils::error::ModbusError;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigurationCommand {
    pub command_id: String,
    pub timestamp: DateTime<Utc>,
    pub operator: String,
    pub command_type: ConfigCommandType,
    pub target: ConfigTarget,
    pub parameters: HashMap<String, String>,
    pub apply_immediately: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ConfigCommandType {
    Set,        // Set parameter value
    Add,        // Add new device/output
    Remove,     // Remove device/output
    Enable,     // Enable device/feature
    Disable,    // Disable device/feature
    Reset,      // Reset to default
    Backup,     // Create backup
    Restore,    // Restore from backup
}

// Implement Display for ConfigCommandType
impl fmt::Display for ConfigCommandType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConfigCommandType::Set => write!(f, "SET"),
            ConfigCommandType::Add => write!(f, "ADD"),
            ConfigCommandType::Remove => write!(f, "REMOVE"),
            ConfigCommandType::Enable => write!(f, "ENABLE"),
            ConfigCommandType::Disable => write!(f, "DISABLE"),
            ConfigCommandType::Reset => write!(f, "RESET"),
            ConfigCommandType::Backup => write!(f, "BACKUP"),
            ConfigCommandType::Restore => write!(f, "RESTORE"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ConfigTarget {
    Serial,
    Monitoring,
    Site,
    System,
    Gps,
    Device { address: u8 },
    Output { output_type: String },
}

// Implement Display for ConfigTarget
impl fmt::Display for ConfigTarget {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConfigTarget::Serial => write!(f, "Serial"),
            ConfigTarget::Device { address } => write!(f, "Device[{}]", address),
            ConfigTarget::Monitoring => write!(f, "Monitoring"),
            ConfigTarget::Output { output_type } => write!(f, "Output[{}]", output_type),
            ConfigTarget::Site => write!(f, "Site"),
            ConfigTarget::System => write!(f, "System"),
            ConfigTarget::Gps => write!(f, "GPS"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigurationResponse {
    pub command_id: String,
    pub timestamp: DateTime<Utc>,
    pub success: bool,
    pub message: String,
    pub old_value: Option<String>,
    pub new_value: Option<String>,
    pub requires_restart: bool,
}

pub struct DynamicConfigManager {
    config: Arc<Mutex<Config>>,
    command_history: Arc<Mutex<Vec<ConfigurationCommand>>>, // tokio::sync::Mutex
    change_sender: broadcast::Sender<ConfigurationResponse>,
    current_config: Arc<Mutex<Config>>, // Changed to tokio::sync::Mutex
    config_file_path: String,
    backup_dir: String,
}

impl DynamicConfigManager {
    pub fn new(config_file_path: &str, backup_dir: &str) -> Result<Self, ModbusError> {
        // Create backup directory if it doesn't exist
        std::fs::create_dir_all(backup_dir).map_err(|e| {
            ModbusError::CommunicationError(format!("Failed to create backup directory: {}", e))
        })?;

        // Load existing config or create default
        let config = if Path::new(config_file_path).exists() {
            Config::from_file(config_file_path).unwrap_or_else(|e| {
                warn!("Failed to load config file: {}, using defaults", e);
                Config::default()
            })
        } else {
            Config::default()
        };

        //  Increase channel capacity to prevent closure
        let (change_sender, _receiver) = broadcast::channel(1000);

        // Create shared config
        let shared_config = Arc::new(Mutex::new(config));

        Ok(Self {
            config: shared_config.clone(),
            command_history: Arc::new(Mutex::new(Vec::new())),
            change_sender,
            current_config: shared_config,
            config_file_path: config_file_path.to_string(),
            backup_dir: backup_dir.to_string(),
        })
    }

    //  ADD: Method to get current config for saving
    pub async fn get_current_config(&self) -> Config {
        self.config.lock().await.clone()
    }

    //  Update the broadcast to handle potential channel closure gracefully
    pub async fn execute_command(&self, command: ConfigurationCommand) -> ConfigurationResponse {
        let command_id = command.command_id.clone();
        let timestamp = Utc::now();

        info!("🔧 Executing configuration command: {} by {}", command.command_type, command.operator);

        // Store command in history
        {
            let mut history = self.command_history.lock().await;
            history.push(command.clone());
            if history.len() > 100 {
                history.remove(0);
            }
        }

        let response = match self.process_command(&command).await {
            Ok((success, message, old_value, new_value, requires_restart)) => {
                if success && command.apply_immediately {
                    if let Err(e) = self.save_config().await {
                        error!("Failed to save config after command: {}", e);
                        ConfigurationResponse {
                            command_id,
                            timestamp,
                            success: false,
                            message: format!("Command executed but failed to save: {}", e),
                            old_value,
                            new_value,
                            requires_restart,
                        }
                    } else {
                        ConfigurationResponse {
                            command_id,
                            timestamp,
                            success: true,
                            message,
                            old_value,
                            new_value,
                            requires_restart,
                        }
                    }
                } else {
                    ConfigurationResponse {
                        command_id,
                        timestamp,
                        success,
                        message,
                        old_value,
                        new_value,
                        requires_restart,
                    }
                }
            }
            Err(e) => ConfigurationResponse {
                command_id,
                timestamp,
                success: false,
                message: format!("Command failed: {}", e),
                old_value: None,
                new_value: None,
                requires_restart: false,
            }
        };

        //  Broadcast the response with error handling
        match self.change_sender.send(response.clone()) {
            Ok(_) => info!("📢 Configuration change broadcasted successfully"),
            Err(e) => {
                // Log warning but don't fail the command
                warn!("Failed to broadcast configuration change (no active listeners): {}", e);
            }
        }

        response
    }

    async fn process_command(&self, command: &ConfigurationCommand) -> Result<(bool, String, Option<String>, Option<String>, bool), ModbusError> {
        let mut config = self.current_config.lock().await;
        
        match &command.command_type {
            ConfigCommandType::Set => self.handle_set_command(&mut config, command),
            ConfigCommandType::Add => self.handle_add_command(&mut config, command),
            ConfigCommandType::Remove => self.handle_remove_command(&mut config, command),
            ConfigCommandType::Enable => self.handle_enable_command(&mut config, command),
            ConfigCommandType::Disable => self.handle_disable_command(&mut config, command),
            ConfigCommandType::Reset => self.handle_reset_command(&mut config, command),
            ConfigCommandType::Backup => self.handle_backup_command(&config, command).await,
            ConfigCommandType::Restore => self.handle_restore_command(&mut config, command).await,
        }
    }

    // ═══════════════════════════════════════════════════════════════════
    // CORE LOGIC HANDLERS - ONLY BUSINESS LOGIC, NO CLI INTERACTION
    // ═══════════════════════════════════════════════════════════════════

    fn handle_set_command(&self, config: &mut Config, command: &ConfigurationCommand) -> Result<(bool, String, Option<String>, Option<String>, bool), ModbusError> {
        match &command.target {
            ConfigTarget::System => {
                for (key, value) in &command.parameters {
                    match key.as_str() {
                        "ipc_name" => {
                            let old_value = config.get_ipc_name().to_string();
                            config.set_ipc_name(value.clone());
                            return Ok((true, 
                                     format!("IPC name updated from '{}' to '{}'", old_value, value),
                                     Some(old_value),
                                     Some(value.clone()),
                                     false));
                        }
                        "regenerate_ipc_uuid" => {
                            let old_uuid = config.get_ipc_uuid().to_string();
                            config.regenerate_ipc_uuid();
                            let new_uuid = config.get_ipc_uuid().to_string();
                            return Ok((true,
                                     format!("IPC UUID regenerated: {} -> {}", 
                                            &old_uuid[..8], &new_uuid[..8]),
                                     Some(old_uuid),
                                     Some(new_uuid),
                                     true));
                        }
                        _ => {
                            return Ok((false, 
                                     format!("Unknown system parameter: {}", key), 
                                     None, None, false));
                        }
                    }
                }
            }
            ConfigTarget::Monitoring => {
                for (key, value) in &command.parameters {
                    match key.as_str() {
                        "update_interval_seconds" => {
                            let new_interval: u64 = value.parse().map_err(|_| 
                                ModbusError::InvalidData("Invalid interval value".to_string()))?;
                            
                            if new_interval < 1 || new_interval > 3600 {
                                return Ok((false, 
                                         "Interval must be between 1 and 3600 seconds".to_string(), 
                                         None, None, false));
                            }
                            
                            let old_value = config.update_interval_seconds.to_string();
                            config.update_interval_seconds = new_interval;
                            return Ok((true, 
                                     format!("Polling interval updated from {} to {} seconds", old_value, new_interval),
                                     Some(old_value),
                                     Some(value.clone()),
                                     false));
                        }
                        "max_retries" => {
                            let new_retries: u32 = value.parse().map_err(|_| 
                                ModbusError::InvalidData("Invalid retry count".to_string()))?;
                            
                            let old_value = config.max_retries.to_string();
                            config.max_retries = new_retries;
                            return Ok((true, 
                                     format!("Max retries updated from {} to {}", old_value, new_retries),
                                     Some(old_value),
                                     Some(value.clone()),
                                     false));
                        }
                        "retry_delay_ms" => {
                            let new_delay: u64 = value.parse().map_err(|_| 
                                ModbusError::InvalidData("Invalid delay value".to_string()))?;
                            
                            let old_value = config.retry_delay_ms.to_string();
                            config.retry_delay_ms = new_delay;
                            return Ok((true, 
                                     format!("Retry delay updated from {} to {} ms", old_value, new_delay),
                                     Some(old_value),
                                     Some(value.clone()),
                                     false));
                        }
                        _ => {
                            return Ok((false, 
                                     format!("Unknown monitoring parameter: {}", key), 
                                     None, None, false));
                        }
                    }
                }
            }
            ConfigTarget::Serial => {
                for (key, value) in &command.parameters {
                    match key.as_str() {
                        "port" => {
                            let old_value = config.serial_port.clone();
                            config.serial_port = value.clone();
                            return Ok((true, format!("Serial port changed from {} to {}", old_value, value), Some(old_value), Some(value.clone()), true));
                        }
                        "baud_rate" => {
                            let old_value = config.baud_rate.to_string();
                            config.baud_rate = value.parse().map_err(|_| ModbusError::InvalidData("Invalid baud rate".to_string()))?;
                            return Ok((true, format!("Baud rate changed from {} to {}", old_value, value), Some(old_value), Some(value.clone()), true));
                        }
                        "parity" => {
                            let old_value = format!("{:?}", config.parity);
                            config.parity = match value.to_lowercase().as_str() {
                                "none" => ParityConfig::None,
                                "even" => ParityConfig::Even,
                                "odd" => ParityConfig::Odd,
                                _ => return Err(ModbusError::InvalidData("Invalid parity value".to_string())),
                            };
                            return Ok((true, format!("Parity changed from {} to {}", old_value, value), Some(old_value), Some(value.clone()), true));
                        }
                        _ => return Err(ModbusError::InvalidData(format!("Unknown serial parameter: {}", key))),
                    }
                }
            }
            ConfigTarget::Gps => {
                for (key, value) in &command.parameters {
                    match key.as_str() {
                        "enabled" => {
                            let enabled = value.parse::<bool>().map_err(|_| 
                                ModbusError::InvalidData("Invalid boolean value for GPS enabled".to_string()))?;
                            
                            let old_value = config.gps.enabled.to_string();
                            config.gps.enabled = enabled;
                            
                            let restart_required = if enabled != config.gps.enabled { false } else { false };
                            
                            return Ok((true, 
                                     format!("GPS enabled changed from {} to {}", old_value, enabled),
                                     Some(old_value),
                                     Some(value.clone()),
                                     restart_required));
                        },
                        "port" => {
                            let old_value = config.gps.port.clone();
                            config.gps.port = value.clone();
                            
                            return Ok((true, 
                                     format!("GPS port changed from '{}' to '{}'", old_value, value),
                                     Some(old_value),
                                     Some(value.clone()),
                                     true)); // Port change requires restart
                        },
                        "baud_rate" => {
                            let baud_rate = value.parse::<u32>().map_err(|_| 
                                ModbusError::InvalidData("Invalid baud rate for GPS".to_string()))?;
                            
                            // Validate common GPS baud rates
                            if ![4800, 9600, 19200, 38400, 57600, 115200].contains(&baud_rate) {
                                return Ok((false, 
                                         format!("Invalid GPS baud rate: {}. Supported: 4800, 9600, 19200, 38400, 57600, 115200", baud_rate),
                                         None, None, false));
                            }
                            
                            let old_value = config.gps.baud_rate.to_string();
                            config.gps.baud_rate = baud_rate;
                            
                            return Ok((true, 
                                     format!("GPS baud rate changed from {} to {}", old_value, baud_rate),
                                     Some(old_value),
                                     Some(value.clone()),
                                     true)); // Baud rate change requires restart
                        },
                        "auto_start" => {
                            let auto_start = value.parse::<bool>().map_err(|_| 
                                ModbusError::InvalidData("Invalid boolean value for GPS auto_start".to_string()))?;
                            
                            let old_value = config.gps.auto_start.to_string();
                            config.gps.auto_start = auto_start;
                            
                            return Ok((true, 
                                     format!("GPS auto start changed from {} to {}", old_value, auto_start),
                                     Some(old_value),
                                     Some(value.clone()),
                                     false)); // Auto start change doesn't require restart
                        },
                        _ => {
                            return Ok((false, 
                                     format!("Unknown GPS parameter: {}. Available: enabled, port, baud_rate, auto_start", key),
                                     None, None, false));
                        }
                    }
                }
            },
            ConfigTarget::Device { address } => {
                if let Some(device) = config.devices.iter_mut().find(|d| d.address == *address) {
                    for (key, value) in &command.parameters {
                        match key.as_str() {
                            "name" => {
                                let old_value = device.name.clone();
                                device.name = value.clone();
                                return Ok((true, format!("Device {} name changed from '{}' to '{}'", address, old_value, value), Some(old_value), Some(value.clone()), false));
                            }
                            "location" => {
                                let old_value = device.location.clone();
                                device.location = value.clone();
                                return Ok((true, format!("Device {} location changed from '{}' to '{}'", address, old_value, value), Some(old_value), Some(value.clone()), false));
                            }
                            "polling_interval" => {
                                let old_value = device.polling_interval.map(|v| v.to_string()).unwrap_or("none".to_string());
                                device.polling_interval = Some(value.parse().map_err(|_| ModbusError::InvalidData("Invalid polling interval".to_string()))?);
                                return Ok((true, format!("Device {} polling interval changed from {} to {}", address, old_value, value), Some(old_value), Some(value.clone()), false));
                            }
                            _ => {
                                // Handle metadata
                                let old_value = device.metadata.get(key).cloned().unwrap_or("none".to_string());
                                device.metadata.insert(key.clone(), value.clone());
                                return Ok((true, format!("Device {} metadata '{}' changed from '{}' to '{}'", address, key, old_value, value), Some(old_value), Some(value.clone()), false));
                            }
                        }
                    }
                } else {
                    return Err(ModbusError::DeviceNotFound(format!("Device with address {} not found", address)));
                }
            }
            ConfigTarget::Site => {
                for (key, value) in &command.parameters {
                    match key.as_str() {
                        "site_name" => {
                            let old_value = config.site_info.site_name.clone();
                            config.site_info.site_name = value.clone();
                            return Ok((true, format!("Site name changed from '{}' to '{}'", old_value, value), Some(old_value), Some(value.clone()), false));
                        }
                        "operator" => {
                            let old_value = config.site_info.operator.clone();
                            config.site_info.operator = value.clone();
                            return Ok((true, format!("Operator changed from '{}' to '{}'", old_value, value), Some(old_value), Some(value.clone()), false));
                        }
                        _ => {
                            let old_value = config.site_info.metadata.get(key).cloned().unwrap_or("none".to_string());
                            config.site_info.metadata.insert(key.clone(), value.clone());
                            return Ok((true, format!("Site metadata '{}' changed from '{}' to '{}'", key, old_value, value), Some(old_value), Some(value.clone()), false));
                        }
                    }
                }
            }
            //  Add support for Output target
            ConfigTarget::Output { output_type } => {
                match output_type.as_str() {
                    "database" => {
                        for (key, value) in &command.parameters {
                            match key.as_str() {
                                "enabled" => {
                                    let enabled = value.parse::<bool>().map_err(|_| 
                                        ModbusError::InvalidData("Invalid boolean value".to_string()))?;
                                    
                                    if config.output.database_output.is_none() {
                                        config.output.database_output = Some(crate::config::DatabaseOutputConfig::default());
                                    }
                                    config.output.database_output.as_mut().unwrap().enabled = enabled;
                                    return Ok((true, format!("Database enabled set to: {}", enabled), None, Some(value.clone()), false));
                                }
                                "batch_size" => {
                                    let batch_size = value.parse::<usize>().map_err(|_| 
                                        ModbusError::InvalidData("Invalid batch size".to_string()))?;
                                    
                                    if config.output.database_output.is_none() {
                                        config.output.database_output = Some(crate::config::DatabaseOutputConfig::default());
                                    }
                                    config.output.database_output.as_mut().unwrap().batch_size = batch_size;
                                    return Ok((true, format!("Database batch_size set to: {}", batch_size), None, Some(value.clone()), false));
                                }
                                "flush_interval_seconds" => {
                                    let flush_interval = value.parse::<u64>().map_err(|_| 
                                        ModbusError::InvalidData("Invalid flush interval".to_string()))?;
                                    
                                    if config.output.database_output.is_none() {
                                        config.output.database_output = Some(crate::config::DatabaseOutputConfig::default());
                                    }
                                    config.output.database_output.as_mut().unwrap().flush_interval_seconds = flush_interval;
                                    return Ok((true, format!("Database flush_interval_seconds set to: {}", flush_interval), None, Some(value.clone()), false));
                                }
                                _ => return Err(ModbusError::InvalidData(format!("Unknown database parameter: {}", key))),
                            }
                        }
                    }
                    _ => return Err(ModbusError::InvalidData(format!("Unknown output type: {}", output_type))),
                }
            }
            //  Add support for SocketServer target
            
            // Keep the old catch-all for backward compatibility but make it more specific
            _ => return Err(ModbusError::InvalidData(format!("Unsupported target for SET command: {:?}", command.target))),
        }

        Ok((false, "No matching parameters found".to_string(), None, None, false))
    }

    fn handle_add_command(&self, config: &mut Config, command: &ConfigurationCommand) -> Result<(bool, String, Option<String>, Option<String>, bool), ModbusError> {
        match &command.target {
            ConfigTarget::Device { address } => {
                if config.devices.iter().any(|d| d.address == *address) {
                    return Err(ModbusError::InvalidData(format!("Device with address {} already exists", address)));
                }

                let device_type = command.parameters.get("device_type").cloned().unwrap_or_else(|| "flowmeter".to_string());
                let name = command.parameters.get("name").cloned().unwrap_or_else(|| format!("Device {}", address));
                let location = command.parameters.get("location").cloned().unwrap_or_else(|| "Unknown".to_string());

                // Build metadata from parameters
                let mut metadata = HashMap::new();
                
                // Extract metadata from parameters (anything that's not core device info)
                for (key, value) in &command.parameters {
                    match key.as_str() {
                        "device_type" | "name" | "location" => {
                            // Skip core parameters
                        }
                        _ => {
                            metadata.insert(key.clone(), value.clone());
                        }
                    }
                }

                // Set default parameters based on device type
                let parameters = match device_type.as_str() {
                    "flowmeter" => vec![
                        "MassFlowRate".to_string(),
                        "Temperature".to_string(),
                        "DensityFlow".to_string(),
                        "VolumeTotal".to_string(),
                    ],
                    "rpm" => {
                        let channels = metadata.get("total_channels")
                            .and_then(|s| s.parse::<u8>().ok())
                            .unwrap_or(2);
                        
                        let mut params = vec![
                            "TotalChannels".to_string(),
                            "GlobalErrorCode".to_string(),
                            "RunningEngines".to_string(),
                        ];
                        
                        // Add channel-specific parameters
                        for i in 1..=channels {
                            params.extend([
                                format!("RPM{}", i),
                                format!("Freq{}", i),
                                format!("EngineDuration{}", i),
                                format!("IsRunning{}", i),
                                format!("ErrorCode{}", i),
                            ]);
                        }
                        params
                    }
                    "gps" => vec![
                        "Latitude".to_string(),
                        "Longitude".to_string(),
                        "Speed".to_string(),
                        "Course".to_string(),
                        "Altitude".to_string(),
                        "Satellites".to_string(),
                    ],
                    _ => vec!["Status".to_string()],
                };

                let new_device = DeviceConfig {
                    address: *address,
                    device_type: device_type.clone(),
                    name: name.clone(),
                    location,
                    enabled: true,
                    polling_interval: None,
                    parameters,
                    metadata,
                    uuid: uuid::Uuid::new_v4().to_string(),
                };

                config.devices.push(new_device);
                
                let message = if device_type == "rpm" {
                let channels = command.parameters.get("total_channels")
                    .cloned()
                    .unwrap_or_else(|| "2".to_string());
                    format!("Added new {} device: {} at address {} with {} channels", device_type, name, address, channels)
                } else {
                    format!("Added new {} device: {} at address {}", device_type, name, address)
                };
                
                Ok((true, message, None, Some(format!("Device {}", address)), true))
            }
            _ => Err(ModbusError::InvalidData("ADD command only supports Device target".to_string())),
        }
    }

    fn handle_enable_command(&self, config: &mut Config, command: &ConfigurationCommand) -> Result<(bool, String, Option<String>, Option<String>, bool), ModbusError> {
        match &command.target {
            ConfigTarget::Device { address } => {
                if let Some(device) = config.devices.iter_mut().find(|d| d.address == *address) {
                    let old_value = device.enabled.to_string();
                    device.enabled = true;
                    Ok((true, format!("Device {} enabled", address), Some(old_value), Some("true".to_string()), false))
                } else {
                    Err(ModbusError::DeviceNotFound(format!("Device with address {} not found", address)))
                }
            }
            _ => Err(ModbusError::InvalidData("ENABLE command only supports Device target".to_string())),
        }
    }

    fn handle_disable_command(&self, config: &mut Config, command: &ConfigurationCommand) -> Result<(bool, String, Option<String>, Option<String>, bool), ModbusError> {
        match &command.target {
            ConfigTarget::Device { address } => {
                if let Some(device) = config.devices.iter_mut().find(|d| d.address == *address) {
                    let old_value = device.enabled.to_string();
                    device.enabled = false;
                    Ok((true, format!("Device {} disabled", address), Some(old_value), Some("false".to_string()), false))
                } else {
                    Err(ModbusError::DeviceNotFound(format!("Device with address {} not found", address)))
                }
            }
            _ => Err(ModbusError::InvalidData("DISABLE command only supports Device target".to_string())),
        }
    }

    fn handle_remove_command(&self, config: &mut Config, command: &ConfigurationCommand) -> Result<(bool, String, Option<String>, Option<String>, bool), ModbusError> {
        match &command.target {
            ConfigTarget::Device { address } => {
                if let Some(pos) = config.devices.iter().position(|d| d.address == *address) {
                    let removed_device = config.devices.remove(pos);
                    Ok((true, format!("Removed device: {} ({})", removed_device.name, address), Some(removed_device.name), None, true))
                } else {
                    Err(ModbusError::DeviceNotFound(format!("Device with address {} not found", address)))
                }
            }
            _ => Err(ModbusError::InvalidData("REMOVE command only supports Device target".to_string())),
        }
    }

    fn handle_reset_command(&self, config: &mut Config, _command: &ConfigurationCommand) -> Result<(bool, String, Option<String>, Option<String>, bool), ModbusError> {
        let old_devices_count = config.devices.len();
        *config = Config::default();
        Ok((true, format!("Configuration reset to defaults (removed {} devices)", old_devices_count), Some(old_devices_count.to_string()), Some("0".to_string()), true))
    }

    async fn handle_backup_command(&self, config: &Config, command: &ConfigurationCommand) -> Result<(bool, String, Option<String>, Option<String>, bool), ModbusError> {
        let backup_name = command.parameters.get("name")
            .cloned()
            .unwrap_or_else(|| format!("backup_{}", Utc::now().format("%Y%m%d_%H%M%S")));
        
        let backup_path = format!("{}/{}.toml", self.backup_dir, backup_name);
        
        config.save_to_file(&backup_path).map_err(|e| {
            ModbusError::CommunicationError(format!("Failed to create backup: {}", e))
        })?;

        Ok((true, format!("Configuration backed up to: {}", backup_path), None, Some(backup_path), false))
    }

    async fn handle_restore_command(&self, config: &mut Config, command: &ConfigurationCommand) -> Result<(bool, String, Option<String>, Option<String>, bool), ModbusError> {
        let backup_name = command.parameters.get("name")
            .ok_or_else(|| ModbusError::InvalidData("Backup name required for restore".to_string()))?;
        
        let backup_path = format!("{}/{}.toml", self.backup_dir, backup_name);
        
        let restored_config = Config::from_file(&backup_path).map_err(|e| {
            ModbusError::CommunicationError(format!("Failed to load backup: {}", e))
        })?;

        let old_devices_count = config.devices.len();
        *config = restored_config;

        Ok((true, format!("Configuration restored from: {}", backup_path), Some(old_devices_count.to_string()), Some(config.devices.len().to_string()), true))
    }

    async fn save_config(&self) -> Result<(), ModbusError> {
        let config = self.current_config.lock().await;
        config.save_to_file(&self.config_file_path).map_err(|e| {
            ModbusError::CommunicationError(format!("Failed to save config: {}", e))
        })?;
        info!("💾 Configuration saved to: {}", self.config_file_path);
        Ok(())
    }

}