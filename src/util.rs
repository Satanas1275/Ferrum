use md5::{Digest, Md5};

pub fn parse_rel_coord(s: &str, current: f64) -> Option<f64> {
    if s == "~" || s == "~0" {
        Some(current)
    } else if let Some(rest) = s.strip_prefix('~') {
        let offset: f64 = rest.parse().ok()?;
        Some(current + offset)
    } else {
        s.parse::<f64>().ok()
    }
}

pub fn face_offset(x: i32, y: u8, z: i32, face: u8) -> (i32, i32, i32) {
    match face {
        0 => (x, y as i32 - 1, z),
        1 => (x, y as i32 + 1, z),
        2 => (x, y as i32, z - 1),
        3 => (x, y as i32, z + 1),
        4 => (x - 1, y as i32, z),
        5 => (x + 1, y as i32, z),
        _ => (x, y as i32, z),
    }
}

pub fn offline_uuid(username: &str) -> String {
    let mut hasher = Md5::new();
    hasher.update(b"OfflinePlayer:");
    hasher.update(username.as_bytes());
    let digest = hasher.finalize();
    let mut bytes: [u8; 16] = *digest.as_ref();
    bytes[6] = (bytes[6] & 0x0F) | 0x30;
    bytes[8] = (bytes[8] & 0x3F) | 0x80;
    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        bytes[0], bytes[1], bytes[2], bytes[3],
        bytes[4], bytes[5],
        bytes[6], bytes[7],
        bytes[8], bytes[9],
        bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15],
    )
}
