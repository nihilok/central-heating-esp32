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
| **Downstairs Relay** | GPIO 26 | D26 | Output, active-low, default HIGH (OFF) |
| **Upstairs Relay** | GPIO 27 | D27 | Output, active-low, default HIGH (OFF) |
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

# Flash and open serial monitor (initial USB flash)
cargo run
# or
espflash flash --monitor

# Subsequent updates over Wi-Fi (OTA):
./scripts/ota.sh 192.168.1.168
```

## Web UI (`ui/`)

The embedded web interface in [`ui/`](./ui) is built with **Svelte 5** (Runes), **TypeScript**, **Vite**, and **Tailwind CSS v4**. It compiles and compresses into ~37 KB total (`web/assets/index.js.gz` + `web/assets/index.css.gz`) and is embedded directly into the ESP32 firmware binary via `include_bytes!`.

```sh
# Local development with hot reload (proxies /api, /token, /check_token to 192.168.1.168):
cd ui && npm run dev

# Build and gzip into web/ (also run automatically by ./scripts/ota.sh):
./scripts/build-ui.sh
```

## Architecture Roadmap

- [x] Base ESP-IDF project configuration & GPIO pin setup
- [x] Wi-Fi connection manager (`EspWifi` / `BlockingWifi`) with DHCP & reconnection handling
- [x] SNTP synchronization (`EspSntp`) with timezone handling for wall-clock timing
- [x] Background heating control loop thread (polling remote sensors, NVS schedule persistence & evaluating target periods)
- [x] Embedded HTTP server (`EspHttpServer`) exposing REST API & serving UI assets
- [x] Over-the-Air (OTA) Wi-Fi firmware updates (`POST /api/v3/ota/` & `scripts/ota.sh`)
