use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GpsData {
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
    pub altitude: Option<f32>,
    pub speed: Option<f32>,
    pub course: Option<f32>,
    pub satellites: Option<u32>,
    pub fix_type: Option<String>,
    pub timestamp: Option<i64>, // Always Unix timestamp
}

impl Default for GpsData {
    fn default() -> Self {
        Self {
            latitude: None,
            longitude: None,
            altitude: None,
            speed: None,
            course: None,
            satellites: None,
            fix_type: None,
            timestamp: Some(Utc::now().timestamp()), // Default to current Unix timestamp
        }
    }
}

impl fmt::Display for GpsData {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "GPS Data:")?;
        if let Some(lat) = self.latitude {
            writeln!(f, "  Latitude: {:.6}°", lat)?;
        }
        if let Some(lon) = self.longitude {
            writeln!(f, "  Longitude: {:.6}°", lon)?;
        }
        if let Some(alt) = self.altitude {
            writeln!(f, "  Altitude: {:.2}m", alt)?;
        }
        if let Some(speed) = self.speed {
            writeln!(f, "  Speed: {:.2} knots", speed)?;
        }
        if let Some(course) = self.course {
            writeln!(f, "  Course: {:.2}°", course)?;
        }
        if let Some(sats) = self.satellites {
            writeln!(f, "  Satellites: {}", sats)?;
        }
        if let Some(fix_type) = &self.fix_type {
            writeln!(f, "  Fix Type: {}", fix_type)?;
        }
        if let Some(timestamp) = self.timestamp {
            writeln!(f, "  Unix Timestamp: {}", timestamp)?;
            // Format as readable date/time for display
            if let Some(dt) = chrono::DateTime::from_timestamp(timestamp, 0) {
                writeln!(f, "  Date/Time: {}", dt.format("%Y-%m-%d %H:%M:%S UTC"))?;
            }
        }
        Ok(())
    }
}

impl GpsData {
    pub fn has_valid_fix(&self) -> bool {
        self.latitude.is_some() && self.longitude.is_some()
    }

    /// Create a new GpsData with current Unix timestamp
    pub fn new() -> Self {
        Self {
            latitude: None,
            longitude: None,
            altitude: None,
            speed: None,
            course: None,
            satellites: None,
            fix_type: None,
            timestamp: Some(Utc::now().timestamp()),
        }
    }
}

// Helper function to parse coordinates like "0617.5017S" or "10647.6637E"
pub fn parse_coordinate(coord_str: &str) -> Option<f64> {
    if coord_str.len() < 2 {
        return None;
    }

    let (num_str, direction) = coord_str.split_at(coord_str.len() - 1);
    let direction = direction.chars().next()?;

    let coord_val = num_str.parse::<f64>().ok()?;

    // Convert DDMM.MMMM to decimal degrees
    let degrees = (coord_val / 100.0) as i32;
    let minutes = coord_val - (degrees as f64 * 100.0);
    let decimal_degrees = degrees as f64 + minutes / 60.0;

    // Apply direction
    match direction {
        'N' | 'E' => Some(decimal_degrees),
        'S' | 'W' => Some(-decimal_degrees),
        _ => None,
    }
}