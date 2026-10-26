//! Background heating control loop and shared controller state.

use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use anyhow::{Context, Result};
use esp_idf_svc::nvs::{EspDefaultNvsPartition, EspNvs, NvsDefault};
use log::{error, info, warn};

use crate::model::{
    default_systems, HeatingSystem, CHECK_FREQUENCY_SECONDS, DISABLED_COOLDOWN_SECS,
};
use crate::relay::Relay;
use crate::sensor::fetch_sensor_temperature;
use crate::time;

const NVS_NAMESPACE: &str = "heating";
const NVS_SYSTEMS_KEY: &str = "systems";
const NVS_TOKEN_KEY: &str = "auth_token";
const CONTROL_THREAD_STACK_SIZE: usize = 10_240;

const API_USER: &str = env!("API_USER");
const API_PASS: &str = env!("API_PASS");
const SENSOR_TOKEN: &str = env!("SENSOR_TOKEN");

pub type SharedController = Arc<Mutex<HeatingController>>;

pub struct HeatingController {
    pub systems: Vec<HeatingSystem>,
    pub loop_enabled: bool,
    auth_token: String,
    downstairs_relay: Relay<'static>,
    upstairs_relay: Relay<'static>,
    nvs: EspNvs<NvsDefault>,
}

impl HeatingController {
    /// Initialise the heating controller, loading persisted zone schedules from NVS
    /// (or seeding defaults on first boot) and ensuring both relays start OFF.
    pub fn new(
        nvs_partition: EspDefaultNvsPartition,
        mut downstairs_relay: Relay<'static>,
        mut upstairs_relay: Relay<'static>,
    ) -> Result<Self> {
        // Fail-safe: ensure both relays are OFF before doing anything else.
        downstairs_relay
            .off()
            .context("Failed to turn off downstairs relay")?;
        upstairs_relay
            .off()
            .context("Failed to turn off upstairs relay")?;

        let nvs = EspNvs::new(nvs_partition, NVS_NAMESPACE, true)
            .context("Failed to open NVS namespace 'heating'")?;

        let systems = match Self::load_systems_from_nvs(&nvs) {
            Ok(Some(loaded)) => {
                info!("Loaded {} heating zones from NVS", loaded.len());
                loaded
            }
            Ok(None) => {
                info!("No saved schedule in NVS; seeding default zones (downstairs, upstairs)");
                let defaults = default_systems();
                if let Err(e) = Self::write_systems_to_nvs(&nvs, &defaults) {
                    warn!("Failed to persist initial default systems to NVS: {e:?}");
                }
                defaults
            }
            Err(e) => {
                warn!("Failed to load systems from NVS ({e:?}); falling back to defaults");
                default_systems()
            }
        };

        let auth_token = Self::load_or_init_token(&nvs);

        Ok(Self {
            systems,
            loop_enabled: true,
            auth_token,
            downstairs_relay,
            upstairs_relay,
            nvs,
        })
    }

    fn generate_random_hex_token() -> String {
        let mut bytes = [0u8; 32];
        unsafe {
            esp_idf_svc::sys::esp_fill_random(bytes.as_mut_ptr() as *mut _, bytes.len());
        }
        let mut hex = String::with_capacity(64);
        for b in bytes {
            use core::fmt::Write;
            let _ = write!(&mut hex, "{b:02x}");
        }
        hex
    }

    fn load_or_init_token(nvs: &EspNvs<NvsDefault>) -> String {
        if let Ok(Some(len)) = nvs.str_len(NVS_TOKEN_KEY) {
            let mut buf = vec![0u8; len];
            if let Ok(Some(saved)) = nvs.get_str(NVS_TOKEN_KEY, &mut buf) {
                if saved.len() >= 32 {
                    return saved.to_string();
                }
            }
        }
        let token = Self::generate_random_hex_token();
        let _ = nvs.set_str(NVS_TOKEN_KEY, &token);
        token
    }

    /// Validates login credentials and returns the active bearer token on success.
    pub fn authenticate(&mut self, username: &str, password: &str) -> Option<String> {
        let user_ok = if API_USER.is_empty() {
            !username.trim().is_empty()
        } else {
            constant_time_eq(username.trim().as_bytes(), API_USER.as_bytes())
        };
        let pass_ok = !API_PASS.is_empty() && constant_time_eq(password.as_bytes(), API_PASS.as_bytes());

        if user_ok && pass_ok {
            if self.auth_token.is_empty() {
                self.auth_token = Self::load_or_init_token(&self.nvs);
            }
            Some(self.auth_token.clone())
        } else {
            None
        }
    }

