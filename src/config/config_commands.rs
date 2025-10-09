use clap::ArgMatches;
use std::collections::HashMap;
use chrono::Utc;
use uuid::Uuid;

use crate::config::dynamic_manager::{DynamicConfigManager, ConfigurationCommand, ConfigCommandType, ConfigTarget};

pub async fn handle_config_commands(
    matches: &ArgMatches,
    config_manager: &DynamicConfigManager,
) -> Result<bool, Box<dyn std::error::Error>> {

    // Handle show command
    if let Some(_show_matches) = matches.subcommand_matches("show") {
        return handle_show_command(config_manager).await;
    }

    // Handle IPC-specific commands
    if let Some(ipc_matches) = matches.subcommand_matches("ipc") {
        return handle_ipc_command(ipc_matches, config_manager).await;
    }

    // Handle set-interval command
    if let Some(interval_matches) = matches.subcommand_matches("set-interval") {
        return handle_set_interval_command(interval_matches, config_manager).await;
    }

    // Handle set command
    if let Some(set_matches) = matches.subcommand_matches("set") {
        return handle_set_command(set_matches, config_manager).await;
    }

    // Handle RPM-specific commands
    if let Some(rpm_matches) = matches.subcommand_matches("rpm") {
        return handle_rpm_config_commands(rpm_matches, config_manager).await;
    }

    // Enhanced add command with RPM support
    if let Some(add_matches) = matches.subcommand_matches("add") {
        return handle_enhanced_add_command(add_matches, config_manager).await;
    }

    // Handle enable command
    if let Some(enable_matches) = matches.subcommand_matches("enable") {
        return handle_enable_command(enable_matches, config_manager).await;
    }

    // Handle disable command
    if let Some(disable_matches) = matches.subcommand_matches("disable") {
        return handle_disable_command(disable_matches, config_manager).await;
    }

    // Handle remove command
    if let Some(remove_matches) = matches.subcommand_matches("remove") {
        return handle_remove_command(remove_matches, config_manager).await;
    }

    // Handle backup command
    if let Some(backup_matches) = matches.subcommand_matches("backup") {
        return handle_backup_command(backup_matches, config_manager).await;
    }

    // Handle restore command
    if let Some(restore_matches) = matches.subcommand_matches("restore") {
        return handle_restore_command(restore_matches, config_manager).await;
    }

    // Handle reset command
    if let Some(_reset_matches) = matches.subcommand_matches("reset") {
        return handle_reset_command(config_manager).await;
    }

    Ok(false)
}

// ═══════════════════════════════════════════════════════════════════
// ALL COMMAND HANDLERS IN ONE FILE - CONSISTENT AND MAINTAINABLE
// ═══════════════════════════════════════════════════════════════════

