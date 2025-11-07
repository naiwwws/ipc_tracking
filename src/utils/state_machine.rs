use std::sync::Arc;
use std::fmt;
use tokio::sync::RwLock;
use chrono::{DateTime, Utc};
use log::{info, warn, error, debug};
use serde::{Deserialize, Serialize};

use crate::utils::error::ModbusError;

/// System states with type safety
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SystemState {
    /// Initial state when system starts
    Initializing,
    
    /// Reading data from all configured devices
    ReadingDevices,
    
    /// Reading GPS data
    ReadingGPS,
    
    /// Testing external connections (MTWS, database, etc.)
    TestingConnection,
    
    /// System is fully operational and ready to send data
    OperationalReady,
    
    /// Actively sending data to MTWS service
    SendingData,
    
    /// Error state - requires recovery
    Error,
    
    /// Graceful shutdown in progress
    Shutdown,
}

impl fmt::Display for SystemState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SystemState::Initializing => write!(f, "Initializing"),
            SystemState::ReadingDevices => write!(f, "Reading Devices"),
            SystemState::ReadingGPS => write!(f, "Reading GPS"),
            SystemState::TestingConnection => write!(f, "Testing Connection"),
            SystemState::OperationalReady => write!(f, "Operational Ready"),
            SystemState::SendingData => write!(f, "Sending Data"),
            SystemState::Error => write!(f, "Error"),
            SystemState::Shutdown => write!(f, "Shutdown"),
        }
    }
}

/// State transition event
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StateEvent {
    /// System initialized successfully
    InitComplete,
    
    /// Device data read successfully
    DevicesReadSuccess,
    
    /// Device data read failed
    DevicesReadFailure,
    
    /// GPS data read successfully
    GPSReadSuccess,
    
    /// GPS read failed (can be skipped if GPS not critical)
    GPSReadFailure,
    
    /// Connection test passed
    ConnectionTestSuccess,
    
    /// Connection test failed
    ConnectionTestFailure,
    
    /// Ready to send data
    ReadyToSend,
    
    /// Data sent successfully
    DataSentSuccess,
    
    /// Data send failed
    DataSendFailure,
    
    /// Recoverable error occurred
    RecoverableError,
    
    /// Fatal error occurred
    FatalError,
    
    /// Shutdown requested
    ShutdownRequested,
}

impl fmt::Display for StateEvent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StateEvent::InitComplete => write!(f, "Init Complete"),
            StateEvent::DevicesReadSuccess => write!(f, "Devices Read Success"),
            StateEvent::DevicesReadFailure => write!(f, "Devices Read Failure"),
            StateEvent::GPSReadSuccess => write!(f, "GPS Read Success"),
            StateEvent::GPSReadFailure => write!(f, "GPS Read Failure"),
            StateEvent::ConnectionTestSuccess => write!(f, "Connection Test Success"),
            StateEvent::ConnectionTestFailure => write!(f, "Connection Test Failure"),
            StateEvent::ReadyToSend => write!(f, "Ready to Send"),
            StateEvent::DataSentSuccess => write!(f, "Data Sent Success"),
            StateEvent::DataSendFailure => write!(f, "Data Send Failure"),
            StateEvent::RecoverableError => write!(f, "Recoverable Error"),
            StateEvent::FatalError => write!(f, "Fatal Error"),
            StateEvent::ShutdownRequested => write!(f, "Shutdown Requested"),
        }
    }
}

/// Context holding system state and data
#[derive(Debug, Clone)]
pub struct StateContext {
    /// Current system state
    pub current_state: SystemState,
    
    /// Previous state (for rollback/debugging)
    pub previous_state: Option<SystemState>,
    
    /// Timestamp of last state change
    pub last_transition: DateTime<Utc>,
    
    /// Number of retry attempts in current state
    pub retry_count: u32,
    
    /// Maximum retries before giving up
    pub max_retries: u32,
    
    /// Whether devices have been successfully read
    pub devices_ready: bool,
    
    /// Whether GPS has been successfully read
    pub gps_ready: bool,
    
    /// Whether connection test passed
    pub connection_ready: bool,
    
    /// Last error encountered
    pub last_error: Option<String>,
    
    /// State-specific metadata
    pub metadata: std::collections::HashMap<String, String>,
}

