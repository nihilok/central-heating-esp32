mod control;
mod model;
mod relay;
mod sensor;
mod server;
mod time;
mod wifi;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::Result;
use esp_idf_svc::eventloop::EspSystemEventLoop;
use esp_idf_svc::hal::peripherals::Peripherals;
use esp_idf_svc::log::EspLogger;
use esp_idf_svc::nvs::EspDefaultNvsPartition;
use esp_idf_svc::ota::EspOta;
use esp_idf_svc::sys::link_patches;
use log::{info, warn};

use control::{spawn_control_loop, HeatingController};
use relay::Relay;
use server::start_http_server;
use time::TimeService;
use wifi::WifiManager;

fn main() -> Result<()> {
    // Necessary patches for ESP-IDF
    link_patches();

    // Initialize logger
    EspLogger::initialize_default();
    info!("Starting Central Heating ESP32 Controller...");

    // Mark the currently running OTA slot as valid after successful boot
    if let Ok(mut ota) = EspOta::new() {
        if let Ok(slot) = ota.get_running_slot() {
            info!("Running from OTA partition: {}", slot.label);
        }
        let _ = ota.mark_running_slot_valid();
    }

    // Take board peripherals
    let peripherals = Peripherals::take()?;
    let sys_loop = EspSystemEventLoop::take()?;
    let nvs = EspDefaultNvsPartition::take()?;

    // 1. Initialize relay outputs (active-low; start OFF with no ON pulse)
    // Downstairs: GPIO 26 (D26 on ESP32 board)
    // Upstairs:   GPIO 27 (D27 on ESP32 board)
    // Avoid GPIO 12 (strapping pin) and GPIO 14 (PWM at boot) - see AGENTS.md.
    let downstairs_relay = Relay::new("downstairs", peripherals.pins.gpio26)?;
    let upstairs_relay = Relay::new("upstairs", peripherals.pins.gpio27)?;

    // 2. Initialize Wi-Fi
    info!("Initializing Wi-Fi subsystem...");
    let mut wifi = WifiManager::new(peripherals.modem, sys_loop, Some(nvs.clone()))?;
    let wifi_connected = wifi.connect()?;

    // 3. Initialize SNTP for wall-clock time sync
    let mut time_service = None;
    if wifi_connected {
        match TimeService::init() {
            Ok(svc) => {
                svc.wait_for_sync(Duration::from_secs(15));
                time_service = Some(svc);
            }
            Err(e) => {
                warn!("Failed to initialize SNTP: {e:?}");
            }
        }
    } else {
        warn!("Wi-Fi not connected at boot. SNTP sync deferred.");
    }

    // 4. Initialize shared HeatingController state (loads schedules from NVS)
    //    and spawn the background heating control loop thread.
    let controller = Arc::new(Mutex::new(HeatingController::new(
        nvs,
        downstairs_relay,
        upstairs_relay,
    )?));
    let _control_thread = spawn_control_loop(controller.clone())?;

    // 5. Start HTTP Server (EspHttpServer) to serve REST API and UI
    let _http_server = start_http_server(controller.clone())?;

    info!("System initialized. Entering supervisory monitoring loop.");

    // Supervisory monitoring loop
    loop {
        std::thread::sleep(Duration::from_secs(10));

        // Fail-safe: if Wi-Fi is down, ensure relays are switched OFF before attempting reconnect
        if !wifi.is_connected() {
            warn!("Wi-Fi disconnected — holding heating relays OFF");
            if let Ok(mut guard) = controller.lock() {
                guard.switch_all_off();
            }
        }

        // Ensure Wi-Fi connection is maintained
        if let Err(e) = wifi.ensure_connected() {
            warn!("Wi-Fi check error: {e:?}");
        }

        // Initialize SNTP if it wasn't initialized at boot due to lack of network
        if time_service.is_none() && wifi.is_connected() {
            if let Ok(svc) = TimeService::init() {
                svc.wait_for_sync(Duration::from_secs(15));
                time_service = Some(svc);
            }
        }

        let time_synced = time_service
            .as_ref()
            .map(|svc| svc.is_synced())
            .unwrap_or_else(time::is_time_sane);

        if let Some(t) = time::now() {
            info!("Heartbeat: system healthy (time_synced={time_synced}), time is {t}");
        } else {
            warn!("Heartbeat: time unsynchronized (failsafe: heating relays held OFF)");
        }
    }
}
