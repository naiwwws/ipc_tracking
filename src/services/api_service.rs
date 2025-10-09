use actix_web::{web, App, HttpServer, HttpResponse, Result as ActixResult, middleware::Logger};
use log::{info, error, warn};
use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};
use std::sync::Arc;

use crate::config::Config;
use crate::utils::error::ModbusError;
use crate::storage::{SqliteManager};
use crate::services::data_service::DataService;
use crate::services::mtws_service::{MtwsService}; // Add MTWS import

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(rename_all = "snake_case")]
pub enum FlowType {
    FlowIn,
    FlowOut,
}

// MTWS API Request/Response types
#[derive(Debug, Deserialize, Serialize)]
pub struct MtwsConfig {
    pub interval_seconds: u64,
    pub endpoint_url: String,
}

#[derive(Debug, Deserialize)]
pub struct SendMtwsPayloadRequest {
    pub endpoint_url: Option<String>,
}

// Error response
#[derive(Debug, Serialize)]
pub struct ErrorResponse {
    pub success: bool,
    pub error: String,
    pub code: String,
    pub timestamp: DateTime<Utc>,
}

// API Service state
#[derive(Clone)]
pub struct ApiServiceState {
    pub sqlite_manager: SqliteManager,
    pub config: Config,
    pub data_service: Option<Arc<DataService>>,
    pub mtws_service: Option<Arc<tokio::sync::Mutex<MtwsService>>>, // Use Mutex for mutability
}

impl ApiServiceState {
    pub fn new(config: Config, sqlite_manager: SqliteManager, data_service: Option<Arc<DataService>>) -> Self {
        // Create MTWS service if enabled and data_service is available
        let mtws_service = if config.mtws.enabled && data_service.is_some() {
            let data_service_arc = data_service.as_ref().unwrap().clone();
            Some(Arc::new(tokio::sync::Mutex::new(
                MtwsService::new(data_service_arc, config.clone())
            )))
        } else {
            None
        };

        Self {
            sqlite_manager,
            config,
            data_service,
            mtws_service,
        }
    }
}

// API Service
pub struct ApiService {
    state: ApiServiceState,
    server_handle: Option<actix_web::dev::ServerHandle>,
}

impl ApiService {
    pub fn new(config: Config, sqlite_manager: SqliteManager) -> Self {
        let state = ApiServiceState::new(config, sqlite_manager, None);
        Self {
            state,
            server_handle: None,
        }
    }
    
    pub fn new_with_state(state: ApiServiceState) -> Result<Self, ModbusError> {
        Ok(Self {
            state,
            server_handle: None,
        })
    }

    pub async fn start(&mut self, port: u16) -> Result<(), ModbusError> {
        info!("🌐 Starting HTTP API server on port {}", port);
        
        let state_data = web::Data::new(self.state.clone());
        
        let server = HttpServer::new(move || {
            App::new()
                .app_data(state_data.clone())
                .wrap(Logger::default())
                .service(
                    web::scope("/api")
                        .route("/health", web::get().to(health_check))
                        // New combined data endpoints
                        .route("/data/send", web::post().to(send_combined_data))
                        .service(
                            web::scope("/mtws")
                                .route("/configure", web::post().to(configure_mtws))
                                .route("/start", web::post().to(start_mtws_transmission))
                                .route("/stop", web::post().to(stop_mtws_transmission))
                                .route("/send", web::post().to(send_mtws_payload))
                                .route("/status", web::get().to(mtws_status))
                                .route("/config", web::get().to(get_mtws_config))
                        )
                        .service(
                            web::scope("/aio_module")
                                .route("/devices", web::get().to(get_aio_module_devices))
                                .route("/data/{address}", web::get().to(get_aio_module_data))
                                .route("/readings/recent", web::get().to(get_recent_aio_module_readings))
                                .route("/readings/{address}", web::get().to(get_aio_module_readings_by_address))
                        )
                )
        })
        .bind(format!("0.0.0.0:{}", port))?
        .run();

        let handle = server.handle();
        self.server_handle = Some(handle.clone());

        tokio::spawn(server);
        info!("✅ HTTP API server started successfully on port {}", port);
        Ok(())
    }