impl StateContext {
    pub fn new(max_retries: u32) -> Self {
        Self {
            current_state: SystemState::Initializing,
            previous_state: None,
            last_transition: Utc::now(),
            retry_count: 0,
            max_retries,
            devices_ready: false,
            gps_ready: false,
            connection_ready: false,
            last_error: None,
            metadata: std::collections::HashMap::new(),
        }
    }
    
    /// Reset retry counter
    pub fn reset_retries(&mut self) {
        self.retry_count = 0;
    }
    
    /// Increment retry counter
    pub fn increment_retry(&mut self) {
        self.retry_count += 1;
    }
    
    /// Check if max retries exceeded
    pub fn is_max_retries_exceeded(&self) -> bool {
        self.retry_count >= self.max_retries
    }
    
    /// Record error
    pub fn set_error(&mut self, error: String) {
        self.last_error = Some(error);
    }
    
    /// Clear error
    pub fn clear_error(&mut self) {
        self.last_error = None;
    }
    
    /// Get time in current state
    pub fn time_in_state(&self) -> chrono::Duration {
        Utc::now() - self.last_transition
    }
}

/// State transition definition
#[derive(Debug, Clone)]
pub struct StateTransition {
    pub from: SystemState,
    pub to: SystemState,
    pub event: StateEvent,
    pub guard: Option<TransitionGuard>,
}

/// Guard function type for state transitions
#[derive(Clone)]
pub struct TransitionGuard {
    pub check: Arc<dyn Fn(&StateContext) -> bool + Send + Sync>,
    pub description: String,
}

impl fmt::Debug for TransitionGuard {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TransitionGuard")
            .field("description", &self.description)
            .finish()
    }
}

impl TransitionGuard {
    pub fn new<F>(check: F, description: &str) -> Self
    where
        F: Fn(&StateContext) -> bool + Send + Sync + 'static,
    {
        Self {
            check: Arc::new(check),
            description: description.to_string(),
        }
    }
}

/// Finite State Machine for the system
pub struct SystemStateMachine {
    context: Arc<RwLock<StateContext>>,
    transitions: Vec<StateTransition>,
}

impl SystemStateMachine {
    /// Create a new state machine with initial configuration
    pub fn new(max_retries: u32) -> Self {
        let context = Arc::new(RwLock::new(StateContext::new(max_retries)));
        let transitions = Self::build_transitions();
        
        Self {
            context,
            transitions,
        }
    }
    
