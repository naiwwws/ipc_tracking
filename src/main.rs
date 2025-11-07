mod services;
mod cli;
mod config;
mod modbus;
mod devices;
mod utils;
mod output;
mod storage;

use anyhow::Result;
use log::{info, warn};

use services::DataService;
#[cfg(feature = "api")]
use services::ApiService;
#[cfg(feature = "api")]
use crate::services::api_service::ApiServiceState;
use config::{Config, DynamicConfigManager};
use ipc_tracking::{VERSION};
use cli::{CliBuilder, commands::{handle_subcommands}};

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logger with better formatting
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .format_timestamp_millis()
        .init();

    info!("🚀 IPC Tracking System v{}", VERSION);
    info!("📂 Working directory: {:?}", std::env::current_dir().unwrap_or_default());

    let matches = CliBuilder::build_complete().get_matches(); 

    // Handle config commands FIRST, before creating the service
    if let Some(config_matches) = matches.subcommand_matches("config") {
        let config_file = matches.get_one::<String>("config-file").unwrap();
        let backup_dir = "setup/backups";
        
        // Create config directory if it doesn't exist
        std::fs::create_dir_all("config").unwrap_or_else(|e| {
            eprintln!("Warning: Could not create config directory: {}", e);
        });
        std::fs::create_dir_all(backup_dir).unwrap_or_else(|e| {
            eprintln!("Warning: Could not create backup directory: {}", e);
        });
        
        // Create config manager
        let config_manager = DynamicConfigManager::new(config_file, backup_dir)?;
        
        // Handle config commands
        let handled = crate::config::config_commands::handle_config_commands(config_matches, &config_manager).await
            .map_err(|e| anyhow::anyhow!("Config command error: {}", e))?;
        
        if handled {
            return Ok(());
        }
    }

    // ✅ ENHANCED: Config loading with proper path handling
    let config_file = matches.get_one::<String>("config-file").unwrap();
    
    // Convert to PathBuf for better handling
    let config_path = std::path::PathBuf::from(config_file);
    let config_path_canonical = if config_path.is_absolute() {
        config_path.clone()
    } else {
        std::env::current_dir()
            .unwrap_or_default()
            .join(&config_path)
    };
    
    info!("🔍 Loading configuration from: {}", config_path_canonical.display());
    
    let config = if config_path_canonical.exists() {
        info!("📁 Config file found: {}", config_path_canonical.display());
        
        // Check file permissions
        match std::fs::metadata(&config_path_canonical) {
            Ok(metadata) => {
                info!("� Config file size: {} bytes", metadata.len());
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    info!("🔒 Config permissions: {:o}", metadata.permissions().mode());
                }
            }
            Err(e) => {
                warn!("⚠️  Could not read config metadata: {}", e);
            }
        }
        
        match Config::from_file(config_file) {
            Ok(mut config) => {
                info!("✅ Successfully loaded TOML config");
                // Override API server config ONLY if CLI flags are provided
                if matches.get_flag("api") || matches.contains_id("api-port") {
                    info!("🔧 CLI override: Enabling API server");
                    config.api_server.enabled = true;
                    
                    if let Some(port_str) = matches.get_one::<String>("api-port") {
                        config.api_server.port = port_str.parse::<u16>()
                            .unwrap_or_else(|_| {
                                eprintln!("Invalid API port number, using TOML/default");
                                config.api_server.port
                            });
                    }
                } else {
                    info!("📋 Using TOML API server config: enabled={}, port={}", 
                          config.api_server.enabled, config.api_server.port);
                }

                // Override database config ONLY if CLI flags are provided
                if matches.contains_id("output-db") {
                    if let Some(db_connection) = matches.get_one::<String>("output-db") {
                        info!("🔧 CLI override: Enabling database output");
                        if config.output.database_output.is_none() {
                            config.output.database_output = Some(crate::config::DatabaseOutputConfig::default());
                        }
                        config.output.database_output.as_mut().unwrap().enabled = true;
                        
                        // Parse database connection string
                        if db_connection.starts_with("sqlite:") {
                            let db_path = db_connection.strip_prefix("sqlite:").unwrap_or("data/sensor_data.db");
                            config.output.database_output.as_mut().unwrap().sqlite_config.database_path = db_path.to_string();
                        }
                    }
                } else {
                    let db_enabled = config.output.database_output.as_ref().map(|db| db.enabled).unwrap_or(false);
                    info!("📋 Using TOML database config: enabled={}", db_enabled);
                }

                config
            },
            Err(e) => {
                eprintln!("❌ Failed to load config file: {}", e);
                eprintln!("📂 Tried path: {}", config_path_canonical.display());
                eprintln!("📂 Working directory: {:?}", std::env::current_dir().unwrap_or_default());
                
                // Check if directory exists
                if let Some(parent) = config_path_canonical.parent() {
                    if !parent.exists() {
                        eprintln!("❌ Config directory does not exist: {}", parent.display());
                    }
                }
                
                info!("🔄 Using default configuration and saving it");
                let default_config = Config::default();
                
                // Ensure directory exists before saving
                if let Some(parent) = config_path_canonical.parent() {
                    if let Err(dir_err) = std::fs::create_dir_all(parent) {
                        warn!("⚠️  Could not create config directory: {}", dir_err);
                    }
                }
                
                // Try to save default config
                if let Err(save_err) = default_config.save_to_file(config_file) {
                    warn!("⚠️  Failed to save default config: {}", save_err);
                } else {
                    info!("💾 Saved default config to: {}", config_path_canonical.display());
                }
                
                default_config
            }
        }
    } else {
        info!("📁 Config file not found at: {}", config_path_canonical.display());
        info!("📁 Creating from CLI args and defaults");
        
        // Check if directory exists
        if let Some(parent) = config_path_canonical.parent() {
            if !parent.exists() {
                info!("📁 Creating config directory: {}", parent.display());
                if let Err(e) = std::fs::create_dir_all(parent) {
                    warn!("⚠️  Failed to create directory: {}", e);
                }
            }
        }
        
        let config = match Config::from_matches(&matches) {
            Ok(config) => config,
            Err(e) => {
                eprintln!("❌ Failed to parse CLI args: {}", e);
                info!("🔄 Using default configuration");
                Config::default()
            }
        };
        
        // Save the config file for future use
        if let Err(e) = config.save_to_file(config_file) {
            warn!("⚠️  Failed to save initial config file: {}", e);
            warn!("⚠️  Check directory permissions: {:?}", config_path_canonical.parent());
        } else {
            info!("💾 Created initial config file: {}", config_path_canonical.display());
        }
        
        config
    };

    // ✅ ENHANCED: Config validation and display
    info!("🔍 Final Configuration:");
    info!("  IPC Name: {}", config.ipc_name);
    info!("  IPC UUID: {}", config.ipc_uuid);
    info!("  Serial Port: {}", config.serial_port);
    info!("  Devices: {}", config.devices.len());
    info!("  API Server: {} (port: {})", 
          config.api_server.enabled, config.api_server.port);
    
    let db_enabled = config.output.database_output.as_ref().map(|db| db.enabled).unwrap_or(false);
    info!("  Database: {}", if db_enabled { "enabled" } else { "disabled" });

    info!("Mtws Config: {:?}", config.mtws);

    // Initialize DataService
    let mut service = DataService::new(config.clone()).await?;


    info!("Mtws Config: {:?}", config.mtws);
    #[cfg(feature = "sqlite")]
    if config.mtws.enabled {
        if let Err(e) = service.initialize_mtws_service() {
            warn!("⚠️ Failed to initialize MTWS service: {}", e);
        } else {
            info!("🛰️ MTWS service initialized successfully");
            
            // Auto-start MTWS if configured
            if config.mtws.auto_start {
                if let Some(mtws_service) = service.get_mtws_service() {
                    match mtws_service.start_transmission().await {
                        Ok(_) => {
                            info!("✅ MTWS transmission auto-started");
                            info!("📡 Sending to: {}", config.get_mtws_endpoint_url());
                            info!("⏱️ Interval: {} seconds", config.mtws.transmission_interval_seconds);
                        }
                        Err(e) => {
                            warn!("❌ Failed to auto-start MTWS: {}", e);
                        }
                    }
                }
            }
        }
    }
    
    #[cfg(not(feature = "sqlite"))]
    if config.mtws.enabled {
        warn!("⚠️ MTWS is enabled in config but sqlite feature is not compiled. MTWS requires sqlite feature.");
    }

    // ✅ FIXED: Start API service based on TOML config, not just CLI
    #[cfg(feature = "api")]
    let mut api_service_handle: Option<crate::services::ApiService> = None;
    #[cfg(feature = "api")]
    if config.api_server.enabled {
        if let Some(db_service) = service.get_database_service() {
            let sqlite_manager = db_service.get_sqlite_manager().clone();
            
            // FIXED: Create proper Arc for DataService
            let data_service_arc = std::sync::Arc::new(service.clone());
            
            // Create API state with proper DataService connection
            let api_state = ApiServiceState::new(
                config.clone(),
                sqlite_manager.clone(),
                Some(data_service_arc.clone()) // Pass DataService as Arc
            );
            
            // FIXED: Initialize API service with the state
            let mut api_service = ApiService::new_with_state(api_state)?;
            api_service.start(config.api_server.port).await?;
            info!("🌐 HTTP API server started on port {} with DataService connection", config.api_server.port);
            api_service_handle = Some(api_service);
        } else {
            info!("⚠️  Database not available for API service");
        }
    } else {
        info!("📝 API server disabled in config");
    }

    // NEW: Handle GPS commands - add this right after websocket commands
    if let Some(gps_matches) = matches.subcommand_matches("gps") {
        if cli::commands::handle_gps_commands(gps_matches, &service).await? {
            // Stop services before returning
            #[cfg(feature = "api")]
            if let Some(mut api_service) = api_service_handle {
                api_service.stop().await?;
            }
            return Ok(());
        }
    }

    // Handle other subcommands
    if handle_subcommands(&matches, &mut service).await? {
        // Stop services before returning
        #[cfg(feature = "api")]
        if let Some(mut api_service) = api_service_handle {
            api_service.stop().await?;
        }
        return Ok(());
    }

    // Setup graceful shutdown
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    
    // Handle Ctrl+C
    let shutdown_tx = std::sync::Arc::new(tokio::sync::Mutex::new(Some(shutdown_tx)));
    let shutdown_tx_clone = shutdown_tx.clone();
    
    tokio::spawn(async move {
        tokio::signal::ctrl_c().await.expect("Failed to listen for Ctrl+C");
        info!("🛑 Received Ctrl+C, initiating graceful shutdown...");
        
        if let Some(tx) = shutdown_tx_clone.lock().await.take() {
            let _ = tx.send(());
        }
    });

    // Configure debug mode
    let debug_mode = matches.get_flag("debug");
    if debug_mode {
        info!("🐛 Debug mode enabled - additional data output will be shown");
    }
    
    // Check if FSM mode is enabled
    let fsm_mode = matches.get_flag("fsm-mode");
    if fsm_mode {
        info!("🎯 FSM Mode enabled - Starting with finite state machine");
        return run_with_fsm(service, shutdown_rx, config, api_service_handle).await;
    }

    // Configure output format if specified
    if let Some(format) = matches.get_one::<String>("format") {
        match format.as_str() {
            "json" => {
                info!("🎨 Using JSON formatter");
                // service.set_formatter(Box::new(crate::output::JsonFormatter));
            }
            "csv" => {
                info!("🎨 Using CSV formatter");
                // service.set_formatter(Box::new(crate::output::CsvFormatter));
            }
            "hex" => {
                info!("🔍 Using Hex formatter");
                // service.set_formatter(Box::new(crate::output::HexFormatter));
            }
            _ => {} // Keep default console formatter
        }
    }

    if let Some(output_file) = matches.get_one::<String>("output-file") {
        info!("📝 Adding file output: {}", output_file);
        // service.add_sender(Box::new(crate::output::FileSender::new(output_file, true)));
    }

    // Start continuous service
    info!("🚀 Starting Industrial Modbus Service version {}", VERSION);
    info!("📡 Serial port: {}", config.serial_port);
    info!("⚙️  Baud rate: {}", config.baud_rate);
    info!("🎯 Device addresses: {:?}", config.device_addresses);
    info!("⏱️  Update interval: {} seconds", config.update_interval_seconds);
    
    if debug_mode {
        info!("🐛 Debug mode enabled - automatic data printing");
    } else {
        info!("ℹ️  Use --debug flag for automatic data printing");
    }

    // Run service in a select block for graceful shutdown
    tokio::select! {
        result = service.run(debug_mode) => {
            if let Err(e) = result {
                eprintln!("❌ Service error: {}", e);
            }
        }
        _ = shutdown_rx => {
            info!("🛑 Shutdown signal received");
        }
    }

    // Graceful shutdown sequence
    info!("🔄 Shutting down services...");
    
    // Stop API service first
    #[cfg(feature = "api")]
    if let Some(mut api_service) = api_service_handle {
        info!("🛑 Stopping API service...");
        if let Err(e) = api_service.stop().await {
            eprintln!("❌ Failed to stop API service: {}", e);
        } else {
            info!("✅ API service stopped");
        }
    }

    // Stop main service
    info!("🛑 Stopping main service...");
    // Add a stop method to DataService if it doesn't exist
    
    info!("✅ All services stopped gracefully");
    Ok(())
}