    pub async fn stop(&mut self) -> Result<(), ModbusError> {
        info!("🛑 Stopping HTTP API server...");
        
        if let Some(handle) = self.server_handle.take() {
            tokio::select! {
                _ = handle.stop(true) => {
                    info!("✅ HTTP API server stopped gracefully");
                }
                _ = tokio::time::sleep(tokio::time::Duration::from_secs(10)) => {
                    warn!("⚠️  HTTP API server shutdown timeout, forcing stop");
                    handle.stop(false).await;
                }
            }
        }
        
        Ok(())
    }

    // Add missing MTWS methods
    pub async fn start_mtws_transmission(&self) -> Result<(), ModbusError> {
        if let Some(mtws_service) = &self.state.mtws_service {
            let mtws = mtws_service.lock().await;
            mtws.start_transmission().await
        } else {
            Err(ModbusError::ServiceNotAvailable("MTWS service not available".to_string()))
        }
    }

    pub async fn stop_mtws_transmission(&self) -> Result<(), ModbusError> {
        if let Some(mtws_service) = &self.state.mtws_service {
            let mtws = mtws_service.lock().await;
            mtws.stop_transmission().await
        } else {
            Err(ModbusError::ServiceNotAvailable("MTWS service not available".to_string()))
        }
    }

    pub async fn send_mtws_payload(&self, endpoint_url: Option<String>) -> Result<(), ModbusError> {
        if let Some(mtws_service) = &self.state.mtws_service {
            let mtws = mtws_service.lock().await;
            let endpoint = endpoint_url.clone().unwrap_or_else(|| {
                mtws.get_endpoint_url()
            });
            mtws.send_single_payload(endpoint).await?;
            Ok(())
        } else {
            Err(ModbusError::ServiceNotAvailable("MTWS service not available".to_string()))
        }
    }

    pub async fn get_mtws_status(&self) -> Result<MtwsStatus, ModbusError> {
        if let Some(mtws_service) = &self.state.mtws_service {
            let mtws = mtws_service.lock().await;
            let (is_running, interval_seconds, endpoint_url, enabled) = mtws.get_status().await;
            Ok(MtwsStatus {
                is_running,
                interval_seconds,
                endpoint_url,
                enabled,
            })
        } else {
            Err(ModbusError::ServiceNotAvailable("MTWS service not available".to_string()))
        }
    }
}

// Add MtwsStatus struct
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MtwsStatus {
    pub is_running: bool,
    pub interval_seconds: u64,
    pub endpoint_url: String,
    pub enabled: bool,
}

// API Endpoints

// GET /api/health - Health check
async fn health_check() -> ActixResult<HttpResponse> {
    Ok(HttpResponse::Ok().json(serde_json::json!({
        "status": "healthy",
        "service": "IPC API Service",
        "timestamp": Utc::now(),
        "version": crate::VERSION
    })))
}

// MTWS API Endpoints

// POST /api/mtws/configure
async fn configure_mtws(
    data: web::Data<ApiServiceState>,
    config: web::Json<MtwsConfig>,
) -> ActixResult<HttpResponse> {
    if let Some(mtws_service) = &data.mtws_service {
        let mtws = mtws_service.lock().await;
        
        // Convert MtwsConfig to serde_json::Value
        let config_value = match serde_json::to_value(config.into_inner()) {
            Ok(val) => val,
            Err(e) => {
                return Ok(HttpResponse::BadRequest().json(ErrorResponse {
                    success: false,
                    error: format!("Failed to serialize config: {}", e),
                    code: "SERIALIZE_FAILED".to_string(),
                    timestamp: Utc::now(),
                }));
            }
        };
        
        match mtws.update_config(config_value).await {
            Ok(_) => {
                Ok(HttpResponse::Ok().json(serde_json::json!({
                    "success": true,
                    "message": "MTWS service configured successfully"
                })))
            }
            Err(e) => {
                Ok(HttpResponse::BadRequest().json(ErrorResponse {
                    success: false,
                    error: format!("Failed to configure MTWS: {}", e),
                    code: "CONFIG_FAILED".to_string(),
                    timestamp: Utc::now(),
                }))
            }
        }
    } else {
        Ok(HttpResponse::ServiceUnavailable().json(ErrorResponse {
            success: false,
            error: "MTWS service not available".to_string(),
            code: "SERVICE_UNAVAILABLE".to_string(),
            timestamp: Utc::now(),
        }))
    }
}

