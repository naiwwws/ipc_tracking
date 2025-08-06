#[derive(Debug, Clone)]
pub enum ModbusRequest {
    ReadHoldingRegisters {
        address: u8,
        start_register: u16,
        quantity: u16,
        requester: String,
    },
    WriteSingleCoil {
        address: u8,
        coil_addr: u16,
        value: bool,
        requester: String,
    },
}

#[derive(Debug, Clone)]
pub enum ModbusResponse {
    Success(Vec<u8>),
    Error(String),
}