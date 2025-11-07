use clap::{Arg, Command, ArgAction};

/// CLI Builder for IPC Tracking Application
pub struct CliBuilder;

impl CliBuilder {
    /// Build the complete CLI structure
    pub fn build() -> Command {
        Command::new("ipc_tracking")
            .version(env!("CARGO_PKG_VERSION"))
            .about("Modular Industrial Device Communication Service")
            .args(Self::build_global_args())
            .subcommands(Self::build_subcommands())
    }
    
    /// Build global arguments (available to all commands)
    fn build_global_args() -> Vec<Arg> {
        vec![
            Arg::new("config-file")
                .long("config-file")
                .short('c')
                .value_name("FILE")
                .help("Configuration file path")
                .default_value("setup/default.toml"),
            
            Arg::new("port")
                .short('p')
                .long("port")
                .value_name("PORT")
                .help("Serial port path")
                .default_value("/dev/ttyS0"),
            
            Arg::new("baud")
                .short('b')
                .long("baud")
                .value_name("BAUD")
                .help("Baud rate")
                .default_value("9600"),
            
            Arg::new("devices")
                .short('d')
                .long("devices")
                .value_name("DEVICES")
                .help("Device addresses (comma-separated)")
                .default_value("2,3"),
            
            Arg::new("interval")
                .short('i')
                .long("interval")
                .value_name("SECONDS")
                .help("Update interval in seconds")
                .default_value("10"),
            
            Arg::new("debug")
                .long("debug")
                .short('D')
                .help("Enable debug mode with automatic data printing")
                .action(ArgAction::SetTrue),
            
            Arg::new("fsm-mode")
                .long("fsm-mode")
                .help("Enable Finite State Machine mode with strict startup sequence")
                .action(ArgAction::SetTrue),
            
            Arg::new("format")
                .long("format")
                .value_name("FORMAT")
                .help("Output format: console, json, csv, hex")
                .value_parser(["console", "json", "csv", "hex"])
                .default_value("console"),
            
            Arg::new("output-file")
                .long("output-file")
                .value_name("FILE")
                .help("Write output to file"),
            
            Arg::new("output-http")
                .long("output-http")
                .value_name("URL")
                .help("Send output to HTTP endpoint"),
            
            Arg::new("output-db")
                .long("output-db")
                .value_name("CONNECTION")
                .help("Send output to database"),
            
            Arg::new("output-mqtt")
                .long("output-mqtt")
                .value_name("BROKER,TOPIC")
                .help("Send output to MQTT broker (format: broker_url,topic)"),
            
            Arg::new("socket-port")
                .long("socket-port")
                .value_name("PORT")
                .help("Enable socket server on specified port (default: 8080)"),
            
            Arg::new("socket")
                .long("socket")
                .action(ArgAction::SetTrue)
                .help("Enable socket server on default port (8080)"),
            
            Arg::new("websocket")
                .long("websocket")
                .action(ArgAction::SetTrue)
                .help("Enable WebSocket server on default port (8080)"),
            
            Arg::new("websocket-port")
                .long("websocket-port")
                .value_name("PORT")
                .help("Enable WebSocket server on specified port"),
            
            Arg::new("disable-socket")
                .long("disable-socket")
                .action(ArgAction::SetTrue)
                .help("Disable all socket/websocket servers"),
            
            Arg::new("api-port")
                .long("api-port")
                .value_name("PORT")
                .help("Enable HTTP API server on specified port (default: 3000)"),
            
            Arg::new("api")
                .long("api")
                .action(ArgAction::SetTrue)
                .help("Enable HTTP API server on default port (3000)"),
        ]
    }
    
    /// Build all subcommands
    fn build_subcommands() -> Vec<Command> {
        vec![
            Self::build_data_commands(),
            Self::build_config_commands(),
            Self::build_db_commands(),
            Self::build_websocket_commands(),
            Self::build_gps_commands(),
            Self::build_mtws_commands(),
            Self::build_device_commands(),
            Self::build_engine_commands(),
        ]
    }
    
    /// Build data-related commands
    fn build_data_commands() -> Command {
        Command::new("getdata")
            .about("Get all device data")
    }
    
