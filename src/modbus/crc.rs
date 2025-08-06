pub fn crc16_modbus(data: &[u8]) -> u16 {
    let mut crc: u16 = 0xFFFF;
    let poly: u16 = 0xA001;

    for &byte in data {
        crc ^= byte as u16;
        for _ in 0..8 {
            if crc & 0x0001 != 0 {
                crc = (crc >> 1) ^ poly;
            } else {
                crc >>= 1;
            }
        }
    }
    crc
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_crc16_modbus() {
        let data = vec![0x01, 0x03, 0x00, 0xF4, 0x00, 0x16];
        let crc = crc16_modbus(&data);
        assert!(crc != 0);
    }
}