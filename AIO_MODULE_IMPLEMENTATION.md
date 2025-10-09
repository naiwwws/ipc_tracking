# AIO Module Implementation for IPC Track

## Overview

This implementation successfully translates the Lua-based AIO module functionality into Rust for the IPC Track system. The implementation maintains the same core functionality as the original Lua version while providing better type safety, performance, and integration with the existing Rust codebase.

## Architecture Comparison

### Lua Implementation (Original)
- **Language**: Lua with custom scheduling and modbus libraries
- **Data Flow**: Event-driven with message queues
- **Modbus Communication**: Custom tt_modbus service
- **Data Structure**: Lua tables with manual bit manipulation
- **Storage**: Real-time processing without persistent storage

### Rust Implementation (New)
- **Language**: Rust with async/await and strong typing
- **Data Flow**: Async functions with structured error handling
- **Modbus Communication**: Integrated ModbusClient trait
- **Data Structure**: Strongly typed structs with automatic serialization
- **Storage**: SQLite database with full persistence

## Core Functionality Mapping

### 1. Device Configuration

**Lua Version:**
```lua
properties.DeviceAddress -- Byte string of device addresses
properties.UpdateIntervalSecond -- Update interval
```

**Rust Version:**
```rust
pub struct AioModuleDevice {
    pub address: u8,
    pub name: String,
    pub location: String,
    pub update_interval_seconds: u64,
    pub timeout_ms: u64,
    pub channels: Vec<AioChannelConfig>,
}
```

### 2. Modbus Data Reading

**Lua Version:**
```lua
sched.post(svc.tt_modbus.requestQueue:source(), "READ_HOLDING_REGISTER", {
    address = properties.DeviceAddress:byte(i),
    start_register_addr = 1,
    quantity = 39,
    requester = requester,
    timeout_ms = 100,
})
```

**Rust Version:**
```rust
let raw_data = client
    .read_holding_registers(self.address, 1, 39)
    .await?;
```

### 3. Data Parsing

**Lua Version:**
```lua
tt_aio_module_devices[device_address].baud_rate = (arg[1] << 8) | arg[2]
for y = 0, 3 do
    tt_aio_module_devices[device_address]["pulse_CH" .. (y + 1)] = (arg[3 + 2*y] << 8) | arg[4 + 2*y]
    -- ... other channel data
end
```

**Rust Version:**
```rust
let baud_rate = ((raw_data[0] as u16) << 8) | (raw_data[1] as u16);

for ch in 0..4 {
    let pulse_count = ((raw_data[2 + base_offset] as u16) << 8) 
                    | (raw_data[3 + base_offset] as u16);
    // ... other channel data parsing
}
```

### 4. Digital Input Processing

**Lua Version:**
```lua
local dinTmp = (arg[43] << 8) | arg[44]
for j = 0, 15 do
    tt_aio_module_devices[device_address]["DIN" .. (j + 1)] = (dinTmp >> j) & 0x1
end
```

**Rust Version:**
```rust
let din_word = ((raw_data[42] as u16) << 8) | (raw_data[43] as u16);
let mut digital_inputs = Vec::new();
for i in 0..16 {
    digital_inputs.push((din_word >> i) & 0x1 == 1);
}
```

## Data Structure Equivalence

### Channel Data Mapping

| Lua Field | Rust Struct Field | Description |
|-----------|------------------|-------------|
| `pulse_CH1-4` | `pulse_count` | Pulse counter values |
| `threshold_CH1-4` | `threshold` | Threshold settings |
| `freq_CH1-4` | `frequency` | Frequency measurements |
| `rpm_CH1-4` | `rpm_value` | RPM values |
| `avg_CH1-4` | `average_value` | Average measurements |
| `dur_rpm_CH1-4` | `duration_rpm` | RPM operation duration |
| `dur_ae1-4` | `duration_ae` | Auxiliary engine duration |
| `DIN1-16` | `digital_inputs[0-15]` | Digital input states |

### Register Layout (39 registers = 78 bytes)

