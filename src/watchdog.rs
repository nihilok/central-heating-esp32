//! Hardware Watchdog Timers:
//! 1. Task Watchdog Timer (TWDT, 60s) — monitors registered FreeRTOS tasks (`main`, `heating-ctrl`).
//! 2. Hardware RTC Watchdog Timer (RWDT, 90s) — pure silicon hardware reset if the CPU or
//!    interrupts lock up completely and prevent TWDT from running.

use anyhow::Result;
use log::{info, warn};

/// Task Watchdog timeout in seconds before triggering a panic/reset.
pub const WATCHDOG_TIMEOUT_SECS: u32 = 60;

/// Hardware RTC Watchdog timeout in milliseconds before triggering a silicon system reset.
pub const RTC_WATCHDOG_TIMEOUT_MS: u32 = 90_000;

extern "C" {
    fn rtc_wdt_protect_off();
    fn rtc_wdt_protect_on();
    fn rtc_wdt_disable();
    fn rtc_wdt_enable();
    fn rtc_wdt_feed();
    fn rtc_wdt_set_length_of_reset_signal(reset_src: u32, reset_signal_length: u32) -> i32;
    fn rtc_wdt_set_stage(stage: u32, stage_sel: u32) -> i32;
    fn rtc_wdt_set_time(stage: u32, timeout_ms: u32) -> i32;
}

/// Initializes or reconfigures the ESP-IDF Task Watchdog Timer (TWDT) and
/// enables the hardware RTC Watchdog Timer (RWDT).
pub fn init_watchdog() -> Result<()> {
    let config = esp_idf_svc::sys::esp_task_wdt_config_t {
        timeout_ms: WATCHDOG_TIMEOUT_SECS * 1000,
        idle_core_mask: (1 << 0) | (1 << 1),
        trigger_panic: true,
    };

    unsafe {
        let err = esp_idf_svc::sys::esp_task_wdt_reconfigure(&config);
        if err != esp_idf_svc::sys::ESP_OK {
            let init_err = esp_idf_svc::sys::esp_task_wdt_init(&config);
            if init_err != esp_idf_svc::sys::ESP_OK {
                warn!("Failed to init TWDT (err={init_err}), trying fallback without idle cores");
                let fallback = esp_idf_svc::sys::esp_task_wdt_config_t {
                    timeout_ms: WATCHDOG_TIMEOUT_SECS * 1000,
                    idle_core_mask: 0,
                    trigger_panic: true,
                };
                let _ = esp_idf_svc::sys::esp_task_wdt_init(&fallback);
            }
        }
    }

    init_rtc_watchdog();

    info!(
        "Watchdogs configured: TWDT={}s, hardware RWDT={}s",
        WATCHDOG_TIMEOUT_SECS,
        RTC_WATCHDOG_TIMEOUT_MS / 1000
    );
    Ok(())
}

/// Configures the hardware RTC Watchdog (`RWDT`) to perform a full system reset
/// if not fed within `RTC_WATCHDOG_TIMEOUT_MS`.
fn init_rtc_watchdog() {
    const RTC_WDT_SYS_RESET_SIG: u32 = 0;
    const RTC_WDT_LENGTH_3_2US: u32 = 7;
    const RTC_WDT_STAGE0: u32 = 0;

    unsafe {
        rtc_wdt_protect_off();
        rtc_wdt_disable();
        rtc_wdt_set_length_of_reset_signal(RTC_WDT_SYS_RESET_SIG, RTC_WDT_LENGTH_3_2US);
        rtc_wdt_set_stage(
            RTC_WDT_STAGE0,
            esp_idf_svc::sys::RTC_WDT_STG_SEL_RESET_SYSTEM,
        );
        rtc_wdt_set_time(RTC_WDT_STAGE0, RTC_WATCHDOG_TIMEOUT_MS);
        rtc_wdt_enable();
        rtc_wdt_feed();
        rtc_wdt_protect_on();
    }
}

/// Feeds the hardware RTC Watchdog (`RWDT`). Should be called from the main supervisory loop.
pub fn feed_rtc_watchdog() {
    unsafe {
        rtc_wdt_feed();
    }
}

/// Subscribes the current FreeRTOS task to the Task Watchdog.
pub fn register_current_task() {
    unsafe {
        let err = esp_idf_svc::sys::esp_task_wdt_add(core::ptr::null_mut());
        if err != esp_idf_svc::sys::ESP_OK && err != esp_idf_svc::sys::ESP_ERR_INVALID_STATE {
            warn!("Failed to add current task to TWDT: {err}");
        }
    }
}

/// Feeds / resets the watchdog timer for the current task.
pub fn feed_watchdog() {
    unsafe {
        let _ = esp_idf_svc::sys::esp_task_wdt_reset();
    }
}
