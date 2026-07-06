use tokio::io::AsyncRead;
use tokio::io::AsyncReadExt;

pub async fn read_packet<R: AsyncRead + Unpin>(socket: &mut R) -> std::io::Result<(i32, Vec<u8>)> {
    let length = read_varint(socket).await?;
    let mut buf = vec![0u8; length as usize];
    socket.read_exact(&mut buf).await?;
    let mut idx = 0;
    let id = read_varint_buf(&buf, &mut idx);
    let data = buf[idx..].to_vec();
    Ok((id, data))
}

pub async fn read_varint<R: AsyncRead + Unpin>(socket: &mut R) -> std::io::Result<i32> {
    let mut value: i32 = 0;
    let mut offset = 0;
    loop {
        let mut byte = [0u8; 1];
        socket.read_exact(&mut byte).await?;
        let byte = byte[0];
        value |= ((byte & 0x7F) as i32) << offset;
        if byte & 0x80 == 0 {
            break;
        }
        offset += 7;
        if offset >= 32 {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "VarInt too long"));
        }
    }
    Ok(value)
}

pub fn read_varint_buf(buf: &[u8], idx: &mut usize) -> i32 {
    let mut value: i32 = 0;
    let mut offset = 0;
    loop {
        let byte = buf[*idx];
        *idx += 1;
        value |= ((byte & 0x7F) as i32) << offset;
        if byte & 0x80 == 0 {
            break;
        }
        offset += 7;
    }
    value
}

pub fn read_string_buf(buf: &[u8], idx: &mut usize) -> String {
    let length = read_varint_buf(buf, idx) as usize;
    let s = String::from_utf8_lossy(&buf[*idx..*idx + length]).to_string();
    *idx += length;
    s
}

pub fn read_f64_buf(buf: &[u8], idx: &mut usize) -> f64 {
    let v = f64::from_be_bytes(buf[*idx..*idx + 8].try_into().unwrap());
    *idx += 8;
    v
}

pub fn read_f32_buf(buf: &[u8], idx: &mut usize) -> f32 {
    let v = f32::from_be_bytes(buf[*idx..*idx + 4].try_into().unwrap());
    *idx += 4;
    v
}

pub fn read_i32_buf(buf: &[u8], idx: &mut usize) -> i32 {
    let v = i32::from_be_bytes(buf[*idx..*idx + 4].try_into().unwrap());
    *idx += 4;
    v
}

pub fn read_i64_buf(buf: &[u8], idx: &mut usize) -> i64 {
    let v = i64::from_be_bytes(buf[*idx..*idx + 8].try_into().unwrap());
    *idx += 8;
    v
}

pub fn read_u8_buf(buf: &[u8], idx: &mut usize) -> u8 {
    let v = buf[*idx];
    *idx += 1;
    v
}

pub fn read_i16_buf(buf: &[u8], idx: &mut usize) -> i16 {
    let v = i16::from_be_bytes([buf[*idx], buf[*idx + 1]]);
    *idx += 2;
    v
}

pub fn read_slot(buf: &[u8], idx: &mut usize) -> i16 {
    read_slot_full(buf, idx).0
}

pub fn read_slot_full(buf: &[u8], idx: &mut usize) -> (i16, u8) {
    let item_id = i16::from_be_bytes([buf[*idx], buf[*idx + 1]]);
    *idx += 2;
    let mut count = 1u8;
    if item_id >= 0 {
        count = buf[*idx];
        *idx += 1;
        *idx += 2;
        let nbt_len = i16::from_be_bytes([buf[*idx], buf[*idx + 1]]);
        *idx += 2;
        if nbt_len > 0 {
            *idx += nbt_len as usize;
        }
    }
    (item_id, count)
}