    /// Validates a Bearer token from an `Authorization` header.
    pub fn verify_token(&self, token: &str) -> bool {
        !self.auth_token.is_empty()
            && !token.is_empty()
            && constant_time_eq(token.as_bytes(), self.auth_token.as_bytes())
    }

    /// Validates the `X-Sensor-Token` header for `POST /api/v3/receive/{sensor_id}/`.
    pub fn verify_sensor_token(&self, provided: Option<&str>) -> std::result::Result<(), (u16, &'static str)> {
        if SENSOR_TOKEN.is_empty() {
            return Err((503, "Sensor ingest token is not configured"));
        }
        match provided {
            Some(tok) if constant_time_eq(tok.as_bytes(), SENSOR_TOKEN.as_bytes()) => Ok(()),
            _ => Err((401, "Could not validate sensor token")),
        }
    }

    fn load_systems_from_nvs(nvs: &EspNvs<NvsDefault>) -> Result<Option<Vec<HeatingSystem>>> {
        let Some(len) = nvs
            .str_len(NVS_SYSTEMS_KEY)
            .context("NVS str_len check failed")?
        else {
            return Ok(None);
        };

        let mut buf = vec![0u8; len];
        let Some(json_str) = nvs
            .get_str(NVS_SYSTEMS_KEY, &mut buf)
            .context("NVS get_str failed")?
        else {
            return Ok(None);
        };

        let mut systems: Vec<HeatingSystem> =
            serde_json::from_str(json_str).context("Failed to deserialize systems from NVS")?;

        // Do not carry over transient runtime state across reboots.
        for sys in &mut systems {
            sys.relay_on = false;
            sys.temperature = None;
            sys.temperature_expiry = None;
            sys.error_count = 0;
            sys.disabled = false;
            sys.disabled_until = None;
        }

        Ok(Some(systems))
    }

    fn write_systems_to_nvs(nvs: &EspNvs<NvsDefault>, systems: &[HeatingSystem]) -> Result<()> {
        let json =
            serde_json::to_string(systems).context("Failed to serialize systems to JSON")?;
        nvs.set_str(NVS_SYSTEMS_KEY, &json)
            .context("Failed to write systems JSON to NVS")?;
        Ok(())
    }

    /// Persist current zone configurations and schedules to NVS.
    pub fn save_to_nvs(&self) -> Result<()> {
        Self::write_systems_to_nvs(&self.nvs, &self.systems)
    }

    /// Fail-safe helper: switch all relays OFF immediately and update state.
    pub fn switch_all_off(&mut self) {
        if let Err(e) = self.downstairs_relay.off() {
            error!("Failed to switch OFF downstairs relay: {e:?}");
        }
        if let Err(e) = self.upstairs_relay.off() {
            error!("Failed to switch OFF upstairs relay: {e:?}");
        }
        for sys in &mut self.systems {
            sys.relay_on = false;
        }
    }

    /// Actuate the hardware relay for a specific zone (`downstairs` or `upstairs`).
    pub fn set_zone_relay(&mut self, system_id: &str, active: bool) {
        let res = match system_id {
            "downstairs" => self.downstairs_relay.set(active),
            "upstairs" => self.upstairs_relay.set(active),
            other => {
                warn!("Unknown zone relay '{other}'");
                return;
            }
        };

        if let Err(e) = res {
            error!("GPIO error switching relay '{system_id}' to {active}: {e:?}");
            return;
        }

        let actual_on = match system_id {
            "downstairs" => self.downstairs_relay.is_on(),
            "upstairs" => self.upstairs_relay.is_on(),
            _ => false,
        };

        if let Some(sys) = self.systems.iter_mut().find(|s| s.system_id == system_id) {
            sys.relay_on = actual_on;
        }
    }