// Handle show command - displays current configuration
async fn handle_show_command(
    config_manager: &DynamicConfigManager,
) -> Result<bool, Box<dyn std::error::Error>> {
    let config = config_manager.get_current_config().await;
    
    println!("📋 Industrial PC Configuration:");
    println!("═══════════════════════════════════════");
    println!("🏭 IPC Information:");
    println!("   🆔 UUID: {}", config.get_ipc_uuid());
    println!("   🏷️  Name: {}", config.get_ipc_name());
    println!("   📦 Version: {}", config.get_ipc_version());
    
    println!("\n🏢 Site Information:");
    println!("   🆔 Site ID: {}", config.site_info.site_id);
    println!("   🏷️  Site Name: {}", config.site_info.site_name);
    println!("   📍 Location: {}", config.site_info.location);
    println!("   👤 Operator: {}", config.site_info.operator);
    println!("   📧 Contact: {}", config.site_info.contact_email);
    
    println!("\n🔌 Communication Settings:");
    println!("   📡 Serial Port: {} @ {} baud", config.serial_port, config.baud_rate);
    println!("   🔧 Parity: {:?}", config.parity);
    
    println!("\n⏱️  Monitoring Settings:");
    println!("   🔄 Polling Interval: {} seconds", config.update_interval_seconds);
    println!("   🔁 Max Retries: {}", config.max_retries);
    println!("   ⏳ Retry Delay: {} ms", config.retry_delay_ms);

    // NEW: GPS Configuration Information
    println!("\n🧭 GPS Configuration:");
    println!("═══════════════════════════════════════");
    let gps_status = if config.gps.enabled { "✅ ENABLED" } else { "❌ DISABLED" };
    println!("   📡 Status: {}", gps_status);
    println!("   🔌 Port: {}", config.gps.port);
    println!("   📶 Baud Rate: {}", config.gps.baud_rate);
    println!("   🚀 Auto Start: {}", if config.gps.auto_start { "Yes" } else { "No" });

    println!("\n📡 Devices ({}):", config.devices.len());
    println!("═══════════════════════════════════════════════════════════");
    for device in &config.devices {
        let status = if device.enabled { "" } else { "❌" };
        println!("🏷️  Name: {}", device.name);
        println!("🆔 Device UUID: {}", device.uuid);
        println!("📡 Address: {} | Type: {} | Status: {}", 
                 device.address, device.device_type, status);
        println!("📍 Location: {}", device.location);
        
        if !device.parameters.is_empty() {
            println!("📊 Parameters: {}", device.parameters.join(", "));
        }
        
        if !device.metadata.is_empty() {
            println!("🏷️  Metadata:");
            for (key, value) in &device.metadata {
                println!("   • {}: {}", key, value);
            }
        }
        
        if let Some(interval) = device.polling_interval {
            println!("⏱️  Custom Polling: {} seconds", interval);
        }
        println!("───────────────────────────────────────────────────────────");
    }

    // ✅ ADD: API Server Information if missing
    println!("\n🌐 API Server Configuration:");
    println!("═══════════════════════════════════════");
    let api_status = if config.api_server.enabled { "✅ ENABLED" } else { "❌ DISABLED" };
    println!("   📡 Status: {}", api_status);
    println!("   🔌 Port: {}", config.api_server.port);
    println!("   🏠 Host: {}", config.api_server.host);
    println!("   🔒 CORS: {}", if config.api_server.cors_enabled { "Enabled" } else { "Disabled" });
    println!("   🌍 Origins: {}", config.api_server.cors_origins.join(", "));
    Ok(true)
}

// Handle IPC configuration commands (set-name, regenerate-uuid)
async fn handle_ipc_command(
    matches: &ArgMatches,
    config_manager: &DynamicConfigManager,
) -> Result<bool, Box<dyn std::error::Error>> {
    
    if let Some(set_matches) = matches.subcommand_matches("set-name") {
        let name = set_matches.get_one::<String>("name").unwrap();
        let operator = set_matches.get_one::<String>("operator").unwrap_or(&"CLI".to_string()).clone();

        let mut parameters = HashMap::new();
        parameters.insert("ipc_name".to_string(), name.clone());

        let command = ConfigurationCommand {
            command_id: Uuid::new_v4().to_string(),
            timestamp: Utc::now(),
            operator,
            command_type: ConfigCommandType::Set,
            target: ConfigTarget::System,
            parameters,
            apply_immediately: true,
        };

        let response = config_manager.execute_command(command).await;
        
        if response.success {
            println!(" IPC name updated to: {}", name);
        } else {
            println!("❌ Failed to update IPC name: {}", response.message);
        }

        return Ok(true);
    }

    if let Some(_regenerate_matches) = matches.subcommand_matches("regenerate-uuid") {
        println!("⚠️  This will generate a new UUID for this IPC. Continue? (yes/no)");
        
        let mut input = String::new();
        std::io::stdin().read_line(&mut input)?;
        
        if input.trim().to_lowercase() != "yes" {
            println!("❌ Operation cancelled");
            return Ok(true);
        }

        let command = ConfigurationCommand {
            command_id: Uuid::new_v4().to_string(),
            timestamp: Utc::now(),
            operator: "CLI".to_string(),
            command_type: ConfigCommandType::Set,
            target: ConfigTarget::System,
            parameters: {
                let mut params = HashMap::new();
                params.insert("regenerate_ipc_uuid".to_string(), "true".to_string());
                params
            },
            apply_immediately: true,
        };

        let response = config_manager.execute_command(command).await;
        
        if response.success {
            println!(" New IPC UUID generated");
            if response.requires_restart {
                println!("⚠️  Service restart required to apply new UUID");
            }
        } else {
            println!("❌ Failed to regenerate UUID: {}", response.message);
        }

        return Ok(true);
    }

    Ok(false)
}