| Registers | Bytes | Lua Access | Rust Parsing | Description |
|-----------|-------|------------|--------------|-------------|
| 1 | 0-1 | `arg[1-2]` | `raw_data[0-1]` | Baud rate |
| 2-5 | 2-9 | `arg[3-10]` | `raw_data[2-9]` | Pulse counts (4 channels) |
| 6-9 | 10-17 | `arg[11-18]` | `raw_data[10-17]` | Thresholds (4 channels) |
| 10-13 | 18-25 | `arg[19-26]` | `raw_data[18-25]` | Frequencies (4 channels) |
| 14-17 | 26-33 | `arg[27-34]` | `raw_data[26-33]` | RPM values (4 channels) |
| 18-21 | 34-41 | `arg[35-42]` | `raw_data[34-41]` | Average values (4 channels) |
| 22 | 42-43 | `arg[43-44]` | `raw_data[42-43]` | Digital inputs (16 bits) |
| 23-30 | 44-59 | `arg[47-62]` | `raw_data[46-61]` | Duration RPM (4×4 bytes) |
| 31-38 | 60-75 | `arg[63-78]` | `raw_data[62-77]` | Duration AE (4×4 bytes) |

## API Endpoints

The Rust implementation provides RESTful HTTP endpoints for accessing AIO module data:

### Device Management
- `GET /api/aio_module/devices` - List configured AIO modules
- `GET /api/aio_module/data/{address}` - Get current data from specific device

### Historical Data
- `GET /api/aio_module/readings/recent?limit=100` - Recent readings across all devices
- `GET /api/aio_module/readings/{address}?start_time=X&end_time=Y` - Time-range queries

### Response Format
```json
{
  "success": true,
  "data": {
    "device_address": 1,
    "device_name": "Main Engine AIO",
    "timestamp": "2025-10-05T10:30:00Z",
    "baud_rate": 9600,
    "channels": [
      {
        "channel_id": 1,
        "pulse_count": 1250,
        "threshold": 100,
        "frequency": 25,
        "rpm_value": 1500,
        "average_value": 1480,
        "duration_rpm": 36000,
        "duration_ae": 0
      }
    ],
    "digital_inputs": [
      {"din_number": 1, "state": true},
      {"din_number": 2, "state": false}
    ]
  }
}
```

## Configuration Example

To add an AIO module to your system, update the configuration file:

```toml
[[devices]]
uuid = "aio-main-engine"
address = 1
device_type = "aio_module"
name = "Main Engine AIO Module"
location = "Engine Room"
enabled = true

[[devices]]
uuid = "aio-aux-systems"
address = 2
device_type = "aio_module"
name = "Auxiliary Systems AIO"
location = "Control Panel"
enabled = true
```

## Error Handling

The Rust implementation provides comprehensive error handling:

- **Connection Errors**: Automatic retry with exponential backoff
- **Data Validation**: Type-safe parsing with descriptive error messages
- **Database Errors**: Graceful degradation with error logging
- **API Errors**: Structured JSON error responses with error codes

## Performance Improvements

Compared to the Lua implementation:

1. **Type Safety**: Compile-time error checking prevents runtime failures
2. **Memory Management**: Automatic memory management without garbage collection pauses
3. **Concurrency**: True async/await support for better resource utilization
4. **Database Integration**: Persistent storage with automatic batching and indexing
5. **JSON Serialization**: Automatic serialization/deserialization with serde

## Usage Examples

### Reading Current AIO Data (Rust)
```rust
let data_service = DataService::new(config).await?;
if let Some(aio_data) = data_service.get_current_aio_module_data(1).await {
    println!("Channel 1 RPM: {}", aio_data.channels[0].rpm_value);
    println!("DIN 1 State: {}", aio_data.digital_inputs[0]);
}
```

### API Usage (HTTP)
```bash
# Get all AIO devices
curl http://localhost:8080/api/aio_module/devices

# Get current data from device address 1
curl http://localhost:8080/api/aio_module/data/1

# Get recent readings
curl "http://localhost:8080/api/aio_module/readings/recent?limit=50"

# Get historical data for device 1
curl "http://localhost:8080/api/aio_module/readings/1?start_time=1696500000&end_time=1696586400"
```

## Integration with Existing Systems

The AIO module implementation seamlessly integrates with existing IPC Track components:

- **DataService**: Manages device lifecycle and data collection
- **DatabaseService**: Handles persistent storage and querying
- **MTWS Service**: Can include AIO data in satellite transmissions
- **API Service**: Provides REST endpoints for external systems
- **Configuration System**: Dynamic device configuration without restarts

## Conclusion

This Rust implementation provides a complete, production-ready AIO module system that maintains full compatibility with the original Lua functionality while adding:

- Better error handling and logging
- Persistent data storage
- RESTful API access
- Type safety and performance improvements
- Integration with the broader IPC Track ecosystem

The implementation follows the exact same modbus communication protocol and data parsing logic as the Lua version, ensuring drop-in compatibility with existing AIO module hardware.