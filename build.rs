fn main() {
    embuild::espidf::sysenv::output();

    println!("cargo:rerun-if-changed=cfg.toml");

    let mut ssid = std::env::var("WIFI_SSID").unwrap_or_default();
    let mut psk = std::env::var("WIFI_PASS").unwrap_or_default();
    let mut tz = std::env::var("SNTP_TZ").unwrap_or_else(|_| "GMT0BST,M3.5.0/1,M10.5.0/2".into());
    let mut ntp_server = std::env::var("SNTP_SERVER").unwrap_or_else(|_| "pool.ntp.org".into());
    let mut api_user = std::env::var("API_USER").unwrap_or_default();
    let mut api_pass = std::env::var("API_PASS").unwrap_or_default();
    let mut sensor_token = std::env::var("SENSOR_TOKEN").unwrap_or_default();

    if let Ok(content) = std::fs::read_to_string("cfg.toml") {
        let mut section = String::new();
        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if line.starts_with('[') && line.ends_with(']') {
                section = line[1..line.len() - 1].trim().to_lowercase();
                continue;
            }
            if let Some((k, v)) = line.split_once('=') {
                let k = k.trim();
                let v = v.trim().trim_matches('"').trim_matches('\'');
                match (section.as_str(), k) {
                    ("wifi", "ssid") | ("", "ssid") => ssid = v.to_string(),
                    ("wifi", "psk") | ("wifi", "password") | ("", "psk") => psk = v.to_string(),
                    ("sntp", "timezone") | ("", "timezone") => tz = v.to_string(),
                    ("sntp", "server") | ("sntp", "ntp_server") | ("", "ntp_server") => {
                        ntp_server = v.to_string()
                    }
                    ("auth", "username") | ("", "api_user") => api_user = v.to_string(),
                    ("auth", "password") | ("", "api_pass") => api_pass = v.to_string(),
                    ("auth", "sensor_token") | ("", "sensor_token") => {
                        sensor_token = v.to_string()
                    }
                    _ => {}
                }
            }
        }
    }

    if api_pass.is_empty() {
        api_pass = psk.clone();
    }

    println!("cargo:rustc-env=WIFI_SSID={ssid}");
    println!("cargo:rustc-env=WIFI_PASS={psk}");
    println!("cargo:rustc-env=SNTP_TZ={tz}");
    println!("cargo:rustc-env=SNTP_SERVER={ntp_server}");
    println!("cargo:rustc-env=API_USER={api_user}");
    println!("cargo:rustc-env=API_PASS={api_pass}");
    println!("cargo:rustc-env=SENSOR_TOKEN={sensor_token}");
}
