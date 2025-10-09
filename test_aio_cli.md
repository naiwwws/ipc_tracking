# AIO Module CLI Command Test

## Add AIO Device Command

Based on the register map in your screenshot, here's how to add an AIO module:

```bash
# Basic AIO module with 4 analog channels
cargo run -- config add \
  --type aio \
  --address 33 \
  --name "Main AIO Module" \
  --location "Control Panel" \
  --analog-channels 4 \
  --channel-types rpm,rpm,pulse,frequency \
  --aio-thresholds 200,300,0,0 \
  --auto-detect-aio

# Advanced AIO module with 8 analog channels
cargo run -- config add \
  --type aio \
  --address 5 \
  --name "Secondary AIO Module" \
  --location "Engine Room" \
  --analog-channels 8 \
  --digital-inputs 16 \
  --channel-types rpm,rpm,pulse,frequency,rpm,pulse,frequency,pulse \
  --aio-thresholds 500,600,0,0,400,0,0,0 \
  --auto-detect-aio
```

## Expected Configuration

The CLI should generate a device entry like:

```toml
[[devices]]
uuid = "auto-generated"
address = 33
device_type = "aio"
name = "Main AIO Module"
location = "Control Panel"
enabled = true
parameters = [
    "BaudRate", "ChannelCount",
    "AIN1", "AIN2", "AIN3", "AIN4", "AIN5", "AIN6", "AIN7", "AIN8",
    "DIN1", "DIN2", "DIN3", "DIN4", "DIN5", "DIN6", "DIN7", "DIN8",
    "DIN9", "DIN10", "DIN11", "DIN12", "DIN13", "DIN14", "DIN15", "DIN16"
]

[devices.metadata]
total_analog_channels = "4"
digital_inputs_count = "16"
channel_types = "rpm,rpm,pulse,frequency"
rpm_thresholds = "200,300,0,0"
auto_detect_channels = "true"
baud_rate = "9600"
channel_1_type = "rpm"
channel_1_threshold = "200"
channel_2_type = "rpm"
channel_2_threshold = "300"
channel_3_type = "pulse"
channel_3_threshold = "0"
channel_4_type = "frequency"
channel_4_threshold = "0"
```

## Register Map Alignment

Based on your screenshot, the AIO module register map is:
- Register 0: SLAVE_ID (33)
- Register 1: BAUD_RATE (96 → 9600 baud)
- Registers 2-5: PULSE_CH1-4
- Registers 6-9: THRESHOLD_CH1-4
- Registers 10-13: FREQ_CH1-4  
- Registers 14-17: RPM_CH1-4
- Registers 18-21: AVG_CH1-4
- Register 22: DIN_STATE (digital inputs)
- Register 23: Reserved
- Registers 24-31: DUR_RPM_CH1-4 (32-bit values)
- Registers 32-39: DUR_AE1-4 (32-bit values)

The implementation now correctly reads registers 0-39 (40 total) and parses them according to this map.