// POST /api/mtws/start
async fn start_mtws_transmission(
    data: web::Data<ApiServiceState>,
) -> ActixResult<HttpResponse> {
    if let Some(mtws_service) = &data.mtws_service {
        let mtws = mtws_service.lock().await;
        match mtws.start_transmission().await {
            Ok(_) => Ok(HttpResponse::Ok().json(serde_json::json!({
                "success": true,
                "message": "MTWS transmission started"
            }))),
            Err(e) => Ok(HttpResponse::BadRequest().json(ErrorResponse {
                success: false,
                error: format!("Failed to start MTWS transmission: {}", e),
                code: "START_FAILED".to_string(),
                timestamp: Utc::now(),
            }))
        }
    } else {
        Ok(HttpResponse::ServiceUnavailable().json(ErrorResponse {
            success: false,
            error: "MTWS service not available".to_string(),
            code: "SERVICE_UNAVAILABLE".to_string(),
            timestamp: Utc::now(),
        }))
    }
}

// POST /api/mtws/stop
async fn stop_mtws_transmission(
    data: web::Data<ApiServiceState>,
) -> ActixResult<HttpResponse> {
    if let Some(mtws_service) = &data.mtws_service {
        let mtws = mtws_service.lock().await;
        match mtws.stop_transmission().await {
            Ok(_) => Ok(HttpResponse::Ok().json(serde_json::json!({
                "success": true,
                "message": "MTWS transmission stopped"
            }))),
            Err(e) => Ok(HttpResponse::BadRequest().json(ErrorResponse {
                success: false,
                error: format!("Failed to stop MTWS transmission: {}", e),
                code: "STOP_FAILED".to_string(),
                timestamp: Utc::now(),
            }))
        }
    } else {
        Ok(HttpResponse::ServiceUnavailable().json(ErrorResponse {
            success: false,
            error: "MTWS service not available".to_string(),
            code: "SERVICE_UNAVAILABLE".to_string(),
            timestamp: Utc::now(),
        }))
    }
}

// POST /api/mtws/send
async fn send_mtws_payload(
    data: web::Data<ApiServiceState>,
    request: web::Json<SendMtwsPayloadRequest>,
) -> ActixResult<HttpResponse> {
    if let Some(mtws_service) = &data.mtws_service {
        let mtws = mtws_service.lock().await;
        let endpoint = request.endpoint_url.clone().unwrap_or_else(|| {
            mtws.get_endpoint_url()
        });
        match mtws.send_single_payload(endpoint).await {
            Ok(payload_summary) => Ok(HttpResponse::Ok().json(serde_json::json!({
                "success": true,
                "message": "MTWS payload sent successfully",
                "payload_summary": payload_summary
            }))),
            Err(e) => Ok(HttpResponse::BadRequest().json(ErrorResponse {
                success: false,
                error: format!("Failed to send MTWS payload: {}", e),
                code: "SEND_FAILED".to_string(),
                timestamp: Utc::now(),
            }))
        }
    } else {
        Ok(HttpResponse::ServiceUnavailable().json(ErrorResponse {
            success: false,
            error: "MTWS service not available".to_string(),
            code: "SERVICE_UNAVAILABLE".to_string(),
            timestamp: Utc::now(),
        }))
    }
}

