# Central Heating ESP32 (Rust)

Embedded Rust (`std` on ESP-IDF) firmware for the central heating controller on an ESP32 (ESP-WROOM-32), consolidating the relay switching node and the API server into a single device.

## Prerequisites

1. **Rust**: Make sure you have standard Rust installed.
2. **espup** (Espressif toolchain installer for Xtensa):
   ```sh
   cargo install espup
   espup install
   ```
   After installation, activate the exported environment variables:
   ```sh
   source ~/export-esp.sh
   ```
3. **espflash** and **ldproxy**:
   ```sh
   cargo install espflash ldproxy
   ```
   *(Or on macOS: `brew install espflash`)*

## Pinout Mapping

| Function | ESP32 GPIO | Board Label | Notes |
| :--- | :--- | :--- | :--- |
| **Downstairs Relay** | GPIO 27 | D27 | Output, active-low, default HIGH (OFF) |
| **Upstairs Relay** | GPIO 26 | D26 | Output, active-low, default HIGH (OFF) |
| **Relay GND** | GND | GND | Common ground |
| **Relay VCC** | VIN (5V) | VIN | Power supply |

> **Avoid** GPIO 12 (strapping pin – can prevent boot) and GPIO 14 (PWM at boot) for relays. See `AGENTS.md` for the full list of pins to avoid.

## Building & Flashing

Connect the ESP32 board via USB, then:

```sh
# Once per shell: load the Xtensa toolchain environment
source ~/export-esp.sh

# Build (the first build downloads ESP-IDF into .embuild/ and takes a while)
cargo build

# Flash and open serial monitor
cargo run
# or
espflash flash --monitor
```

## Architecture Roadmap

- [x] Base ESP-IDF project configuration & GPIO pin setup
- [x] Wi-Fi connection manager (`EspWifi` / `BlockingWifi`) with DHCP & reconnection handling
- [x] SNTP synchronization (`EspSntp`) with timezone handling for wall-clock timing
- [x] Background heating control loop thread (polling remote sensors, NVS schedule persistence & evaluating target periods)
- [x] Embedded HTTP server (`EspHttpServer`) exposing REST API & serving UI assets
