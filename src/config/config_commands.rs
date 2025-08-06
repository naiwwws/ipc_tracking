use clap::ArgMatches;
use std::collections::HashMap;
use uuid::Uuid;
use chrono::Utc;

use crate::config::dynamic_manager::{DynamicConfigManager, ConfigurationCommand, ConfigCommandType, ConfigTarget};
 // Add this import

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

    // Handle add command
    if let Some(add_matches) = matches.subcommand_matches("add") {
        return handle_add_command(add_matches, config_manager).await;
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

// Handle add command - adds new device
async fn handle_add_command(
    matches: &ArgMatches,
    config_manager: &DynamicConfigManager,
) -> Result<bool, Box<dyn std::error::Error>> {
    let address: u8 = matches.get_one::<String>("address").unwrap().parse().map_err(|_| {
        "Invalid device address"
    })?;
    
    let operator = matches.get_one::<String>("operator").unwrap_or(&"CLI".to_string()).clone();

    let mut parameters = HashMap::new();
    
    if let Some(device_id) = matches.get_one::<String>("device-id") {
        parameters.insert("device_id".to_string(), device_id.clone());
    }
    
    if let Some(device_type) = matches.get_one::<String>("device-type") {
        parameters.insert("device_type".to_string(), device_type.clone());
    }
    
    if let Some(name) = matches.get_one::<String>("name") {
        parameters.insert("name".to_string(), name.clone());
    }
    
    if let Some(location) = matches.get_one::<String>("location") {
        parameters.insert("location".to_string(), location.clone());
    }

    let command = ConfigurationCommand {
        command_id: Uuid::new_v4().to_string(),
        timestamp: Utc::now(),
        operator,
        command_type: ConfigCommandType::Add,
        target: ConfigTarget::Device { address },
        parameters,
        apply_immediately: true,
    };

    let response = config_manager.execute_command(command).await;
    
    if response.success {
        println!(" {}", response.message);
        if response.requires_restart {
            println!("⚠️  Service restart required to activate new device");
        }
    } else {
        println!("❌ Failed to add device: {}", response.message);
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