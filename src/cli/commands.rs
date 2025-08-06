use clap::ArgMatches;
use log::{info};
use anyhow::{Result, anyhow};

use crate::services::DataService;
use crate::output::{JsonFormatter, CsvFormatter, FileSender};

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

    // NEW: Handle GPS commands
    if let Some(matches) = matches.subcommand_matches("gps") {
        return handle_gps_commands(matches, service).await;
    }

    // Handle subcommands
    if let Some(_matches) = matches.subcommand_matches("getdata") {
        info!("🔍 Executing getdata command...");
        service.read_all_devices_once().await?;
        service.print_all_device_data().await?;
        
        return Ok(true);
    }

    if let Some(matches) = matches.subcommand_matches("getvolatile") {
        info!("📈 Executing getvolatile command...");
        
        service.read_all_devices_once().await?;
        
        let parameter = matches.get_one::<String>("parameter").unwrap();
        let value = service.get_volatile_data(parameter);
        
        if value.is_empty() {
            println!("❌ No data found for parameter: {}", parameter);
            println!("💡 Available parameters: MassFlowRate, Temperature, DensityFlow, VolumeFlowRate, MassTotal, VolumeTotal, MassInventory, VolumeInventory, ErrorCode");
        } else {
            println!("📈 {}: {}", parameter, value);
        }
        
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

    // Handle compare-raw command
    if let Some(matches) = matches.subcommand_matches("compare-raw") {
        info!("🔍 Executing compare-raw command...");
        
        let device_address: u8 = matches.get_one::<String>("device").unwrap().parse()
            .map_err(|_| anyhow!("Invalid device address"))?;
        
        service.compare_raw_vs_processed(device_address).await?;
        
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

    // Handle flowmeter commands
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
