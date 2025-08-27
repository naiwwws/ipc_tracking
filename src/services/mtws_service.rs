use log::{info, error, warn};
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::time::{Duration, sleep};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use reqwest::Client;
use std::collections::HashMap;

use crate::config::Config;
use crate::utils::error::ModbusError;

#[derive(Clone)] // Add Clone trait
pub struct MtwsService {
    config: Config,
    is_running: Arc<RwLock<bool>>,
    transmission_interval: Arc<RwLock<Duration>>,
    client: Client,
}

impl MtwsService {
    // Simplified constructor without DataService dependency
    pub fn new(config: Config) -> Self {
        let client = Client::new();
        let transmission_interval = Duration::from_secs(config.mtws.transmission_interval_seconds);
        
        Self {
            config,
            is_running: Arc::new(RwLock::new(false)),
            transmission_interval: Arc::new(RwLock::new(transmission_interval)),
            client,
        }
    }

    pub async fn start_transmission(&self) -> Result<(), ModbusError> {
        let mut is_running = self.is_running.write().await;
        if *is_running {
            return Err(ModbusError::ServiceNotAvailable("MTWS transmission already running".to_string()));
        }

        if !self.config.mtws.enabled {
            return Err(ModbusError::ServiceNotAvailable("MTWS service is disabled".to_string()));
        }

        *is_running = true;
        info!("🛰️ Starting MTWS transmission service");
        info!("📡 Endpoint: {}", self.config.get_mtws_endpoint_url());
        info!("⏱️ Interval: {} seconds", self.config.mtws.transmission_interval_seconds);

        // Start the transmission loop
        let config = self.config.clone();
        let is_running_clone = Arc::clone(&self.is_running);
        let interval_clone = Arc::clone(&self.transmission_interval);
        let client = self.client.clone();

        tokio::spawn(async move {
            Self::transmission_loop(config, is_running_clone, interval_clone, client).await;
        });

        Ok(())
    }

    async fn transmission_loop(
        config: Config,
        is_running: Arc<RwLock<bool>>,
        interval: Arc<RwLock<Duration>>,
        client: Client,
    ) {
        info!("🔄 MTWS transmission loop started");
        
        loop {
            // Check if we should continue running
            let should_run = *is_running.read().await;
            if !should_run {
                info!("🛑 MTWS transmission loop stopped");
                break;
            }

            // Get current interval
            let current_interval = *interval.read().await;
            
            // Send data
            match Self::send_transmission_cycle(&config, &client).await {
                Ok(_) => {
                    info!("✅ MTWS transmission cycle completed successfully");
                }
                Err(e) => {
                    error!("❌ MTWS transmission cycle failed: {}", e);
                    // Continue running even if one cycle fails
                }
            }

            // Wait for next cycle
            sleep(current_interval).await;
        }
    }

    async fn send_transmission_cycle(
        config: &Config,
        client: &Client,
    ) -> Result<(), ModbusError> {
        info!("📡 Starting MTWS transmission cycle");

        // Get endpoint URL
        let endpoint_url = config.get_mtws_endpoint_url();
        
        // Build payload with current timestamp and static/demo data
        let payload = Self::build_demo_payload(config);
        
        // Send POST request
        Self::send_post_request(client, &endpoint_url, &payload).await?;
        
        Ok(())
    }

    fn build_demo_payload(config: &Config) -> HashMap<String, String> {
        let mut payload = HashMap::new();
        
        // Add timestamp
        payload.insert("timestamp".to_string(), Utc::now().timestamp().to_string());
        
        // Add GPS data (demo values)
        payload.insert("longitude".to_string(), "1234567".to_string());
        payload.insert("latitude".to_string(), "5678901".to_string());
        payload.insert("speed".to_string(), "15".to_string());
        payload.insert("heading".to_string(), "180".to_string());
        payload.insert("altitude".to_string(), "10".to_string());
        payload.insert("gpsNumSats".to_string(), "8".to_string());
        
        // Add flowmeter data (demo values)
        payload.insert("flowmeterVolumeTotal1".to_string(), "1250.5".to_string());
        payload.insert("flowmeterDensity1".to_string(), "850.2".to_string());
        payload.insert("flowmeterTemperature1".to_string(), "25.4".to_string());
        payload.insert("flowmeterFlowrate1".to_string(), "45.2".to_string());
        
        // Add power data (demo values)
        payload.insert("batteryVoltage".to_string(), "8086".to_string());
        payload.insert("extPowerVoltage".to_string(), "27838".to_string());
        
        // Add device identifier
        payload.insert("imei".to_string(), config.mtws.imei.clone());
        
        info!("📦 Built MTWS payload with {} fields", payload.len());
        payload
    }

