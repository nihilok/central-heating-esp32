//! Domain models and schedule/thermostat evaluation logic for heating zones.

use std::collections::VecDeque;

use serde::{Deserialize, Serialize};

use crate::time::{WallTime, Weekday};

/// Hysteresis threshold in °C when switching heating ON from OFF state.
pub const THERMOSTAT_THRESHOLD: f64 = 0.2;

/// Maximum target temperature (°C) during a manual boost window.
pub const BOOST_THRESHOLD: f64 = 26.0;

/// Sentinel target value returned by `current_target` when boost is active so the UI displays "BOOST".
pub const BOOST_DISPLAY_TARGET: f64 = 999.0;

/// Baseline / frost-protection target (°C) when outside scheduled periods or program is off.
pub const DEFAULT_MINIMUM_TARGET: f64 = 5.0;

/// Fallback target (°C) for `advance` when no scheduled periods exist.
pub const DEFAULT_ROOM_TEMP: f64 = 22.0;

/// Control loop polling interval in seconds.
pub const CHECK_FREQUENCY_SECONDS: u64 = 10;

/// Consecutive sensor error limit before disabling a zone temporarily.
pub const DEFAULT_MAX_ERROR_COUNT: u32 = 5;

/// Cooldown duration (15 minutes) when a zone is disabled after repeated sensor failures.
pub const DISABLED_COOLDOWN_SECS: f64 = 15.0 * 60.0;

/// Default TTL in seconds for cached/pushed temperature readings.
pub const DEFAULT_EXPIRY_SECS: u64 = 20;

/// Maximum number of 5-minute history samples retained per zone in RAM (24 hours = 288 points).
pub const HISTORY_MAX_POINTS: usize = 288;

/// Interval in seconds between recorded history samples (5 minutes).
pub const HISTORY_SAMPLE_INTERVAL_SECS: u64 = 300;

/// A 5-minute historical sample of zone temperature and heating relay activity.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct HistoryPoint {
    /// Unix epoch timestamp in seconds.
    pub ts: u64,
    /// Adjusted room temperature in °C.
    pub temp: f64,
    /// Whether the zone relay was active during this sample window.
    pub heating: bool,
}

fn default_true() -> bool {
    true
}

fn default_max_errors() -> u32 {
    DEFAULT_MAX_ERROR_COUNT
}

fn default_expiry_secs() -> u64 {
    DEFAULT_EXPIRY_SECS
}

/// Days of the week a heating period is active.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Days {
    #[serde(default = "default_true")]
    pub monday: bool,
    #[serde(default = "default_true")]
    pub tuesday: bool,
    #[serde(default = "default_true")]
    pub wednesday: bool,
    #[serde(default = "default_true")]
    pub thursday: bool,
    #[serde(default = "default_true")]
    pub friday: bool,
    #[serde(default = "default_true")]
    pub saturday: bool,
    #[serde(default = "default_true")]
    pub sunday: bool,
}

impl Default for Days {
    fn default() -> Self {
        Self {
            monday: true,
            tuesday: true,
            wednesday: true,
            thursday: true,
            friday: true,
            saturday: true,
            sunday: true,
        }
    }
}

impl Days {
    pub fn is_enabled(&self, weekday: Weekday) -> bool {
        match weekday {
            Weekday::Monday => self.monday,
            Weekday::Tuesday => self.tuesday,
            Weekday::Wednesday => self.wednesday,
            Weekday::Thursday => self.thursday,
            Weekday::Friday => self.friday,
            Weekday::Saturday => self.saturday,
            Weekday::Sunday => self.sunday,
        }
    }
}

/// A scheduled time window with a target temperature (°C).
///
/// `start` and `end` are decimal hours since midnight (e.g. `6.5` = 06:30, `21.0` = 21:00).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Period {
    pub id: String,
    pub start: f64,
    pub end: f64,
    pub target: f64,
    #[serde(default)]
    pub days: Days,
}

/// Remote temperature sensor node configuration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SensorNode {
    pub url: String,
    #[serde(default)]
    pub adjustment: Option<f64>,
}

