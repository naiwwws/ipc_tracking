use log::{info, error, warn};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::time::{Duration, interval};
use chrono::Utc;
use serde::{Deserialize, Serialize};

use crate::config::Config;
use crate::services::DataService;
use crate::utils::error::ModbusError;
use crate::storage::models::{MtwsPayload, MtwsField};

#[derive(Clone)]
pub struct MtwsService {
    data_service: Arc<DataService>,
    config: Config,
    is_running: Arc<RwLock<bool>>,
    transmission_interval: Arc<RwLock<Duration>>,
    endpoint_url: Arc<RwLock<Option<String>>>,
}

// Add the missing FlowmeterValues struct
#[derive(Debug, Clone, Serialize, Deserialize)]
struct FlowmeterValues {
    pub volume_total: f32,
    pub density_flow: f32,
    pub temperature: f32,
    pub mass_flow_rate: f32,
}

impl MtwsService {
    pub fn new(data_service: Arc<DataService>, config: Config) -> Self {
        let endpoint_url = if config.mtws.enabled {
            Some(config.get_mtws_endpoint_url())
        } else {
            None
        };

        Self {
            data_service,
            config: config.clone(),
            is_running: Arc::new(RwLock::new(false)),
            transmission_interval: Arc::new(RwLock::new(Duration::from_secs(config.mtws.transmission_interval_seconds))),
            endpoint_url: Arc::new(RwLock::new(endpoint_url)),
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

        // Validate configuration
        self.config.validate_mtws_config()
            .map_err(|e| ModbusError::InvalidData(format!("MTWS config error: {}", e)))?;

        *is_running = true;
        info!("🛰️ Starting MTWS transmission service");
        info!("📡 Endpoint: {}", self.config.get_mtws_endpoint_url());
        info!("⏱️ Interval: {} seconds", self.config.mtws.transmission_interval_seconds);

        // Clone necessary data for the background task
        let data_service = Arc::clone(&self.data_service);
        let endpoint_url = self.config.get_mtws_endpoint_url();
        let interval_duration = Duration::from_secs(self.config.mtws.transmission_interval_seconds);
        let is_running_clone = Arc::clone(&self.is_running);

        // Start background transmission task
        tokio::spawn(async move {
            let mut interval_timer = interval(interval_duration);
            
            while *is_running_clone.read().await {
                interval_timer.tick().await;
                
                if !*is_running_clone.read().await {
                    break;
                }

                info!("🛰️ Periodic MTWS transmission starting...");
                match Self::generate_and_send_payload(&data_service, &endpoint_url).await {
                    Ok(_) => info!("✅ Periodic MTWS transmission completed successfully"),
                    Err(e) => error!("❌ Periodic MTWS transmission failed: {}", e),
                }
            }
            
            info!("🛰️ MTWS transmission service stopped");
        });

        Ok(())
    }

    // Fix stop_transmission to return Result
    pub async fn stop_transmission(&self) -> Result<(), ModbusError> {
        let mut is_running = self.is_running.write().await;
        *is_running = false;
        info!("🛰️ MTWS transmission service stop requested");
        Ok(())
    }

    pub async fn send_data_once(&self) -> Result<(), ModbusError> {
        if !self.config.mtws.enabled {
            return Err(ModbusError::ServiceNotAvailable("MTWS service is disabled".to_string()));
        }

        let endpoint_url = self.config.get_mtws_endpoint_url();
        info!("🛰️ Sending single MTWS payload to: {}", endpoint_url);
        
        Self::generate_and_send_payload(&self.data_service, &endpoint_url).await?;
        
        Ok(())
    }

    // Add missing send_single_payload method
    pub async fn send_single_payload(&self, endpoint_url: String) -> Result<(), ModbusError> {
        info!("🛰️ Sending single MTWS payload to custom endpoint: {}", endpoint_url);
        Self::generate_and_send_payload(&self.data_service, &endpoint_url).await?;
        Ok(())
    }

    async fn generate_and_send_payload(data_service: &DataService, endpoint_url: &str) -> Result<MtwsPayload, ModbusError> {
        let payload = Self::build_payload(data_service).await?;
        
        // Convert payload to form data using correct field names
        let form_data: Vec<(String, String)> = payload.fields.iter()
            .map(|field| (field.name.clone(), field.value.clone()))
            .collect();
        
        info!("🛰️ Sending MTWS payload to {} with {} fields", endpoint_url, form_data.len());
        info!("📊 Sample fields: {:?}", form_data.iter().take(5).collect::<Vec<_>>());
        
        // Send payload as form data (POST)
        let client = reqwest::Client::new();
        let response = client
            .post(endpoint_url)
            .header("Content-Type", "application/x-www-form-urlencoded")
            .header("User-Agent", "IPC-Track-Device/1.0")
            .form(&form_data)
            .timeout(Duration::from_secs(30))
            .send()
            .await
            .map_err(|e| ModbusError::CommunicationError(format!("HTTP POST request failed: {}", e)))?;

        let status = response.status();
        let response_text = response.text().await.unwrap_or_default();

        if status.is_success() {
            info!("✅ MTWS payload sent successfully to {} - Status: {} - Response: {}", 
                  endpoint_url, status, response_text);
        } else {
            error!("❌ MTWS endpoint returned error: {} - {}", status, response_text);
            return Err(ModbusError::CommunicationError(format!("HTTP {} - {}", status, response_text)));
        }

        Ok(payload)
    }

    async fn build_payload(data_service: &DataService) -> Result<MtwsPayload, ModbusError> {
        let mut payload = MtwsPayload::new();

        // 1. Add timestamp
        payload.add_field("timestamp".to_string(), Utc::now().timestamp().to_string());

        // 2. Add GPS data
        if let Some(gps_data) = data_service.get_current_gps_data().await {
            payload.add_field("longitude".to_string(), ((gps_data.longitude.unwrap_or(0.0) * 1_000_000.0) as i64).to_string());
            payload.add_field("latitude".to_string(), ((gps_data.latitude.unwrap_or(0.0) * 1_000_000.0) as i64).to_string());
            payload.add_field("speed".to_string(), (gps_data.speed.unwrap_or(0.0) as i32).to_string());
            payload.add_field("heading".to_string(), (gps_data.course.unwrap_or(0.0) as i32).to_string());
            payload.add_field("altitude".to_string(), (gps_data.altitude.unwrap_or(0.0) as i32).to_string());
            payload.add_field("gpsNumSats".to_string(), gps_data.satellites.unwrap_or(0).to_string());
        } else {
            // Default GPS values
            payload.add_field("longitude".to_string(), "0".to_string());
            payload.add_field("latitude".to_string(), "0".to_string());
            payload.add_field("speed".to_string(), "0".to_string());
            payload.add_field("heading".to_string(), "0".to_string());
            payload.add_field("altitude".to_string(), "0".to_string());
            payload.add_field("gpsNumSats".to_string(), "0".to_string());
        }

        // 3. Add power/battery data
        payload.add_field("batteryVoltage".to_string(), "8086".to_string());
        payload.add_field("extPowerVoltage".to_string(), "27838".to_string());

        // 4. Add environmental data
        payload.add_field("windSpeed".to_string(), "0".to_string());
        payload.add_field("windDirection".to_string(), "0".to_string());

        // 5. DYNAMIC FLOWMETER DATA
        let flowmeter_devices = data_service.get_flowmeter_devices();
        info!("🔍 Found {} configured flowmeter devices", flowmeter_devices.len());

        let mut flowmeter_data_map: HashMap<usize, FlowmeterValues> = HashMap::new();

        // Get real data from all configured flowmeters
        for (index, device_config) in flowmeter_devices.iter().enumerate() {
            let flowmeter_number = index + 1;
            let device_address = device_config.address;
            
            info!("🌊 Processing flowmeter {} at address {} ({})", 
                  flowmeter_number, device_address, device_config.name);

            // Try to get fresh flowmeter data directly from Modbus
            if let Some(flowmeter_data) = data_service.get_current_flowmeter_data(device_address).await {
                let values = FlowmeterValues {
                    volume_total: flowmeter_data.volume_total,
                    density_flow: flowmeter_data.density_flow,
                    temperature: flowmeter_data.temperature,
                    mass_flow_rate: flowmeter_data.mass_flow_rate,
                };
                
                flowmeter_data_map.insert(flowmeter_number, values);
                
                info!("✅ Got fresh flowmeter {} data: VT={}, D={}, T={}, FR={}", 
                      flowmeter_number, 
                      flowmeter_data.volume_total,
                      flowmeter_data.density_flow,
                      flowmeter_data.temperature,
                      flowmeter_data.mass_flow_rate);
            } else {
                warn!("⚠️ No data found for flowmeter {} (address {})", flowmeter_number, device_address);
            }
        }

        // Add flowmeter data to payload - support up to 8 flowmeters dynamically
        let max_flowmeters = std::cmp::max(4, flowmeter_devices.len());
        
        for flowmeter_number in 1..=max_flowmeters {
            if let Some(values) = flowmeter_data_map.get(&flowmeter_number) {
                // Add real data
                payload.add_field(
                    format!("flowmeterVolumeTotal{}", flowmeter_number),
                    values.volume_total.to_string()
                );
                payload.add_field(
                    format!("flowmeterDensity{}", flowmeter_number),
                    values.density_flow.to_string()
                );
                payload.add_field(
                    format!("flowmeterTemperature{}", flowmeter_number),
                    values.temperature.to_string()
                );
                payload.add_field(
                    format!("flowmeterFlowrate{}", flowmeter_number),
                    values.mass_flow_rate.to_string()
                );
                
                info!("📦 Added flowmeter {} to payload with real data", flowmeter_number);
            } else {
                // Add default values for missing flowmeters
                Self::add_default_flowmeter_values(&mut payload, flowmeter_number);
                info!("📦 Added flowmeter {} to payload with default values", flowmeter_number);
            }
        }

        // 6. Add fuel level
        payload.add_field("fuelLevelMM".to_string(), "0".to_string());

        info!("📦 Built MTWS payload with {} fields", payload.fields.len());
        Ok(payload)
    }

    fn add_default_flowmeter_values(payload: &mut MtwsPayload, flowmeter_number: usize) {
        payload.add_field(format!("flowmeterVolumeTotal{}", flowmeter_number), "0".to_string());
        payload.add_field(format!("flowmeterDensity{}", flowmeter_number), "0".to_string());
        payload.add_field(format!("flowmeterTemperature{}", flowmeter_number), "0".to_string());
        payload.add_field(format!("flowmeterFlowrate{}", flowmeter_number), "0".to_string());
    }

    // Status and configuration methods
    pub async fn is_running(&self) -> bool {
        *self.is_running.read().await
    }

    pub async fn get_transmission_interval(&self) -> Duration {
        *self.transmission_interval.read().await
    }

    pub fn get_imei(&self) -> String {
        self.config.mtws.imei.clone()
    }

    pub fn get_endpoint_url(&self) -> String {
        self.config.get_mtws_endpoint_url()
    }

    pub async fn set_imei(&mut self, imei: String) -> Result<(), ModbusError> {
        // Validate IMEI (should be 15 digits)
        if imei.len() != 15 || !imei.chars().all(|c| c.is_ascii_digit()) {
            return Err(ModbusError::InvalidData("IMEI must be exactly 15 digits".to_string()));
        }

        // Update config
        self.config.mtws.imei = imei.clone();
        
        // Update endpoint URL
        let new_endpoint = self.config.get_mtws_endpoint_url();
        let mut endpoint = self.endpoint_url.write().await;
        *endpoint = Some(new_endpoint.clone());
        
        info!("🆔 IMEI updated to: {} - New endpoint: {}", imei, new_endpoint);
        
        Ok(())
    }

    pub async fn set_transmission_interval(&mut self, seconds: u64) -> Result<(), ModbusError> {
        if seconds < 60 {
            return Err(ModbusError::InvalidData("Transmission interval must be at least 60 seconds".to_string()));
        }

        self.config.mtws.transmission_interval_seconds = seconds;
        let mut interval = self.transmission_interval.write().await;
        *interval = Duration::from_secs(seconds);
        
        info!("⏱️ MTWS transmission interval set to {} seconds", seconds);
        Ok(())
    }

    pub async fn set_endpoint(&mut self, base_url: String) -> Result<(), ModbusError> {
        if base_url.is_empty() {
            return Err(ModbusError::InvalidData("Endpoint URL cannot be empty".to_string()));
        }

        self.config.mtws.base_endpoint_url = base_url.clone();
        let new_endpoint = self.config.get_mtws_endpoint_url();
        let mut endpoint = self.endpoint_url.write().await;
        *endpoint = Some(new_endpoint.clone());
        
        info!("🔗 MTWS base endpoint set to: {} - Full endpoint: {}", base_url, new_endpoint);
        Ok(())
    }

    // Add missing get_status method
    pub async fn get_status(&self) -> (bool, u64, String, bool) {
        let is_running = *self.is_running.read().await;
        let interval = self.transmission_interval.read().await.as_secs();
        let endpoint = self.get_endpoint_url();
        let enabled = self.config.mtws.enabled;
        
        (is_running, interval, endpoint, enabled)
    }

    // Add missing set_endpoint_url method
    pub async fn set_endpoint_url(&self, endpoint_url: String) -> Result<(), ModbusError> {
        // Parse to extract base URL and potentially update IMEI
        if let Some(base_url) = endpoint_url.rsplit_once('/').map(|(base, _)| base.to_string()) {
            // Update base endpoint
            let mut config = self.config.clone();
            config.mtws.base_endpoint_url = base_url;
            
            // Update endpoint URL
            let new_endpoint = config.get_mtws_endpoint_url();
            let mut endpoint = self.endpoint_url.write().await;
            *endpoint = Some(new_endpoint.clone());
            
            info!("🔗 MTWS endpoint URL updated to: {}", new_endpoint);
            Ok(())
        } else {
            Err(ModbusError::InvalidData("Invalid endpoint URL format".to_string()))
        }
    }

    // Add missing get_current_payload method
    pub async fn get_current_payload(data_service: &DataService) -> Result<crate::storage::models::MtwsPayload, ModbusError> {
        Self::build_payload(data_service).await
    }

    // Add missing update_config method
    pub async fn update_config(&self, config: crate::storage::models::MtwsConfig) -> Result<(), ModbusError> {
        // Update internal config - this is a simplified version
        // In a real implementation, you'd want to properly update the config
        info!("🔧 MTWS configuration update requested");
        // For now, just log the update request
        info!("📝 New config: enabled={}, interval={}s", config.enabled, config.transmission_interval_seconds);
        Ok(())
    }
}