    /// Build configuration management commands
    fn build_config_commands() -> Command {
        Command::new("config")
            .about("Configuration management")
            .subcommand(
                Command::new("show")
                    .about("Show current configuration")
            )
            .subcommand(
                Command::new("ipc")
                    .about("IPC configuration management")
                    .subcommand(
                        Command::new("set-name")
                            .about("Set IPC name")
                            .arg(Arg::new("name").help("IPC name").required(true).index(1))
                            .arg(Arg::new("operator").long("operator").help("Operator name").default_value("CLI"))
                    )
                    .subcommand(
                        Command::new("regenerate-uuid")
                            .about("Generate new UUID for this IPC")
                    )
            )
            .subcommand(
                Command::new("set-interval")
                    .about("Set polling interval")
                    .arg(Arg::new("seconds").help("Polling interval in seconds").required(true).index(1))
                    .arg(Arg::new("operator").long("operator").help("Operator name").default_value("CLI"))
            )
            .subcommand(
                Command::new("set")
                    .about("Set configuration parameter")
                    .arg(Arg::new("target").help("Target (serial, device:ADDRESS, monitoring, site)").required(true).index(1))
                    .arg(Arg::new("key").help("Parameter key").required(true).index(2))
                    .arg(Arg::new("value").help("Parameter value").required(true).index(3))
                    .arg(Arg::new("operator").long("operator").help("Operator name").default_value("CLI"))
            )
            .subcommand(Self::build_device_add_command())
            .subcommand(
                Command::new("enable")
                    .about("Enable device")
                    .arg(Arg::new("address").help("Device address").required(true).index(1))
                    .arg(Arg::new("operator").long("operator").help("Operator name").default_value("CLI"))
            )
            .subcommand(
                Command::new("disable")
                    .about("Disable device")
                    .arg(Arg::new("address").help("Device address").required(true).index(1))
                    .arg(Arg::new("operator").long("operator").help("Operator name").default_value("CLI"))
            )
            .subcommand(
                Command::new("remove")
                    .about("Remove device")
                    .arg(Arg::new("address").help("Device address").required(true).index(1))
                    .arg(Arg::new("operator").long("operator").help("Operator name").default_value("CLI"))
            )
            .subcommand(
                Command::new("backup")
                    .about("Backup configuration")
                    .arg(Arg::new("name").long("name").help("Backup name"))
                    .arg(Arg::new("operator").long("operator").help("Operator name").default_value("CLI"))
            )
            .subcommand(
                Command::new("restore")
                    .about("Restore configuration")
                    .arg(Arg::new("name").help("Backup name").required(true).index(1))
                    .arg(Arg::new("operator").long("operator").help("Operator name").default_value("CLI"))
            )
            .subcommand(
                Command::new("reset")
                    .about("Reset configuration to defaults")
            )
            .subcommand(Self::build_rpm_config_commands())
    }
    
    /// Build device add command with all device-specific arguments
    fn build_device_add_command() -> Command {
        Command::new("add")
            .about("Add new device")
            .arg(Arg::new("type")
                .long("type")
                .short('t')
                .value_name("TYPE")
                .help("Device type (flowmeter, rpm, gps, aio)")
                .required(true))
            .arg(Arg::new("address")
                .long("address")
                .short('a')
                .value_name("ADDRESS")
                .help("Device Modbus address (1-255)")
                .required(true))
            .arg(Arg::new("name")
                .long("name")
                .short('n')
                .value_name("NAME")
                .help("Device name")
                .required(true))
            .arg(Arg::new("location")
                .long("location")
                .short('l')
                .value_name("LOCATION")
                .help("Device location")
                .default_value("Unknown"))
            // RPM-specific arguments
            .arg(Arg::new("channels")
                .long("channels")
                .short('c')
                .value_name("COUNT")
                .help("Number of RPM channels (1-8) - only for RPM devices"))
            .arg(Arg::new("thresholds")
                .long("thresholds")
                .value_name("THRESHOLDS")
                .help("Comma-separated RPM thresholds for each channel")
                .value_delimiter(','))
            .arg(Arg::new("engine-types")
                .long("engine-types")
                .value_name("TYPES")
                .help("Comma-separated engine types for each channel")
                .value_delimiter(','))
            .arg(Arg::new("auto-detect")
                .long("auto-detect")
                .help("Enable auto-detection of channels (RPM only)")
                .action(ArgAction::SetTrue))
            // AIO-specific arguments
            .arg(Arg::new("analog-channels")
                .long("analog-channels")
                .value_name("COUNT")
                .help("Number of analog input channels (1-8) - only for AIO devices"))
            .arg(Arg::new("digital-inputs")
                .long("digital-inputs")
                .value_name("COUNT")
                .help("Number of digital input channels (1-16) - only for AIO devices")
                .default_value("16"))
            .arg(Arg::new("channel-types")
                .long("channel-types")
                .value_name("TYPES")
                .help("Comma-separated channel types (rpm,pulse,frequency) for each analog channel")
                .value_delimiter(','))
            .arg(Arg::new("aio-thresholds")
                .long("aio-thresholds")
                .value_name("THRESHOLDS")
                .help("Comma-separated RPM thresholds for AIO channels (only for RPM type channels)")
                .value_delimiter(','))
            .arg(Arg::new("auto-detect-aio")
                .long("auto-detect-aio")
                .help("Enable auto-detection of AIO channel types")
                .action(ArgAction::SetTrue))
    }
    
