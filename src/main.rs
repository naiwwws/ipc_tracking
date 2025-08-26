mod services;
mod cli;
mod config;
mod modbus;
mod devices;
mod utils;
mod output;
mod storage;

use anyhow::Result;
use clap::{Arg, Command, ArgAction, ArgMatches}; // Add ArgMatches here
use log::info;

use services::{DataService, ApiService}; // Fixed import
use crate::services::api_service::ApiServiceState;
use config::{Config, DynamicConfigManager};
use ipc_dev_rust::{VERSION};
use cli::commands::{handle_subcommands};

fn build_cli() -> Command {
    Command::new("ipc_dev_rust")
        .version(VERSION)
        .about("Modular Industrial Device Communication Service")
        .arg(
            Arg::new("config-file")
                .long("config-file")
                .short('c')
                .value_name("FILE")
                .help("Configuration file path")
                .default_value("setup/default.toml"),
        )
        .arg(
            Arg::new("port")
                .short('p')
                .long("port")
                .value_name("PORT")
                .help("Serial port path")
                .default_value("/dev/ttyS0"),
        )
        .arg(
            Arg::new("baud")
                .short('b')
                .long("baud")
                .value_name("BAUD")
                .help("Baud rate")
                .default_value("9600"),
        )
        .arg(
            Arg::new("devices")
                .short('d')
                .long("devices")
                .value_name("DEVICES")
                .help("Device addresses (comma-separated)")
                .default_value("2,3"),
        )
        .arg(
            Arg::new("interval")
                .short('i')
                .long("interval")
                .value_name("SECONDS")
                .help("Update interval in seconds")
                .default_value("10"),
        )
        .arg(
            Arg::new("debug")
                .long("debug")
                .short('D')
                .help("Enable debug mode with automatic data printing")
                .action(clap::ArgAction::SetTrue),
        )
        .arg(
            Arg::new("format")
                .long("format")
                .value_name("FORMAT")
                .help("Output format: console, json, csv, hex")
                .value_parser(["console", "json", "csv", "hex"])
                .default_value("console"),
        )
        .arg(
            Arg::new("output-file")
                .long("output-file")
                .value_name("FILE")
                .help("Write output to file"),
        )
        .arg(
            Arg::new("output-http")
                .long("output-http")
                .value_name("URL")
                .help("Send output to HTTP endpoint"),
        )
        .arg(
            Arg::new("output-db")
                .long("output-db")
                .value_name("CONNECTION")
                .help("Send output to database"),
        )
        .arg(
            Arg::new("output-mqtt")
                .long("output-mqtt")
                .value_name("BROKER,TOPIC")
                .help("Send output to MQTT broker (format: broker_url,topic)"),
        )
        .arg(
            Arg::new("socket-port")
                .long("socket-port")
                .value_name("PORT")
                .help("Enable socket server on specified port (default: 8080)"),
        )
        .arg(
            Arg::new("socket")
                .long("socket")
                .action(clap::ArgAction::SetTrue)
                .help("Enable socket server on default port (8080)"),
        )
        .arg(
            Arg::new("websocket")
                .long("websocket")
                .action(clap::ArgAction::SetTrue)
                .help("Enable WebSocket server on default port (8080)"),
        )
        .arg(
            Arg::new("websocket-port")
                .long("websocket-port")
                .value_name("PORT")
                .help("Enable WebSocket server on specified port"),
        )
        .arg(
            Arg::new("disable-socket")
                .long("disable-socket")
                .action(clap::ArgAction::SetTrue)
                .help("Disable all socket/websocket servers"),
        )
        .arg(
            Arg::new("api-port")
                .long("api-port")
                .value_name("PORT")
                .help("Enable HTTP API server on specified port (default: 3000)"),
        )
        .arg(
            Arg::new("api")
                .long("api")
                .action(clap::ArgAction::SetTrue)
                .help("Enable HTTP API server on default port (3000)"),
        )
        .subcommand(
            Command::new("getdata")
                .about("Get all device data")
        )
        .subcommand(
            Command::new("getvolatile")
                .about("Get volatile data for specific parameter")
                .arg(
                    Arg::new("parameter")
                        .help("Parameter name")
                        .required(true)
                        .index(1),
                )
        )
        .subcommand(
            Command::new("resetaccumulation")
                .about("Reset accumulation for a device")
                .arg(
                    Arg::new("device_address")
                        .help("Device address to reset")
                        .required(true)
                        .index(1),
                ),
        )
        .subcommand(
            Command::new("config")
                .about("Configuration management")
                .subcommand(
                    Command::new("show")
                        .about("Show current configuration")
                )
                .subcommand(
                    Command::new("ipc")
                        .about("IPC configuration management")
                        .subcommand(
                            Command::new("set-name")
                                .about("Set IPC name")
                                .arg(Arg::new("name").help("IPC name").required(true).index(1))
                                .arg(Arg::new("operator").long("operator").help("Operator name").default_value("CLI"))
                        )
                        .subcommand(
                            Command::new("regenerate-uuid")
                                .about("Generate new UUID for this IPC")
                        )
                )
                .subcommand(
                    Command::new("set-interval")
                        .about("Set polling interval")
                        .arg(Arg::new("seconds").help("Polling interval in seconds").required(true).index(1))
                        .arg(Arg::new("operator").long("operator").help("Operator name").default_value("CLI"))
                )
                .subcommand(
                    Command::new("set")
                        .about("Set configuration parameter")
                        .arg(Arg::new("target").help("Target (serial, device:ADDRESS, monitoring, site)").required(true).index(1))
                        .arg(Arg::new("key").help("Parameter key").required(true).index(2))
                        .arg(Arg::new("value").help("Parameter value").required(true).index(3))
                        .arg(Arg::new("operator").long("operator").help("Operator name").default_value("CLI"))
                )
                .subcommand(
                    Command::new("add")
                        .about("Add new device")
                        .arg(Arg::new("type")
                            .long("type")
                            .short('t')
                            .value_name("TYPE")
                            .help("Device type (flowmeter, rpm, gps)")
                            .required(true))
                        .arg(Arg::new("address")
                            .long("address")
                            .short('a')
                            .value_name("ADDRESS")
                            .help("Device Modbus address (1-255)")
                            .required(true))
                        .arg(Arg::new("name")
                            .long("name")
                            .short('n')
                            .value_name("NAME")
                            .help("Device name")
                            .required(true))
                        .arg(Arg::new("location")
                            .long("location")
                            .short('l')
                            .value_name("LOCATION")
                            .help("Device location")
                            .default_value("Unknown"))
                        // RPM-specific arguments
                        .arg(Arg::new("channels")
                            .long("channels")
                            .short('c')
                            .value_name("COUNT")
                            .help("Number of RPM channels (1-8) - only for RPM devices"))
                        .arg(Arg::new("thresholds")
                            .long("thresholds")
                            .value_name("THRESHOLDS")
                            .help("Comma-separated RPM thresholds for each channel")
                            .value_delimiter(','))
                        .arg(Arg::new("engine-types")
                            .long("engine-types")
                            .value_name("TYPES")
                            .help("Comma-separated engine types for each channel")
                            .value_delimiter(','))
                        .arg(Arg::new("auto-detect")
                            .long("auto-detect")
                            .help("Enable auto-detection of channels (RPM only)")
                            .action(ArgAction::SetTrue))
                )
                .subcommand(
                    Command::new("enable")
                        .about("Enable device")
                        .arg(Arg::new("address").help("Device address").required(true).index(1))
                        .arg(Arg::new("operator").long("operator").help("Operator name").default_value("CLI"))
                )
                .subcommand(
                    Command::new("disable")
                        .about("Disable device")
                        .arg(Arg::new("address").help("Device address").required(true).index(1))
                        .arg(Arg::new("operator").long("operator").help("Operator name").default_value("CLI"))
                )
                .subcommand(
                    Command::new("remove")
                        .about("Remove device")
                        .arg(Arg::new("address").help("Device address").required(true).index(1))
                        .arg(Arg::new("operator").long("operator").help("Operator name").default_value("CLI"))
                )
                .subcommand(
                    Command::new("backup")
                        .about("Backup configuration")
                        .arg(Arg::new("name").long("name").help("Backup name"))
                        .arg(Arg::new("operator").long("operator").help("Operator name").default_value("CLI"))
                )
                .subcommand(
                    Command::new("restore")
                        .about("Restore configuration")
                        .arg(Arg::new("name").help("Backup name").required(true).index(1))
                        .arg(Arg::new("operator").long("operator").help("Operator name").default_value("CLI"))
                )
                .subcommand(
                    Command::new("reset")
                        .about("Reset configuration to defaults")
                )
                .subcommand(
                    Command::new("rpm")
                        .about("RPM device management")
                        .subcommand(
                            Command::new("register")
                                .about("Register new RPM device")
                                .arg(Arg::new("address").long("address").required(true))
                                .arg(Arg::new("name").long("name").required(true))
                                .arg(Arg::new("location").long("location").default_value("Unknown"))
                                .arg(Arg::new("channels").long("channels").required(true))
                                .arg(Arg::new("thresholds").long("thresholds").value_delimiter(','))
                                .arg(Arg::new("engine-types").long("engine-types").value_delimiter(','))
                                .arg(Arg::new("auto-detect").long("auto-detect").action(ArgAction::SetTrue))
                        )
                        .subcommand(
                            Command::new("update")
                                .about("Update RPM device")
                                .arg(Arg::new("address").long("address").required(true))
                                .arg(Arg::new("channel").long("channel"))
                                .arg(Arg::new("threshold").long("threshold"))
                                .arg(Arg::new("engine-type").long("engine-type"))
                        )
                )
        )
        .subcommand(
            Command::new("db")
                .about("Database operations")
                .subcommand(Command::new("init").about("Initialize database"))
                .subcommand(Command::new("stats").about("Show database statistics"))
                .subcommand(Command::new("query").about("Query database")
                    .arg(Arg::new("table").short('t').long("table").help("Table name").default_value("device_readings"))
                    .arg(Arg::new("limit").short('l').long("limit").help("Limit results").default_value("10"))
                    .arg(Arg::new("device").short('d').long("device").help("Device address filter"))
                )
                .subcommand(Command::new("schema").about("Show database schema"))
        )
        .subcommand(
            Command::new("websocket")
                .about("WebSocket server management")
                .subcommand(Command::new("status").about("Show WebSocket server status"))
                .subcommand(Command::new("clients").about("Show connected WebSocket clients"))
        )
        .subcommand(
            Command::new("gps")
                .about("GPS location and tracking commands")
                .subcommand(
                    Command::new("start")
                        .about("Start GPS service")
                )
                .subcommand(
                    Command::new("stop")
                        .about("Stop GPS service")
                )
                .subcommand(
                    Command::new("status")
                        .about("Show GPS service status")
                )
                .subcommand(
                    Command::new("data")
                        .about("Show current GPS data and location")
                )
                .subcommand(
                    Command::new("test")
                        .about("Test GPS connection and wait for fix")
                )
        )
        .subcommand(
            Command::new("mtws")
                .about("MTWS (Marine Transport and Warehouse System) operations")
                .subcommand(
                    Command::new("config")
                        .about("Configure MTWS settings")
                        .arg(Arg::new("imei")
                            .long("imei")
                            .help("Device IMEI (15 digits)")
                            .value_name("IMEI"))
                        .arg(Arg::new("endpoint")
                            .long("endpoint")
                            .help("Base endpoint URL (without IMEI)")
                            .value_name("URL"))
                        .arg(Arg::new("interval")
                            .long("interval")
                            .help("Transmission interval in seconds (minimum 60)")
                            .value_name("SECONDS"))
                )
                .subcommand(
                    Command::new("start")
                        .about("Start automatic MTWS transmission")
                )
                .subcommand(
                    Command::new("stop")
                        .about("Stop automatic MTWS transmission")
                )
                .subcommand(
                    Command::new("send")
                        .about("Send MTWS data immediately (one-time)")
                )
                .subcommand(
                    Command::new("test")
                        .about("Test MTWS endpoint connectivity")
                )
                .subcommand(
                    Command::new("status")
                        .about("Show MTWS service status and configuration")
                )
                .subcommand(
                    Command::new("enable")
                        .about("Enable MTWS service")
                )
                .subcommand(
                    Command::new("disable")
                        .about("Disable MTWS service")
                )
        )
        // Add missing subcommands:
        .subcommand(
            Command::new("getrawdata")
                .about("Get raw data from device")
                .arg(Arg::new("device")
                    .help("Device address")
                    .required(true)
                    .index(1))
                .arg(Arg::new("format")
                    .long("format")
                    .help("Output format")
                    .value_parser(["hex", "raw", "json"])
                    .default_value("hex"))
                .arg(Arg::new("output")
                    .long("output")
                    .help("Output file path")
                    .value_name("FILE"))
        )
        .subcommand(
            Command::new("flowmeter")
                .about("Flowmeter device operations")
                .subcommand(
                    Command::new("query")
                        .about("Query flowmeter data")
                        .arg(Arg::new("device")
                            .help("Device address")
                            .required(true)
                            .index(1))
                        .arg(Arg::new("limit")
                            .help("Number of records")
                            .default_value("10"))
                )
                .subcommand(
                    Command::new("stats")
                        .about("Show flowmeter statistics")
                )
                .subcommand(
                    Command::new("recent")
                        .about("Show recent flowmeter readings")
                        .arg(Arg::new("limit")
                            .help("Number of records")
                            .default_value("20"))
                )
        )
        .subcommand(
            Command::new("rpm")
                .about("RPM device operations")
                .subcommand(
                    Command::new("read")
                        .about("Read RPM device data")
                        .arg(Arg::new("address")
                            .help("Device address")
                            .required(true))
                        .arg(Arg::new("channel")
                            .help("Specific channel (optional)")
                            .long("channel"))
                )
                .subcommand(
                    Command::new("status")
                        .about("Show RPM device status")
                )
        )
        .subcommand(
            Command::new("engine")
                .about("Engine management commands")
                .subcommand(
                    Command::new("duration")
                        .about("Engine duration commands")
                        .subcommand(
                            Command::new("reset")
                                .about("Reset engine duration")
                                .arg(Arg::new("address")
                                    .help("Engine address")
                                    .required(true))
                        )
                )
        )
}


