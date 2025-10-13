use std::sync::Arc;
use tokio::sync::{RwLock, Mutex as TokioMutex};
use tokio::time::{Duration, interval};
use log::{info, warn, error, debug};
use crate::utils::error::ModbusError;
use super::{GpsReader, GpsData};

pub struct GpsService {
    port_name: String,
    baud_rate: u32,
    reader: Arc<TokioMutex<Option<GpsReader>>>,
    current_data: Arc<RwLock<GpsData>>,
    status: Arc<RwLock<String>>,
    is_running: Arc<RwLock<bool>>,
    reading_handle: Arc<TokioMutex<Option<tokio::task::JoinHandle<()>>>>,
}

impl Clone for GpsService {
    fn clone(&self) -> Self {
        Self {
            port_name: self.port_name.clone(),
            baud_rate: self.baud_rate,
            reader: Arc::clone(&self.reader),
            current_data: Arc::clone(&self.current_data),
            status: Arc::clone(&self.status),
            is_running: Arc::clone(&self.is_running),
            reading_handle: Arc::clone(&self.reading_handle),
        }
    }
}

impl GpsService {
    pub fn new(port_name: String, baud_rate: u32) -> Self {
        Self {
            port_name,
            baud_rate,
            reader: Arc::new(TokioMutex::new(None)),
            current_data: Arc::new(RwLock::new(GpsData::new())),
            status: Arc::new(RwLock::new("Initialized".to_string())),
            is_running: Arc::new(RwLock::new(false)),
            reading_handle: Arc::new(TokioMutex::new(None)),
        }
    }

    pub async fn start(&self) -> Result<(), ModbusError> {
        let mut is_running = self.is_running.write().await;
        if *is_running {
            info!("🧭 GPS service already running");
            return Ok(());
        }

        info!("🧭 Starting GPS service on {} at {} baud", self.port_name, self.baud_rate);
        
        // Update status
        {
            let mut status = self.status.write().await;
            *status = "Connecting".to_string();
        }

        // Initialize GPS reader
        let reader = match GpsReader::new(&self.port_name, self.baud_rate) {
            Ok(mut reader) => {
                // Configure Quectel GPS if needed
                if let Err(e) = reader.configure_quectel_gps() {
                    warn!("⚠️ Failed to configure Quectel GPS (continuing anyway): {}", e);
                }
                reader
            }
            Err(e) => {
                let mut status = self.status.write().await;
                *status = format!("Connection Failed: {}", e);
                return Err(e);
            }
        };

        // Store the reader
        {
            let mut reader_guard = self.reader.lock().await;
            *reader_guard = Some(reader);
        }

        *is_running = true;
        
        // Update status
        {
            let mut status = self.status.write().await;
            *status = "Connected - Starting continuous reading".to_string();
        }

        // Start continuous reading task
        self.start_continuous_reading().await;

        info!("✅ GPS service started successfully");
        Ok(())
    }