    /// Build RPM configuration commands
    fn build_rpm_config_commands() -> Command {
        Command::new("rpm")
            .about("RPM device management")
            .subcommand(
                Command::new("register")
                    .about("Register new RPM device")
                    .arg(Arg::new("address").long("address").required(true))
                    .arg(Arg::new("name").long("name").required(true))
                    .arg(Arg::new("location").long("location").default_value("Unknown"))
                    .arg(Arg::new("channels").long("channels").required(true))
                    .arg(Arg::new("thresholds").long("thresholds").value_delimiter(','))
                    .arg(Arg::new("engine-types").long("engine-types").value_delimiter(','))
                    .arg(Arg::new("auto-detect").long("auto-detect").action(ArgAction::SetTrue))
            )
            .subcommand(
                Command::new("update")
                    .about("Update RPM device")
                    .arg(Arg::new("address").long("address").required(true))
                    .arg(Arg::new("channel").long("channel"))
                    .arg(Arg::new("threshold").long("threshold"))
                    .arg(Arg::new("engine-type").long("engine-type"))
            )
    }
    
    /// Build database commands
    fn build_db_commands() -> Command {
        Command::new("db")
            .about("Database operations")
            .subcommand(Command::new("init").about("Initialize database"))
            .subcommand(Command::new("stats").about("Show database statistics"))
            .subcommand(
                Command::new("query")
                    .about("Query database")
                    .arg(Arg::new("table").short('t').long("table").help("Table name").default_value("device_readings"))
                    .arg(Arg::new("limit").short('l').long("limit").help("Limit results").default_value("10"))
                    .arg(Arg::new("device").short('d').long("device").help("Device address filter"))
            )
            .subcommand(Command::new("schema").about("Show database schema"))
    }
    
    /// Build WebSocket commands
    fn build_websocket_commands() -> Command {
        Command::new("websocket")
            .about("WebSocket server management")
            .subcommand(Command::new("status").about("Show WebSocket server status"))
            .subcommand(Command::new("clients").about("Show connected WebSocket clients"))
    }
    
    /// Build GPS commands
    fn build_gps_commands() -> Command {
        Command::new("gps")
            .about("GPS location and tracking commands")
            .subcommand(Command::new("start").about("Start GPS service"))
            .subcommand(Command::new("stop").about("Stop GPS service"))
            .subcommand(Command::new("status").about("Show GPS service status"))
            .subcommand(Command::new("data").about("Show current GPS data and location"))
            .subcommand(Command::new("test").about("Test GPS connection and wait for fix"))
    }
    
    /// Build MTWS commands
    fn build_mtws_commands() -> Command {
        Command::new("mtws")
            .about("MTWS (Marine Transport and Warehouse System) operations")
            .subcommand(
                Command::new("config")
                    .about("Configure MTWS settings")
                    .arg(Arg::new("imei")
                        .long("imei")
                        .help("Device IMEI (15 digits)")
                        .value_name("IMEI"))
                    .arg(Arg::new("endpoint")
                        .long("endpoint")
                        .help("Base endpoint URL (without IMEI)")
                        .value_name("URL"))
                    .arg(Arg::new("interval")
                        .long("interval")
                        .help("Transmission interval in seconds (minimum 1)")
                        .value_name("SECONDS"))
                    .arg(Arg::new("auto-start")
                        .long("auto-start")
                        .help("Enable/disable auto-start (true/false, yes/no, 1/0, on/off)")
                        .value_name("BOOL"))
            )
            .subcommand(Command::new("start").about("Start automatic MTWS transmission"))
            .subcommand(Command::new("stop").about("Stop automatic MTWS transmission"))
            .subcommand(Command::new("send").about("Send MTWS data immediately (one-time)"))
            .subcommand(Command::new("test").about("Test MTWS endpoint connectivity"))
            .subcommand(Command::new("status").about("Show MTWS service status and configuration"))
            .subcommand(Command::new("enable").about("Enable MTWS service"))
            .subcommand(Command::new("disable").about("Disable MTWS service"))
    }
    
