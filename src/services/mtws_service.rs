use log::{info, error, warn};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::time::{Duration, interval};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_cbor;
use reqwest::Client;

use crate::config::Config;
use crate::modbus::crc16_modbus;
use crate::services::DataService;
use crate::utils::error::ModbusError;
#[cfg(feature = "sqlite")]
use crate::storage::models::{MtwsPayload};

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
        let payload = Self::build_payload(data_service).await?; // JSON Format
        let payload_cbor = serde_cbor::to_vec(&payload).unwrap();   // CBOR Format
        let payload_raw_hex = Self::build_payload_hex(data_service).await?; // raw hex format
        if let Err(e) = data_service.store_combined_device_reading().await {
            warn!("⚠️ Failed to store combined reading during MTWS transmission: {}", e);
        } else {
            info!("💾 Stored combined reading during MTWS transmission");
        }
        
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
        let max_flowmeters = std::cmp::max(1, flowmeter_count);
        
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

        // 8. DYNAMIC AIO MODULE DATA - Read all AIO devices from config
        let aio_devices = data_service.get_aio_module_devices();
        let aio_device_count = aio_devices.len();
        info!("🔍 Found {} configured AIO module devices", aio_device_count);

        for (device_index, device_config) in aio_devices.iter().enumerate() {
            let device_address = device_config.address;

            info!("📊 Processing AIO module {} at address {}: '{}'", 
                  device_index + 1, device_address, device_config.name);

            if let Some(aio_data) = data_service.get_current_aio_module_data(device_address).await {
                // Add RPM channel data (4 RPM sensor channels per device)
                for channel_data in &aio_data.rpm_channels {
                    payload.add_field(format!("aio{}Ch{}GearPulse", device_address, channel_data.channel_id), channel_data.gear_pulse_count.to_string());
                    payload.add_field(format!("aio{}Ch{}Threshold", device_address, channel_data.channel_id), channel_data.rpm_threshold.to_string());
                    payload.add_field(format!("aio{}Ch{}Frequency", device_address, channel_data.channel_id), channel_data.frequency_hz.to_string());
                    payload.add_field(format!("aio{}Ch{}RPM", device_address, channel_data.channel_id), channel_data.rpm_value.to_string());
                    payload.add_field(format!("aio{}Ch{}Average", device_address, channel_data.channel_id), channel_data.rpm_average.to_string());
                    payload.add_field(format!("aio{}Ch{}DurationRPM", device_address, channel_data.channel_id), channel_data.duration_rpm_minutes.to_string());
                    payload.add_field(format!("aio{}Ch{}DurationAE", device_address, channel_data.channel_id), channel_data.duration_ae_minutes.to_string());
                }

                // Add digital input state (convert Vec<bool> to u16 bitmask)
                for (i, din_value) in aio_data.digital_inputs.iter().enumerate(){
                    payload.add_field(format!("aio{}DigitalInput{}", device_address, i+1), din_value.to_string());
                }

                info!("✅ Added AIO RPM module {} data: {} RPM channels", 
                      device_address, aio_data.rpm_channels.len(), );
            } else {
                warn!("⚠️ No data found for AIO module at address {}", device_address);
                
                // Add default values for offline AIO RPM device (4 RPM channels)
                for channel_id in 1..=4 {
                    payload.add_field(format!("aio{}Ch{}GearPulse", device_address, channel_id), "0".to_string());
                    payload.add_field(format!("aio{}Ch{}Threshold", device_address, channel_id), "0".to_string());
                    payload.add_field(format!("aio{}Ch{}Frequency", device_address, channel_id), "0".to_string());
                    payload.add_field(format!("aio{}Ch{}RPM", device_address, channel_id), "0".to_string());
                    payload.add_field(format!("aio{}Ch{}Average", device_address, channel_id), "0".to_string());
                    payload.add_field(format!("aio{}Ch{}DurationRPM", device_address, channel_id), "0".to_string());
                    payload.add_field(format!("aio{}Ch{}DurationAE", device_address, channel_id), "0".to_string());
                }
                payload.add_field(format!("aio{}DigitalInputs", device_address), "0".to_string());
                for input in 0..16 {
                    payload.add_field(format!("aio{}DigitalInputs{}", device_address, input), "false".to_string());
                }
            }
        }

        info!("📊 Added {} AIO module devices with comprehensive channel data", aio_device_count);

        // 9. Add status fields (these can also be made dynamic based on config)
        // payload.add_field("statusDoorOpenStarboard".to_string(), "false".to_string());
        // payload.add_field("statusDoorOpenPort".to_string(), "false".to_string());
        // payload.add_field("statusDCOK".to_string(), "true".to_string());
        // payload.add_field("statusBattFail".to_string(), "false".to_string());

        info!("📦 Built dynamic MTWS payload: {} flowmeters, {} total engines, {} AIO modules, {} fields", 
              max_flowmeters, all_engines.len(), aio_device_count, payload.fields.len());
        
        Ok(payload)
    }

    async fn build_payload_hex (data_service: &DataService) -> Result<MtwsPayload, ModbusError> {

        // FORMAT : [Header][SIN][nameLength][name][is_forward][sensorDataLength][sensorData].....[DIN][MIN][CRC]
        // sensor : flowmeter, rpm, aio module, etc etc

        // it seems manual bytes counting is still wrong

        let mut payload = MtwsPayload::new();

        // HEADER
        payload.add_raw_field([0xFF, 0xFF]);                                // Byte 1 - 2: header (2 bytes)
        // OPTIONAL: add mtws format sin, name, is_forward,
        payload.add_raw_field(payload.sin.to_be_bytes());                   // Byte 3: SIN value u16
        payload.add_raw_field((payload.name.len() as u8).to_be_bytes());    // Byte 4: Name Length (MAX 255 Char) u8

        let name_bytes: [u8; 9] = *b"ReportMsg"; // compile-time fixed array
        payload.add_raw_field(name_bytes);                                  // Byte 5 - 13: Name Value  string (default "ReportMsg": 9 Bytes)

        payload.add_raw_field((payload.is_forward as u8).to_be_bytes());    // Byte 14: is_forward TRUE/FALSE (kind of waste of space to store bool as u8 whatev)

        // 1. Add timestamp
        payload.add_raw_field(Utc::now().timestamp().to_be_bytes());        // Bytes 15 - 22: timestamp (8 Bytes)

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
                speed_knots * 1.852
            } else {
                0.00
            };

            // Add fields with cloud-expected format
            payload.add_raw_field( longitude_cloud.to_be_bytes());                          // Bytes 23 - 30: longitude (8 bytes) i64
            payload.add_raw_field( latitude_cloud.to_be_bytes());                           // Bytes 31 - 38: latutude  (8 Bytes) i64
            payload.add_raw_field( speed_cloud.to_be_bytes());                              // Bytes 39 - 42: speed     (4 Bytes) f32
            payload.add_raw_field( heading_cloud.to_be_bytes());                            // Bytes 43 - 46: heading  (4 Bytes)  i32
            payload.add_raw_field( altitude_cloud.to_be_bytes());                           // Bytes 47 - 50: altitude  (4 Bytes) i32
            payload.add_raw_field( gps_data.satellites.unwrap_or(0).to_be_bytes()); // Bytes 51 - 54: satellites (4 Bytes) i32
              
            // Log original values for debugging
            info!("🔍 Original GPS values: lat={:.6}°, lon={:.6}°, speed={:.2}kts, heading={:.1}°, alt={:.1}m", 
                   gps_data.latitude.unwrap_or(0.0), 
                   gps_data.longitude.unwrap_or(0.0),
                   gps_data.speed.unwrap_or(0.0),
                   gps_data.course.unwrap_or(0.0),
                   gps_data.altitude.unwrap_or(0.0));
        } else {
            // Default values when no GPS available
            payload.add_raw_field( (0_i64).to_be_bytes());
            payload.add_raw_field( (0_i64).to_be_bytes());
            payload.add_raw_field( (0.0_f32).to_be_bytes());
            payload.add_raw_field( (0_i32).to_be_bytes());
            payload.add_raw_field( (0_i32).to_be_bytes());
            payload.add_raw_field( (0_i32).to_be_bytes());
            warn!("⚠️ No GPS data available, using cloud-format default values");
        }

        // 3. Add power/battery data
        payload.add_raw_field( (8086 as i32).to_be_bytes());    // Bytes 55 - 58: BatteryVoltage (4 Bytes) i32
        payload.add_raw_field( (27838 as i32).to_be_bytes());  // Bytes 59 - 62: extPowerVoltage (4 Bytes) i32

        // 4. Add environmental data
        payload.add_raw_field((0_i32).to_be_bytes());   // Bytes 63 - 66: WindSpeed (default 4 Bytes) i32 [need to change later]
        payload.add_raw_field((0_i32).to_be_bytes());   // Bytes 67 - 70: WindDirection (default 4 Bytes) i32 [need to change later]

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
        let max_flowmeters = std::cmp::max(1, flowmeter_count);
        
        // 16 Bytes: 1 VolumeTotal, 1 Density, 1 Temperature, 1 Flowrate
        // 32 Bytes: 2 VolumeTotal, 2 Density, 2 Temperature, 2 Flowrate
        // 64 Bytes: 3 VolumeTotal, 3 Density, 3 Temperature, 3 Flowrate
        // .....
        // 240 Bytes: 15 VolumeTotal, 15 Density, 15 Temperature, 15 Flowrate
        // MAX u8 = 255 Bytes
        payload.add_raw_field(((max_flowmeters as u8) * 16u8).to_be_bytes()); // Byte 71: Byte Length of flowmeter data // default to 4 flowmeter
        
        // TODO: (apply to all sensor since this is data length constraint max u8 = 255 Bytes)
        // in order to support more than 15 flowmeter in this raw format, require some logic to make server take 2 bytes for flowmeter Bytes length 
        // or maybe just add 1 more byte to tell how many byte it take for bytes length. simple but add more byte to the payload
        // other solution, search: Variable-length Quantity (Varint) Encoding

        // Add all VolumeTotal fields
        for flowmeter_number in 1..=max_flowmeters { // Bytes 72 - 75: VolumeTotal1 - 4 (4 Byte f32) per flowmeter
            if let Some(values) = flowmeter_data_collected.get(&flowmeter_number) {
                payload.add_raw_field(
                    values.volume_total.to_be_bytes()
                );
            } else {
                payload.add_raw_field((0.0_f32).to_be_bytes());
            }
        }

        // Add all Density fields
        for flowmeter_number in 1..=max_flowmeters { // Bytes 76 - 79: Density1 - 4 f32 (4 Byte f32) per flowmeter
            if let Some(values) = flowmeter_data_collected.get(&flowmeter_number) {
                payload.add_raw_field(
                    values.density_flow.to_be_bytes()
                );
            } else {
                payload.add_raw_field((0.0_f32).to_be_bytes());
            }
        }

        // Add all Temperature fields
        for flowmeter_number in 1..=max_flowmeters { // Bytes 80 - 83: Temperature1 - 4 f32 (4 Byte f32) per flowmeter
            if let Some(values) = flowmeter_data_collected.get(&flowmeter_number) {
                payload.add_raw_field(
                    values.temperature.to_be_bytes()
                );
            } else {
                payload.add_raw_field((0.0_f32).to_be_bytes());
            }
        }

        // Add all Flowrate fields
        for flowmeter_number in 1..=max_flowmeters { // Bytes 84 - 87: Flowrate1 - 4 f32 (4 Byte f32) per flowmeter
            if let Some(values) = flowmeter_data_collected.get(&flowmeter_number) {
                payload.add_raw_field(
                    values.mass_flow_rate.to_be_bytes()
                );
            } else {
                payload.add_raw_field((0.0_f32).to_be_bytes());
            }
        }

        info!("📦 Added {} flowmeter devices (with {} minimum for compatibility)", flowmeter_count, max_flowmeters);

        // 6. Add fuel level
        payload.add_raw_field( (0.0_f32).to_be_bytes()); // Bytes 88 - 91: Fuel level (4 Bytes f32)

        // 7. DYNAMIC RPM/ENGINE DATA - Read all RPM devices from config
        let rpm_devices = data_service.get_rpm_devices();
        let rpm_device_count = rpm_devices.len();
        info!("🔍 Found {} configured RPM devices", rpm_device_count);

        payload.add_raw_field(((rpm_device_count as u8) * 6u8).to_be_bytes()); // Byte 92: Byte Length of rpm data // default to 2 rpm
        
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
        for (index, engine) in main_engines.iter().enumerate() { // Bytes 93 - 96: rpm value1 - 2 i16 (4 Bytes)
            let engine_number = index + 1;
            payload.add_raw_field( engine.rpm.to_be_bytes());
        }

        // Add Main Engine durations (dynamic count)
        for (index, engine) in main_engines.iter().enumerate() { // Bytes 97 - 104: MEDuration1 - 2 value u32 (8 bytes)
            let engine_number = index + 1;
            payload.add_raw_field( engine.duration.to_be_bytes());
        }

        // Add Auxiliary Engine durations (dynamic count, minimum 3 for compatibility)
        let max_aux_engines = std::cmp::max(3, aux_engines.len());
        // add AE length
        payload.add_raw_field(((max_aux_engines as u8) * 5u8).to_be_bytes()); // Bytes 105: Byte Length of AE data // default to 2 AE
        for aux_number in 1..=max_aux_engines {                               // Bytes 106 - 115: AEDuration1 - 3 and statusAE1 - 3 u32 and u8 (10 Bytes)
            if let Some(engine) = aux_engines.get(aux_number - 1) {
                payload.add_raw_field( engine.duration.to_be_bytes());
                payload.add_raw_field((engine.is_running as u8).to_be_bytes());    // another bool as u8......
            } else {
                payload.add_raw_field( (0u32).to_be_bytes());
                payload.add_raw_field( (0u8).to_be_bytes());
            }
        }

        info!("🔧 Added {} main engines and {} auxiliary engines", main_engine_count, aux_engines.len());

        // 8. DYNAMIC AIO MODULE DATA - Read all AIO devices from config
        let aio_devices = data_service.get_aio_module_devices();
        let aio_device_count = aio_devices.len();
        info!("🔍 Found {} configured AIO module devices", aio_device_count);

        payload.add_raw_field(((aio_device_count as u8) * 78u8).to_be_bytes()); // Bytes 161: Byte length of aio module, default to 1 AIO (72 Bytes)

        for (device_index, device_config) in aio_devices.iter().enumerate() {
            let device_address = device_config.address;

            info!("📊 Processing AIO module {} at address {}: '{}'", 
                  device_index + 1, device_address, device_config.name);

            if let Some(aio_data) = data_service.get_current_aio_module_data(device_address).await {
                // Add RPM channel data (4 RPM sensor channels per device)

                // TODO: Is it better to keep it this way, or change it so it follow flowmeter format? e.g temperature1 temperature2 temperature3 temperature4,
                // rather than temperature1 flowrate1 temperature2 flowrate2 temperature3 flowrate3 temperature4 flowrate4 etc etc
                for channel_data in &aio_data.rpm_channels { // Bytes 116 - 187: AIO data (72 Bytes)
                    payload.add_raw_field(channel_data.gear_pulse_count.to_be_bytes());
                    payload.add_raw_field( channel_data.rpm_threshold.to_be_bytes());
                    payload.add_raw_field( channel_data.frequency_hz.to_be_bytes());
                    payload.add_raw_field( channel_data.rpm_value.to_be_bytes());
                    payload.add_raw_field( channel_data.rpm_average.to_be_bytes());
                    payload.add_raw_field( channel_data.duration_rpm_minutes.to_be_bytes());
                    payload.add_raw_field( channel_data.duration_ae_minutes.to_be_bytes());
                }

                // Add digital input state (convert Vec<bool> to u16 bitmask)
                let mut digital_input: u16 = 0;
                for (i, &bit) in aio_data.digital_inputs.iter().enumerate() {
                    if bit {
                        digital_input |= 1 << i;
                    }
                }
                payload.add_raw_field(digital_input.to_be_bytes()); // Bytes 188 - 189: DIN (2 bytes u16)

                info!("✅ Added AIO RPM module {} data: {} RPM channels", 
                      device_address, aio_data.rpm_channels.len(), );
            } else {
                warn!("⚠️ No data found for AIO module at address {}", device_address);
                
                // Add default values for offline AIO RPM device (4 RPM channels)
                for channel_id in 1..=4 {
                    payload.add_raw_field((0u16).to_be_bytes());
                    payload.add_raw_field((0u16).to_be_bytes());
                    payload.add_raw_field((0u16).to_be_bytes());
                    payload.add_raw_field((0u16).to_be_bytes());
                    payload.add_raw_field((0u16).to_be_bytes());
                    payload.add_raw_field((0u32).to_be_bytes());
                    payload.add_raw_field((0u32).to_be_bytes());
                }
                payload.add_raw_field((0u16).to_be_bytes());
            }
        }

        info!("📊 Added {} AIO module devices with comprehensive channel data", aio_device_count);

        // 9. Add status fields (these can also be made dynamic based on config)
        // payload.add_field("statusDoorOpenStarboard".to_string(), "false".to_string());
        // payload.add_field("statusDoorOpenPort".to_string(), "false".to_string());
        // payload.add_field("statusDCOK".to_string(), "true".to_string());
        // payload.add_field("statusBattFail".to_string(), "false".to_string());

        payload.add_raw_field(payload.min.to_be_bytes()); // Bytes 190 - 191: MIN (2 Bytes u16)
        payload.add_raw_field((crc16_modbus(&payload.raw_data)).to_be_bytes()); // Bytes 192 - 193: crc (2 Bytes u16)


        info!("📦 Built HEX MTWS payload: {} flowmeters, {} total engines, {} AIO modules, {} fields", 
              max_flowmeters, all_engines.len(), aio_device_count, payload.fields.len());
        
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
        let payload_raw = Self::build_payload_hex(data_service).await?;
        
        // Serialize payload to JSON
        let json_str = match serde_json::to_string_pretty(&payload) {
            Ok(json) => json,
            Err(e) => {
                error!("Failed to serialize payload to JSON: {}", e);
                return Err(ModbusError::InvalidData(format!("Serialization error: {}", e)));
            }
        };

        // Print JSON for easier viewing
        println!("\n🔍 JSON REPRESENTATION:");
        println!("=======================");
        println!("{}", json_str);

        // Print JSON payload size
        let size_bytes = json_str.len();
        println!("\n📦 Payload size: {} bytes", size_bytes);

        // Convert payload to CBOR
        let cbor_byte = serde_cbor::to_vec(&payload).unwrap();

        // Print CBOR as hex
        println!("\n🔍 CBOR REPRESENTATION (Hex):");
        println!("==============================");
        for byte in &cbor_byte {
            print!("{:02X} ", byte);
        }
        println!();
        println!("\n📦 CBOR size: {} bytes", cbor_byte.len());

        // Print raw payload bytes
        println!("\n🔍 RAW PAYLOAD (Hex):");
        println!("======================");
        for byte in &payload_raw.raw_data {
            print!("{:02X} ", byte);
        }
        println!();
        println!("\n📦 Raw payload size: {} bytes", payload_raw.raw_data.len());

        Ok(())
    }

}