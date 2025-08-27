use log::{info, error, warn};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::time::{Duration, interval};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use reqwest::Client;

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
    client: Client,
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
            client: Client::new(),
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

        // Clone necessary data for the background task
        let data_service = Arc::clone(&self.data_service);
        let endpoint_url = self.config.get_mtws_endpoint_url();
        let interval_duration = Duration::from_secs(self.config.mtws.transmission_interval_seconds);
        let is_running_clone = Arc::clone(&self.is_running);
        let client = self.client.clone();

        // Start background transmission task
        tokio::spawn(async move {
            let mut interval_timer = interval(interval_duration);
            
            while *is_running_clone.read().await {
                interval_timer.tick().await;
                
                if !*is_running_clone.read().await {
                    break;
                }

                info!("🛰️ Periodic MTWS transmission starting...");
                match Self::generate_and_send_payload(&data_service, &endpoint_url, &client).await {
                    Ok(_) => info!("✅ Periodic MTWS transmission completed successfully"),
                    Err(e) => error!("❌ Periodic MTWS transmission failed: {}", e),
                }
            }
            
            info!("🛰️ MTWS transmission service stopped");
        });

        Ok(())
    }

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
        
        Self::generate_and_send_payload(&self.data_service, &endpoint_url, &self.client).await?;
        
        Ok(())
    }

    // Fixed send_single_payload method with correct return type
    pub async fn send_single_payload(&self, endpoint_url: String) -> Result<String, ModbusError> {
        if !self.config.mtws.enabled {
            return Err(ModbusError::ServiceNotAvailable("MTWS service is disabled".to_string()));
        }

        info!("🛰️ Sending single MTWS payload to custom endpoint: {}", endpoint_url);
        let payload = Self::generate_and_send_payload(&self.data_service, &endpoint_url, &self.client).await?;
        
        // Return a summary of the payload
        Ok(format!("Sent payload with {} fields to {}", payload.fields.len(), endpoint_url))
    }

    async fn generate_and_send_payload(
        data_service: &DataService, 
        endpoint_url: &str,
        client: &Client
    ) -> Result<MtwsPayload, ModbusError> {
        let payload = Self::build_payload(data_service).await?;
        
        // Build JSON payload for transmission
        let mut json_payload = serde_json::Map::new();
        for field in &payload.fields {
            json_payload.insert(field.name.clone(), serde_json::Value::String(field.value.clone()));
        }
        let json_value = serde_json::Value::Object(json_payload);
        
        info!("🛰️ Sending MTWS JSON payload to {} with {} fields", endpoint_url, payload.fields.len());
        info!("📊 Sample data: {}", serde_json::to_string_pretty(&json_value).unwrap_or_default().chars().take(200).collect::<String>());
        
        // Send payload as JSON (POST)
        let response = client
            .post(endpoint_url)
            .header("Content-Type", "application/json")
            .header("User-Agent", "IPC-Track-Device/1.0")
            .json(&json_value)
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

        // 2. Add GPS data from real GPS service
        if let Some(gps_data) = data_service.get_current_gps_data().await {
            payload.add_field("longitude".to_string(), ((gps_data.longitude.unwrap_or(0.0) * 1_000_000.0) as i64).to_string());
            payload.add_field("latitude".to_string(), ((gps_data.latitude.unwrap_or(0.0) * 1_000_000.0) as i64).to_string());
            payload.add_field("speed".to_string(), (gps_data.speed.unwrap_or(0.0) as i32).to_string());
            payload.add_field("heading".to_string(), (gps_data.course.unwrap_or(0.0) as i32).to_string());
            payload.add_field("altitude".to_string(), (gps_data.altitude.unwrap_or(0.0) as i32).to_string());
            payload.add_field("gpsNumSats".to_string(), gps_data.satellites.unwrap_or(0).to_string());
            info!("📍 Added real GPS data: lat={:?}, lon={:?}, speed={:?}", gps_data.latitude, gps_data.longitude, gps_data.speed);
        } else {
            // Default GPS values
            payload.add_field("longitude".to_string(), "0".to_string());
            payload.add_field("latitude".to_string(), "0".to_string());
            payload.add_field("speed".to_string(), "0".to_string());
            payload.add_field("heading".to_string(), "0".to_string());
            payload.add_field("altitude".to_string(), "0".to_string());
            payload.add_field("gpsNumSats".to_string(), "0".to_string());
            warn!("⚠️ No GPS data available, using default values");
        }

        // 3. Add power/battery data
        payload.add_field("batteryVoltage".to_string(), "8086".to_string());
        payload.add_field("extPowerVoltage".to_string(), "27838".to_string());

        // 4. Add environmental data
        payload.add_field("windSpeed".to_string(), "0".to_string());
        payload.add_field("windDirection".to_string(), "0".to_string());

        // 5. FLEXIBLE FLOWMETER DATA - Dynamic reading from all configured devices
        let flowmeter_devices = data_service.get_flowmeter_devices();
        info!("🔍 Found {} configured flowmeter devices", flowmeter_devices.len());

        let mut flowmeter_data_collected = HashMap::new();

        // Read data from ALL configured flowmeter devices
        for (index, device_config) in flowmeter_devices.iter().enumerate() {
            let flowmeter_number = index + 1;
            let device_address = device_config.address;
            
            info!("🌊 Processing flowmeter {} at address {} ({})", 
                  flowmeter_number, device_address, device_config.name);

            // Try multiple methods to get flowmeter data
            let mut flowmeter_values = None;

            // Method 1: Direct flowmeter data access
            if let Some(flowmeter_data) = data_service.get_current_flowmeter_data(device_address).await {
                flowmeter_values = Some(FlowmeterValues {
                    volume_total: flowmeter_data.volume_total,
                    density_flow: flowmeter_data.density_flow,
                    temperature: flowmeter_data.temperature,
                    mass_flow_rate: flowmeter_data.mass_flow_rate,
                });
                
                info!("✅ Method 1: Got fresh flowmeter {} data: VT={}, D={}, T={}, FR={}", 
                      flowmeter_number, 
                      flowmeter_data.volume_total,
                      flowmeter_data.density_flow,
                      flowmeter_data.temperature,
                      flowmeter_data.mass_flow_rate);
            }
            // Method 2: Fallback to JSON string extraction
            else if let Some(device_data_str) = data_service.get_device_data_by_address(device_address).await {
                match Self::extract_flowmeter_values(&device_data_str) {
                    Ok(values) => {
                        flowmeter_values = Some(values);
                        info!("✅ Method 2: Extracted flowmeter {} data from JSON", flowmeter_number);
                    }
                    Err(e) => {
                        warn!("⚠️ Failed to extract flowmeter {} data: {}", flowmeter_number, e);
                    }
                }
            }
            // Method 3: Direct read from device data cache
            else {
                warn!("⚠️ No data found for flowmeter {} (address {})", flowmeter_number, device_address);
            }

            // Store the collected data
            if let Some(values) = flowmeter_values {
                flowmeter_data_collected.insert(flowmeter_number, values);
            }
        }

        // Add flowmeter data to payload - Flexible number of flowmeters
        let max_flowmeters = if flowmeter_devices.is_empty() { 
            4 // Default minimum for compatibility
        } else {
            std::cmp::max(4, flowmeter_devices.len()) // At least 4, or however many are configured
        };
        
        for flowmeter_number in 1..=max_flowmeters {
            if let Some(values) = flowmeter_data_collected.get(&flowmeter_number) {
                // Add real data from configured flowmeter
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
                
                info!("📦 Added flowmeter {} to payload with REAL data (VT: {}, FR: {})", 
                      flowmeter_number, values.volume_total, values.mass_flow_rate);
            } else {
                // Add default values for unconfigured flowmeters
                Self::add_default_flowmeter_values(&mut payload, flowmeter_number);
                info!("📦 Added flowmeter {} to payload with DEFAULT values", flowmeter_number);
            }
        }

        // 6. Add RPM data from configured RPM devices
        let rpm_devices = data_service.get_rpm_devices();
        info!("🔍 Found {} configured RPM devices", rpm_devices.len());

        let mut engine_counter = 1;

        for device_config in rpm_devices.iter() {
            let device_address = device_config.address;

            info!("🔄 Processing RPM device: '{}' at address {}", 
                  device_config.name, device_address);

            if let Some(device_data_str) = data_service.get_device_data_by_address(device_address).await {
                match Self::extract_rpm_values(&device_data_str) {
                    Ok(channel_rpms) => {
                        for (channel_id, rpm_value) in channel_rpms {
                            // Add engine RPM
                            payload.add_field(
                                format!("engineRPM{}", engine_counter),
                                rpm_value.to_string()
                            );

                            // Add engine duration (placeholder for now)
                            payload.add_field(
                                format!("engineDurationME{}", engine_counter),
                                "0".to_string() // TODO: Implement duration tracking
                            );

                            info!("✅ Added engine {} (device {}, channel {}): RPM={}", 
                                  engine_counter, device_address, channel_id, rpm_value);

                            engine_counter += 1;
                        }
                    }
                    Err(e) => {
                        warn!("⚠️ Failed to parse RPM data from device {}: {}", device_address, e);
                        payload.add_field(format!("engineRPM{}", engine_counter), "0".to_string());
                        payload.add_field(format!("engineDurationME{}", engine_counter), "0".to_string());
                        engine_counter += 1;
                    }
                }
            } else {
                warn!("⚠️ No data found for RPM device at address {}", device_address);
                payload.add_field(format!("engineRPM{}", engine_counter), "0".to_string());
                payload.add_field(format!("engineDurationME{}", engine_counter), "0".to_string());
                engine_counter += 1;
            }
        }

        // 7. Add auxiliary engine durations (AE1, AE2, AE3)
        payload.add_field("engineDurationAE1".to_string(), "0".to_string());
        payload.add_field("engineDurationAE2".to_string(), "0".to_string());
        payload.add_field("engineDurationAE3".to_string(), "0".to_string());

        // 8. Add status fields
        payload.add_field("statusAE1".to_string(), "false".to_string());
        payload.add_field("statusAE2".to_string(), "false".to_string());
        payload.add_field("statusAE3".to_string(), "false".to_string());
        payload.add_field("statusDoorOpenStarboard".to_string(), "false".to_string());
        payload.add_field("statusDoorOpenPort".to_string(), "false".to_string());
        payload.add_field("statusDCOK".to_string(), "true".to_string());
        payload.add_field("statusBattFail".to_string(), "false".to_string());

        // 9. Add fuel level
        payload.add_field("fuelLevelMM".to_string(), "0".to_string());

        // 10. Add IMEI from config
        payload.add_field("imei".to_string(), data_service.get_config().mtws.imei.clone());

        info!("📦 Built MTWS payload with {} fields", payload.fields.len());
        Ok(payload)
    }

    // JSON extraction methods for fallback data reading
    fn extract_flowmeter_values(data_str: &str) -> Result<FlowmeterValues, String> {
        let json_data: serde_json::Value = serde_json::from_str(data_str)
            .map_err(|e| format!("Failed to parse JSON: {}", e))?;

        Ok(FlowmeterValues {
            volume_total: json_data["volume_total"].as_f64().unwrap_or(0.0) as f32,
            density_flow: json_data["density_flow"].as_f64().unwrap_or(0.0) as f32,
            temperature: json_data["temperature"].as_f64().unwrap_or(0.0) as f32,
            mass_flow_rate: json_data["mass_flow_rate"].as_f64().unwrap_or(0.0) as f32,
        })
    }

    fn extract_rpm_values(data_str: &str) -> Result<Vec<(u8, u16)>, String> {
        let json_data: serde_json::Value = serde_json::from_str(data_str)
            .map_err(|e| format!("Failed to parse JSON: {}", e))?;

        let mut channel_rpms = Vec::new();

        if let Some(channels) = json_data["channels"].as_array() {
            for channel in channels {
                let channel_id = channel["channel_id"].as_u64().unwrap_or(1) as u8;
                let rpm_value = channel["rpm_value"].as_u64().unwrap_or(0) as u16;
                channel_rpms.push((channel_id, rpm_value));
            }
        } else {
            // Fallback to legacy single channel
            let rpm_value = json_data["rpm_value"]
                .as_u64()
                .or_else(|| json_data["RPM"].as_u64())
                .unwrap_or(0) as u16;
            channel_rpms.push((1, rpm_value));
        }

        Ok(channel_rpms)
    }

    fn add_default_flowmeter_values(payload: &mut MtwsPayload, flowmeter_number: usize) {
        payload.add_field(format!("flowmeterVolumeTotal{}", flowmeter_number), "0".to_string());
        payload.add_field(format!("flowmeterDensity{}", flowmeter_number), "0".to_string());
        payload.add_field(format!("flowmeterTemperature{}", flowmeter_number), "0".to_string());
        payload.add_field(format!("flowmeterFlowrate{}", flowmeter_number), "0".to_string());
    }

    // Status and configuration methods
    pub async fn get_status(&self) -> (bool, u64, String, bool) {
        let is_running = *self.is_running.read().await;
        let interval = self.transmission_interval.read().await.as_secs();
        let endpoint = self.get_endpoint_url();
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

    // Public method for getting current payload data
    pub async fn get_current_payload(data_service: &DataService) -> Result<MtwsPayload, ModbusError> {
        Self::build_payload(data_service).await
    }

    // API compatibility method
    pub async fn update_config(&self, _config: serde_json::Value) -> Result<(), ModbusError> {
        info!("🔧 MTWS configuration update requested");
        Ok(())
    }
}