    /// Build device operation commands
    fn build_device_commands() -> Command {
        // Create a wrapper command that includes multiple device-related subcommands
        // We'll use a custom approach since we need multiple top-level device commands
        
        // Return a dummy command and handle the others separately
        Command::new("getvolatile")
            .about("Get volatile data for specific parameter")
            .arg(
                Arg::new("parameter")
                    .help("Parameter name")
                    .required(true)
                    .index(1),
            )
    }
    
    /// Build additional device commands (these will be added separately)
    pub fn build_additional_device_commands() -> Vec<Command> {
        vec![
            Command::new("resetaccumulation")
                .about("Reset accumulation for a device")
                .arg(
                    Arg::new("device_address")
                        .help("Device address to reset")
                        .required(true)
                        .index(1),
                ),
            
            Command::new("getrawdata")
                .about("Get raw data from device")
                .arg(Arg::new("device")
                    .help("Device address")
                    .required(true)
                    .index(1))
                .arg(Arg::new("format")
                    .long("format")
                    .help("Output format")
                    .value_parser(["hex", "raw", "json"])
                    .default_value("hex"))
                .arg(Arg::new("output")
                    .long("output")
                    .help("Output file path")
                    .value_name("FILE")),
            
            Command::new("flowmeter")
                .about("Flowmeter device operations")
                .subcommand(
                    Command::new("query")
                        .about("Query flowmeter data")
                        .arg(Arg::new("device")
                            .help("Device address")
                            .required(true)
                            .index(1))
                        .arg(Arg::new("limit")
                            .help("Number of records")
                            .default_value("10"))
                )
                .subcommand(Command::new("stats").about("Show flowmeter statistics"))
                .subcommand(
                    Command::new("recent")
                        .about("Show recent flowmeter readings")
                        .arg(Arg::new("limit")
                            .help("Number of records")
                            .default_value("20"))
                ),
            
            Command::new("rpm")
                .about("RPM device operations")
                .subcommand(
                    Command::new("read")
                        .about("Read RPM device data")
                        .arg(Arg::new("address")
                            .help("Device address")
                            .required(true))
                        .arg(Arg::new("channel")
                            .help("Specific channel (optional)")
                            .long("channel"))
                )
                .subcommand(Command::new("status").about("Show RPM device status")),
        ]
    }
    
    /// Build engine management commands
    fn build_engine_commands() -> Command {
        Command::new("engine")
            .about("Engine management commands")
            .subcommand(
                Command::new("duration")
                    .about("Engine duration commands")
                    .subcommand(
                        Command::new("reset")
                            .about("Reset engine duration")
                            .arg(Arg::new("address")
                                .help("Engine address")
                                .required(true))
                    )
            )
    }
    
    /// Build complete CLI with all commands properly structured
    pub fn build_complete() -> Command {
        let mut app = Command::new("ipc_tracking")
            .version(env!("CARGO_PKG_VERSION"))
            .about("Modular Industrial Device Communication Service")
            .args(Self::build_global_args());
        
        // Add all subcommands
        app = app
            .subcommand(Self::build_data_commands())
            .subcommand(Self::build_device_commands())
            .subcommand(Self::build_config_commands())
            .subcommand(Self::build_db_commands())
            .subcommand(Self::build_websocket_commands())
            .subcommand(Self::build_gps_commands())
            .subcommand(Self::build_mtws_commands())
            .subcommand(Self::build_engine_commands());
        
        // Add additional device commands
        for cmd in Self::build_additional_device_commands() {
            app = app.subcommand(cmd);
        }
        
        app
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cli_builds() {
        let cli = CliBuilder::build_complete();
        assert_eq!(cli.get_name(), "ipc_tracking");
    }

    #[test]
    fn test_has_config_subcommand() {
        let cli = CliBuilder::build_complete();
        let subcommands: Vec<_> = cli.get_subcommands().map(|cmd| cmd.get_name()).collect();
        assert!(subcommands.contains(&"config"));
    }

    #[test]
    fn test_has_gps_subcommand() {
        let cli = CliBuilder::build_complete();
        let subcommands: Vec<_> = cli.get_subcommands().map(|cmd| cmd.get_name()).collect();
        assert!(subcommands.contains(&"gps"));
    }

    #[test]
    fn test_has_mtws_subcommand() {
        let cli = CliBuilder::build_complete();
        let subcommands: Vec<_> = cli.get_subcommands().map(|cmd| cmd.get_name()).collect();
        assert!(subcommands.contains(&"mtws"));
    }
}
