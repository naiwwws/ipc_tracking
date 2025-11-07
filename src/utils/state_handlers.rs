use std::sync::Arc;
use log::{info, warn, error, debug};
use tokio::time::{sleep, Duration};

use crate::utils::error::ModbusError;
use crate::services::DataService;
use super::state_machine::{SystemStateMachine, StateEvent, SystemState};

/// Handler for system initialization state
pub async fn handle_initialization(
    fsm: &SystemStateMachine,
    service: &DataService,
) -> Result<(), ModbusError> {
    info!("🚀 FSM Handler: Initializing system...");
    
    // Update context
    fsm.update_context(|ctx| {
        ctx.metadata.insert("init_start".to_string(), chrono::Utc::now().to_rfc3339());
    }).await;
    
    // Perform initialization tasks
    // 1. Verify configuration
    debug!("✓ Configuration verified");
    
    // 2. Initialize serial connections
    debug!("✓ Serial connections initialized");
    
    // 3. Initialize database (if enabled)
    #[cfg(feature = "sqlite")]
    {
        debug!("✓ Database initialized");
    }
    
    // 4. Initialize output channels
    debug!("✓ Output channels initialized");
    
    info!("✅ FSM Handler: System initialization complete");
    
    // Transition to next state
    fsm.transition(StateEvent::InitComplete).await?;
    
    Ok(())
}

/// Handler for reading device data
pub async fn handle_read_devices(
    fsm: &SystemStateMachine,
    service: &DataService,
) -> Result<(), ModbusError> {
    info!("📊 FSM Handler: Reading device data...");
    
    let context = fsm.get_context().await;
    
    // Check if we should retry
    if context.retry_count > 0 {
        info!("🔄 Retry attempt {}/{}", context.retry_count, context.max_retries);
        sleep(Duration::from_secs(2)).await;
    }
    
    // Attempt to read from all devices
    match service.read_all_devices().await {
        Ok(_) => {
            info!("✅ FSM Handler: Device data read successfully");
            
            // Update context
            fsm.update_context(|ctx| {
                ctx.devices_ready = true;
                ctx.clear_error();
                ctx.metadata.insert("devices_read_at".to_string(), chrono::Utc::now().to_rfc3339());
            }).await;
            
            // Transition to GPS reading
            fsm.transition(StateEvent::DevicesReadSuccess).await?;
            Ok(())
        }
        Err(e) => {
            error!("❌ FSM Handler: Failed to read device data: {}", e);
            
            // Update context with error
            fsm.update_context(|ctx| {
                ctx.increment_retry();
                ctx.set_error(format!("Device read error: {}", e));
            }).await;
            
            let context = fsm.get_context().await;
            
            if context.is_max_retries_exceeded() {
                warn!("⚠️  FSM Handler: Max retries exceeded for device reading");
                fsm.transition(StateEvent::DevicesReadFailure).await?;
                Err(e)
            } else {
                // Retry
                fsm.transition(StateEvent::RecoverableError).await?;
                // Recursive retry
                Box::pin(handle_read_devices(fsm, service)).await
            }
        }
    }
}

/// Handler for reading GPS data
pub async fn handle_read_gps(
    fsm: &SystemStateMachine,
    service: &DataService,
) -> Result<(), ModbusError> {
    info!("🧭 FSM Handler: Reading GPS data...");
    
    // Attempt to read GPS data
    match service.read_gps_data().await {
        Ok(gps_data) => {
            info!("✅ FSM Handler: GPS data read successfully");
            debug!("GPS: {:?}", gps_data);
            
            // Update context
            fsm.update_context(|ctx| {
                ctx.gps_ready = true;
                ctx.metadata.insert("gps_read_at".to_string(), chrono::Utc::now().to_rfc3339());
            }).await;
            
            // Transition to connection testing
            fsm.transition(StateEvent::GPSReadSuccess).await?;
            Ok(())
        }
        Err(e) => {
            warn!("⚠️  FSM Handler: GPS read failed: {} (non-critical, continuing)", e);
            
            // GPS failure is not critical - we can continue without it
            fsm.update_context(|ctx| {
                ctx.gps_ready = false;
                ctx.metadata.insert("gps_error".to_string(), e.to_string());
            }).await;
            
            // Still transition to connection test
            fsm.transition(StateEvent::GPSReadFailure).await?;
            Ok(())
        }
    }
}

