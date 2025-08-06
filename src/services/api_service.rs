use actix_web::{web, App, HttpServer, HttpResponse, Result as ActixResult, middleware::Logger};
use log::{info, error, warn};
use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};
use std::sync::Arc;

use crate::config::Config;
use crate::utils::error::ModbusError;
use crate::storage::{SqliteManager};
use crate::services::data_service::DataService; // FIXED: Updated import path

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(rename_all = "snake_case")]
pub enum FlowType {
    FlowIn,
    FlowOut,
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
    pub data_service: Option<Arc<DataService>>, // NEW field
}

impl ApiServiceState {
    pub fn new(config: Config, sqlite_manager: SqliteManager, data_service: Option<Arc<DataService>>) -> Self {
        Self {
            sqlite_manager,
            config,
            data_service,
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
    
    // NEW: Constructor that accepts pre-built state with DataService
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
                )
        })
        .bind(format!("0.0.0.0:{}", port))?
        .run();
        
        // Store server handle for graceful shutdown
        self.server_handle = Some(server.handle());
        
        // Start the server in background
        tokio::spawn(async move {
            if let Err(e) = server.await {
                error!("❌ HTTP API server error: {}", e);
            }
        });
        
        info!("✅ HTTP API server started successfully on port {}", port);
        Ok(())
    }

    pub async fn stop(&mut self) -> Result<(), ModbusError> {
        info!("🛑 Stopping HTTP API server...");
        
        if let Some(handle) = self.server_handle.take() {
            // Use graceful shutdown with timeout
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
