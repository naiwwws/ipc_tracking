use log::{info, warn, error};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::broadcast;
use uuid::Uuid;
use chrono::Utc;

use super::dynamic_manager::{DynamicConfigManager, ConfigurationCommand, ConfigurationResponse};
use crate::utils::error::ModbusError;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MqttConfigMessage {
    pub message_id: String,
    pub timestamp: String,
    pub operator: String,
    pub command: ConfigurationCommand,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MqttConfigResponse {
    pub message_id: String,
    pub response: ConfigurationResponse,
}

pub struct MqttConfigHandler {
    config_manager: Arc<DynamicConfigManager>,
    mqtt_client: Option<Arc<dyn MqttClientTrait>>,
    command_topic: String,
    response_topic: String,
}

#[async_trait::async_trait]
pub trait MqttClientTrait: Send + Sync {
    async fn publish(&self, topic: &str, payload: &str) -> Result<(), ModbusError>;
    async fn subscribe(&self, topic: &str) -> Result<broadcast::Receiver<String>, ModbusError>;
}

impl MqttConfigHandler {
    pub fn new(
        config_manager: Arc<DynamicConfigManager>,
        command_topic: String,
        response_topic: String,
    ) -> Self {
        Self {
            config_manager,
            mqtt_client: None,
            command_topic,
            response_topic,
        }
    }

    pub fn set_mqtt_client(&mut self, client: Arc<dyn MqttClientTrait>) {
        self.mqtt_client = Some(client);
    }

    pub async fn start_listening(&self) -> Result<(), ModbusError> {
        if let Some(mqtt_client) = &self.mqtt_client {
            info!("🔧 Starting MQTT configuration listener on topic: {}", self.command_topic);
            
            let mut receiver = mqtt_client.subscribe(&self.command_topic).await?;
            let config_manager = self.config_manager.clone();
            let mqtt_client = mqtt_client.clone();
            let response_topic = self.response_topic.clone();

            tokio::spawn(async move {
                while let Ok(message) = receiver.recv().await {
                    match Self::handle_mqtt_message(&message, &config_manager, &mqtt_client, &response_topic).await {
                        Ok(_) => info!(" Successfully processed MQTT configuration message"),
                        Err(e) => error!("❌ Failed to handle MQTT configuration message: {}", e),
                    }
                }
            });

            Ok(())
        } else {
            Err(ModbusError::CommunicationError("MQTT client not configured".to_string()))
        }
    }

    async fn handle_mqtt_message(
        message: &str,
        config_manager: &Arc<DynamicConfigManager>,
        mqtt_client: &Arc<dyn MqttClientTrait>,
        response_topic: &str,
    ) -> Result<(), ModbusError> {
        info!("📨 Received MQTT configuration message: {}", message);

        // Parse MQTT message
        let mqtt_message: MqttConfigMessage = serde_json::from_str(message)
            .map_err(|e| ModbusError::InvalidData(format!("Invalid MQTT message format: {}", e)))?;

        info!("📨 Processing configuration command: {} by {}", 
              mqtt_message.command.command_type, mqtt_message.operator);

        // Execute configuration command using the existing method
        let response = config_manager.execute_command(mqtt_message.command).await;

        // Send response back via MQTT
        let mqtt_response = MqttConfigResponse {
            message_id: mqtt_message.message_id,
            response,
        };

        let response_json = serde_json::to_string(&mqtt_response)
            .map_err(|e| ModbusError::InvalidData(format!("Failed to serialize response: {}", e)))?;

        mqtt_client.publish(response_topic, &response_json).await?;

        info!("📤 Sent MQTT configuration response");
        Ok(())
    }

    // Helper method to create MQTT command messages
    pub fn create_command_message(
        operator: &str,
        command: ConfigurationCommand,
    ) -> Result<String, ModbusError> {
        let mqtt_message = MqttConfigMessage {
            message_id: Uuid::new_v4().to_string(),
            timestamp: Utc::now().to_rfc3339(),
            operator: operator.to_string(),
            command,
        };

        serde_json::to_string(&mqtt_message)
            .map_err(|e| ModbusError::InvalidData(format!("Failed to serialize MQTT message: {}", e)))
    }
}

// Mock MQTT client for testing/placeholder
pub struct MockMqttClient;

#[async_trait::async_trait]
impl MqttClientTrait for MockMqttClient {
    async fn publish(&self, topic: &str, payload: &str) -> Result<(), ModbusError> {
        info!("📻 [MOCK MQTT] Publishing to topic '{}': {}", topic, payload);
        Ok(())
    }

    async fn subscribe(&self, topic: &str) -> Result<broadcast::Receiver<String>, ModbusError> {
        info!("📻 [MOCK MQTT] Subscribing to topic: {}", topic);
        let (sender, receiver) = broadcast::channel(100);
        
        // Simulate incoming messages for testing
        tokio::spawn(async move {
            tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
            let _ = sender.send("Mock MQTT message".to_string());
        });

        Ok(receiver)
    }
}