    async fn start_continuous_reading(&self) {
        let reader = Arc::clone(&self.reader);
        let current_data = Arc::clone(&self.current_data);
        let status = Arc::clone(&self.status);
        let is_running = Arc::clone(&self.is_running);

        let reading_task = tokio::spawn(async move {
            info!("🧭 Starting GPS continuous reading loop");
            let mut read_interval = interval(Duration::from_secs(2)); // Read every 2 seconds
            let mut last_fix_time = std::time::Instant::now();
            let mut consecutive_failures = 0;

            while *is_running.read().await {
                read_interval.tick().await;

                if !*is_running.read().await {
                    break;
                }

                // Try to get GPS data
                let gps_result = {
                    let mut reader_guard = reader.lock().await;
                    if let Some(ref mut gps_reader) = *reader_guard {
                        // Use a shorter timeout for continuous reading
                        gps_reader.get_single_gps_fix(3).map_err(|e| e.to_string())
                    } else {
                        Err("GPS reader not available".to_string())
                    }
                };

                match gps_result {
                    Ok(Some(gps_data)) => {
                        // Successfully got GPS data
                        consecutive_failures = 0;
                        last_fix_time = std::time::Instant::now();
                        
                        // Update current data
                        {
                            let mut data = current_data.write().await;
                            *data = gps_data.clone();
                        }

                        // Update status
                        {
                            let mut status_guard = status.write().await;
                            if gps_data.has_valid_fix() {
                                *status_guard = format!(
                                    "Reading - Valid Fix: {:.6}, {:.6} ({} sats)",
                                    gps_data.latitude.unwrap_or(0.0),
                                    gps_data.longitude.unwrap_or(0.0),
                                    gps_data.satellites.unwrap_or(0)
                                );
                            } else {
                                *status_guard = "Reading - Searching for GPS signal".to_string();
                            }
                        }

                        if gps_data.has_valid_fix() {
                            debug!("🧭 GPS fix: lat={:.6}, lon={:.6}, sats={}", 
                                   gps_data.latitude.unwrap_or(0.0),
                                   gps_data.longitude.unwrap_or(0.0),
                                   gps_data.satellites.unwrap_or(0));
                        }
                    }
                    Ok(None) => {
                        // No data but no error
                        consecutive_failures += 1;
                        debug!("🧭 No GPS data received (attempt {})", consecutive_failures);

                        if consecutive_failures >= 5 {
                            let mut status_guard = status.write().await;
                            *status_guard = format!("Reading - No signal ({} attempts)", consecutive_failures);
                        }
                    }
                    Err(e) => {
                        // Error occurred
                        consecutive_failures += 1;
                        warn!("⚠️ GPS read error (attempt {}): {}", consecutive_failures, e);

                        if consecutive_failures >= 10 {
                            let mut status_guard = status.write().await;
                            *status_guard = format!("Error - Connection issues: {}", e);
                            
                            // Try to reconnect after many failures
                            warn!("🔄 Attempting GPS reconnection after {} failures", consecutive_failures);
                            
                            // Reset reader
                            {
                                let mut reader_guard = reader.lock().await;
                                *reader_guard = None;
                                tokio::time::sleep(Duration::from_secs(5)).await;
                                
                                // Try to reconnect
                                match GpsReader::new(&reader_guard.as_ref().map(|_| "").unwrap_or("/dev/ttyUSB1"), 9600) {
                                    Ok(mut new_reader) => {
                                        if let Err(e) = new_reader.configure_quectel_gps() {
                                            warn!("⚠️ Failed to reconfigure GPS: {}", e);
                                        }
                                        *reader_guard = Some(new_reader);
                                        consecutive_failures = 0;
                                        info!("✅ GPS reconnected successfully");
                                    }
                                    Err(e) => {
                                        error!("❌ GPS reconnection failed: {}", e);
                                    }
                                }
                            }
                        }
                    }
                }

                // Update data age status
                if last_fix_time.elapsed() > Duration::from_secs(30) {
                    let mut status_guard = status.write().await;
                    if !status_guard.contains("Error") {
                        *status_guard = format!("Reading - Last fix {:.0}s ago", last_fix_time.elapsed().as_secs_f32());
                    }
                }
            }

            info!("🧭 GPS continuous reading loop stopped");
        });

        // Store the handle
        {
            let mut handle_guard = self.reading_handle.lock().await;
            *handle_guard = Some(reading_task);
        }
    }

    pub async fn stop(&self) -> Result<(), ModbusError> {
        info!("🧭 Stopping GPS service");
        
        // Set running flag to false
        {
            let mut is_running = self.is_running.write().await;
            *is_running = false;
        }

        // Stop the reading task
        {
            let mut handle_guard = self.reading_handle.lock().await;
            if let Some(handle) = handle_guard.take() {
                handle.abort();
                info!("🧭 GPS reading task stopped");
            }
        }

        // Clean up reader
        {
            let mut reader_guard = self.reader.lock().await;
            *reader_guard = None;
        }

        // Update status
        {
            let mut status = self.status.write().await;
            *status = "Stopped".to_string();
        }

        info!("✅ GPS service stopped");
        Ok(())
    }

    pub async fn get_status(&self) -> String {
        self.status.read().await.clone()
    }

    pub async fn get_current_data(&self) -> GpsData {
        self.current_data.read().await.clone()
    }

    pub async fn get_current_gps_fix(&self) -> Result<Option<GpsData>, ModbusError> {
        // Return current data if it's recent and valid
        let current_data = self.current_data.read().await;
        
        if current_data.has_valid_fix() {
            // Check if data is recent (within last 30 seconds)
            if let Some(timestamp) = current_data.timestamp {
                let now = chrono::Utc::now().timestamp();
                if (now - timestamp) <= 30 {
                    debug!("🧭 Returning fresh GPS data from continuous reading");
                    return Ok(Some(current_data.clone()));
                } else {
                    debug!("🧭 GPS data is {} seconds old", now - timestamp);
                }
            }
        }

        debug!("🧭 No recent valid GPS fix available");
        Ok(None)
    }

    pub async fn is_running(&self) -> bool {
        *self.is_running.read().await
    }

    // Method to force immediate GPS reading (for testing)
    pub async fn force_read(&self) -> Result<Option<GpsData>, ModbusError> {
        let mut reader_guard = self.reader.lock().await;
        if let Some(ref mut reader) = *reader_guard {
            match reader.get_single_gps_fix(10) {
                Ok(data) => {
                    if let Some(ref gps_data) = data {
                        // Update current data
                        {
                            let mut current_data = self.current_data.write().await;
                            *current_data = gps_data.clone();
                        }
                        info!("🧭 Force read successful: {:?}", gps_data.has_valid_fix());
                    }
                    Ok(data)
                }
                Err(e) => Err(e),
            }
        } else {
            Err(ModbusError::ServiceNotAvailable("GPS reader not available".to_string()))
        }
    }
}