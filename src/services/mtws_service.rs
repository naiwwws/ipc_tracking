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
#[cfg(feature = "sqlite")]
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

#[derive(Debug, Clone, Serialize, Deserialize)]
struct FlowmeterValues {
    pub volume_total: f32,
    pub density_flow: f32,
    pub temperature: f32,
    pub mass_flow_rate: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct EngineData {
    pub rpm: u16,
    pub duration: u32,
    pub is_running: bool,
    pub engine_type: String, // "main", "aux", "generator", etc.
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
                #[cfg(feature = "sqlite")]
                {
                    match Self::generate_and_send_payload(&data_service, &endpoint_url, &client).await {
                        Ok(_) => info!("✅ Periodic MTWS transmission completed successfully"),
                        Err(e) => error!("❌ Periodic MTWS transmission failed: {}", e),
                    }
                }
                #[cfg(not(feature = "sqlite"))]
                {
                    error!("❌ MTWS transmission requires sqlite feature");
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

        #[cfg(feature = "sqlite")]
        {
            let endpoint_url = self.config.get_mtws_endpoint_url();
            info!("🛰️ Sending single MTWS payload to: {}", endpoint_url);
            
            Self::generate_and_send_payload(&self.data_service, &endpoint_url, &self.client).await?;
        }
        
        #[cfg(not(feature = "sqlite"))]
        {
            return Err(ModbusError::ServiceNotAvailable("MTWS service requires sqlite feature".to_string()));
        }
        
        Ok(())
    }

    pub async fn send_single_payload(&self, endpoint_url: String) -> Result<String, ModbusError> {
        if !self.config.mtws.enabled {
            return Err(ModbusError::ServiceNotAvailable("MTWS service is disabled".to_string()));
        }

        #[cfg(feature = "sqlite")]
        {
            info!("🛰️ Sending single MTWS payload to custom endpoint: {}", endpoint_url);
            let payload = Self::generate_and_send_payload(&self.data_service, &endpoint_url, &self.client).await?;
            
            Ok(format!("Sent MTWS payload with {} fields to {}", payload.fields.len(), endpoint_url))
        }
        
        #[cfg(not(feature = "sqlite"))]
        {
            Err(ModbusError::ServiceNotAvailable("MTWS service requires sqlite feature".to_string()))
        }
    }

    #[cfg(feature = "sqlite")]
    async fn generate_and_send_payload(
        data_service: &DataService, 
        endpoint_url: &str,
        client: &Client
    ) -> Result<MtwsPayload, ModbusError> {
        let payload = Self::build_payload(data_service).await?;
        
        info!("🛰️ Sending MTWS structured payload to {} with {} fields", endpoint_url, payload.fields.len());
        // Print payload summary for debugging
        Self::print_current_payload_static(data_service).await.ok();

        let response = client
            .post(endpoint_url)
            .header("Content-Type", "application/json")
            .header("User-Agent", "IPC-Track-Device/1.0")
            .json(&payload)
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

    #[cfg(feature = "sqlite")]
    async fn build_payload(data_service: &DataService) -> Result<MtwsPayload, ModbusError> {
        let mut payload = MtwsPayload::new();

        // 1. Add timestamp
        payload.add_field("timestamp".to_string(), Utc::now().timestamp().to_string());

        // 2. CLOUD-COMPATIBLE GPS data processing with proper multipliers
        if let Some(gps_data) = data_service.get_current_gps_data().await {
            // LATITUDE & LONGITUDE: multiply by 60000 (as integers)
            let longitude_cloud = if let Some(lon) = gps_data.longitude {
                (lon * 60000.0) as i64
            } else {
                0
            };
            
            let latitude_cloud = if let Some(lat) = gps_data.latitude {
                (lat * 60000.0) as i64
            } else {
                0
            };

            // HEADING & ALTITUDE: multiply by 10 (as integers)
            let heading_cloud = if let Some(course) = gps_data.course {
                (course * 10.0) as i32
            } else {
                0
            };

            let altitude_cloud = if let Some(altitude) = gps_data.altitude {
                (altitude * 10.0) as i32
            } else {
                0
            };

            // SPEED: keep as decimal with 2 decimal places
            let speed_cloud = if let Some(speed_knots) = gps_data.speed {
                // Convert knots to km/h and format with 2 decimals
                format!("{:.2}", speed_knots * 1.852)
            } else {
                "0.00".to_string()
            };

            // Add fields with cloud-expected format
            payload.add_field("longitude".to_string(), longitude_cloud.to_string());
            payload.add_field("latitude".to_string(), latitude_cloud.to_string());
            payload.add_field("speed".to_string(), speed_cloud);
            payload.add_field("heading".to_string(), heading_cloud.to_string());
            payload.add_field("altitude".to_string(), altitude_cloud.to_string());
            payload.add_field("gpsNumSats".to_string(), gps_data.satellites.unwrap_or(0).to_string());
              
            // Log original values for debugging
            info!("🔍 Original GPS values: lat={:.6}°, lon={:.6}°, speed={:.2}kts, heading={:.1}°, alt={:.1}m", 
                   gps_data.latitude.unwrap_or(0.0), 
                   gps_data.longitude.unwrap_or(0.0),
                   gps_data.speed.unwrap_or(0.0),
                   gps_data.course.unwrap_or(0.0),
                   gps_data.altitude.unwrap_or(0.0));
        } else {
            // Default values when no GPS available
            payload.add_field("longitude".to_string(), "0".to_string());
            payload.add_field("latitude".to_string(), "0".to_string());
            payload.add_field("speed".to_string(), "0.00".to_string());
            payload.add_field("heading".to_string(), "0".to_string());
            payload.add_field("altitude".to_string(), "0".to_string());
            payload.add_field("gpsNumSats".to_string(), "0".to_string());
            warn!("⚠️ No GPS data available, using cloud-format default values");
        }

        // 3. Add power/battery data
        payload.add_field("batteryVoltage".to_string(), "8086".to_string());
        payload.add_field("extPowerVoltage".to_string(), "27838".to_string());

        // 4. Add environmental data
        payload.add_field("windSpeed".to_string(), "0".to_string());
        payload.add_field("windDirection".to_string(), "0".to_string());

        // 5. DYNAMIC FLOWMETER DATA - Read from config, no limits
        let flowmeter_devices = data_service.get_flowmeter_devices();
        let flowmeter_count = flowmeter_devices.len();
        info!("🔍 Found {} configured flowmeter devices", flowmeter_count);

        let mut flowmeter_data_collected = HashMap::new();

        // Read data from ALL configured flowmeter devices
        for (index, device_config) in flowmeter_devices.iter().enumerate() {
            let flowmeter_number = index + 1;
            let device_address = device_config.address;
            
            info!("🌊 Processing flowmeter {} at address {} ({})", 
                  flowmeter_number, device_address, device_config.name);

            let mut flowmeter_values = None;

            // Method 1: Direct flowmeter data access
            if let Some(flowmeter_data) = data_service.get_current_flowmeter_data(device_address).await {
                flowmeter_values = Some(FlowmeterValues {
                    volume_total: flowmeter_data.volume_total,
                    density_flow: flowmeter_data.density_flow,
                    temperature: flowmeter_data.temperature,
                    mass_flow_rate: flowmeter_data.mass_flow_rate,
                });
                
                info!("✅ Got fresh flowmeter {} data: VT={}, D={}, T={}, FR={}", 
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
            else {
                warn!("⚠️ No data found for flowmeter {} (address {})", flowmeter_number, device_address);
            }

            if let Some(values) = flowmeter_values {
                flowmeter_data_collected.insert(flowmeter_number, values);
            }
        }

        // Add flowmeter data dynamically based on actual device count
        // Ensure minimum of 4 for legacy compatibility, but support unlimited
        let max_flowmeters = std::cmp::max(4, flowmeter_count);
        
        // Add all VolumeTotal fields
        for flowmeter_number in 1..=max_flowmeters {
            if let Some(values) = flowmeter_data_collected.get(&flowmeter_number) {
                payload.add_field(
                    format!("flowmeterVolumeTotal{}", flowmeter_number),
                    values.volume_total.to_string()
                );
            } else {
                payload.add_field(format!("flowmeterVolumeTotal{}", flowmeter_number), "0".to_string());
            }
        }

        // Add all Density fields
        for flowmeter_number in 1..=max_flowmeters {
            if let Some(values) = flowmeter_data_collected.get(&flowmeter_number) {
                payload.add_field(
                    format!("flowmeterDensity{}", flowmeter_number),
                    values.density_flow.to_string()
                );
            } else {
                payload.add_field(format!("flowmeterDensity{}", flowmeter_number), "0".to_string());
            }
        }

        // Add all Temperature fields
        for flowmeter_number in 1..=max_flowmeters {
            if let Some(values) = flowmeter_data_collected.get(&flowmeter_number) {
                payload.add_field(
                    format!("flowmeterTemperature{}", flowmeter_number),
                    values.temperature.to_string()
                );
            } else {
                payload.add_field(format!("flowmeterTemperature{}", flowmeter_number), "0".to_string());
            }
        }

        // Add all Flowrate fields
        for flowmeter_number in 1..=max_flowmeters {
            if let Some(values) = flowmeter_data_collected.get(&flowmeter_number) {
                payload.add_field(
                    format!("flowmeterFlowrate{}", flowmeter_number),
                    values.mass_flow_rate.to_string()
                );
            } else {
                payload.add_field(format!("flowmeterFlowrate{}", flowmeter_number), "0".to_string());
            }
        }

        info!("📦 Added {} flowmeter devices (with {} minimum for compatibility)", flowmeter_count, max_flowmeters);

        // 6. Add fuel level
        payload.add_field("fuelLevelMM".to_string(), "0".to_string());

        // 7. DYNAMIC RPM/ENGINE DATA - Read all RPM devices from config
        let rpm_devices = data_service.get_rpm_devices();
        let rpm_device_count = rpm_devices.len();
        info!("🔍 Found {} configured RPM devices", rpm_device_count);

        let mut all_engines = Vec::new();

        for (device_index, device_config) in rpm_devices.iter().enumerate() {
            let device_address = device_config.address;

            info!("🔄 Processing RPM device {} at address {}: '{}'", 
                  device_index + 1, device_address, device_config.name);

            // Get channel configuration from device metadata
            let (total_channels, _thresholds) = data_service.get_config().get_rpm_channel_config(device_address);
            
            if let Some(device_data_str) = data_service.get_device_data_by_address(device_address).await {
                match Self::extract_rpm_values(&device_data_str) {
                    Ok(channel_rpms) => {
                        for (channel_id, rpm_value) in channel_rpms {
                            // Determine engine type from metadata
                            let engine_type = device_config.metadata
                                .get("engine_types")
                                .and_then(|types| types.split(',').nth((channel_id - 1) as usize))
                                .unwrap_or("main")
                                .to_string();

                            let engine_data = EngineData {
                                rpm: rpm_value,
                                duration: 0, // TODO: Get from database
                                is_running: rpm_value > 500, // Configurable threshold
                                engine_type: engine_type.clone(),
                            };

                            all_engines.push((device_address, channel_id, engine_data));
                            
                            info!("✅ Added engine from device {} channel {}: type={}, RPM={}", 
                                  device_address, channel_id, engine_type, rpm_value);
                        }
                    }
                    Err(e) => {
                        warn!("⚠️ Failed to parse RPM data from device {}: {}", device_address, e);
                        
                        // Add default engines for failed device
                        for channel_id in 1..=total_channels {
                            let engine_data = EngineData {
                                rpm: 0,
                                duration: 0,
                                is_running: false,
                                engine_type: "main".to_string(),
                            };
                            all_engines.push((device_address, channel_id, engine_data));
                        }
                    }
                }
            } else {
                warn!("⚠️ No data found for RPM device at address {}", device_address);
                
                // Add default engines for offline device
                for channel_id in 1..=total_channels {
                    let engine_data = EngineData {
                        rpm: 0,
                        duration: 0,
                        is_running: false,
                        engine_type: "main".to_string(),
                    };
                    all_engines.push((device_address, channel_id, engine_data));
                }
            }
        }

        // Separate engines by type
        let mut main_engines = Vec::new();
        let mut aux_engines = Vec::new();

        for (_address, _channel, engine_data) in &all_engines {
            match engine_data.engine_type.as_str() {
                "main" | "ME" => main_engines.push(engine_data),
                "aux" | "AE" | "auxiliary" => aux_engines.push(engine_data),
                _ => main_engines.push(engine_data), // Default to main
            }
        }

        // Add Main Engine RPM values (dynamic count)
        let main_engine_count = main_engines.len();
        for (index, engine) in main_engines.iter().enumerate() {
            let engine_number = index + 1;
            payload.add_field(format!("engineRPM{}", engine_number), engine.rpm.to_string());
        }

        // Add Main Engine durations (dynamic count)
        for (index, engine) in main_engines.iter().enumerate() {
            let engine_number = index + 1;
            payload.add_field(format!("engineDurationME{}", engine_number), engine.duration.to_string());
        }

        // Add Auxiliary Engine durations (dynamic count, minimum 3 for compatibility)
        let max_aux_engines = std::cmp::max(3, aux_engines.len());
        for aux_number in 1..=max_aux_engines {
            if let Some(engine) = aux_engines.get(aux_number - 1) {
                payload.add_field(format!("engineDurationAE{}", aux_number), engine.duration.to_string());
                payload.add_field(format!("statusAE{}", aux_number), engine.is_running.to_string());
            } else {
                payload.add_field(format!("engineDurationAE{}", aux_number), "0".to_string());
                payload.add_field(format!("statusAE{}", aux_number), "false".to_string());
            }
        }

        info!("🔧 Added {} main engines and {} auxiliary engines", main_engine_count, aux_engines.len());

        // 8. Add status fields (these can also be made dynamic based on config)
        payload.add_field("statusDoorOpenStarboard".to_string(), "false".to_string());
        payload.add_field("statusDoorOpenPort".to_string(), "false".to_string());
        payload.add_field("statusDCOK".to_string(), "true".to_string());
        payload.add_field("statusBattFail".to_string(), "false".to_string());

        info!("📦 Built dynamic MTWS payload: {} flowmeters, {} total engines, {} fields", 
              max_flowmeters, all_engines.len(), payload.fields.len());
        
        Ok(payload)
    }

    // JSON extraction methods
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


    pub async fn update_config(&self, _config: serde_json::Value) -> Result<(), ModbusError> {
        info!("🔧 MTWS configuration update requested");
        Ok(())
    }

    // Add this method to print payload for debugging
    #[cfg(feature = "sqlite")]
    pub async fn print_current_payload(&self) -> Result<(), ModbusError> {
        Self::print_current_payload_static(&self.data_service).await
    }

    // Static version for use with &DataService
    #[cfg(feature = "sqlite")]
    pub async fn print_current_payload_static(data_service: &DataService) -> Result<(), ModbusError> {
        let payload = Self::build_payload(data_service).await?;
        
        // Also print as JSON for easier viewing
        println!("\n🔍 JSON REPRESENTATION:");
        println!("=======================");
        match serde_json::to_string_pretty(&payload) {
            Ok(json_str) => println!("{}", json_str),
            Err(e) => error!("Failed to serialize payload to JSON: {}", e),
        }
        
        Ok(())
    }

    // Add this method to get payload as formatted string
    #[cfg(feature = "sqlite")]
    pub async fn get_payload_summary(&self) -> Result<String, ModbusError> {
        let payload = Self::build_payload(&self.data_service).await?;
        
        let mut summary = format!(
            "MTWS Payload Summary:\n\
             - SIN: {}\n\
             - Name: {}\n\
             - IsForward: {}\n\
             - MIN: {}\n\
             - Total Fields: {}\n\n\
             Field Details:\n",
            payload.sin, payload.name, payload.is_forward, payload.min, payload.fields.len()
        );
        
        // Group fields by category for better readability
        let mut gps_fields = Vec::new();
        let mut flowmeter_fields = Vec::new();
        let mut engine_fields = Vec::new();
        let mut status_fields = Vec::new();
        let mut other_fields = Vec::new();
        
        for field in &payload.fields {
            if field.name.contains("longitude") || field.name.contains("latitude") || 
               field.name.contains("speed") || field.name.contains("heading") || 
               field.name.contains("altitude") || field.name.contains("gps") {
                gps_fields.push(field);
            } else if field.name.contains("flowmeter") {
                flowmeter_fields.push(field);
            } else if field.name.contains("engine") || field.name.contains("RPM") {
                engine_fields.push(field);
            } else if field.name.contains("status") {
                status_fields.push(field);
            } else {
                other_fields.push(field);
            }
        }
        
        summary.push_str("\n📍 GPS Fields:\n");
        for field in gps_fields {
            summary.push_str(&format!("  {} = {}\n", field.name, field.value));
        }
        
        summary.push_str("\n🌊 Flowmeter Fields:\n");
        for field in flowmeter_fields {
            summary.push_str(&format!("  {} = {}\n", field.name, field.value));
        }
        
        summary.push_str("\n🔧 Engine/RPM Fields:\n");
        for field in engine_fields {
            summary.push_str(&format!("  {} = {}\n", field.name, field.value));
        }
        
        summary.push_str("\n⚡ Status Fields:\n");
        for field in status_fields {
            summary.push_str(&format!("  {} = {}\n", field.name, field.value));
        }
        
        summary.push_str("\n📦 Other Fields:\n");
        for field in other_fields {
            summary.push_str(&format!("  {} = {}\n", field.name, field.value));
        }
        
        Ok(summary)
    }
}