/// Handler for testing external connections
pub async fn handle_test_connection(
    fsm: &SystemStateMachine,
    service: &DataService,
) -> Result<(), ModbusError> {
    info!("🔌 FSM Handler: Testing external connections...");
    
    let context = fsm.get_context().await;
    
    // Check if we should retry
    if context.retry_count > 0 {
        info!("🔄 Retry attempt {}/{}", context.retry_count, context.max_retries);
        sleep(Duration::from_secs(5)).await;
    }
    
    let mut connection_ok = true;
    let mut error_messages = Vec::new();
    
    // Test database connection (if enabled)
    #[cfg(feature = "sqlite")]
    {
        match service.test_database_connection().await {
            Ok(_) => {
                debug!("✓ Database connection OK");
            }
            Err(e) => {
                error!("❌ Database connection failed: {}", e);
                connection_ok = false;
                error_messages.push(format!("Database: {}", e));
            }
        }
    }
    
    // Test MTWS connection (if enabled)
    #[cfg(feature = "sqlite")]
    {
        match service.test_mtws_connection().await {
            Ok(_) => {
                debug!("✓ MTWS connection OK");
            }
            Err(e) => {
                warn!("⚠️  MTWS connection test failed: {} (non-critical)", e);
                // MTWS failure might not be critical depending on config
            }
        }
    }
    
    if connection_ok {
        info!("✅ FSM Handler: Connection tests passed");
        
        // Update context
        fsm.update_context(|ctx| {
            ctx.connection_ready = true;
            ctx.clear_error();
            ctx.reset_retries();
            ctx.metadata.insert("connection_tested_at".to_string(), chrono::Utc::now().to_rfc3339());
        }).await;
        
        // Transition to operational ready
        fsm.transition(StateEvent::ConnectionTestSuccess).await?;
        Ok(())
    } else {
        error!("❌ FSM Handler: Connection tests failed");
        
        // Update context with error
        fsm.update_context(|ctx| {
            ctx.increment_retry();
            ctx.set_error(error_messages.join("; "));
        }).await;
        
        let context = fsm.get_context().await;
        
        if context.is_max_retries_exceeded() {
            warn!("⚠️  FSM Handler: Max retries exceeded for connection testing");
            fsm.transition(StateEvent::ConnectionTestFailure).await?;
            Err(ModbusError::ServiceNotAvailable("Connection test failed".to_string()))
        } else {
            // Retry
            fsm.transition(StateEvent::RecoverableError).await?;
            // Recursive retry
            Box::pin(handle_test_connection(fsm, service)).await
        }
    }
}

/// Handler for operational ready state
pub async fn handle_operational_ready(
    fsm: &SystemStateMachine,
    service: &DataService,
) -> Result<(), ModbusError> {
    info!("✅ FSM Handler: System is operational and ready");
    
    // Update context
    fsm.update_context(|ctx| {
        ctx.metadata.insert("operational_since".to_string(), chrono::Utc::now().to_rfc3339());
    }).await;
    
    // Check if we have data to send
    let has_data = service.has_pending_data().await;
    
    if has_data {
        debug!("📤 FSM Handler: Pending data detected, transitioning to send");
        fsm.transition(StateEvent::ReadyToSend).await?;
    } else {
        debug!("⏸️  FSM Handler: No pending data, remaining in ready state");
    }
    
    Ok(())
}

/// Handler for sending data state
pub async fn handle_send_data(
    fsm: &SystemStateMachine,
    service: &DataService,
) -> Result<(), ModbusError> {
    info!("📤 FSM Handler: Sending data to MTWS...");
    
    // Attempt to send data
    #[cfg(feature = "sqlite")]
    match service.send_data_to_mtws().await {
        Ok(_) => {
            info!("✅ FSM Handler: Data sent successfully");
            
            // Update context
            fsm.update_context(|ctx| {
                ctx.metadata.insert("last_send_at".to_string(), chrono::Utc::now().to_rfc3339());
            }).await;
            
            // Transition back to operational ready
            fsm.transition(StateEvent::DataSentSuccess).await?;
            Ok(())
        }
        Err(e) => {
            error!("❌ FSM Handler: Failed to send data: {}", e);
            
            // Update context with error (but don't fail - just log and continue)
            fsm.update_context(|ctx| {
                ctx.set_error(format!("Data send error: {}", e));
                ctx.metadata.insert("last_send_error".to_string(), e.to_string());
            }).await;
            
            // Transition back to operational ready (will retry next cycle)
            fsm.transition(StateEvent::DataSendFailure).await?;
            Ok(())
        }
    }
    
    #[cfg(not(feature = "sqlite"))]
    {
        warn!("⚠️  SQLite feature not enabled, skipping data send");
        fsm.transition(StateEvent::DataSentSuccess).await?;
        Ok(())
    }
}

