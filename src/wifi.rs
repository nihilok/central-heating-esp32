//! Wi-Fi manager for establishing and monitoring network connectivity.

use anyhow::{Context, Result};
use esp_idf_svc::eventloop::EspSystemEventLoop;
use esp_idf_svc::hal::modem::Modem;
use esp_idf_svc::ipv4::IpInfo;
use esp_idf_svc::nvs::EspDefaultNvsPartition;
use esp_idf_svc::wifi::{
    AuthMethod, BlockingWifi, ClientConfiguration, Configuration, EspWifi,
};
use log::{info, warn};

pub const CONFIG_SSID: &str = env!("WIFI_SSID");
pub const CONFIG_PASS: &str = env!("WIFI_PASS");

pub struct WifiManager {
    wifi: BlockingWifi<EspWifi<'static>>,
}

impl WifiManager {
    /// Initialise the Wi-Fi driver and attach it to the system event loop and NVS partition.
    pub fn new(
        modem: Modem<'static>,
        sys_loop: EspSystemEventLoop,
        nvs: Option<EspDefaultNvsPartition>,
    ) -> Result<Self> {
        let esp_wifi = EspWifi::new(modem, sys_loop.clone(), nvs)
            .context("Failed to initialize EspWifi driver")?;
        let wifi = BlockingWifi::wrap(esp_wifi, sys_loop)
            .context("Failed to wrap BlockingWifi")?;

        Ok(Self { wifi })
    }

    /// Connect to the configured Wi-Fi network.
    ///
    /// If no SSID is configured in `cfg.toml`, logs a warning and returns `Ok(false)`
    /// without blocking boot.
    pub fn connect(&mut self) -> Result<bool> {
        if CONFIG_SSID.is_empty() {
            warn!("Wi-Fi SSID is not configured. Populate `cfg.toml` (see cfg.toml.example).");
            return Ok(false);
        }

        info!("Configuring Wi-Fi for SSID: '{}'", CONFIG_SSID);

        let auth_method = if CONFIG_PASS.is_empty() {
            AuthMethod::None
        } else {
            AuthMethod::WPA2Personal
        };

        let ssid = CONFIG_SSID
            .try_into()
            .map_err(|_| anyhow::anyhow!("SSID exceeds max length (32 bytes)"))?;
        let password = CONFIG_PASS
            .try_into()
            .map_err(|_| anyhow::anyhow!("Password exceeds max length (64 bytes)"))?;

        let wifi_configuration = Configuration::Client(ClientConfiguration {
            ssid,
            password,
            auth_method,
            ..Default::default()
        });

        self.wifi
            .set_configuration(&wifi_configuration)
            .context("Failed to set Wi-Fi configuration")?;

        info!("Starting Wi-Fi subsystem...");
        self.wifi.start().context("Failed to start Wi-Fi")?;

        // Disable Wi-Fi modem sleep (WIFI_PS_NONE) so the ESP32 never misses broadcast ARP
        // requests or incoming HTTP SYN packets on the LAN.
        unsafe {
            esp_idf_svc::sys::esp_wifi_set_ps(esp_idf_svc::sys::wifi_ps_type_t_WIFI_PS_NONE);
        }

        info!("Connecting to '{}'...", CONFIG_SSID);
        if let Err(e) = self.wifi.connect() {
            warn!("Initial Wi-Fi connect attempt failed ({e:?}); will retry in background loop");
            return Ok(false);
        }

        info!("Waiting for DHCP lease...");
        if let Err(e) = self.wifi.wait_netif_up() {
            warn!("Initial DHCP wait failed ({e:?}); will retry in background loop");
            return Ok(false);
        }

        let ip_info = self.ip_info()?;
        if let Some(info) = ip_info {
            info!(
                "Wi-Fi connected! Assigned IP: {}, Subnet: {}, Gateway: {}",
                info.ip, info.subnet.mask, info.subnet.gateway
            );
        }

        Ok(true)
    }

    /// Returns `true` if station interface is connected.
    pub fn is_connected(&self) -> bool {
        self.wifi.is_connected().unwrap_or(false)
    }

    /// Retrieve the current IPv4 lease information.
    pub fn ip_info(&self) -> Result<Option<IpInfo>> {
        if !self.is_connected() {
            return Ok(None);
        }
        let info = self
            .wifi
            .wifi()
            .sta_netif()
            .get_ip_info()
            .context("Failed to query sta_netif IP info")?;
        Ok(Some(info))
    }

    /// Attempt reconnection if connection dropped.
    pub fn ensure_connected(&mut self) -> Result<()> {
        if !self.is_connected() && !CONFIG_SSID.is_empty() {
            warn!("Wi-Fi connection lost, re-connecting...");
            let _ = self.wifi.start();
            unsafe {
                esp_idf_svc::sys::esp_wifi_set_ps(esp_idf_svc::sys::wifi_ps_type_t_WIFI_PS_NONE);
            }
            if let Err(e) = self.wifi.connect() {
                warn!("Reconnection attempt failed: {e:?}");
            } else if let Err(e) = self.wifi.wait_netif_up() {
                warn!("Waiting for IP lease failed: {e:?}");
            } else {
                info!("Wi-Fi reconnected successfully!");
            }
        }
        Ok(())
    }
}