/// State and schedule configuration for a single heating zone (`downstairs` or `upstairs`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeatingSystem {
    pub system_id: String,
    pub sensor: SensorNode,
    #[serde(default)]
    pub program: bool,
    #[serde(default)]
    pub periods: Vec<Period>,
    #[serde(default)]
    pub advance: Option<f64>,
    #[serde(default)]
    pub boost: Option<f64>,
    #[serde(default)]
    pub disabled: bool,
    #[serde(default)]
    pub error_count: u32,
    #[serde(default)]
    pub disabled_until: Option<f64>,
    #[serde(default = "default_max_errors")]
    pub max_error_count: u32,
    #[serde(default)]
    pub temperature: Option<f64>,
    #[serde(default)]
    pub temperature_expiry: Option<f64>,
    #[serde(default = "default_expiry_secs")]
    pub expiry_seconds: u64,
    #[serde(skip)]
    pub relay_on: bool,
    #[serde(skip)]
    pub history: VecDeque<HistoryPoint>,
    #[serde(skip)]
    pub pending_heating_since_last_sample: bool,
}

impl HeatingSystem {
    /// Records a 5-minute history sample into the in-RAM ring buffer (`HISTORY_MAX_POINTS` = 288).
    /// Also latches `heating = true` if the relay was switched ON at any point during the current 5-minute window.
    pub fn record_history_sample(&mut self, temp: f64, relay_on: bool, now_epoch: f64) {
        if relay_on {
            self.pending_heating_since_last_sample = true;
        }
        let now_ts = now_epoch as u64;
        if now_ts == 0 {
            return;
        }

        let should_push = match self.history.back() {
            None => true,
            Some(last) => now_ts >= last.ts.saturating_add(HISTORY_SAMPLE_INTERVAL_SECS),
        };

        if should_push {
            if self.history.len() >= HISTORY_MAX_POINTS {
                self.history.pop_front();
            }
            self.history.push_back(HistoryPoint {
                ts: now_ts,
                temp,
                heating: relay_on || self.pending_heating_since_last_sample,
            });
            self.pending_heating_since_last_sample = relay_on;
        } else if let Some(last) = self.history.back_mut() {
            // Keep the latest bucket's temperature fresh and latch heating if relay turned on right after a sample.
            if now_ts.saturating_sub(last.ts) < 30 && relay_on {
                last.heating = true;
            }
        }
    }

    /// Returns a copy of `periods` sorted chronologically by `(start, end)`.
    pub fn sorted_periods(&self) -> Vec<&Period> {
        let mut sorted: Vec<&Period> = self.periods.iter().collect();
        sorted.sort_by(|a, b| {
            a.start
                .partial_cmp(&b.start)
                .unwrap_or(core::cmp::Ordering::Equal)
                .then_with(|| {
                    a.end
                        .partial_cmp(&b.end)
                        .unwrap_or(core::cmp::Ordering::Equal)
                })
        });
        sorted
    }

    /// Computes the active target temperature (°C) for the given wall-clock time.
    /// Returns `BOOST_DISPLAY_TARGET` (`999.0`) when boost is active (matching `central-heating-api`).
    pub fn current_target(&self, wall: &WallTime) -> f64 {
        if self.boost.is_some() {
            return BOOST_DISPLAY_TARGET;
        }
        if self.advance.is_some() {
            return self.next_target(wall);
        }
        if !self.program {
            return DEFAULT_MINIMUM_TARGET;
        }

        let check_time = wall.decimal_hours();
        let check_day = wall.weekday;

        for period in &self.periods {
            if period.start <= check_time
                && check_time < period.end
                && period.days.is_enabled(check_day)
            {
                return period.target;
            }
        }

        DEFAULT_MINIMUM_TARGET
    }

    /// Computes the target temperature of the next upcoming scheduled period
    /// (used when `advance` is active).
    pub fn next_target(&self, wall: &WallTime) -> f64 {
        let check_time = wall.decimal_hours();
        let today = wall.weekday;
        let sorted = self.sorted_periods();

        if let Some(p) = sorted
            .iter()
            .find(|p| p.end > check_time && p.days.is_enabled(today))
        {
            return p.target;
        }

        let tomorrow = today.next();
        if let Some(p) = sorted.iter().find(|p| p.days.is_enabled(tomorrow)) {
            return p.target;
        }

        DEFAULT_ROOM_TEMP
    }

