//! Hardware Task Watchdog Timer (TWDT) for failure detection and automatic reset.

use anyhow::Result;
use log::{info, warn};

/// Watchdog timeout in seconds before triggering a hardware reset.
pub const WATCHDOG_TIMEOUT_SECS: u32 = 60;

/// Initializes or reconfigures the ESP-IDF Task Watchdog Timer.
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

    info!("Task Watchdog Timer (TWDT) configured (timeout: {WATCHDOG_TIMEOUT_SECS}s)");
    Ok(())
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