// Handle set-interval command - sets polling interval
async fn handle_set_interval_command(
    matches: &ArgMatches,
    config_manager: &DynamicConfigManager,
) -> Result<bool, Box<dyn std::error::Error>> {
    let interval: u64 = *matches.get_one::<u64>("seconds").unwrap();
    let operator = matches.get_one::<String>("operator").unwrap_or(&"CLI".to_string()).clone();

    // Validate interval range
    if interval < 1 || interval > 3600 {
        println!("❌ Invalid interval: {} seconds. Must be between 1 and 3600 seconds (1 hour)", interval);
        return Ok(true);
    }

    let mut parameters = HashMap::new();
    parameters.insert("update_interval_seconds".to_string(), interval.to_string());

    let command = ConfigurationCommand {
        command_id: Uuid::new_v4().to_string(),
        timestamp: Utc::now(),
        operator,
        command_type: ConfigCommandType::Set,
        target: ConfigTarget::Monitoring,
        parameters,
        apply_immediately: true,
    };

    let response = config_manager.execute_command(command).await;
    
    if response.success {
        println!(" Polling interval updated to {} seconds", interval);
        if response.requires_restart {
            println!("⚠️  Service restart required to apply new polling interval");
        } else {
            println!("🔄 New polling interval will take effect on next cycle");
        }
    } else {
        println!("❌ Failed to update polling interval: {}", response.message);
    }

    Ok(true)
}

// Handle generic set command - sets any parameter
async fn handle_set_command(
    matches: &ArgMatches,
    config_manager: &DynamicConfigManager,
) -> Result<bool, Box<dyn std::error::Error>> {
    let target = matches.get_one::<String>("target").unwrap();
    let key = matches.get_one::<String>("key").unwrap();
    let value = matches.get_one::<String>("value").unwrap();
    
    let default_operator = "CLI".to_string();
    let operator = matches.get_one::<String>("operator").unwrap_or(&default_operator);

    let mut parameters = HashMap::new();
    parameters.insert(key.clone(), value.clone());

    // Parse target to determine ConfigTarget
    let config_target = if target == "serial" {
        ConfigTarget::Serial
    } else if target == "monitoring" {
        ConfigTarget::Monitoring
    } else if target == "site" {
        ConfigTarget::Site
    } else if target == "system" {
        ConfigTarget::System
    } else if target == "gps" {
        ConfigTarget::Gps
    } else if target.starts_with("device:") {
        let address_str = target.strip_prefix("device:").unwrap();
        let address = address_str.parse::<u8>()
            .map_err(|_| "Invalid device address")?;
        ConfigTarget::Device { address }
    } else if target.starts_with("output:") {
        let output_type = target.strip_prefix("output:").unwrap().to_string();
        ConfigTarget::Output { output_type }
    } else {
        return Err(format!("Unknown target: {}", target).into());
    };

    // Determine if changes can be applied immediately
    let apply_immediately = match &config_target {
        ConfigTarget::Serial => false,
        ConfigTarget::Monitoring => true,
        ConfigTarget::Site => true,
        ConfigTarget::System => true,
        ConfigTarget::Gps => true,
        ConfigTarget::Device { .. } => true,
        ConfigTarget::Output { .. } => true,
    };

    let command = ConfigurationCommand {
        command_id: Uuid::new_v4().to_string(),
        command_type: ConfigCommandType::Set,
        target: config_target,
        parameters,
        timestamp: Utc::now(),
        operator: operator.clone(),
        apply_immediately,
    };

    let response = config_manager.execute_command(command).await;
    
    if response.success {
        println!("✅ Configuration updated: {} = {}", key, value);
        if response.requires_restart {
            println!("⚠️  Service restart required to apply changes");
        }
        
        // Save to TOML file after successful update
        if let Err(e) = save_config_to_file(config_manager, "setup/default.toml").await {
            println!("⚠️  Warning: Failed to save to TOML file: {}", e);
            println!("💡 Changes are active but won't persist after restart");
        } else {
            println!("� Configuration saved to setup/default.toml");
        }
    } else {
        println!("❌ Failed to update configuration: {}", response.message);
    }

    Ok(true)
}