    /// Returns `true` if the zone is currently inside an active scheduled period (or override).
    pub fn is_within_period(&self, wall: &WallTime) -> bool {
        self.current_target(wall) > DEFAULT_MINIMUM_TARGET
    }

    /// Updates the cached temperature (e.g. from an HTTP sensor poll or `/receive` push),
    /// applying any configured calibration adjustment and rounding to 0.1°C.
    pub fn update_temperature(&mut self, raw_temp: f64, now_epoch: f64) {
        let adjusted = raw_temp + self.sensor.adjustment.unwrap_or(0.0);
        let rounded = (adjusted * 10.0).round() / 10.0;
        self.temperature = Some(rounded);
        self.temperature_expiry = Some(now_epoch + self.expiry_seconds as f64);
        self.error_count = 0;
    }

    /// Returns the cached temperature if it exists and has not expired.
    pub fn valid_cached_temperature(&self, now_epoch: f64) -> Option<f64> {
        match (self.temperature, self.temperature_expiry) {
            (Some(temp), Some(exp)) if exp > now_epoch => Some(temp),
            _ => None,
        }
    }

    /// Clears expired `advance` and `boost` timers.
    pub fn clear_expired_overrides(&mut self, now_epoch: f64) {
        if let Some(adv) = self.advance {
            if adv <= now_epoch {
                self.advance = None;
            }
        }
        if let Some(bst) = self.boost {
            if bst <= now_epoch {
                self.boost = None;
            }
        }
    }

    /// Evaluates whether the zone relay should be switched ON given the current
    /// room `temperature`, wall-clock time, and Unix timestamp.
    pub fn evaluate_should_heat(
        &mut self,
        temperature: f64,
        wall: &WallTime,
        now_epoch: f64,
    ) -> bool {
        self.clear_expired_overrides(now_epoch);

        if self.boost.is_some() {
            return temperature < BOOST_THRESHOLD;
        }

        if self.advance.is_some() {
            let target = self.next_target(wall);
            return temperature < target;
        }

        let target = self.current_target(wall);
        if self.relay_on {
            // Relay is currently ON: stay ON until temperature reaches or exceeds target.
            temperature < target
        } else {
            // Relay is currently OFF: switch ON only when temperature drops below (target - threshold).
            temperature <= target - THERMOSTAT_THRESHOLD
        }
    }

    /// Converts this system into the `SystemOut` representation expected by the REST API / UI.
    pub fn as_system_out(&mut self, wall: Option<&WallTime>, now_epoch: Option<f64>) -> SystemOut {
        if let Some(epoch) = now_epoch {
            self.clear_expired_overrides(epoch);
        }
        let is_within_period = wall.map(|w| self.is_within_period(w)).unwrap_or(false);
        let current_target = wall
            .map(|w| self.current_target(w))
            .unwrap_or(DEFAULT_MINIMUM_TARGET);
        SystemOut {
            system_id: self.system_id.clone(),
            sensor: self.sensor.clone(),
            periods: self.periods.clone(),
            program: self.program,
            advance: self.advance,
            boost: self.boost,
            is_within_period,
            temperature: self.temperature,
            current_target,
            relay_on: self.relay_on,
            disabled: self.disabled,
            error_count: self.error_count,
            history: self.history.iter().copied().collect(),
        }
    }
}

/// Public API representation of a heating zone (`GET /api/v3/systems/`, etc.).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemOut {
    pub system_id: String,
    pub sensor: SensorNode,
    pub periods: Vec<Period>,
    pub program: bool,
    pub advance: Option<f64>,
    pub boost: Option<f64>,
    pub is_within_period: bool,
    pub temperature: Option<f64>,
    pub current_target: f64,
    pub relay_on: bool,
    pub disabled: bool,
    pub error_count: u32,
    pub history: Vec<HistoryPoint>,
}