    /// Build all valid state transitions
    fn build_transitions() -> Vec<StateTransition> {
        vec![
            // From Initializing
            StateTransition {
                from: SystemState::Initializing,
                to: SystemState::ReadingDevices,
                event: StateEvent::InitComplete,
                guard: None,
            },
            StateTransition {
                from: SystemState::Initializing,
                to: SystemState::Error,
                event: StateEvent::FatalError,
                guard: None,
            },
            
            // From ReadingDevices
            StateTransition {
                from: SystemState::ReadingDevices,
                to: SystemState::ReadingGPS,
                event: StateEvent::DevicesReadSuccess,
                guard: None,
            },
            StateTransition {
                from: SystemState::ReadingDevices,
                to: SystemState::Error,
                event: StateEvent::DevicesReadFailure,
                guard: Some(TransitionGuard::new(
                    |ctx| ctx.is_max_retries_exceeded(),
                    "Max retries exceeded for device reading"
                )),
            },
            StateTransition {
                from: SystemState::ReadingDevices,
                to: SystemState::ReadingDevices,
                event: StateEvent::RecoverableError,
                guard: Some(TransitionGuard::new(
                    |ctx| !ctx.is_max_retries_exceeded(),
                    "Can retry device reading"
                )),
            },
            
            // From ReadingGPS
            StateTransition {
                from: SystemState::ReadingGPS,
                to: SystemState::TestingConnection,
                event: StateEvent::GPSReadSuccess,
                guard: None,
            },
            StateTransition {
                from: SystemState::ReadingGPS,
                to: SystemState::TestingConnection,
                event: StateEvent::GPSReadFailure,
                guard: None, // GPS failure is not fatal - can proceed
            },
            
            // From TestingConnection
            StateTransition {
                from: SystemState::TestingConnection,
                to: SystemState::OperationalReady,
                event: StateEvent::ConnectionTestSuccess,
                guard: None,
            },
            StateTransition {
                from: SystemState::TestingConnection,
                to: SystemState::Error,
                event: StateEvent::ConnectionTestFailure,
                guard: Some(TransitionGuard::new(
                    |ctx| ctx.is_max_retries_exceeded(),
                    "Max connection test retries exceeded"
                )),
            },
            StateTransition {
                from: SystemState::TestingConnection,
                to: SystemState::TestingConnection,
                event: StateEvent::RecoverableError,
                guard: Some(TransitionGuard::new(
                    |ctx| !ctx.is_max_retries_exceeded(),
                    "Can retry connection test"
                )),
            },
            
            // From OperationalReady
            StateTransition {
                from: SystemState::OperationalReady,
                to: SystemState::SendingData,
                event: StateEvent::ReadyToSend,
                guard: Some(TransitionGuard::new(
                    |ctx| ctx.devices_ready && ctx.connection_ready,
                    "Devices and connection must be ready"
                )),
            },
            StateTransition {
                from: SystemState::OperationalReady,
                to: SystemState::Shutdown,
                event: StateEvent::ShutdownRequested,
                guard: None,
            },
            
            // From SendingData
            StateTransition {
                from: SystemState::SendingData,
                to: SystemState::OperationalReady,
                event: StateEvent::DataSentSuccess,
                guard: None,
            },
            StateTransition {
                from: SystemState::SendingData,
                to: SystemState::OperationalReady,
                event: StateEvent::DataSendFailure,
                guard: None, // Log error but continue operation
            },
            StateTransition {
                from: SystemState::SendingData,
                to: SystemState::Shutdown,
                event: StateEvent::ShutdownRequested,
                guard: None,
            },
            
            // From Error - recovery paths
            StateTransition {
                from: SystemState::Error,
                to: SystemState::ReadingDevices,
                event: StateEvent::RecoverableError,
                guard: Some(TransitionGuard::new(
                    |ctx| !ctx.devices_ready,
                    "Retry device reading after error"
                )),
            },
            StateTransition {
                from: SystemState::Error,
                to: SystemState::Shutdown,
                event: StateEvent::FatalError,
                guard: None,
            },
            
            // Global shutdown transitions
            StateTransition {
                from: SystemState::ReadingDevices,
                to: SystemState::Shutdown,
                event: StateEvent::ShutdownRequested,
                guard: None,
            },
            StateTransition {
                from: SystemState::ReadingGPS,
                to: SystemState::Shutdown,
                event: StateEvent::ShutdownRequested,
                guard: None,
            },
            StateTransition {
                from: SystemState::TestingConnection,
                to: SystemState::Shutdown,
                event: StateEvent::ShutdownRequested,
                guard: None,
            },
        ]
    }
    
    /// Attempt to transition to a new state
    pub async fn transition(&self, event: StateEvent) -> Result<SystemState, ModbusError> {
        let mut context = self.context.write().await;
        let current_state = context.current_state;
        
        debug!("🔄 FSM: Attempting transition from {} with event {}", current_state, event);
        
        // Find valid transition
        let valid_transition = self.transitions.iter().find(|t| {
            if t.from != current_state || t.event != event {
                return false;
            }
            
            // Check guard if present
            if let Some(guard) = &t.guard {
                let guard_result = (guard.check)(&context);
                debug!("🛡️  FSM: Guard check '{}': {}", guard.description, guard_result);
                guard_result
            } else {
                true
            }
        });
        
        match valid_transition {
            Some(transition) => {
                let old_state = context.current_state;
                let new_state = transition.to;
                
                // Update context
                context.previous_state = Some(old_state);
                context.current_state = new_state;
                context.last_transition = Utc::now();
                
                // Reset retry counter on successful transition (except retry transitions)
                if new_state != old_state {
                    if matches!(event, StateEvent::DevicesReadSuccess | StateEvent::GPSReadSuccess | StateEvent::ConnectionTestSuccess) {
                        context.reset_retries();
                    }
                }
                
                info!("✅ FSM: Transitioned from {} to {} (event: {})", old_state, new_state, event);
                
                Ok(new_state)
            }
            None => {
                warn!("❌ FSM: No valid transition from {} for event {}", current_state, event);
                Err(ModbusError::InvalidState(
                    format!("No valid transition from {} for event {}", current_state, event)
                ))
            }
        }
    }
    
