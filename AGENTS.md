# AGENTS.md

Guidance for AI coding agents working in this repository.

## Project

Rust firmware for a central heating controller on an **ESP32 (ESP-WROOM-32)**.
It uses **`std` on ESP-IDF** (via `esp-idf-svc` / `esp-idf-hal`), not bare-metal / `no_std`.
A single device both switches the heating relays and serves the HTTP API/UI.

## Build & toolchain

- Toolchain: the Espressif `esp` Rust toolchain (pinned in `rust-toolchain.toml`), installed via `espup`.
- The build needs the env from `~/export-esp.sh` (sets `LIBCLANG_PATH` and the Xtensa GCC path). On the dev machine it is sourced from `~/.zshrc` (via `~/.zsh.local`), so interactive shells already have it. In non-interactive shells/CI, or if `LIBCLANG_PATH` is unset, source it explicitly:
  ```sh
  source ~/export-esp.sh && cargo build
  ```
- Target is `xtensa-esp32-espidf` (set in `.cargo/config.toml`); ESP-IDF version is set via `ESP_IDF_VERSION` there.
- The first build downloads ESP-IDF and its tools into `.embuild/` (several GB, gitignored). It is slow; later builds are incremental.
- After changing `sdkconfig.defaults` or `ESP_IDF_VERSION`, a clean build may be needed (`cargo clean`).
- Verify changes with `cargo build` (and `cargo clippy` where practical). There is no host-side test harness yet.
- **Do not flash or monitor the device (`cargo run` / `espflash`) unless the user explicitly asks.** It is connected to real heating hardware.

## Hardware rules (safety-critical)

| Function | GPIO | Notes |
| :--- | :--- | :--- |
| Downstairs relay | 26 | Active-low: LOW = ON, HIGH = OFF |
| Upstairs relay | 27 | Active-low: LOW = ON, HIGH = OFF |

- **Fail safe = heating OFF.** On boot, panic, lost Wi-Fi, unsynced time, or unreachable sensors, the relays must end up OFF unless the design explicitly says otherwise.
- **No ON pulse at boot.** Drive the pin HIGH (OFF) *before* setting it as an output. `PinDriver::output()` on its own briefly drives the pin LOW.
- **Avoid these pins for relays/outputs:**
  - Strapping pins: 0, 2, 5, 12, 15. GPIO12 in particular can stop the WROOM-32 from booting.
  - Pins that output PWM or go HIGH at boot: 1, 3, 5, 14, 15.
  - Flash pins: 6–11.
  - Input-only pins: 34–39.
- Safe general-purpose outputs: 16–19, 21–23, 25–27, 32, 33.
- ESP32 GPIOs are 3.3 V and **not** 5 V tolerant.
- Keep raw `set_high()` / `set_low()` out of business logic. Go through a relay abstraction that encodes the active-low polarity.

## Conventions

- Depend on `esp-idf-svc` and prefer its re-exports (`esp_idf_svc::hal`, `esp_idf_svc::sys`) to avoid version drift between the Espressif crates.
- Errors: `anyhow::Result` at the application level; avoid `unwrap()`/`expect()` outside of startup code.
- Logging via the `log` crate (`info!`, `warn!`, ...), initialised with `EspLogger`.
- **Never commit secrets** (Wi-Fi credentials, API tokens). Use a gitignored config file or NVS.
- Keep `README.md` (pinout, roadmap) in sync with code changes.