/// Handler for error state
pub async fn handle_error_state(
    fsm: &SystemStateMachine,
    service: &DataService,
) -> Result<(), ModbusError> {
    error!("❌ FSM Handler: System in error state");
    
    let context = fsm.get_context().await;
    
    if let Some(error) = &context.last_error {
        error!("Last error: {}", error);
    }
    
    // Determine if we can recover
    let can_recover = !context.devices_ready;
    
    if can_recover {
        warn!("🔄 Attempting recovery...");
        
        // Reset retry counter for recovery attempt
        fsm.update_context(|ctx| {
            ctx.reset_retries();
        }).await;
        
        // Try to recover by going back to device reading
        sleep(Duration::from_secs(5)).await;
        fsm.transition(StateEvent::RecoverableError).await?;
        
        // Start recovery
        Box::pin(handle_read_devices(fsm, service)).await
    } else {
        error!("❌ Cannot recover - fatal error occurred");
        fsm.transition(StateEvent::FatalError).await?;
        Err(ModbusError::FatalError("System in unrecoverable error state".to_string()))
    }
}

/// Handler for shutdown state
pub async fn handle_shutdown(
    fsm: &SystemStateMachine,
    service: &DataService,
) -> Result<(), ModbusError> {
    info!("🛑 FSM Handler: Shutting down system...");
    
    // Perform cleanup tasks
    
    // 1. Stop polling
    service.stop_polling().await?;
    debug!("✓ Polling stopped");
    
    // 2. Save any pending data
    #[cfg(feature = "sqlite")]
    {
        service.flush_pending_data().await?;
        debug!("✓ Pending data flushed");
    }
    
    // 3. Close connections
    service.close_connections().await?;
    debug!("✓ Connections closed");
    
    // Update context
    fsm.update_context(|ctx| {
        ctx.metadata.insert("shutdown_at".to_string(), chrono::Utc::now().to_rfc3339());
    }).await;
    
    info!("✅ FSM Handler: Shutdown complete");
    
    Ok(())
}

/// Main FSM execution loop
pub async fn run_state_machine(
    fsm: Arc<SystemStateMachine>,
    service: Arc<DataService>,
    mut shutdown_rx: tokio::sync::oneshot::Receiver<()>,
) -> Result<(), ModbusError> {
    info!("🎯 Starting FSM execution loop");
    
    loop {
        let current_state = fsm.current_state().await;
        
        // Log current state
        debug!("FSM: {}", fsm.get_state_summary().await);
        
        // Check for shutdown signal
        if tokio::select! {
            _ = &mut shutdown_rx => true,
            else => false,
        } {
            info!("🛑 Shutdown signal received");
            fsm.transition(StateEvent::ShutdownRequested).await?;
            handle_shutdown(&fsm, &service).await?;
            break;
        }
        
        // Handle current state
        let result = match current_state {
            SystemState::Initializing => {
                handle_initialization(&fsm, &service).await
            }
            SystemState::ReadingDevices => {
                handle_read_devices(&fsm, &service).await
            }
            SystemState::ReadingGPS => {
                handle_read_gps(&fsm, &service).await
            }
            SystemState::TestingConnection => {
                handle_test_connection(&fsm, &service).await
            }
            SystemState::OperationalReady => {
                handle_operational_ready(&fsm, &service).await?;
                // Wait for polling interval before checking again
                sleep(Duration::from_secs(10)).await;
                Ok(())
            }
            SystemState::SendingData => {
                handle_send_data(&fsm, &service).await
            }
            SystemState::Error => {
                handle_error_state(&fsm, &service).await
            }
            SystemState::Shutdown => {
                info!("✅ FSM: System shutdown complete");
                break;
            }
        };
        
        // Handle errors from state handlers
        if let Err(e) = result {
            error!("❌ FSM: Error in state {}: {}", current_state, e);
            
            // If not already in error state, transition to it
            if current_state != SystemState::Error {
                fsm.update_context(|ctx| {
                    ctx.set_error(e.to_string());
                }).await;
                
                // Determine if error is fatal
                let is_fatal = matches!(e, ModbusError::FatalError(_));
                
                if is_fatal {
                    fsm.transition(StateEvent::FatalError).await?;
                } else {
                    fsm.transition(StateEvent::RecoverableError).await.ok();
                }
            }
        }
        
        // Small delay to prevent tight loop
        sleep(Duration::from_millis(100)).await;
    }
    
    info!("🏁 FSM execution loop terminated");
    Ok(())
}