// GET /api/mtws/status
async fn mtws_status(
    data: web::Data<ApiServiceState>,
) -> ActixResult<HttpResponse> {
    if let Some(mtws_service) = &data.mtws_service {
        let mtws = mtws_service.lock().await;
        let (is_running, interval_seconds, endpoint_url, enabled) = mtws.get_status().await;
        
        Ok(HttpResponse::Ok().json(serde_json::json!({
            "success": true,
            "status": {
                "is_running": is_running,
                "interval_seconds": interval_seconds,
                "endpoint_url": endpoint_url,
                "enabled": enabled,
                "service_available": true
            }
        })))
    } else {
        Ok(HttpResponse::Ok().json(serde_json::json!({
            "success": true,
            "status": {
                "is_running": false,
                "interval_seconds": 0,
                "endpoint_url": null,
                "enabled": false,
                "service_available": false
            }
        })))
    }
}

async fn send_combined_data(
    data: web::Data<ApiServiceState>,
    request: web::Json<SendMtwsPayloadRequest>,
) -> ActixResult<HttpResponse> {
    if let Some(mtws_service) = &data.mtws_service {
        let mtws = mtws_service.lock().await;
        let endpoint = request.endpoint_url.clone().unwrap_or_else(|| mtws.get_endpoint_url());
        match mtws.send_single_payload(endpoint).await {
            Ok(payload) => Ok(HttpResponse::Ok().json(serde_json::json!({
                "success": true,
                "message": "Combined data sent successfully",
                "payload": payload,
                "timestamp": Utc::now()
            }))),
            Err(e) => Ok(HttpResponse::BadRequest().json(ErrorResponse {
                success: false,
                error: format!("Failed to send combined data: {}", e),
                code: "SEND_FAILED".to_string(),
                timestamp: Utc::now(),
            }))
        }
    } else {
        Ok(HttpResponse::ServiceUnavailable().json(ErrorResponse {
            success: false,
            error: "MTWS service not available".to_string(),
            code: "SERVICE_UNAVAILABLE".to_string(),
            timestamp: Utc::now(),
        }))
    }
}

async fn get_mtws_config(
    data: web::Data<ApiServiceState>,
) -> ActixResult<HttpResponse> {
    if let Some(mtws_service) = &data.mtws_service {
        let mtws = mtws_service.lock().await;
        let (is_running, interval_seconds, endpoint_url, enabled) = mtws.get_status().await;
        
        Ok(HttpResponse::Ok().json(serde_json::json!({
            "success": true,
            "config": {
                "enabled": enabled,
                "is_running": is_running,
                "interval_seconds": interval_seconds,
                "endpoint_url": endpoint_url
            }
        })))
    } else {
        Ok(HttpResponse::ServiceUnavailable().json(ErrorResponse {
            success: false,
            error: "MTWS service not available".to_string(),
            code: "SERVICE_UNAVAILABLE".to_string(),
            timestamp: Utc::now(),
        }))
    }
}
// AIO Module API handlers

// GET /api/aio_module/devices - Get list of configured AIO module devices
async fn get_aio_module_devices(
    data: web::Data<ApiServiceState>,
) -> ActixResult<HttpResponse> {
    if let Some(data_service) = &data.data_service {
        let aio_devices = data_service.get_aio_module_devices();
        
        let devices_info: Vec<serde_json::Value> = aio_devices.iter().map(|device| {
            serde_json::json!({
                "address": device.address,
                "name": device.name,
                "location": device.location,
                "device_type": device.device_type,
                "enabled": device.enabled
            })
        }).collect();

        Ok(HttpResponse::Ok().json(serde_json::json!({
            "success": true,
            "devices": devices_info,
            "count": devices_info.len()
        })))
    } else {
        Ok(HttpResponse::ServiceUnavailable().json(ErrorResponse {
            success: false,
            error: "Data service not available".to_string(),
            code: "SERVICE_UNAVAILABLE".to_string(),
            timestamp: Utc::now(),
        }))
    }
}