/// Run the system with FSM mode
#[cfg(feature = "api")]
async fn run_with_fsm(
    service: DataService,
    shutdown_rx: tokio::sync::oneshot::Receiver<()>,
    config: Config,
    api_service_handle: Option<ApiService>,
) -> Result<()> {
    use std::sync::Arc;
    use utils::state_machine::SystemStateMachine;
    use utils::state_handlers::run_state_machine;
    
    info!("🎯 Starting system with Finite State Machine");
    info!("📋 FSM will ensure proper initialization sequence:");
    info!("   1. Initialize system");
    info!("   2. Read all device data");
    info!("   3. Read GPS data");
    info!("   4. Test connections");
    info!("   5. Enter operational mode");
    
    // Create FSM with max retries from config
    let max_retries = config.max_retries;
    let fsm = Arc::new(SystemStateMachine::new(max_retries));
    let service_arc = Arc::new(service);
    
    // Run the FSM
    let fsm_result = run_state_machine(fsm.clone(), service_arc.clone(), shutdown_rx).await;
    
    // Cleanup
    info!("🔄 Shutting down FSM services...");
    
    #[cfg(feature = "api")]
    if let Some(mut api_service) = api_service_handle {
        info!("🛑 Stopping API service...");
        if let Err(e) = api_service.stop().await {
            eprintln!("❌ Failed to stop API service: {}", e);
        } else {
            info!("✅ API service stopped");
        }
    }
    
    match fsm_result {
        Ok(_) => {
            info!("✅ FSM execution completed successfully");
            Ok(())
        }
        Err(e) => {
            eprintln!("❌ FSM execution failed: {}", e);
            Err(e.into())
        }
    }
}

#[cfg(not(feature = "api"))]
async fn run_with_fsm(
    service: DataService,
    shutdown_rx: tokio::sync::oneshot::Receiver<()>,
    _config: Config,
    _api_service_handle: Option<()>,
) -> Result<()> {
    use std::sync::Arc;
    use utils::state_machine::SystemStateMachine;
    use utils::state_handlers::run_state_machine;
    
    info!("🎯 Starting system with Finite State Machine");
    info!("📋 FSM will ensure proper initialization sequence:");
    info!("   1. Initialize system");
    info!("   2. Read all device data");
    info!("   3. Read GPS data");
    info!("   4. Test connections");
    info!("   5. Enter operational mode");
    
    // Create FSM with default max retries
    let fsm = Arc::new(SystemStateMachine::new(3));
    let service_arc = Arc::new(service);
    
    // Run the FSM
    let fsm_result = run_state_machine(fsm.clone(), service_arc.clone(), shutdown_rx).await;
    
    match fsm_result {
        Ok(_) => {
            info!("✅ FSM execution completed successfully");
            Ok(())
        }
        Err(e) => {
            eprintln!("❌ FSM execution failed: {}", e);
            Err(e.into())
        }
    }
}