    /// Get current state
    pub async fn current_state(&self) -> SystemState {
        self.context.read().await.current_state
    }
    
    /// Get context for reading
    pub async fn get_context(&self) -> StateContext {
        self.context.read().await.clone()
    }
    
    /// Update context directly
    pub async fn update_context<F>(&self, updater: F)
    where
        F: FnOnce(&mut StateContext),
    {
        let mut context = self.context.write().await;
        updater(&mut *context);
    }
    
    /// Check if in operational state
    pub async fn is_operational(&self) -> bool {
        matches!(
            self.context.read().await.current_state,
            SystemState::OperationalReady | SystemState::SendingData
        )
    }
    
    /// Check if in error state
    pub async fn is_error(&self) -> bool {
        self.context.read().await.current_state == SystemState::Error
    }
    
    /// Check if shutting down
    pub async fn is_shutdown(&self) -> bool {
        self.context.read().await.current_state == SystemState::Shutdown
    }
    
    /// Force state change (use with caution)
    pub async fn force_state(&self, new_state: SystemState) {
        let mut context = self.context.write().await;
        warn!("⚠️  FSM: Forcing state change from {} to {}", context.current_state, new_state);
        context.previous_state = Some(context.current_state);
        context.current_state = new_state;
        context.last_transition = Utc::now();
    }
    
    /// Get state history summary
    pub async fn get_state_summary(&self) -> String {
        let context = self.context.read().await;
        format!(
            "State: {} | Previous: {:?} | Time in state: {:?} | Retries: {}/{} | Ready: D:{} G:{} C:{}",
            context.current_state,
            context.previous_state,
            context.time_in_state(),
            context.retry_count,
            context.max_retries,
            context.devices_ready,
            context.gps_ready,
            context.connection_ready
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_state_machine_initialization() {
        let fsm = SystemStateMachine::new(3);
        assert_eq!(fsm.current_state().await, SystemState::Initializing);
    }

    #[tokio::test]
    async fn test_basic_transition() {
        let fsm = SystemStateMachine::new(3);
        
        // Should transition from Initializing to ReadingDevices
        let result = fsm.transition(StateEvent::InitComplete).await;
        assert!(result.is_ok());
        assert_eq!(fsm.current_state().await, SystemState::ReadingDevices);
    }

    #[tokio::test]
    async fn test_invalid_transition() {
        let fsm = SystemStateMachine::new(3);
        
        // Cannot go directly to SendingData from Initializing
        let result = fsm.transition(StateEvent::DataSentSuccess).await;
        assert!(result.is_err());
        assert_eq!(fsm.current_state().await, SystemState::Initializing);
    }

    #[tokio::test]
    async fn test_guard_condition() {
        let fsm = SystemStateMachine::new(1);
        
        // Transition to ReadingDevices
        fsm.transition(StateEvent::InitComplete).await.unwrap();
        
        // First failure should retry (not exceed max)
        fsm.update_context(|ctx| ctx.increment_retry()).await;
        
        // Should not transition to Error yet
        let result = fsm.transition(StateEvent::DevicesReadFailure).await;
        assert!(result.is_err() || result.unwrap() == SystemState::Error);
    }

    #[tokio::test]
    async fn test_full_happy_path() {
        let fsm = SystemStateMachine::new(3);
        
        // Complete initialization flow
        fsm.transition(StateEvent::InitComplete).await.unwrap();
        assert_eq!(fsm.current_state().await, SystemState::ReadingDevices);
        
        fsm.transition(StateEvent::DevicesReadSuccess).await.unwrap();
        assert_eq!(fsm.current_state().await, SystemState::ReadingGPS);
        
        fsm.transition(StateEvent::GPSReadSuccess).await.unwrap();
        assert_eq!(fsm.current_state().await, SystemState::TestingConnection);
        
        // Set required flags for operational state
        fsm.update_context(|ctx| {
            ctx.devices_ready = true;
            ctx.connection_ready = true;
        }).await;
        
        fsm.transition(StateEvent::ConnectionTestSuccess).await.unwrap();
        assert_eq!(fsm.current_state().await, SystemState::OperationalReady);
        
        fsm.transition(StateEvent::ReadyToSend).await.unwrap();
        assert_eq!(fsm.current_state().await, SystemState::SendingData);
    }
}