// GET /api/aio_module/data/{address} - Get current AIO module data by address
async fn get_aio_module_data(
    path: web::Path<u8>,
    data: web::Data<ApiServiceState>,
) -> ActixResult<HttpResponse> {
    let device_address = path.into_inner();
    
    if let Some(data_service) = &data.data_service {
        match data_service.get_current_aio_module_data(device_address).await {
            Some(aio_data) => {
                Ok(HttpResponse::Ok().json(serde_json::json!({
                    "success": true,
                    "data": aio_data
                })))
            },
            None => {
                Ok(HttpResponse::NotFound().json(ErrorResponse {
                    success: false,
                    error: format!("No AIO module data found for address {}", device_address),
                    code: "DATA_NOT_FOUND".to_string(),
                    timestamp: Utc::now(),
                }))
            }
        }
    } else {
        Ok(HttpResponse::ServiceUnavailable().json(ErrorResponse {
            success: false,
            error: "Data service not available".to_string(),
            code: "SERVICE_UNAVAILABLE".to_string(),
            timestamp: Utc::now(),
        }))
    }
}

// GET /api/aio_module/readings/recent?limit=100 - Get recent AIO module readings
async fn get_recent_aio_module_readings(
    query: web::Query<serde_json::Value>,
    data: web::Data<ApiServiceState>,
) -> ActixResult<HttpResponse> {
    let limit = query.get("limit")
        .and_then(|v| v.as_i64())
        .unwrap_or(100)
        .min(1000); // Max limit of 1000

    match data.sqlite_manager.get_recent_aio_module_readings(limit).await {
        Ok(readings) => {
            let readings_json: Vec<serde_json::Value> = readings.iter().map(|reading| {
                serde_json::json!({
                    "id": reading.id,
                    "device_address": reading.device_address,
                    "unix_timestamp": reading.unix_timestamp,
                    "baud_rate": reading.baud_rate,
                    "channels": reading.get_channels().unwrap_or_default(),
                    "digital_inputs": (0..16).map(|i| reading.get_digital_input(i + 1)).collect::<Vec<bool>>(),
                    "created_at": reading.created_at
                })
            }).collect();

            Ok(HttpResponse::Ok().json(serde_json::json!({
                "success": true,
                "readings": readings_json,
                "count": readings_json.len(),
                "limit": limit
            })))
        },
        Err(e) => {
            error!("Failed to get recent AIO module readings: {}", e);
            Ok(HttpResponse::InternalServerError().json(ErrorResponse {
                success: false,
                error: format!("Database error: {}", e),
                code: "DATABASE_ERROR".to_string(),
                timestamp: Utc::now(),
            }))
        }
    }
}

// GET /api/aio_module/readings/{address}?start_time=123&end_time=456 - Get AIO module readings by address and time range
async fn get_aio_module_readings_by_address(
    path: web::Path<u8>,
    query: web::Query<serde_json::Value>,
    data: web::Data<ApiServiceState>,
) -> ActixResult<HttpResponse> {
    let device_address = path.into_inner();
    
    let start_time = query.get("start_time")
        .and_then(|v| v.as_i64())
        .unwrap_or(0);
        
    let end_time = query.get("end_time")
        .and_then(|v| v.as_i64())
        .unwrap_or(Utc::now().timestamp());

    match data.sqlite_manager.get_aio_module_readings_by_address_and_timerange(
        device_address, start_time, end_time
    ).await {
        Ok(readings) => {
            let readings_json: Vec<serde_json::Value> = readings.iter().map(|reading| {
                serde_json::json!({
                    "id": reading.id,
                    "device_address": reading.device_address,
                    "unix_timestamp": reading.unix_timestamp,
                    "baud_rate": reading.baud_rate,
                    "channels": reading.get_channels().unwrap_or_default(),
                    "digital_inputs": (0..16).map(|i| reading.get_digital_input(i + 1)).collect::<Vec<bool>>(),
                    "created_at": reading.created_at
                })
            }).collect();

            Ok(HttpResponse::Ok().json(serde_json::json!({
                "success": true,
                "readings": readings_json,
                "count": readings_json.len(),
                "device_address": device_address,
                "time_range": {
                    "start_time": start_time,
                    "end_time": end_time
                }
            })))
        },
        Err(e) => {
            error!("Failed to get AIO module readings by address: {}", e);
            Ok(HttpResponse::InternalServerError().json(ErrorResponse {
                success: false,
                error: format!("Database error: {}", e),
                code: "DATABASE_ERROR".to_string(),
                timestamp: Utc::now(),
            }))
        }
    }
}