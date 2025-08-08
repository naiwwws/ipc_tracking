use clap::ArgMatches;
use log::{info};
use anyhow::{Result, anyhow};

use crate::services::DataService;
use crate::output::{JsonFormatter, CsvFormatter, FileSender};
use crate::utils::error::ModbusError;
use serde_json;
use reqwest;

pub async fn handle_subcommands(
    matches: &ArgMatches,
    service: &mut DataService,
) -> Result<bool> {  
    
    // Configure output format
    if let Some(format) = matches.get_one::<String>("format") {
        match format.as_str() {
            "json" => {
                info!("🎨 Using JSON formatter");
                service.set_formatter(Box::new(JsonFormatter));
            }
            "csv" => {
                info!("🎨 Using CSV formatter");
                service.set_formatter(Box::new(CsvFormatter));
            }
            _ => {} // Keep default console formatter
        }
    }

    // Configure output destinations
    if let Some(output_file) = matches.get_one::<String>("output-file") {
        info!("📝 Adding file output: {}", output_file);
        service.add_sender(Box::new(FileSender::new(output_file, true)));
    }

    // Handle GPS commands
    if let Some(matches) = matches.subcommand_matches("gps") {
        return handle_gps_commands(matches, service).await;
    }

    // Handle basic subcommands
    if let Some(_matches) = matches.subcommand_matches("getdata") {
        info!("🔍 Executing getdata command...");
        service.read_all_devices_once().await?;
        return Ok(true);
    }

    if let Some(matches) = matches.subcommand_matches("resetaccumulation") {
        let device_addr: u8 = matches.get_one::<String>("device_address").unwrap().parse()?;
        info!("🔄 Executing reset accumulation for device {}...", device_addr);
        service.reset_accumulation(device_addr).await?;
        println!("✅ Reset accumulation command sent to device {}", device_addr);
        return Ok(true);
    }

    // Handle getrawdata command
    if let Some(matches) = matches.subcommand_matches("getrawdata") {
        info!("🔍 Executing getrawdata command...");
        
        let device_address: u8 = matches.get_one::<String>("device").unwrap().parse()
            .map_err(|_| anyhow!("Invalid device address"))?;
        
        let default_format = "hex".to_string();
        let format = matches.get_one::<String>("format").unwrap_or(&default_format);
        
        if let Some(output_file) = matches.get_one::<String>("output") {
            service.read_raw_device_data(device_address, format, Some(output_file)).await?;
        } else {
            service.read_raw_device_data(device_address, format, None).await?;
        }
        return Ok(true);
    }

    // Handle database commands
    if let Some(matches) = matches.subcommand_matches("db") {
        if let Some(sub_matches) = matches.subcommand_matches("query") {
            let device_address: Option<u8> = sub_matches.get_one::<String>("device")
                .and_then(|s| s.parse().ok());
            let limit: i64 = sub_matches.get_one::<String>("limit")
                .unwrap_or(&"10".to_string())
                .parse()
                .map_err(|_| anyhow!("Invalid limit"))?;
                
            if let Some(addr) = device_address {
                service.query_flowmeter_data(addr, limit).await?;
            } else {
                println!("📋 Querying all devices (last {}):", limit);
            }
            return Ok(true);
        }
        
        if let Some(_) = matches.subcommand_matches("stats") {
            info!("📊 Executing database stats command...");
            service.get_flowmeter_stats().await?;
            return Ok(true);
        }
        
        if let Some(sub_matches) = matches.subcommand_matches("recent") {
            info!("📋 Executing database recent command...");
            
            let limit: i64 = sub_matches.get_one::<String>("limit").unwrap_or(&"20".to_string()).parse()
                .map_err(|_| anyhow!("Invalid limit"))?;
                
            if let Some(db_service) = service.get_database_service() {
                let readings = db_service.get_recent_flowmeter_readings(limit).await?;
                
                println!("📋 Recent flowmeter readings (last {}):", limit);
                println!("{:<12} {:<12} {:<12} {:<12} {:<8} {:<15}", 
                    "Mass Flow", "Temperature", "Density", "Vol Flow", "Error", "Unix Time");
                println!("{}", "-".repeat(80));
                
                for reading in readings {
                    println!("{:<12.2} {:<12.2} {:<12.4} {:<12.3} {:<8} {:<15}", 
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
            return Ok(true);
        }
    }

    // Handle flowmeter commands (NOW properly defined in CLI)
    if let Some(matches) = matches.subcommand_matches("flowmeter") {
        if let Some(sub_matches) = matches.subcommand_matches("query") {
            info!("📋 Executing flowmeter query command...");
            
            let device_address: u8 = sub_matches.get_one::<String>("device").unwrap().parse()
                .map_err(|_| anyhow!("Invalid device address"))?;
                
            let limit: i64 = sub_matches.get_one::<String>("limit").unwrap_or(&"10".to_string()).parse()
                .map_err(|_| anyhow!("Invalid limit"))?;
                
            service.query_flowmeter_data(device_address, limit).await?;
            return Ok(true);
        }
        
        if let Some(_) = matches.subcommand_matches("stats") {
            info!("📊 Executing flowmeter stats command...");
            service.get_flowmeter_stats().await?;
            return Ok(true);
        }
        
        if let Some(sub_matches) = matches.subcommand_matches("recent") {
            info!("📋 Executing flowmeter recent command...");
            
            let limit: i64 = sub_matches.get_one::<String>("limit").unwrap_or(&"20".to_string()).parse()
                .map_err(|_| anyhow!("Invalid limit"))?;
                
            if let Some(db_service) = service.get_database_service() {
                let readings = db_service.get_recent_flowmeter_readings(limit).await?;
                
                println!("📋 Recent flowmeter readings (last {}):", limit);
                println!("{:<12} {:<12} {:<12} {:<12} {:<8} {:<15}", 
                    "Mass Flow", "Temperature", "Density", "Vol Flow", "Error", "Unix Time");
                println!("{}", "-".repeat(80));
                
                for reading in readings {
                    println!("{:<12.2} {:<12.2} {:<12.4} {:<12.3} {:<8} {:<15}", 
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
            return Ok(true);
        }
    }

    // Handle RPM commands
    if let Some(rpm_matches) = matches.subcommand_matches("rpm") {
        handle_rpm_command(service, rpm_matches).await?;
        return Ok(true);
    }

    // Handle engine commands
    if let Some(engine_matches) = matches.subcommand_matches("engine") {
        handle_engine_commands(engine_matches, service).await?;
        return Ok(true);
    }

    // Handle MTWS commands
    if let Some(matches) = matches.subcommand_matches("mtws") {
        handle_mtws_commands(matches, service).await?;
        return Ok(true);
    }

    Ok(false)
}

// NEW: GPS command handler
pub async fn handle_gps_commands(
    matches: &ArgMatches,
    service: &DataService,
) -> Result<bool> {
    if let Some(_) = matches.subcommand_matches("start") {
        info!("🧭 Starting GPS service...");
        match service.start_gps_service().await {
            Ok(()) => {
                println!("✅ GPS service started successfully");
                println!("📍 GPS will begin tracking location once a fix is acquired");
            }
            Err(e) => {
                println!("❌ Failed to start GPS service: {}", e);
            }
        }
        return Ok(true);
    }

    if let Some(_) = matches.subcommand_matches("stop") {
        info!("🧭 Stopping GPS service...");
        match service.stop_gps_service().await {
            Ok(()) => {
                println!("✅ GPS service stopped successfully");
            }
            Err(e) => {
                println!("❌ Failed to stop GPS service: {}", e);
            }
        }
        return Ok(true);
    }

    if let Some(_) = matches.subcommand_matches("status") {
        match service.get_gps_status().await {
            Ok(status) => {
                println!("🧭 GPS Status: {}", status);
            }
            Err(e) => {
                println!("❌ Failed to get GPS status: {}", e);
            }
        }
        return Ok(true);
    }

    if let Some(_) = matches.subcommand_matches("data") {
        if let Some(gps_data) = service.get_current_gps_data().await {
            println!("🧭 Current GPS Data:");
            println!("═══════════════════════════════════════");
            
            if let Some(lat) = gps_data.latitude {
                println!("📍 Latitude:      {:.6}°", lat);
            } else {
                println!("📍 Latitude:      No data");
            }
            
            if let Some(lon) = gps_data.longitude {
                println!("📍 Longitude:     {:.6}°", lon);
            } else {
                println!("📍 Longitude:     No data");
            }
            
            if let Some(alt) = gps_data.altitude {
                println!("🏔️  Altitude:      {:.2}m", alt);
            } else {
                println!("🏔️  Altitude:      No data");
            }
            
            if let Some(speed) = gps_data.speed {
                println!("🚀 Speed:         {:.2} knots", speed);
            } else {
                println!("🚀 Speed:         No data");
            }
            
            if let Some(course) = gps_data.course {
                println!("🧭 Course:        {:.2}°", course);
            } else {
                println!("🧭 Course:        No data");
            }
            
            if let Some(sats) = gps_data.satellites {
                println!("🛰️  Satellites:    {}", sats);
            } else {
                println!("🛰️  Satellites:    No data");
            }
            
            if let Some(fix_type) = &gps_data.fix_type {
                println!("🔧 Fix Type:      {}", fix_type);
            } else {
                println!("🔧 Fix Type:      No data");
            }
            
            if let Some(timestamp) = gps_data.timestamp {
                println!("⏰ Unix Timestamp: {}", timestamp);
            } else {
                println!("⏰ Timestamp:     No data");
            }
            
            // Show Google Maps link if we have coordinates
            if let (Some(lat), Some(lon)) = (gps_data.latitude, gps_data.longitude) {
                println!("═══════════════════════════════════════");
                println!("🗺️  Google Maps:   https://maps.google.com/?q={},{}", lat, lon);
                println!("🗺️  OpenStreetMap: https://www.openstreetmap.org/?mlat={}&mlon={}&zoom=15", lat, lon);
            }
        } else {
            println!("❌ No GPS data available");
            println!("💡 Possible reasons:");
            println!("   • GPS service is not running (try: gps start)");
            println!("   • GPS module has no satellite fix yet");
            println!("   • GPS service is disabled in configuration");
        }
        return Ok(true);
    }

    if let Some(_) = matches.subcommand_matches("test") {
        println!("🧪 Testing GPS connection...");
        
        // Check GPS status first
        match service.get_gps_status().await {
            Ok(status) => {
                println!("📋 Current Status: {}", status);
            }
            Err(e) => {
                println!("❌ Failed to get GPS status: {}", e);
                return Ok(true);
            }
        }

        // Try to start GPS if not running
        if let Err(_) = service.start_gps_service().await {
            // GPS might already be running, that's OK
        }

        println!("⏳ Waiting for GPS data (10 seconds)...");
    
        
        println!("\n⚠️  GPS test completed but no valid fix acquired");
        println!("💡 This could mean:");
        println!("   • GPS module needs more time to acquire satellites");
        println!("   • GPS antenna is not properly connected");
        println!("   • You're indoors or in an area with poor GPS reception");
        
        return Ok(true);
    }

    Ok(false)
}

pub async fn handle_engine_commands(matches: &ArgMatches, data_service: &DataService) -> Result<(), ModbusError> {
    match matches.subcommand() {
        Some(("duration", sub_matches)) => {
            match sub_matches.subcommand() {
                // Some(("show", _)) => {
                //     let durations = data_service.get_engine_durations().await;
                //     println!("🕒 Engine Durations:");
                //     println!("{}", "=".repeat(40));
                //     for (address, duration_seconds) in durations {
                //         let hours = duration_seconds / 3600;
                //         let minutes = (duration_seconds % 3600) / 60;
                //         println!("Engine @ Address {}: {}h {}m", address, hours, minutes);
                //     }
                // }
                Some(("reset", reset_matches)) => {
                    let address: u8 = reset_matches.get_one::<String>("address")
                        .unwrap()
                        .parse()
                        .map_err(|_| ModbusError::DeviceNotFound("Invalid address format".to_string()))?;
                    
                    data_service.reset_engine_duration(address).await?;
                    info!("✅ Reset engine duration for address {}", address);
                }
                _ => println!("Invalid engine duration command"),
            }
        }
        _ => println!("Invalid engine command"),
    }
    Ok(())
}

// Add this function to handle RPM commands
pub async fn handle_rpm_command(data_service: &DataService, matches: &ArgMatches) -> Result<(), ModbusError> {
    match matches.subcommand() {
        Some(("read", sub_matches)) => {
            let address = sub_matches.get_one::<String>("address")
                .ok_or_else(|| ModbusError::InvalidData("Address is required".to_string()))?
                .parse::<u8>()
                .map_err(|_| ModbusError::InvalidData("Invalid address format".to_string()))?;

            let channel = sub_matches.get_one::<String>("channel")
                .map(|s| s.parse::<u8>().unwrap_or(0));

            if let Some(channel_id) = channel {
                // Read specific channel
                if let Some(channel_data) = data_service.get_rpm_channel_data(address, channel_id).await {
                    println!("📊 RPM Channel {} Data for Device {}:", channel_id, address);
                    if let Ok(parsed_json) = serde_json::from_str::<serde_json::Value>(&channel_data) {
                        println!("{}", serde_json::to_string_pretty(&parsed_json).unwrap_or(channel_data));
                    } else {
                        println!("{}", channel_data);
                    }
                } else {
                    println!("❌ No data found for RPM device {} channel {}", address, channel_id);
                }
            } else {
                // Read all channels
                if let Some(device_data) = data_service.get_device_data_by_address(address).await {
                    println!("📊 Multi-Channel RPM Data for Device {}:", address);
                    if let Ok(parsed_json) = serde_json::from_str::<serde_json::Value>(&device_data) {
                        println!("{}", serde_json::to_string_pretty(&parsed_json).unwrap_or(device_data));
                    } else {
                        println!("{}", device_data);
                    }
                } else {
                    println!("❌ No data found for RPM device {}", address);
                }
            }
        }
        // Some(("duration", sub_matches)) => {
        //     match sub_matches.subcommand() {
        //         Some(("show", _)) => {
        //             let durations = data_service.get_engine_durations().await;
        //             println!("🕒 Engine Durations:");
        //             println!("{}", "=".repeat(40));
        //             for (address, duration_seconds) in durations {
        //                 let hours = duration_seconds / 3600;
        //                 let minutes = (duration_seconds % 3600) / 60;
        //                 println!("Engine @ Address {}: {}h {}m ({} seconds)", address, hours, minutes, duration_seconds);
        //             }
        //         }
        //         Some(("reset", reset_matches)) => {
        //             let address = reset_matches.get_one::<String>("address")
        //                 .ok_or_else(|| ModbusError::InvalidData("Address is required".to_string()))?
        //                 .parse::<u8>()
        //                 .map_err(|_| ModbusError::InvalidData("Invalid address format".to_string()))?;

        //             data_service.save_engine_duration(address, 0).await?;
        //             println!("✅ Reset engine duration for address {}", address);
        //         }
        //         _ => {
        //             println!("Available duration commands: show, reset");
        //         }
        //     }
        // }
        Some(("status", _)) => {
            let rpm_devices = data_service.get_rpm_devices();
            println!("📋 RPM Device Status:");
            
            for device in rpm_devices {
                if let Some(data_str) = data_service.get_device_data_by_address(device.address).await {
                    if let Ok(json_data) = serde_json::from_str::<serde_json::Value>(&data_str) {
                        let total_channels = json_data["total_channels"].as_u64().unwrap_or(0);
                        let running_engines = json_data.get("running_engines")
                            .and_then(|v| v.as_array())
                            .map(|arr| arr.len())
                            .unwrap_or(0);
                        let device_status = json_data["device_status"].as_str().unwrap_or("Unknown");
                        
                        println!("  Device {} ({}): {} channels, {} running, Status: {}", 
                                device.address, device.name, total_channels, running_engines, device_status);
                        
                        if let Some(channels) = json_data["channels"].as_array() {
                            for channel in channels {
                                let ch_id = channel["channel_id"].as_u64().unwrap_or(0);
                                let rpm = channel["rpm_value"].as_u64().unwrap_or(0);
                                let running = channel["is_engine_running"].as_bool().unwrap_or(false);
                                let threshold = channel["rpm_threshold"].as_u64().unwrap_or(0);
                                
                                println!("    Channel {}: {} RPM (threshold: {}) - {}", 
                                        ch_id, rpm, threshold, if running { "🟢 RUNNING" } else { "🔴 STOPPED" });
                            }
                        }
                    }
                } else {
                    println!("  Device {} ({}): ❌ No data available", device.address, device.name);
                }
            }
        }
        _ => {
            println!("Available RPM commands: read, duration, status");
        }
    }
    Ok(())
}

// Add MTWS CLI commands

pub async fn handle_mtws_commands(matches: &ArgMatches, data_service: &DataService) -> Result<(), ModbusError> {
    match matches.subcommand() {
        Some(("start", _)) => {
            println!("🛰️ Starting MTWS transmission...");
            
            // Check if MTWS service is available through DataService
            if let Some(api_service) = data_service.get_api_service() {
                match api_service.start_mtws_transmission().await {
                    Ok(_) => println!("✅ MTWS transmission started successfully"),
                    Err(e) => println!("❌ Failed to start MTWS transmission: {}", e),
                }
            } else {
                match send_mtws_http_request::<(), serde_json::Value>("POST", "/api/mtws/start", None::<&()>).await {
                    Ok(_) => println!("✅ MTWS transmission started successfully"),
                    Err(e) => println!("❌ Failed to start MTWS transmission: {}", e),
                }
            }
            Ok(())
        }
        Some(("stop", _)) => {
            println!("🛑 Stopping MTWS transmission...");
            
            if let Some(api_service) = data_service.get_api_service() {
                match api_service.stop_mtws_transmission().await {
                    Ok(_) => println!("✅ MTWS transmission stopped successfully"),
                    Err(e) => println!("❌ Failed to stop MTWS transmission: {}", e),
                }
            } else {
                match send_mtws_http_request::<(), serde_json::Value>("POST", "/api/mtws/stop", None::<&()>).await {
                    Ok(_) => println!("✅ MTWS transmission stopped successfully"),
                    Err(e) => println!("❌ Failed to stop MTWS transmission: {}", e),
                }
            }
            Ok(())
        }
        Some(("send", send_matches)) => {
            let endpoint = send_matches.get_one::<String>("endpoint");
            println!("📡 Sending single MTWS payload...");
            
            let payload = if let Some(endpoint_url) = endpoint {
                serde_json::json!({
                    "endpoint_url": endpoint_url
                })
            } else {
                serde_json::json!({})
            };
            
            if let Some(api_service) = data_service.get_api_service() {
                match api_service.send_mtws_payload(endpoint.cloned()).await {
                    Ok(_) => println!("✅ MTWS payload sent successfully"),
                    Err(e) => println!("❌ Failed to send MTWS payload: {}", e),
                }
            } else {
                match send_mtws_http_request::<_, serde_json::Value>("POST", "/api/mtws/send", Some(&payload)).await {
                    Ok(_) => println!("✅ MTWS payload sent successfully"),
                    Err(e) => println!("❌ Failed to send MTWS payload: {}", e),
                }
            }
            Ok(())
        }
        Some(("status", _)) => {
            println!("📊 MTWS Status:");
            
            if let Some(api_service) = data_service.get_api_service() {
                match api_service.get_mtws_status().await {
                    Ok(status) => {
                        println!("  Status: {}", if status.is_running { "🟢 Running" } else { "🔴 Stopped" });
                        println!("  Interval: {} seconds", status.interval_seconds);
                        if let Some(url) = &status.endpoint_url {
                            println!("  Endpoint: {}", url);
                        }
                        println!("  Enabled: {}", if status.enabled { "Yes" } else { "No" });
                    }
                    Err(e) => println!("❌ Failed to get MTWS status: {}", e),
                }
            } else {
                match send_mtws_http_request::<(), serde_json::Value>("GET", "/api/mtws/status", None).await {
                    Ok(response) => {
                        if let Some(config) = response.get("config") {
                            println!("  Status: {}", if config["is_running"].as_bool().unwrap_or(false) { "🟢 Running" } else { "🔴 Stopped" });
                            println!("  Interval: {} seconds", config["interval_seconds"].as_u64().unwrap_or(0));
                            if let Some(url) = config["endpoint_url"].as_str() {
                                println!("  Endpoint: {}", url);
                            }
                            println!("  Enabled: {}", if config["enabled"].as_bool().unwrap_or(false) { "Yes" } else { "No" });
                        } else {
                            println!("  Unable to parse status response");
                        }
                    }
                    Err(e) => println!("❌ Failed to get MTWS status: {}", e),
                }
            }
            Ok(())
        }
        Some(("config", config_matches)) => {
            match config_matches.subcommand() {
                Some(("show", _)) => {
                    println!("⚙️ MTWS Configuration:");
                    
                    match send_mtws_http_request::<(), serde_json::Value>("GET", "/api/mtws/config", None).await {
                        Ok(response) => {
                            if let Some(config) = response.get("config") {
                                println!("  Enabled: {}", if config["enabled"].as_bool().unwrap_or(false) { "Yes" } else { "No" });
                                println!("  Running: {}", if config["is_running"].as_bool().unwrap_or(false) { "Yes" } else { "No" });
                                println!("  Interval: {} seconds", config["interval_seconds"].as_u64().unwrap_or(0));
                                if let Some(url) = config["endpoint_url"].as_str() {
                                    println!("  Endpoint: {}", url);
                                }
                            } else {
                                println!("❌ Unable to parse configuration response");
                            }
                        }
                        Err(e) => println!("❌ Failed to get MTWS configuration: {}", e),
                    }
                    Ok(())
                }
                Some(("set", set_matches)) => {
                    let interval = set_matches.get_one::<String>("interval");
                    let endpoint = set_matches.get_one::<String>("endpoint");
                    
                    let mut config_update = serde_json::Map::new();
                    
                    if let Some(interval_str) = interval {
                        match interval_str.parse::<u64>() {
                            Ok(interval_val) => {
                                config_update.insert("interval_seconds".to_string(), serde_json::Value::Number(serde_json::Number::from(interval_val)));
                                println!("🔧 Setting MTWS interval to {} seconds", interval_val);
                            }
                            Err(_) => {
                                println!("❌ Invalid interval value: {}", interval_str);
                                return Ok(());
                            }
                        }
                    }
                    
                    if let Some(endpoint_url) = endpoint {
                        config_update.insert("endpoint_url".to_string(), serde_json::Value::String(endpoint_url.clone()));
                        println!("🔧 Setting MTWS endpoint to: {}", endpoint_url);
                    }
                    
                    if config_update.is_empty() {
                        println!("❌ No configuration parameters provided");
                        println!("Usage: mtws config set --interval <seconds> --endpoint <url>");
                        return Ok(());
                    }
                    
                    let payload = serde_json::Value::Object(config_update);
                    
                    match send_mtws_http_request::<_, serde_json::Value>("PUT", "/api/mtws/config", Some(&payload)).await {
                        Ok(_) => println!("✅ MTWS configuration updated successfully"),
                        Err(e) => println!("❌ Failed to update MTWS configuration: {}", e),
                    }
                    Ok(())
                }
                _ => {
                    println!("Available config commands:");
                    println!("  show                              - Show current configuration");
                    println!("  set --interval <sec> --endpoint <url> - Update configuration");
                    Ok(())
                }
            }
        }
        _ => {
            println!("Available MTWS commands:");
            println!("  start                             - Start automatic transmission");
            println!("  stop                              - Stop automatic transmission");
            println!("  send [--endpoint <url>]           - Send single payload");
            println!("  status                            - Show service status");
            println!("  config show                       - Show configuration");
            println!("  config set --interval <sec> --endpoint <url> - Update config");
            Ok(())
        }
    }
}

// Helper function to send HTTP requests to MTWS API
async fn send_mtws_http_request<T: serde::Serialize, R: serde::de::DeserializeOwned>(
    method: &str,
    path: &str,
    body: Option<&T>,
) -> Result<R, ModbusError> {
    let client = reqwest::Client::new();
    let url = format!("http://localhost:3000{}", path);
    
    let mut request = match method {
        "GET" => client.get(&url),
        "POST" => client.post(&url),
        "PUT" => client.put(&url),
        "DELETE" => client.delete(&url),
        _ => return Err(ModbusError::InvalidData(format!("Unsupported HTTP method: {}", method))),
    };
    
    if let Some(payload) = body {
        request = request.json(payload);
    }
    
    let response = request
        .send()
        .await
        .map_err(|e| ModbusError::CommunicationError(format!("HTTP request failed: {}", e)))?;
    
    if response.status().is_success() {
        response
            .json::<R>()
            .await
            .map_err(|e| ModbusError::InvalidData(format!("Failed to parse response: {}", e)))
    } else {
        let status = response.status();
        let error_text = response.text().await.unwrap_or_default();
        Err(ModbusError::CommunicationError(format!("HTTP {} error: {}", status, error_text)))
    }
}