// Handle enable command - enables device
async fn handle_enable_command(
    matches: &ArgMatches,
    config_manager: &DynamicConfigManager,
) -> Result<bool, Box<dyn std::error::Error>> {
    let address: u8 = matches.get_one::<String>("address").unwrap().parse().map_err(|_| {
        "Invalid device address"
    })?;
    
    let operator = matches.get_one::<String>("operator").unwrap_or(&"CLI".to_string()).clone();

    let command = ConfigurationCommand {
        command_id: Uuid::new_v4().to_string(),
        timestamp: Utc::now(),
        operator,
        command_type: ConfigCommandType::Enable,
        target: ConfigTarget::Device { address },
        parameters: HashMap::new(),
        apply_immediately: true,
    };

    let response = config_manager.execute_command(command).await;
    
    if response.success {
        println!(" Device {} enabled", address);
    } else {
        println!("❌ Failed to enable device {}: {}", address, response.message);
    }

    Ok(true)
}

// Handle disable command - disables device
async fn handle_disable_command(
    matches: &ArgMatches,
    config_manager: &DynamicConfigManager,
) -> Result<bool, Box<dyn std::error::Error>> {
    let address: u8 = matches.get_one::<String>("address").unwrap().parse().map_err(|_| {
        "Invalid device address"
    })?;
    
    let operator = matches.get_one::<String>("operator").unwrap_or(&"CLI".to_string()).clone();

    let command = ConfigurationCommand {
        command_id: Uuid::new_v4().to_string(),
        timestamp: Utc::now(),
        operator,
        command_type: ConfigCommandType::Disable,
        target: ConfigTarget::Device { address },
        parameters: HashMap::new(),
        apply_immediately: true,
    };

    let response = config_manager.execute_command(command).await;
    
    if response.success {
        println!(" Device {} disabled", address);
    } else {
        println!("❌ Failed to disable device {}: {}", address, response.message);
    }

    Ok(true)
}

// Handle remove command - removes device
async fn handle_remove_command(
    matches: &ArgMatches,
    config_manager: &DynamicConfigManager,
) -> Result<bool, Box<dyn std::error::Error>> {
    let address: u8 = matches.get_one::<String>("address").unwrap().parse().map_err(|_| {
        "Invalid device address"
    })?;
    
    let operator = matches.get_one::<String>("operator").unwrap_or(&"CLI".to_string()).clone();

    println!("⚠️  This will permanently remove device at address {}. Continue? (yes/no)", address);
    
    let mut input = String::new();
    std::io::stdin().read_line(&mut input)?;
    
    if input.trim().to_lowercase() != "yes" {
        println!("❌ Operation cancelled");
        return Ok(true);
    }

    let command = ConfigurationCommand {
        command_id: Uuid::new_v4().to_string(),
        timestamp: Utc::now(),
        operator,
        command_type: ConfigCommandType::Remove,
        target: ConfigTarget::Device { address },
        parameters: HashMap::new(),
        apply_immediately: true,
    };

    let response = config_manager.execute_command(command).await;
    
    if response.success {
        println!(" {}", response.message);
        if response.requires_restart {
            println!("⚠️  Service restart required to deactivate removed device");
        }
    } else {
        println!("❌ Failed to remove device {}: {}", address, response.message);
    }

    Ok(true)
}

// Handle backup command - creates configuration backup
async fn handle_backup_command(
    matches: &ArgMatches,
    config_manager: &DynamicConfigManager,
) -> Result<bool, Box<dyn std::error::Error>> {
    let operator = matches.get_one::<String>("operator").unwrap_or(&"CLI".to_string()).clone();
    
    let mut parameters = HashMap::new();
    if let Some(name) = matches.get_one::<String>("name") {
        parameters.insert("name".to_string(), name.clone());
    }

    let command = ConfigurationCommand {
        command_id: Uuid::new_v4().to_string(),
        timestamp: Utc::now(),
        operator,
        command_type: ConfigCommandType::Backup,
        target: ConfigTarget::System,
        parameters,
        apply_immediately: true,
    };

    let response = config_manager.execute_command(command).await;
    
    if response.success {
        println!(" {}", response.message);
    } else {
        println!("❌ Failed to create backup: {}", response.message);
    }

    Ok(true)
}