    /// Immediately re-evaluates schedule/override state for `system_id` using its latest
    /// known temperature and actuates its relay without waiting for the next 10s poll tick.
    pub fn evaluate_and_actuate_zone(&mut self, system_id: &str) {
        if !self.loop_enabled {
            self.switch_all_off();
            return;
        }

        let (Some(wall_time), Some(now_epoch)) = (time::now(), time::now_epoch()) else {
            self.switch_all_off();
            return;
        };

        let should_heat = {
            let Some(sys) = self.systems.iter_mut().find(|s| s.system_id == system_id) else {
                return;
            };
            if sys.disabled {
                false
            } else if let Some(temp) = sys.temperature {
                sys.evaluate_should_heat(temp, &wall_time, now_epoch)
            } else {
                return;
            }
        };

        self.set_zone_relay(system_id, should_heat);
    }
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (&x, &y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

/// Snapshot of a zone taken prior to unlocked HTTP polling.
struct ZonePollPlan {
    system_id: String,
    sensor_url: String,
    cached_temp: Option<f64>,
}

/// Spawns the background heating control loop on a dedicated FreeRTOS thread.
pub fn spawn_control_loop(controller: SharedController) -> Result<JoinHandle<()>> {
    thread::Builder::new()
        .name("heating-ctrl".into())
        .stack_size(CONTROL_THREAD_STACK_SIZE)
        .spawn(move || {
            info!(
                "Heating control loop started (interval: {}s)",
                CHECK_FREQUENCY_SECONDS
            );

            loop {
                run_control_tick(&controller);
                thread::sleep(Duration::from_secs(CHECK_FREQUENCY_SECONDS));
            }
        })
        .context("Failed to spawn heating-ctrl thread")
}

/// Executes one evaluation cycle across all configured heating zones.
fn run_control_tick(controller: &SharedController) {
    // 1. Verify wall-clock time is synchronized. Fail-safe OFF if unsynced.
    let (Some(wall_time), Some(now_epoch)) = (time::now(), time::now_epoch()) else {
        warn!("Control loop: clock not synchronized — holding all relays OFF");
        if let Ok(mut guard) = controller.lock() {
            guard.switch_all_off();
        }
        return;
    };

    // 2. Inspect zones under lock and build the polling plan without doing network I/O under lock.
    let mut poll_plans: Vec<ZonePollPlan> = Vec::new();
    {
        let Ok(mut guard) = controller.lock() else {
            error!("Control loop: controller mutex poisoned!");
            return;
        };

        if !guard.loop_enabled {
            guard.switch_all_off();
            return;
        }

        let mut disabled_ids_to_turn_off = Vec::new();

        for sys in &mut guard.systems {
            if sys.disabled {
                if let Some(until) = sys.disabled_until {
                    if now_epoch < until {
                        let remaining_secs = (until - now_epoch) as u64;
                        warn!(
                            "Zone '{}' is disabled due to sensor errors ({}s cooldown remaining)",
                            sys.system_id, remaining_secs
                        );
                        disabled_ids_to_turn_off.push(sys.system_id.clone());
                        continue;
                    }
                }
                info!(
                    "Zone '{}' cooldown elapsed; re-enabling zone",
                    sys.system_id
                );
                sys.disabled = false;
                sys.disabled_until = None;
                sys.error_count = 0;
            }

            poll_plans.push(ZonePollPlan {
                system_id: sys.system_id.clone(),
                sensor_url: sys.sensor.url.clone(),
                cached_temp: sys.valid_cached_temperature(now_epoch),
            });
        }

        for id in disabled_ids_to_turn_off {
            guard.set_zone_relay(&id, false);
        }
    }

    // 3. For each active zone, fetch temperature (outside the lock if polling over HTTP),
    //    then re-lock to update state and actuate the zone relay.
    for plan in poll_plans {
        let fetched_result = match plan.cached_temp {
            Some(temp) => Ok((temp, true)),
            None => fetch_sensor_temperature(&plan.sensor_url).map(|t| (t, false)),
        };

        let Ok(mut guard) = controller.lock() else {
            error!("Control loop: controller mutex poisoned!");
            return;
        };

        match fetched_result {
            Ok((temp, was_cached)) => {
                let Some(sys) = guard
                    .systems
                    .iter_mut()
                    .find(|s| s.system_id == plan.system_id)
                else {
                    continue;
                };

                if !was_cached {
                    sys.update_temperature(temp, now_epoch);
                }

                let effective_temp = sys.temperature.unwrap_or(temp);
                let should_heat = sys.evaluate_should_heat(effective_temp, &wall_time, now_epoch);
                let target = sys.current_target(&wall_time);

                guard.set_zone_relay(&plan.system_id, should_heat);

                info!(
                    "Zone '{}': temp={:.1}°C, target={:.1}°C, relay={}",
                    plan.system_id,
                    effective_temp,
                    target,
                    if should_heat { "ON" } else { "OFF" }
                );
            }
            Err(err) => {
                // Fail-safe: immediately ensure relay is OFF when sensor cannot be reached.
                guard.set_zone_relay(&plan.system_id, false);

                if let Some(sys) = guard
                    .systems
                    .iter_mut()
                    .find(|s| s.system_id == plan.system_id)
                {
                    sys.error_count += 1;
                    warn!(
                        "Zone '{}' sensor error ({}/{}): {err}",
                        sys.system_id, sys.error_count, sys.max_error_count
                    );

                    if sys.error_count >= sys.max_error_count {
                        sys.disabled = true;
                        sys.disabled_until = Some(now_epoch + DISABLED_COOLDOWN_SECS);
                        warn!(
                            "Zone '{}' disabled for 15 minutes after {} consecutive sensor errors",
                            sys.system_id, sys.error_count
                        );
                    }
                }
            }
        }
    }
}
