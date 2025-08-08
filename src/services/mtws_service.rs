use log::{info, error, warn};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::time::Duration;
use chrono::Utc;
use serde::{Deserialize, Serialize};

use crate::config::Config;
use crate::services::DataService;
use crate::utils::error::ModbusError;
use crate::storage::models::{MtwsPayload}; // Import from models

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
        Self {
            data_service,
            config,
            is_running: Arc::new(RwLock::new(false)),
            transmission_interval: Arc::new(RwLock::new(Duration::from_secs(300))), // Default 5 minutes
            endpoint_url: Arc::new(RwLock::new(None)),
        }
    }

    pub async fn set_transmission_interval(&self, seconds: u64) {
        let mut interval = self.transmission_interval.write().await;
        *interval = Duration::from_secs(seconds);
        info!("🛰️ MTWS transmission interval set to {} seconds", seconds);
    }

    pub async fn set_endpoint_url(&self, url: String) {
        let mut endpoint = self.endpoint_url.write().await;
        *endpoint = Some(url.clone());
        info!("🛰️ MTWS endpoint URL set to: {}", url);
    }

    pub async fn start_transmission(&self) -> Result<(), ModbusError> {
        let mut is_running = self.is_running.write().await;
        if *is_running {
            return Err(ModbusError::ServiceNotAvailable("MTWS transmission already running".to_string()));
        }
        *is_running = true;
        drop(is_running);

        let data_service = self.data_service.clone();
        let transmission_interval = self.transmission_interval.clone();
        let endpoint_url = self.endpoint_url.clone();
        let is_running_check = self.is_running.clone();

        tokio::spawn(async move {
            info!("🛰️ Starting MTWS transmission service");
            
            loop {
                let should_continue = {
                    let running = is_running_check.read().await;
                    *running
                };

                if !should_continue {
                    break;
                }

                let interval_duration = {
                    let interval = transmission_interval.read().await;
                    *interval
                };

                let url = {
                    let endpoint = endpoint_url.read().await;
                    endpoint.clone()
                };

                if let Some(url) = url {
                    match Self::generate_and_send_payload(&data_service, &url).await {
                        Ok(_) => info!("✅ MTWS payload sent successfully"),
                        Err(e) => error!("❌ Failed to send MTWS payload: {}", e),
                    }
                } else {
                    warn!("⚠️ No MTWS endpoint URL configured");
                }

                tokio::time::sleep(interval_duration).await;
            }

            info!("🛑 MTWS transmission service stopped");
        });

        Ok(())
    }

    pub async fn stop_transmission(&self) -> Result<(), ModbusError> {
        let mut is_running = self.is_running.write().await;
        *is_running = false;
        info!("🛑 Stopping MTWS transmission service");
        Ok(())
    }

    pub async fn send_single_payload(&self, endpoint_url: Option<String>) -> Result<MtwsPayload, ModbusError> {
        let url = if let Some(url) = endpoint_url {
            url
        } else {
            let endpoint = self.endpoint_url.read().await;
            endpoint.clone().ok_or_else(|| ModbusError::ServiceNotAvailable("No endpoint URL configured".to_string()))?
        };

        Self::generate_and_send_payload(&self.data_service, &url).await
    }

    async fn generate_and_send_payload(data_service: &DataService, endpoint_url: &str) -> Result<MtwsPayload, ModbusError> {
        let payload = Self::build_payload(data_service).await?;
        
        // Send payload to MTWS endpoint
        let client = reqwest::Client::new();
        let response = client
            .post(endpoint_url)
            .json(&payload)
            .send()
            .await
            .map_err(|e| ModbusError::CommunicationError(format!("HTTP request failed: {}", e)))?;

        if response.status().is_success() {
            info!("🛰️ MTWS payload sent successfully to {}", endpoint_url);
        } else {
            error!("❌ MTWS endpoint returned error: {}", response.status());
        }

        Ok(payload)
    }

    // Add a new public method that wraps the private build_payload
    pub async fn get_current_payload(data_service: &DataService) -> Result<MtwsPayload, ModbusError> {
        Self::build_payload(data_service).await
    }

    // Keep build_payload private
    async fn build_payload(data_service: &DataService) -> Result<MtwsPayload, ModbusError> {
        let mut payload = MtwsPayload::new();

        // 1. Add timestamp
        payload.add_field("timestamp".to_string(), Utc::now().timestamp().to_string());

        // 2. Add GPS data (longitude, latitude, speed, heading, altitude, gpsNumSats)
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

        // 5. Add flowmeter data (4 flowmeters: VolumeTotal, Density, Temperature, Flowrate)
        let flowmeter_devices = data_service.get_flowmeter_devices();
        info!("🔍 Found {} configured flowmeter devices", flowmeter_devices.len());

        // Always add 4 flowmeters (fill with defaults if not available)
        for flowmeter_number in 1..=4 {
            if let Some(device_config) = flowmeter_devices.get(flowmeter_number - 1) {
                let device_address = device_config.address;
                
                if let Some(device_data_str) = data_service.get_device_data_by_address(device_address).await {
                    match Self::extract_flowmeter_values(&device_data_str) {
                        Ok(values) => {
                            payload.add_field(format!("flowmeterVolumeTotal{}", flowmeter_number), values.volume_total.to_string());
                            info!("✅ Added flowmeter {} VolumeTotal: {}", flowmeter_number, values.volume_total);
                        }
                        Err(_) => {
                            payload.add_field(format!("flowmeterVolumeTotal{}", flowmeter_number), "0".to_string());
                        }
                    }
                } else {
                    payload.add_field(format!("flowmeterVolumeTotal{}", flowmeter_number), "0".to_string());
                }
            } else {
                payload.add_field(format!("flowmeterVolumeTotal{}", flowmeter_number), "0".to_string());
            }
        }

        // Add flowmeter density fields
        for flowmeter_number in 1..=4 {
            if let Some(device_config) = flowmeter_devices.get(flowmeter_number - 1) {
                let device_address = device_config.address;
                
                if let Some(device_data_str) = data_service.get_device_data_by_address(device_address).await {
                    match Self::extract_flowmeter_values(&device_data_str) {
                        Ok(values) => {
                            payload.add_field(format!("flowmeterDensity{}", flowmeter_number), values.density_flow.to_string());
                        }
                        Err(_) => {
                            payload.add_field(format!("flowmeterDensity{}", flowmeter_number), "0".to_string());
                        }
                    }
                } else {
                    payload.add_field(format!("flowmeterDensity{}", flowmeter_number), "0".to_string());
                }
            } else {
                payload.add_field(format!("flowmeterDensity{}", flowmeter_number), "0".to_string());
            }
        }

        // Add flowmeter temperature fields
        for flowmeter_number in 1..=4 {
            if let Some(device_config) = flowmeter_devices.get(flowmeter_number - 1) {
                let device_address = device_config.address;
                
                if let Some(device_data_str) = data_service.get_device_data_by_address(device_address).await {
                    match Self::extract_flowmeter_values(&device_data_str) {
                        Ok(values) => {
                            payload.add_field(format!("flowmeterTemperature{}", flowmeter_number), values.temperature.to_string());
                        }
                        Err(_) => {
                            payload.add_field(format!("flowmeterTemperature{}", flowmeter_number), "0".to_string());
                        }
                    }
                } else {
                    payload.add_field(format!("flowmeterTemperature{}", flowmeter_number), "0".to_string());
                }
            } else {
                payload.add_field(format!("flowmeterTemperature{}", flowmeter_number), "0".to_string());
            }
        }

        // Add flowmeter flowrate fields
        for flowmeter_number in 1..=4 {
            if let Some(device_config) = flowmeter_devices.get(flowmeter_number - 1) {
                let device_address = device_config.address;
                
                if let Some(device_data_str) = data_service.get_device_data_by_address(device_address).await {
                    match Self::extract_flowmeter_values(&device_data_str) {
                        Ok(values) => {
                            payload.add_field(format!("flowmeterFlowrate{}", flowmeter_number), values.mass_flow_rate.to_string());
                        }
                        Err(_) => {
                            payload.add_field(format!("flowmeterFlowrate{}", flowmeter_number), "0".to_string());
                        }
                    }
                } else {
                    payload.add_field(format!("flowmeterFlowrate{}", flowmeter_number), "0".to_string());
                }
            } else {
                payload.add_field(format!("flowmeterFlowrate{}", flowmeter_number), "0".to_string());
            }
        }

        // 6. Add fuel level
        payload.add_field("fuelLevelMM".to_string(), "0".to_string());

        // 7. Add RPM data (get from SQL database-stored engine durations)
        let rpm_devices = data_service.get_rpm_devices();
        info!("🔍 Found {} configured RPM devices", rpm_devices.len());

        let mut engine_counter = 1;

        let stored_durations: HashMap<u8, i32> = HashMap::new(); // TODO: implement data_service.get_engine_durations().await


        for device_config in rpm_devices.iter() {
            let device_address = device_config.address;

            info!("🔄 Processing multi-channel RPM device: '{}' at address {}", 
                  device_config.name, device_address);

            if let Some(device_data_str) = data_service.get_device_data_by_address(device_address).await {
                match Self::extract_rpm_values(&device_data_str) {
                    Ok(channel_rpms) => {
                        for (channel_id, rpm_value) in channel_rpms {
                            // Create unique engine address for duration tracking
                            let engine_address = (device_address as u16 * 100 + channel_id as u16) as u8;

                            // Add engine RPM
                            payload.add_field(
                                format!("engineRPM{}", engine_counter),
                                rpm_value.to_string()
                            );

                            // Get stored duration from database (in minutes)
                            let duration_minutes = stored_durations.get(&engine_address).copied().unwrap_or(0);

                            // Add engine duration to payload 
                            payload.add_field(
                                format!("engineDurationME{}", engine_counter),
                                duration_minutes.to_string()
                            );

                            info!("✅ Added engine {} (device {}, channel {}): RPM={}, Duration={}min", 
                                  engine_counter, device_address, channel_id, rpm_value, duration_minutes);

                            engine_counter += 1;
                        }
                    }
                    Err(e) => {
                        warn!("⚠️ Failed to parse multi-channel RPM data from device {}: {}", device_address, e);
                        payload.add_field(format!("engineRPM{}", engine_counter), "0".to_string());
                        payload.add_field(format!("engineDurationME{}", engine_counter), "0".to_string());
                        engine_counter += 1;
                    }
                }
            } else {
                payload.add_field(format!("engineRPM{}", engine_counter), "0".to_string());
                payload.add_field(format!("engineDurationME{}", engine_counter), "0".to_string());
                engine_counter += 1;
            }
        }

        // 8. Add auxiliary engine durations (AE1, AE2, AE3)
        payload.add_field("engineDurationAE1".to_string(), "0".to_string());
        payload.add_field("engineDurationAE2".to_string(), "0".to_string());
        payload.add_field("engineDurationAE3".to_string(), "0".to_string());

        // 9. Add status fields
        payload.add_field("statusAE1".to_string(), "false".to_string());
        payload.add_field("statusAE2".to_string(), "false".to_string());
        payload.add_field("statusAE3".to_string(), "false".to_string());
        payload.add_field("statusDoorOpenStarboard".to_string(), "false".to_string());
        payload.add_field("statusDoorOpenPort".to_string(), "false".to_string());
        payload.add_field("statusDCOK".to_string(), "true".to_string());
        payload.add_field("statusBattFail".to_string(), "false".to_string());

        info!("📦 Built MTWS payload with {} fields", payload.fields.len());
        Ok(payload)
    }

    // Helper methods for extracting data
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

     pub async fn get_status(&self) -> (bool, u64, Option<String>, bool) {
        let is_running = {
            let running = self.is_running.read().await;
            *running
        };
        
        let interval_secs = {
            let interval = self.transmission_interval.read().await;
            interval.as_secs()
        };
        
        let endpoint = {
            let url = self.endpoint_url.read().await;
            url.clone()
        };
        
        // Add the fourth boolean value (e.g., is_configured)
        let is_configured = endpoint.is_some();
        
        (is_running, interval_secs, endpoint, is_configured)
    }

    // Add update_config method
    pub async fn update_config(&self, new_config: crate::services::api_service::MtwsConfig) {
        self.set_transmission_interval(new_config.interval_seconds).await;
        self.set_endpoint_url(new_config.endpoint_url).await;
        info!("🔧 MTWS configuration updated");
    }

    // Add the build_combined_payload method as a public method for API use
    pub async fn build_combined_payload(data_service: &DataService) -> Result<MtwsPayload, ModbusError> {
        Self::build_payload(data_service).await
    }
}