// Handle restore command - restores configuration from backup
async fn handle_restore_command(
    matches: &ArgMatches,
    config_manager: &DynamicConfigManager,
) -> Result<bool, Box<dyn std::error::Error>> {
    let backup_name = matches.get_one::<String>("name").unwrap();
    let operator = matches.get_one::<String>("operator").unwrap_or(&"CLI".to_string()).clone();

    println!("⚠️  This will replace current configuration with backup '{}'. Continue? (yes/no)", backup_name);
    
    let mut input = String::new();
    std::io::stdin().read_line(&mut input)?;
    
    if input.trim().to_lowercase() != "yes" {
        println!("❌ Operation cancelled");
        return Ok(true);
    }

    let mut parameters = HashMap::new();
    parameters.insert("name".to_string(), backup_name.clone());

    let command = ConfigurationCommand {
        command_id: Uuid::new_v4().to_string(),
        timestamp: Utc::now(),
        operator,
        command_type: ConfigCommandType::Restore,
        target: ConfigTarget::System,
        parameters,
        apply_immediately: true,
    };

    let response = config_manager.execute_command(command).await;
    
    if response.success {
        println!(" {}", response.message);
        if response.requires_restart {
            println!("⚠️  Service restart required to apply restored configuration");
        }
    } else {
        println!("❌ Failed to restore backup: {}", response.message);
    }

    Ok(true)
}

// Handle reset command - resets configuration to defaults
async fn handle_reset_command(
    config_manager: &DynamicConfigManager,
) -> Result<bool, Box<dyn std::error::Error>> {
    println!("⚠️  This will reset ALL configuration to factory defaults. Continue? (yes/no)");
    
    let mut input = String::new();
    std::io::stdin().read_line(&mut input)?;
    
    if input.trim().to_lowercase() != "yes" {
        println!("❌ Operation cancelled");
        return Ok(true);
    }

    let command = ConfigurationCommand {
        command_id: Uuid::new_v4().to_string(),
        timestamp: Utc::now(),
        operator: "CLI".to_string(),
        command_type: ConfigCommandType::Reset,
        target: ConfigTarget::System,
        parameters: HashMap::new(),
        apply_immediately: true,
    };

    let response = config_manager.execute_command(command).await;
    
    if response.success {
        println!(" {}", response.message);
        if response.requires_restart {
            println!("⚠️  Service restart required to apply reset configuration");
        }
    } else {
        println!("❌ Failed to reset configuration: {}", response.message);
    }

    Ok(true)
}

// NEW: Handle RPM-specific config commands
async fn handle_rpm_config_commands(
    matches: &ArgMatches,
    config_manager: &DynamicConfigManager,
) -> Result<bool, Box<dyn std::error::Error>> {
    match matches.subcommand() {
        Some(("register", register_matches)) => {
            handle_rpm_register_command(register_matches, config_manager).await
        }
        Some(("update", update_matches)) => {
            handle_rpm_update_command(update_matches, config_manager).await
        }
        _ => {
            println!("Available RPM config commands: register, update");
            Ok(true)
        }
    }
}

