use anyhow::Result;
use esp_idf_hal::gpio::{PinDriver, Pull};
use esp_idf_hal::peripherals::Peripherals;
use esp_idf_svc::log::EspLogger;
use esp_idf_svc::sys::link_patches;
use log::info;

fn main() -> Result<()> {
    // Necessary patches for ESP-IDF
    link_patches();

    // Initialize logger
    EspLogger::initialize_default();
    info!("Starting Central Heating ESP32 Controller...");

    // Take board peripherals
    let peripherals = Peripherals::take()?;

    // Initialize relay output pins
    // Downstairs: GPIO 14 (D14 on ESP32 board)
    // Upstairs:   GPIO 12 (D12 on ESP32 board)
    let mut downstairs_relay = PinDriver::output(peripherals.pins.gpio14)?;
    let mut upstairs_relay = PinDriver::output(peripherals.pins.gpio12)?;

    // Set initial state (relays default OFF, typically HIGH if active-low)
    downstairs_relay.set_high()?;
    upstairs_relay.set_high()?;
    info!("Relay GPIOs initialized (Downstairs: GPIO14, Upstairs: GPIO12)");

    // TODO:
    // 1. Initialize Wi-Fi (EspWifi)
    // 2. Initialize SNTP for time synchronization (EspSntp)
    // 3. Start background control loop thread (poll sensors, evaluate schedule)
    // 4. Start HTTP Server (EspHttpServer) to serve API and UI

    info!("System initialized. Entering main idle loop.");
    loop {
        std::thread::sleep(std::time::Duration::from_secs(60));
    }
}
