pub fn build_packet(id: i32, content: &mut Vec<u8>) -> Vec<u8> {
    let mut corps = write_varint(id);
    corps.append(content);
    let mut packet = write_varint(corps.len() as i32);
    packet.extend(corps);
    packet
}

pub fn write_string(s: &str) -> Vec<u8> {
    let mut result = write_varint(s.len() as i32);
    result.extend(s.as_bytes());
    result
}

pub fn write_varint(mut value: i32) -> Vec<u8> {
    let mut result = Vec::new();
    loop {
        let mut byte = (value & 0x7F) as u8;
        value = ((value as u32) >> 7) as i32;
        if value != 0 {
            byte |= 0x80;
        }
        result.push(byte);
        if value == 0 {
            break;
        }
    }
    result
}