// NEW: Handle RPM device registration
async fn handle_rpm_register_command(
    matches: &ArgMatches,
    config_manager: &DynamicConfigManager,
) -> Result<bool, Box<dyn std::error::Error>> {
    let address = matches.get_one::<String>("address")
        .unwrap()
        .parse::<u8>()
        .map_err(|_| "Invalid address format")?;

    let name = matches.get_one::<String>("name").unwrap().clone();
    let location = matches.get_one::<String>("location").unwrap().clone();
    
    let channels = matches.get_one::<String>("channels")
        .unwrap()
        .parse::<u8>()
        .map_err(|_| "Invalid channels count")?;

    if channels == 0 || channels > 8 {
        return Err("Channels must be between 1 and 8".into());
    }

    // Parse thresholds
    let thresholds: Vec<u16> = if let Some(threshold_strs) = matches.get_many::<String>("thresholds") {
        let parsed_thresholds: Result<Vec<u16>, _> = threshold_strs
            .map(|s| s.parse::<u16>())
            .collect();
        
        match parsed_thresholds {
            Ok(mut thresholds) => {
                // Ensure we have enough thresholds, pad with default if needed
                while thresholds.len() < channels as usize {
                    thresholds.push(500); // Default threshold
                }
                // Truncate if too many
                thresholds.truncate(channels as usize);
                thresholds
            }
            Err(_) => return Err("Invalid threshold format. Use comma-separated numbers (e.g., 500,400,600)".into()),
        }
    } else {
        // Default thresholds for all channels
        vec![500; channels as usize]
    };

    // Parse engine types
    let engine_types: Vec<String> = if let Some(type_strs) = matches.get_many::<String>("engine-types") {
        let mut types: Vec<String> = type_strs.cloned().collect();
        // Pad with defaults if needed
        while types.len() < channels as usize {
            types.push(format!("engine{}", types.len() + 1));
        }
        types.truncate(channels as usize);
        types
    } else {
        // Default engine types
        (1..=channels).map(|i| {
            match i {
                1 => "main".to_string(),
                2 => "aux".to_string(),
                _ => format!("engine{}", i),
            }
        }).collect()
    };

    let auto_detect = matches.get_flag("auto-detect");

    // Build metadata for RPM device
    let mut metadata = HashMap::new();
    metadata.insert("total_channels".to_string(), channels.to_string());
    metadata.insert("auto_detect_channels".to_string(), auto_detect.to_string());
    metadata.insert("outlier_detection_threshold".to_string(), "150".to_string());
    metadata.insert("outlier_confirmation_threshold".to_string(), "15".to_string());

    // Add per-channel thresholds
    for (i, &threshold) in thresholds.iter().enumerate() {
        metadata.insert(format!("rpm_threshold_ch{}", i + 1), threshold.to_string());
    }

    // Add engine types
    for (i, engine_type) in engine_types.iter().enumerate() {
        metadata.insert(format!("engine_type_ch{}", i + 1), engine_type.clone());
    }
    metadata.insert("engine_types".to_string(), engine_types.join(","));

    // Create configuration command
    let mut parameters = HashMap::new();
    parameters.insert("device_type".to_string(), "rpm".to_string());
    parameters.insert("name".to_string(), name.clone());
    parameters.insert("location".to_string(), location);

    // Add all metadata as parameters
    for (key, value) in metadata {
        parameters.insert(key, value);
    }

    let command = ConfigurationCommand {
        command_id: Uuid::new_v4().to_string(),
        timestamp: Utc::now(),
        operator: "CLI".to_string(),
        command_type: ConfigCommandType::Add,
        target: ConfigTarget::Device { address },
        parameters,
        apply_immediately: true,
    };

    let response = config_manager.execute_command(command).await;
    
    if response.success {
        println!("✅ Successfully registered RPM device:");
        println!("   📍 Address: {}", address);
        println!("   🏷️  Name: {}", name);
        println!("   🔢 Channels: {}", channels);
        println!("   ⚡ Thresholds: {:?}", thresholds);
        println!("   🏭 Engine Types: {:?}", engine_types);
        println!("   🔍 Auto-detect: {}", auto_detect);
        
        if response.requires_restart {
            println!("⚠️  Service restart required to activate new device");
        }
    } else {
        println!("❌ Failed to register RPM device: {}", response.message);
    }

    Ok(true)
}

