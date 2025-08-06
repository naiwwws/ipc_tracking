use std::sync::Arc;
use tokio::sync::RwLock;
use log::{info, warn, error};

use super::{GpsData, GpsReader};
use crate::utils::error::ModbusError;

#[derive(Clone)]
pub struct GpsService {
    current_data: Arc<RwLock<GpsData>>,
    port_name: String,
    baud_rate: u32,
}

impl GpsService {
    pub fn new(port_name: String, baud_rate: u32) -> Self {
        Self {
            current_data: Arc::new(RwLock::new(GpsData::default())),
            port_name,
            baud_rate,
        }
    }
    
    pub async fn get_current_gps_fix(&self) -> Result<Option<GpsData>, ModbusError> {
        info!("🧭 Requesting GPS fix...");
        
        match GpsReader::new(&self.port_name, self.baud_rate) {
            Ok(mut gps_reader) => {
                // Configure GPS module
                if let Err(e) = gps_reader.configure_quectel_gps() {
                    error!("❌ Failed to configure GPS module: {}", e);
                    return Ok(None);
                }
                
                // Try to get GPS fix with timeout
                let gps_data = tokio::task::spawn_blocking(move || {
                    gps_reader.get_single_gps_fix(30) // 30 second timeout
                }).await
                .map_err(|e| ModbusError::CommunicationError(format!("GPS task failed: {}", e)))?;
                
                match gps_data {
                    Ok(Some(data)) => {
                        info!("✅ GPS fix acquired: {:.6}°, {:.6}°", 
                              data.latitude.unwrap_or(0.0), 
                              data.longitude.unwrap_or(0.0));
                        
                        // Store the data
                        {
                            let mut current = self.current_data.write().await;
                            *current = data.clone();
                        }
                        
                        Ok(Some(data))
                    }
                    Ok(None) => {
                        warn!("⚠️ GPS fix not acquired within timeout");
                        Ok(None)
                    }
                    Err(e) => {
                        error!("❌ GPS error: {}", e);
                        Err(e)
                    }
                }
            }
            Err(e) => {
                error!("❌ Failed to initialize GPS reader: {}", e);
                Err(e)
            }
        }
    }
    
    /// Legacy method for backward compatibility
    pub async fn start(&self) -> Result<(), ModbusError> {
        info!("🧭 GPS service configured for on-demand use");
        Ok(())
    }
    
    pub async fn stop(&self) -> Result<(), ModbusError> {
        info!("🧭 GPS service stopped");
        Ok(())
    }
    
    pub async fn get_current_data(&self) -> GpsData {
        self.current_data.read().await.clone()
    }
    
    
    pub async fn get_status(&self) -> String {
        let data = self.get_current_data().await;
        
        if data.has_valid_fix() {
            format!("🧭 GPS Available - Last Fix: {:.6}°, {:.6}°, Satellites: {}", 
                    data.latitude.unwrap_or(0.0), 
                    data.longitude.unwrap_or(0.0),
                    data.satellites.unwrap_or(0))
        } else {
            "🧭 GPS Available - No recent fix".to_string()
        }
    }
}