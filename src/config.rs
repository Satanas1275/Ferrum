use std::env;
use std::fs;

#[derive(Clone)]
pub struct ServerConfig {
    pub port: u16,
    pub max_players: u16,
    pub motd: String,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self { port: 25565, max_players: 20, motd: "A Rust Minecraft Server".into() }
    }
}

impl ServerConfig {
    pub fn to_json(&self) -> String {
        format!(r#"{{"port":{},"max_players":{},"motd":"{}"}}"#, self.port, self.max_players, self.motd)
    }

    pub fn from_json(s: &str) -> Self {
        let mut c = Self::default();
        let s = s.as_bytes();
        fn extract_int(data: &[u8], key: &[u8]) -> Option<i64> {
            let pos = data.windows(key.len()).position(|w| w == key)?;
            let rest = &data[pos + key.len()..];
            let start = rest.iter().position(|&b| b == b':')? + 1;
            let end = rest[start..].iter().position(|&b| !b.is_ascii_digit()).unwrap_or(rest.len() - start);
            let num: String = rest[start..start + end].iter().map(|&b| b as char).collect();
            num.parse().ok()
        }
        fn extract_string(data: &[u8], key: &[u8]) -> Option<String> {
            let pos = data.windows(key.len()).position(|w| w == key)?;
            let rest = &data[pos + key.len()..];
            let start = rest.iter().position(|&b| b == b'"')? + 1;
            let end = start + rest[start..].iter().position(|&b| b == b'"')?;
            Some(rest[start..end].iter().map(|&b| b as char).collect())
        }
        if let Some(p) = extract_int(s, b"\"port\"") { c.port = p as u16; }
        if let Some(m) = extract_int(s, b"\"max_players\"") { c.max_players = m as u16; }
        if let Some(m) = extract_string(s, b"\"motd\"") { c.motd = m; }
        c
    }

    pub fn load() -> Self {
        let args: Vec<String> = env::args().collect();
        if args.iter().any(|a| a == "--default-config") {
            println!("Using default server config (--default-config flag)");
            return Self::default();
        }
        let path = "server.json";
        if let Ok(data) = fs::read_to_string(path) {
            let cfg = Self::from_json(&data);
            println!("Loaded server config from {}", path);
            return cfg;
        }
        let cfg = Self::default();
        let json = cfg.to_json();
        if fs::write(path, &json).is_ok() {
            println!("Created server config file: {}", path);
        }
        cfg
    }
}