// NEW: Handle RPM device updates
async fn handle_rpm_update_command(
    matches: &ArgMatches,
    config_manager: &DynamicConfigManager,
) -> Result<bool, Box<dyn std::error::Error>> {
    let address = matches.get_one::<String>("address")
        .unwrap()
        .parse::<u8>()
        .map_err(|_| "Invalid address format")?;

    let mut parameters = HashMap::new();

    if let Some(channel_str) = matches.get_one::<String>("channel") {
        let channel = channel_str.parse::<u8>()
            .map_err(|_| "Invalid channel number")?;

        if channel == 0 || channel > 8 {
            return Err("Channel must be between 1 and 8".into());
        }

        if let Some(threshold_str) = matches.get_one::<String>("threshold") {
            let threshold = threshold_str.parse::<u16>()
                .map_err(|_| "Invalid threshold value")?;
            
            parameters.insert(format!("rpm_threshold_ch{}", channel), threshold.to_string());
            println!("📊 Updating channel {} threshold to {} RPM", channel, threshold);
        }

        if let Some(engine_type) = matches.get_one::<String>("engine-type") {
            parameters.insert(format!("engine_type_ch{}", channel), engine_type.clone());
            println!("🏭 Updating channel {} engine type to '{}'", channel, engine_type);
        }
    }

    if parameters.is_empty() {
        return Err("No parameters to update. Specify --threshold and/or --engine-type".into());
    }

    let command = ConfigurationCommand {
        command_id: Uuid::new_v4().to_string(),
        timestamp: Utc::now(),
        operator: "CLI".to_string(),
        command_type: ConfigCommandType::Set,
        target: ConfigTarget::Device { address },
        parameters,
        apply_immediately: true,
    };

    let response = config_manager.execute_command(command).await;
    
    if response.success {
        println!("✅ Successfully updated RPM device at address {}", address);
        if response.requires_restart {
            println!("⚠️  Service restart required to apply changes");
        }
    } else {
        println!("❌ Failed to update RPM device: {}", response.message);
    }

    Ok(true)
}