#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init();

    let matches = build_cli().get_matches(); 

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
        let handled = crate::config::config_commands::handle_config_commands(config_matches, &config_manager).await?;
        
        if handled {
            return Ok(());
        }
    }

    // ✅ ENHANCED: Config loading with proper TOML precedence
    let config_file = matches.get_one::<String>("config-file").unwrap();
    
    info!("🔍 Loading configuration from: {}", config_file);
    
    let config = if std::path::Path::new(config_file).exists() {
        info!("📁 Loading config from existing file: {}", config_file);
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
                info!("🔄 Using default configuration and saving it");
                let default_config = Config::default();
                
                // Try to save default config
                if let Err(save_err) = default_config.save_to_file(config_file) {
                    info!("⚠️  Failed to save default config: {}", save_err);
                } else {
                    info!("💾 Saved default config to: {}", config_file);
                }
                
                default_config
            }
        }
    } else {
        info!("📁 Config file not found, creating from CLI args and defaults");
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
            info!("⚠️  Failed to save initial config file: {}", e);
        } else {
            info!("💾 Created initial config file: {}", config_file);
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

    let mut service = DataService::new(config.clone()).await?;
    let mut api_service_handle: Option<crate::services::ApiService> = None;

    // ✅ FIXED: Start API service based on TOML config, not just CLI
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
            if let Some(mut api_service) = api_service_handle {
                api_service.stop().await?;
            }
            return Ok(());
        }
    }

    // Handle other subcommands
    if handle_subcommands(&matches, &mut service).await? {
        // Stop services before returning
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

    // Configure output format if specified
    if let Some(format) = matches.get_one::<String>("format") {
        match format.as_str() {
            "json" => {
                info!("🎨 Using JSON formatter");
                service.set_formatter(Box::new(crate::output::JsonFormatter));
            }
            "csv" => {
                info!("🎨 Using CSV formatter");
                service.set_formatter(Box::new(crate::output::CsvFormatter));
            }
            "hex" => {
                info!("🔍 Using Hex formatter");
                service.set_formatter(Box::new(crate::output::HexFormatter));
            }
            _ => {} // Keep default console formatter
        }
    }

    if let Some(output_file) = matches.get_one::<String>("output-file") {
        info!("📝 Adding file output: {}", output_file);
        service.add_sender(Box::new(crate::output::FileSender::new(output_file, true)));
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