    async fn send_post_request(
        client: &Client,
        endpoint_url: &str,
        payload: &HashMap<String, String>,
    ) -> Result<(), ModbusError> {
        info!("🛰️ Sending MTWS payload to: {}", endpoint_url);
        
        let response = client
            .post(endpoint_url)
            .header("Content-Type", "application/x-www-form-urlencoded")
            .header("User-Agent", "IPC-Track-Device/1.0")
            .form(payload)
            .timeout(Duration::from_secs(30))
            .send()
            .await
            .map_err(|e| ModbusError::CommunicationError(format!("HTTP request failed: {}", e)))?;

        let status = response.status();
        let response_text = response.text().await.unwrap_or_default();

        if status.is_success() {
            info!("✅ MTWS data sent successfully - Status: {}", status);
            info!("📝 Server response: {}", response_text);
        } else {
            error!("❌ MTWS request failed - Status: {}, Response: {}", status, response_text);
            return Err(ModbusError::CommunicationError(format!("HTTP {} - {}", status, response_text)));
        }

        Ok(())
    }

    pub async fn stop_transmission(&self) -> Result<(), ModbusError> {
        let mut is_running = self.is_running.write().await;
        *is_running = false;
        info!("🛰️ MTWS transmission stop requested");
        Ok(())
    }

    pub async fn send_data_once(&self) -> Result<(), ModbusError> {
        if !self.config.mtws.enabled {
            return Err(ModbusError::ServiceNotAvailable("MTWS service is disabled".to_string()));
        }

        let endpoint_url = self.config.get_mtws_endpoint_url();
        let payload = Self::build_demo_payload(&self.config);
        
        info!("🛰️ Sending single MTWS payload to: {}", endpoint_url);
        Self::send_post_request(&self.client, &endpoint_url, &payload).await?;
        
        Ok(())
    }

    // Add the missing send_single_payload method
    pub async fn send_single_payload(&self, endpoint_url: String) -> Result<String, ModbusError> {
        if !self.config.mtws.enabled {
            return Err(ModbusError::ServiceNotAvailable("MTWS service is disabled".to_string()));
        }

        let payload = Self::build_demo_payload(&self.config);
        
        info!("🛰️ Sending single MTWS payload to: {}", endpoint_url);
        Self::send_post_request(&self.client, &endpoint_url, &payload).await?;
        
        // Return a summary of what was sent
        let payload_summary = serde_json::to_string(&payload)
            .unwrap_or_else(|_| format!("Payload with {} fields", payload.len()));
        
        Ok(payload_summary)
    }

    pub async fn get_status(&self) -> (bool, u64, String, bool) {
        let is_running = *self.is_running.read().await;
        let interval = self.transmission_interval.read().await.as_secs();
        let endpoint = self.config.get_mtws_endpoint_url();
        let enabled = self.config.mtws.enabled;
        
        (is_running, interval, endpoint, enabled)
    }

    pub fn get_imei(&self) -> String {
        self.config.mtws.imei.clone()
    }

    pub fn get_endpoint_url(&self) -> String {
        self.config.get_mtws_endpoint_url()
    }

    pub async fn set_transmission_interval(&self, seconds: u64) -> Result<(), ModbusError> {
        if seconds < 1 {
            return Err(ModbusError::InvalidData("Transmission interval must be at least 1 second".to_string()));
        }

        let mut interval = self.transmission_interval.write().await;
        *interval = Duration::from_secs(seconds);
        
        info!("⏱️ MTWS transmission interval updated to {} seconds", seconds);
        Ok(())
    }
}