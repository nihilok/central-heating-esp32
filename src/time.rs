//! SNTP synchronization and wall-clock time tracking.

use std::ffi::CString;
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use esp_idf_svc::sntp::{EspSntp, SntpConf, SyncStatus};
use log::{info, warn};
use serde::{Deserialize, Serialize};

pub const CONFIG_TZ: &str = env!("SNTP_TZ");
pub const CONFIG_SERVER: &str = env!("SNTP_SERVER");

/// Explicit, strongly-typed weekday enum (ISO 8601 aligned).
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Weekday {
    Monday = 1,
    Tuesday = 2,
    Wednesday = 3,
    Thursday = 4,
    Friday = 5,
    Saturday = 6,
    Sunday = 7,
}

impl Weekday {
    /// Convert from C `struct tm::tm_wday` (0 = Sunday, 1 = Monday, ..., 6 = Saturday).
    pub fn from_tm_wday(wday: i32) -> Self {
        match wday {
            1 => Weekday::Monday,
            2 => Weekday::Tuesday,
            3 => Weekday::Wednesday,
            4 => Weekday::Thursday,
            5 => Weekday::Friday,
            6 => Weekday::Saturday,
            0 => Weekday::Sunday,
            _ => Weekday::Sunday, // safe fallback
        }
    }

    /// Return the following day of the week.
    pub fn next(&self) -> Self {
        match self {
            Weekday::Monday => Weekday::Tuesday,
            Weekday::Tuesday => Weekday::Wednesday,
            Weekday::Wednesday => Weekday::Thursday,
            Weekday::Thursday => Weekday::Friday,
            Weekday::Friday => Weekday::Saturday,
            Weekday::Saturday => Weekday::Sunday,
            Weekday::Sunday => Weekday::Monday,
        }
    }

    /// Full English name.
    pub fn name(&self) -> &'static str {
        match self {
            Weekday::Monday => "Monday",
            Weekday::Tuesday => "Tuesday",
            Weekday::Wednesday => "Wednesday",
            Weekday::Thursday => "Thursday",
            Weekday::Friday => "Friday",
            Weekday::Saturday => "Saturday",
            Weekday::Sunday => "Sunday",
        }
    }
}

impl std::fmt::Display for Weekday {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name())
    }
}

/// Formatted broken-down wall-clock time.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WallTime {
    pub year: i32,
    pub month: u8,
    pub day: u8,
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
    pub weekday: Weekday,
}

impl WallTime {
    /// Decimal hours since midnight (e.g. 06:30 -> 6.5, 22:15 -> 22.25).
    pub fn decimal_hours(&self) -> f64 {
        self.hour as f64 + (self.minute as f64 / 60.0)
    }
}

impl std::fmt::Display for WallTime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{:04}-{:02}-{:02} {:02}:{:02}:{:02} ({})",
            self.year, self.month, self.day, self.hour, self.minute, self.second, self.weekday
        )
    }
}

pub struct TimeService {
    sntp: EspSntp<'static>,
}

impl TimeService {
    /// Initialise SNTP and configure system timezone.
    pub fn init() -> Result<Self> {
        info!("Setting timezone to '{}'...", CONFIG_TZ);
        set_timezone(CONFIG_TZ).context("Failed to set system timezone")?;

        info!("Starting SNTP service (server: '{}')...", CONFIG_SERVER);
        let mut conf = SntpConf::default();
        if !CONFIG_SERVER.is_empty() {
            conf.servers[0] = CONFIG_SERVER;
        }

        let sntp = EspSntp::new(&conf).context("Failed to start EspSntp")?;
        Ok(Self { sntp })
    }

    /// Block up to `timeout` waiting for initial SNTP time synchronization.
    pub fn wait_for_sync(&self, timeout: Duration) -> bool {
        let start = Instant::now();
        info!("Waiting for SNTP time synchronization (up to {}s)...", timeout.as_secs());

        while start.elapsed() < timeout {
            if self.sntp.get_sync_status() == SyncStatus::Completed {
                if let Some(t) = now() {
                    info!("SNTP synchronized! Current wall-clock time: {t}");
                    return true;
                }
            }
            thread::sleep(Duration::from_millis(500));
        }

        warn!("SNTP time sync timed out. Scheduling logic must fail safe.");
        false
    }

    /// Returns `true` if SNTP reported synchronization completed and time is sane (> 2024).
    pub fn is_synced(&self) -> bool {
        self.sntp.get_sync_status() == SyncStatus::Completed && is_time_sane()
    }
}

/// Set the POSIX timezone environment variable and reload libc timezone data.
pub fn set_timezone(tz: &str) -> Result<()> {
    let tz_key = CString::new("TZ")?;
    let tz_val = CString::new(tz)?;
    unsafe {
        esp_idf_svc::sys::setenv(tz_key.as_ptr(), tz_val.as_ptr(), 1);
        esp_idf_svc::sys::tzset();
    }
    Ok(())
}

/// Returns `true` if system clock epoch is greater than 2024-01-01 (time has been synced).
pub fn is_time_sane() -> bool {
    let mut now_epoch: esp_idf_svc::sys::time_t = 0;
    unsafe { esp_idf_svc::sys::time(&mut now_epoch) };
    now_epoch > 1_704_067_200 // 2024-01-01T00:00:00Z
}

/// Returns current Unix timestamp in seconds if clock is synchronized.
pub fn now_epoch() -> Option<f64> {
    let mut epoch: esp_idf_svc::sys::time_t = 0;
    unsafe { esp_idf_svc::sys::time(&mut epoch) };
    if epoch > 1_704_067_200 {
        Some(epoch as f64)
    } else {
        None
    }
}

/// Retrieve current wall-clock time if clock is synchronized.
pub fn now() -> Option<WallTime> {
    if !is_time_sane() {
        return None;
    }

    let mut now_epoch: esp_idf_svc::sys::time_t = 0;
    unsafe { esp_idf_svc::sys::time(&mut now_epoch) };

    let mut tm = esp_idf_svc::sys::tm {
        tm_sec: 0,
        tm_min: 0,
        tm_hour: 0,
        tm_mday: 0,
        tm_mon: 0,
        tm_year: 0,
        tm_wday: 0,
        tm_yday: 0,
        tm_isdst: 0,
    };

    unsafe { esp_idf_svc::sys::localtime_r(&now_epoch, &mut tm) };

    Some(WallTime {
        year: tm.tm_year + 1900,
        month: (tm.tm_mon + 1) as u8,
        day: tm.tm_mday as u8,
        hour: tm.tm_hour as u8,
        minute: tm.tm_min as u8,
        second: tm.tm_sec as u8,
        weekday: Weekday::from_tm_wday(tm.tm_wday),
    })
}