// Enhanced add command with RPM support
async fn handle_enhanced_add_command(
    matches: &ArgMatches,
    config_manager: &DynamicConfigManager,
) -> Result<bool, Box<dyn std::error::Error>> {
    let device_type = matches.get_one::<String>("type").unwrap();
    let address = matches.get_one::<String>("address")
        .unwrap()
        .parse::<u8>()
        .map_err(|_| "Invalid address format")?;

    let name = matches.get_one::<String>("name").unwrap().clone();
    let location = matches.get_one::<String>("location").unwrap().clone();

    let mut parameters = HashMap::new();
    parameters.insert("device_type".to_string(), device_type.clone());
    parameters.insert("name".to_string(), name.clone());
    parameters.insert("location".to_string(), location);

    // Handle RPM-specific parameters
    if device_type == "rpm" {
        if let Some(channels_str) = matches.get_one::<String>("channels") {
            let channels = channels_str.parse::<u8>()
                .map_err(|_| "Invalid channels count")?;

            if channels == 0 || channels > 8 {
                return Err("Channels must be between 1 and 8".into());
            }

            parameters.insert("total_channels".to_string(), channels.to_string());

            // Parse thresholds if provided
            if let Some(threshold_strs) = matches.get_many::<String>("thresholds") {
                let thresholds: Result<Vec<u16>, _> = threshold_strs
                    .map(|s| s.parse::<u16>())
                    .collect();
                
                match thresholds {
                    Ok(mut thresholds) => {
                        // Ensure we have enough thresholds
                        while thresholds.len() < channels as usize {
                            thresholds.push(500);
                        }
                        thresholds.truncate(channels as usize);

                        // Set individual channel thresholds
                        for (i, &threshold) in thresholds.iter().enumerate() {
                            parameters.insert(format!("rpm_threshold_ch{}", i + 1), threshold.to_string());
                        }
                    }
                    Err(_) => return Err("Invalid threshold format".into()),
                }
            } else {
                // Set default thresholds
                for i in 1..=channels {
                    parameters.insert(format!("rpm_threshold_ch{}", i), "500".to_string());
                }
            }

            let auto_detect = matches.get_flag("auto-detect");
            parameters.insert("auto_detect_channels".to_string(), auto_detect.to_string());
        } else {
            return Err("RPM devices require --channels parameter".into());
        }
    }

    // Handle AIO-specific parameters
    if device_type == "aio" {
        // Analog channels (required)
        if let Some(analog_channels_str) = matches.get_one::<String>("analog-channels") {
            let analog_channels = analog_channels_str.parse::<u8>()
                .map_err(|_| "Invalid analog channels count")?;

            if analog_channels == 0 || analog_channels > 8 {
                return Err("Analog channels must be between 1 and 8".into());
            }

            parameters.insert("total_analog_channels".to_string(), analog_channels.to_string());

            // Digital inputs (optional, default 16)
            let digital_inputs = matches.get_one::<String>("digital-inputs")
                .unwrap_or(&"16".to_string())
                .parse::<u8>()
                .map_err(|_| "Invalid digital inputs count")?;

            if digital_inputs > 16 {
                return Err("Digital inputs cannot exceed 16".into());
            }

            parameters.insert("digital_inputs_count".to_string(), digital_inputs.to_string());

            // Parse channel types if provided
            let mut channel_types = Vec::new();
            if let Some(type_strs) = matches.get_many::<String>("channel-types") {
                channel_types = type_strs.cloned().collect();
                
                // Validate channel types
                for channel_type in &channel_types {
                    if !["rpm", "pulse", "frequency"].contains(&channel_type.as_str()) {
                        return Err(format!("Invalid channel type '{}'. Must be one of: rpm, pulse, frequency", channel_type).into());
                    }
                }
            }

            // Pad or truncate channel types to match analog channels count
            while channel_types.len() < analog_channels as usize {
                // Default pattern: first few channels are RPM, rest are pulse
                let channel_type = if channel_types.len() < 4 { "rpm" } else { "pulse" };
                channel_types.push(channel_type.to_string());
            }
            channel_types.truncate(analog_channels as usize);

            parameters.insert("channel_types".to_string(), channel_types.join(","));

            // Set individual channel types
            for (i, channel_type) in channel_types.iter().enumerate() {
                parameters.insert(format!("channel_{}_type", i + 1), channel_type.clone());
            }

            // Parse AIO thresholds for RPM channels
            let mut thresholds = Vec::new();
            if let Some(threshold_strs) = matches.get_many::<String>("aio-thresholds") {
                let parsed_thresholds: Result<Vec<u16>, _> = threshold_strs
                    .map(|s| s.parse::<u16>())
                    .collect();
                
                match parsed_thresholds {
                    Ok(parsed) => thresholds = parsed,
                    Err(_) => return Err("Invalid threshold format for AIO channels".into()),
                }
            }

            // Set thresholds for each channel
            let mut threshold_values = Vec::new();
            for (i, channel_type) in channel_types.iter().enumerate() {
                let threshold = if channel_type == "rpm" {
                    thresholds.get(i).copied().unwrap_or(500)
                } else {
                    0 // Non-RPM channels don't need thresholds
                };
                threshold_values.push(threshold.to_string());
                parameters.insert(format!("channel_{}_threshold", i + 1), threshold.to_string());
            }

            parameters.insert("rpm_thresholds".to_string(), threshold_values.join(","));

            // Auto-detection flag
            let auto_detect = matches.get_flag("auto-detect-aio");
            parameters.insert("auto_detect_channels".to_string(), auto_detect.to_string());

            // Default baud rate for AIO modules
            parameters.insert("baud_rate".to_string(), "9600".to_string());

        } else {
            return Err("AIO devices require --analog-channels parameter".into());
        }
    }

    let command = ConfigurationCommand {
        command_id: Uuid::new_v4().to_string(),
        timestamp: Utc::now(),
        operator: "CLI".to_string(),
        command_type: ConfigCommandType::Add,
        target: ConfigTarget::Device { address },
        parameters,
        apply_immediately: true,
    };

    let response = config_manager.execute_command(command).await;
    
    if response.success {
        println!("✅ {}", response.message);
        if response.requires_restart {
            println!("⚠️  Service restart required to activate new device");
        }
    } else {
        println!("❌ Failed to add device: {}", response.message);
    }

    Ok(true)
}

// ✅ ADD: Helper function to save config to file
async fn save_config_to_file(
    config_manager: &DynamicConfigManager,
    file_path: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    // Get current config from manager
    let config = config_manager.get_current_config().await;
    
    // Use the save_to_file method from Config
    config.save_to_file(file_path)?;
    
    Ok(())
}