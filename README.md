# Central Heating ESP32 (Rust)

Bare-metal / embedded Rust firmware for the central heating controller on an ESP32 (ESP-WROOM-32), consolidating the relay switching node and the API server into a single device.

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
   cargo install cargo-espflash espflash ldproxy
   ```
   *(Or on macOS: `brew install espflash`)*

## Pinout Mapping

| Function | ESP32 GPIO | Board Label | Notes |
| :--- | :--- | :--- | :--- |
| **Downstairs Relay** | GPIO 14 | D14 | Output, default HIGH (OFF) |
| **Upstairs Relay** | GPIO 12 | D12 | Output, default HIGH (OFF) |
| **Relay GND** | GND | GND | Common ground |
| **Relay VCC** | VIN (5V) | VIN | Power supply |

## Building & Flashing

Connect the ESP32 board via USB, then:

```sh
# Build
cargo build

# Flash and open serial monitor
cargo run
# or
espflash flash --monitor
```

## Architecture Roadmap

- [x] Base ESP-IDF project configuration & GPIO pin setup
- [ ] Wi-Fi connection manager (`EspWifi`) with reconnection handling
- [ ] SNTP synchronization (`EspSntp`) for schedule wall-clock timing
- [ ] Background heating control loop thread (polling remote sensors & evaluating target periods)
- [ ] Embedded HTTP server (`EspHttpServer`) exposing REST API & serving UI assets