/// Request body for `POST /api/v3/periods/{system_id}/`.
#[derive(Debug, Clone, Deserialize)]
pub struct PeriodsBody {
    pub periods: Vec<Period>,
}

/// Request body for `POST /api/v3/advance/{system_id}/` and `POST /api/v3/boost/{system_id}/`.
#[derive(Debug, Clone, Deserialize)]
pub struct AdvanceBody {
    pub end_time: f64,
}

/// Request body for `POST /api/v3/receive/{sensor_id}/`.
#[derive(Debug, Clone, Deserialize)]
pub struct SensorReceiveBody {
    pub temperature: f64,
}

/// Request body for `POST /api/v3/systems/`.
#[derive(Debug, Clone, Deserialize)]
pub struct SystemUpdate {
    pub system_id: String,
    #[serde(default)]
    pub sensor: Option<SensorNode>,
    #[serde(default)]
    pub periods: Option<Vec<Period>>,
    #[serde(default)]
    pub program: Option<bool>,
}

/// Default initial systems configuration (matching live `persistence.json` on 192.168.1.90).
pub fn default_systems() -> Vec<HeatingSystem> {
    vec![
        HeatingSystem {
            system_id: "downstairs".to_string(),
            sensor: SensorNode {
                url: "http://192.168.1.185".to_string(),
                adjustment: None,
            },
            program: true,
            periods: vec![
                Period {
                    id: "592604cc-cd7b-4bf6-829d-e589a5f6f209".to_string(),
                    start: 5.5,
                    end: 8.5,
                    target: 21.0,
                    days: Days {
                        monday: true,
                        tuesday: true,
                        wednesday: true,
                        thursday: true,
                        friday: true,
                        saturday: false,
                        sunday: false,
                    },
                },
                Period {
                    id: "92907943-0fb3-456b-a43d-831c863da27e".to_string(),
                    start: 6.0,
                    end: 21.0,
                    target: 21.0,
                    days: Days {
                        monday: false,
                        tuesday: false,
                        wednesday: false,
                        thursday: false,
                        friday: false,
                        saturday: true,
                        sunday: true,
                    },
                },
                Period {
                    id: "773852c6-11ea-4cf4-adc8-4e8918e0dc8b".to_string(),
                    start: 17.5,
                    end: 21.0,
                    target: 21.0,
                    days: Days {
                        monday: true,
                        tuesday: true,
                        wednesday: true,
                        thursday: true,
                        friday: true,
                        saturday: false,
                        sunday: false,
                    },
                },
            ],
            advance: None,
            boost: None,
            disabled: false,
            error_count: 0,
            disabled_until: None,
            max_error_count: DEFAULT_MAX_ERROR_COUNT,
            temperature: None,
            temperature_expiry: None,
            expiry_seconds: DEFAULT_EXPIRY_SECS,
            relay_on: false,
            history: VecDeque::new(),
            pending_heating_since_last_sample: false,
        },
        HeatingSystem {
            system_id: "upstairs".to_string(),
            sensor: SensorNode {
                url: "http://192.168.1.25".to_string(),
                adjustment: None,
            },
            program: true,
            periods: vec![
                Period {
                    id: "70f2fc9b-ebc3-400e-a8d9-e92129086835".to_string(),
                    start: 6.0,
                    end: 8.0,
                    target: 20.0,
                    days: Days::default(),
                },
                Period {
                    id: "2fd5425e-809e-4ff5-b3bb-a11a87407cac".to_string(),
                    start: 17.5,
                    end: 19.0,
                    target: 20.0,
                    days: Days::default(),
                },
            ],
            advance: None,
            boost: None,
            disabled: false,
            error_count: 0,
            disabled_until: None,
            max_error_count: DEFAULT_MAX_ERROR_COUNT,
            temperature: None,
            temperature_expiry: None,
            expiry_seconds: DEFAULT_EXPIRY_SECS,
            relay_on: false,
            history: VecDeque::new(),
            pending_heating_since_last_sample: false,
        },
    ]
}
