//! MQTT client and Home Assistant Auto-Discovery integration.
//!
//! Connects to the local Mosquitto broker, publishes retained Home Assistant
//! discovery configurations (native `climate` thermostats, quick-action `button`s,
//! relay `binary_sensor`s, and diagnostic `sensor`s), streams live state updates,
//! and processes incoming commands from Home Assistant.

use std::sync::mpsc::{self, SyncSender};
use std::sync::OnceLock;
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result};
use esp_idf_svc::mqtt::client::{
    EspMqttClient, EventPayload, LwtConfiguration, MqttClientConfiguration, QoS,
};
use log::{info, warn};
use serde_json::json;

use crate::control::SharedController;
use crate::model::DEFAULT_MINIMUM_TARGET;
use crate::time;

const MQTT_URL: &str = env!("MQTT_URL");
const DEVICE_ID: &str = "central_heating_esp32";
const AVAILABILITY_TOPIC: &str = "central_heating/availability";
const DIAGNOSTICS_TOPIC: &str = "central_heating/diagnostics";
const MQTT_WORKER_STACK_SIZE: usize = 10_240;

enum MqttWorkerEvent {
    Connected,
    Disconnected,
    PublishState,
    Command { topic: String, payload: String },
}

static MQTT_NOTIFIER: OnceLock<SyncSender<MqttWorkerEvent>> = OnceLock::new();

/// Signals the MQTT worker thread to publish the latest zone and system state immediately.
/// Safe and non-blocking to call from any thread (HTTP handlers, control loop, etc.).
pub fn notify_state_changed() {
    if let Some(tx) = MQTT_NOTIFIER.get() {
        let _ = tx.try_send(MqttWorkerEvent::PublishState);
    }
}

/// Starts the MQTT client and spawns the background worker thread.
/// If `MQTT_URL` is empty, logs a message and returns `Ok(())`.
pub fn start_mqtt(controller: SharedController) -> Result<()> {
    if MQTT_URL.trim().is_empty() {
        info!("MQTT integration disabled (MQTT_URL is empty)");
        return Ok(());
    }

    let (tx, rx) = mpsc::sync_channel::<MqttWorkerEvent>(32);
    let _ = MQTT_NOTIFIER.set(tx.clone());

    let conf = MqttClientConfiguration {
        client_id: Some(DEVICE_ID),
        keep_alive_interval: Some(Duration::from_secs(30)),
        reconnect_timeout: Some(Duration::from_secs(10)),
        network_timeout: Duration::from_secs(10),
        lwt: Some(LwtConfiguration {
            topic: AVAILABILITY_TOPIC,
            payload: b"offline",
            qos: QoS::AtLeastOnce,
            retain: true,
        }),
        ..Default::default()
    };

    info!("Initializing MQTT client for broker {MQTT_URL}...");
    let cb_tx = tx.clone();
    let client = EspMqttClient::new_cb(MQTT_URL, &conf, move |event| match event.payload() {
        EventPayload::Connected(_) => {
            info!("MQTT: connected to broker {MQTT_URL}");
            let _ = cb_tx.try_send(MqttWorkerEvent::Connected);
        }
        EventPayload::Disconnected => {
            warn!("MQTT: disconnected from broker");
            let _ = cb_tx.try_send(MqttWorkerEvent::Disconnected);
        }
        EventPayload::Received {
            topic: Some(topic_str),
            data,
            ..
        } => {
            if let Ok(payload_str) = core::str::from_utf8(data) {
                let _ = cb_tx.try_send(MqttWorkerEvent::Command {
                    topic: topic_str.to_string(),
                    payload: payload_str.trim().to_string(),
                });
            }
        }
        EventPayload::Error(err) => {
            warn!("MQTT client error: {err:?}");
        }
        _ => {}
    })
    .context("Failed to create EspMqttClient")?;

    thread::Builder::new()
        .name("mqtt-worker".into())
        .stack_size(MQTT_WORKER_STACK_SIZE)
        .spawn(move || {
            run_mqtt_worker(client, controller, rx);
        })
        .context("Failed to spawn mqtt-worker thread")?;

    Ok(())
}

fn run_mqtt_worker(
    mut client: EspMqttClient<'static>,
    controller: SharedController,
    rx: mpsc::Receiver<MqttWorkerEvent>,
) {
    let mut connected = false;

    for event in rx {
        match event {
            MqttWorkerEvent::Connected => {
                connected = true;
                if let Err(e) = on_connected(&mut client, &controller) {
                    warn!("MQTT: error during on_connected setup: {e:?}");
                }
            }
            MqttWorkerEvent::Disconnected => {
                connected = false;
            }
            MqttWorkerEvent::PublishState => {
                if connected {
                    if let Err(e) = publish_all_states(&mut client, &controller) {
                        warn!("MQTT: failed to publish state update: {e:?}");
                    }
                }
            }
            MqttWorkerEvent::Command { topic, payload } => {
                handle_command(&controller, &topic, &payload);
            }
        }
    }
}

fn on_connected(
    client: &mut EspMqttClient<'static>,
    controller: &SharedController,
) -> Result<()> {
    // 1. Subscribe to all zone command topics
    client.subscribe("central_heating/+/set/#", QoS::AtLeastOnce)?;

    // 2. Publish online availability (retained)
    publish_str(client, AVAILABILITY_TOPIC, "online", true)?;

    // 3. Publish Home Assistant Auto-Discovery configurations (retained)
    let zone_ids: Vec<String> = {
        let Ok(guard) = controller.lock() else {
            return Ok(());
        };
        guard.systems.iter().map(|s| s.system_id.clone()).collect()
    };

    for zone_id in &zone_ids {
        publish_zone_discovery(client, zone_id)?;
    }
    publish_diagnostics_discovery(client)?;

    info!(
        "MQTT: published Home Assistant discovery configs for {:?} and subscribed to commands",
        zone_ids
    );

    // 4. Publish initial state for all zones and diagnostics
    publish_all_states(client, controller)?;

    Ok(())
}

fn device_metadata() -> serde_json::Value {
    json!({
        "identifiers": [DEVICE_ID],
        "name": "Central Heating",
        "manufacturer": "ESP32 Custom",
        "model": "ESP-WROOM-32 Dual-Zone Controller",
        "sw_version": env!("CARGO_PKG_VERSION"),
        "configuration_url": "http://192.168.1.168/"
    })
}

fn title_case(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

fn publish_zone_discovery(client: &mut EspMqttClient<'static>, zone_id: &str) -> Result<()> {
    let dev = device_metadata();
    let zone_title = title_case(zone_id);

    // 1. Native Home Assistant `climate` entity
    let climate_topic = format!("homeassistant/climate/{DEVICE_ID}/{zone_id}/config");
    let climate_cfg = json!({
        "name": format!("{zone_title} Heating"),
        "object_id": format!("{zone_id}_heating"),
        "unique_id": format!("{DEVICE_ID}_climate_{zone_id}"),
        "device": dev,
        "availability_topic": AVAILABILITY_TOPIC,
        "payload_available": "online",
        "payload_not_available": "offline",
        "modes": ["auto", "heat", "off"],
        "mode_state_topic": format!("central_heating/{zone_id}/mode"),
        "mode_command_topic": format!("central_heating/{zone_id}/set/mode"),
        "preset_modes": ["boost", "advance"],
        "preset_mode_state_topic": format!("central_heating/{zone_id}/preset"),
        "preset_mode_command_topic": format!("central_heating/{zone_id}/set/preset"),
        "current_temperature_topic": format!("central_heating/{zone_id}/current_temperature"),
        "temperature_state_topic": format!("central_heating/{zone_id}/target_temperature"),
        "temperature_command_topic": format!("central_heating/{zone_id}/set/temperature"),
        "action_topic": format!("central_heating/{zone_id}/action"),
        "min_temp": 5.0,
        "max_temp": 26.0,
        "temp_step": 0.5,
        "precision": 0.1,
        "temperature_unit": "C",
        "icon": "mdi:radiator"
    });
    publish_json(client, &climate_topic, &climate_cfg, true)?;

    // 2. Binary sensor for zone relay (boiler call for heat)
    let relay_topic = format!("homeassistant/binary_sensor/{DEVICE_ID}/{zone_id}_relay/config");
    let relay_cfg = json!({
        "name": format!("{zone_title} Relay"),
        "object_id": format!("{zone_id}_heating_relay"),
        "unique_id": format!("{DEVICE_ID}_{zone_id}_relay"),
        "device": dev,
        "availability_topic": AVAILABILITY_TOPIC,
        "state_topic": format!("central_heating/{zone_id}/relay"),
        "payload_on": "ON",
        "payload_off": "OFF",
        "device_class": "heat",
        "icon": "mdi:fire"
    });
    publish_json(client, &relay_topic, &relay_cfg, true)?;

    // 3. Quick-action buttons: Boost 15m, Advance +1h, Cancel Override
    let buttons = [
        (
            "boost",
            format!("{zone_title} Boost (15m)"),
            format!("{zone_id}_boost_15m"),
            "mdi:rocket-launch",
        ),
        (
            "advance",
            format!("{zone_title} Advance (+1h)"),
            format!("{zone_id}_advance_1h"),
            "mdi:fast-forward",
        ),
        (
            "cancel",
            format!("{zone_title} Cancel Override"),
            format!("{zone_id}_cancel_override"),
            "mdi:cancel",
        ),
    ];

    for (action, label, obj_id, icon) in buttons {
        let btn_topic = format!("homeassistant/button/{DEVICE_ID}/{zone_id}_{action}/config");
        let btn_cfg = json!({
            "name": label,
            "object_id": obj_id,
            "unique_id": format!("{DEVICE_ID}_{zone_id}_{action}_btn"),
            "device": dev,
            "availability_topic": AVAILABILITY_TOPIC,
            "command_topic": format!("central_heating/{zone_id}/set/{action}"),
            "payload_press": "PRESS",
            "icon": icon
        });
        publish_json(client, &btn_topic, &btn_cfg, true)?;
    }

    Ok(())
}

fn publish_diagnostics_discovery(client: &mut EspMqttClient<'static>) -> Result<()> {
    let dev = device_metadata();

    let sensors = [
        (
            "wifi_rssi",
            "Heating Wi-Fi Signal",
            "central_heating_wifi_rssi",
            "{{ value_json.wifi_rssi }}",
            "dBm",
            Some("signal_strength"),
            "mdi:wifi",
        ),
        (
            "free_heap",
            "Heating Free Heap",
            "central_heating_free_heap",
            "{{ value_json.free_heap_bytes }}",
            "B",
            Some("data_size"),
            "mdi:memory",
        ),
        (
            "uptime",
            "Heating Uptime",
            "central_heating_uptime",
            "{{ value_json.uptime_secs }}",
            "s",
            Some("duration"),
            "mdi:clock-outline",
        ),
    ];

    for (key, name, obj_id, tpl, unit, dev_class, icon) in sensors {
        let topic = format!("homeassistant/sensor/{DEVICE_ID}/{key}/config");
        let mut cfg = json!({
            "name": name,
            "object_id": obj_id,
            "unique_id": format!("{DEVICE_ID}_{key}"),
            "device": dev,
            "availability_topic": AVAILABILITY_TOPIC,
            "state_topic": DIAGNOSTICS_TOPIC,
            "value_template": tpl,
            "unit_of_measurement": unit,
            "state_class": "measurement",
            "entity_category": "diagnostic",
            "icon": icon
        });
        if let Some(dc) = dev_class {
            cfg["device_class"] = json!(dc);
        }
        publish_json(client, &topic, &cfg, true)?;
    }

    Ok(())
}

struct ZoneMqttSnapshot {
    system_id: String,
    mode: &'static str,
    preset: &'static str,
    action: &'static str,
    current_temp: Option<f64>,
    target_temp: f64,
    relay_on: bool,
}

fn publish_all_states(
    client: &mut EspMqttClient<'static>,
    controller: &SharedController,
) -> Result<()> {
    let wall = time::now();
    let now_epoch = time::now_epoch();

    let snapshots: Vec<ZoneMqttSnapshot> = {
        let Ok(mut guard) = controller.lock() else {
            return Ok(());
        };

        guard
            .systems
            .iter_mut()
            .map(|sys| {
                if let Some(epoch) = now_epoch {
                    sys.clear_expired_overrides(epoch);
                }

                let preset = if sys.boost.is_some() {
                    "boost"
                } else if sys.advance.is_some() {
                    "advance"
                } else {
                    "none"
                };

                let mode = if sys.boost.is_some() || sys.advance.is_some() {
                    "heat"
                } else if sys.program {
                    "auto"
                } else {
                    "off"
                };

                let action = if sys.relay_on {
                    "heating"
                } else if !sys.program && sys.boost.is_none() && sys.advance.is_none() {
                    "off"
                } else {
                    "idle"
                };

                let target_temp = wall
                    .as_ref()
                    .map(|w| sys.effective_target(w))
                    .unwrap_or(DEFAULT_MINIMUM_TARGET);

                ZoneMqttSnapshot {
                    system_id: sys.system_id.clone(),
                    mode,
                    preset,
                    action,
                    current_temp: sys.temperature,
                    target_temp,
                    relay_on: sys.relay_on,
                }
            })
            .collect()
    };

    for snap in snapshots {
        let z = &snap.system_id;
        publish_str(client, &format!("central_heating/{z}/mode"), snap.mode, true)?;
        publish_str(
            client,
            &format!("central_heating/{z}/preset"),
            snap.preset,
            true,
        )?;
        publish_str(
            client,
            &format!("central_heating/{z}/action"),
            snap.action,
            true,
        )?;
        publish_str(
            client,
            &format!("central_heating/{z}/relay"),
            if snap.relay_on { "ON" } else { "OFF" },
            true,
        )?;
        publish_str(
            client,
            &format!("central_heating/{z}/target_temperature"),
            &format!("{:.1}", snap.target_temp),
            true,
        )?;
        let curr_temp_str = match snap.current_temp {
            Some(t) => format!("{t:.1}"),
            None => "None".to_string(),
        };
        publish_str(
            client,
            &format!("central_heating/{z}/current_temperature"),
            &curr_temp_str,
            true,
        )?;
    }

    // Publish system diagnostics
    let uptime_secs = unsafe { (esp_idf_svc::sys::esp_timer_get_time() / 1_000_000).max(0) as u64 };
    let free_heap_bytes = unsafe { esp_idf_svc::sys::esp_get_free_heap_size() };
    let wifi_rssi = unsafe {
        let mut ap_info: esp_idf_svc::sys::wifi_ap_record_t = core::mem::zeroed();
        if esp_idf_svc::sys::esp_wifi_sta_get_ap_info(&mut ap_info) == esp_idf_svc::sys::ESP_OK {
            Some(ap_info.rssi)
        } else {
            None
        }
    };

    let diag_json = json!({
        "uptime_secs": uptime_secs,
        "free_heap_bytes": free_heap_bytes,
        "wifi_rssi": wifi_rssi,
    });
    publish_json(client, DIAGNOSTICS_TOPIC, &diag_json, true)?;

    Ok(())
}

fn handle_command(controller: &SharedController, topic: &str, payload: &str) {
    // Expected format: `central_heating/<zone>/set/<command>`
    let Some(rest) = topic.strip_prefix("central_heating/") else {
        return;
    };
    let Some((zone_id, cmd)) = rest.split_once("/set/") else {
        return;
    };

    let (Some(wall), Some(now_epoch)) = (time::now(), time::now_epoch()) else {
        warn!("MQTT command ignored: wall-clock time not synchronized");
        return;
    };

    let Ok(mut guard) = controller.lock() else {
        return;
    };

    let Some(sys) = guard.systems.iter_mut().find(|s| s.system_id == zone_id) else {
        warn!("MQTT command for unknown zone '{zone_id}'");
        return;
    };

    sys.clear_expired_overrides(now_epoch);

    match cmd {
        "mode" => match payload {
            "auto" => {
                sys.program = true;
                sys.boost = None;
                sys.advance = None;
                info!("MQTT: zone '{zone_id}' set to mode=auto");
            }
            "heat" => {
                sys.program = true;
                if sys.boost.is_none() && sys.advance.is_none() && !sys.is_within_period(&wall) {
                    sys.advance = Some(now_epoch + 3600.0);
                }
                info!("MQTT: zone '{zone_id}' set to mode=heat");
            }
            "off" => {
                sys.program = false;
                sys.boost = None;
                sys.advance = None;
                info!("MQTT: zone '{zone_id}' set to mode=off");
            }
            other => {
                warn!("MQTT: unknown mode '{other}' for '{zone_id}'");
                return;
            }
        },
        "preset" => match payload {
            "boost" => {
                sys.boost = Some(now_epoch + 900.0);
                sys.advance = None;
                info!("MQTT: zone '{zone_id}' preset=boost (15m)");
            }
            "advance" => {
                sys.advance = Some(now_epoch + 3600.0);
                sys.boost = None;
                info!("MQTT: zone '{zone_id}' preset=advance (1h)");
            }
            "none" => {
                sys.boost = None;
                sys.advance = None;
                info!("MQTT: zone '{zone_id}' preset=none");
            }
            other => {
                warn!("MQTT: unknown preset '{other}' for '{zone_id}'");
                return;
            }
        },
        "temperature" => {
            let Ok(target) = payload.parse::<f64>() else {
                warn!("MQTT: invalid temperature '{payload}' for '{zone_id}'");
                return;
            };
            sys.set_target_temperature(target, &wall, now_epoch);
            info!("MQTT: zone '{zone_id}' target temperature set to {target:.1}°C");
        }
        "boost" => {
            sys.boost = Some(now_epoch + 900.0);
            sys.advance = None;
            info!("MQTT: zone '{zone_id}' 15m boost button pressed");
        }
        "advance" => {
            let base = sys.advance.unwrap_or(now_epoch).max(now_epoch);
            sys.advance = Some(base + 3600.0);
            sys.boost = None;
            info!("MQTT: zone '{zone_id}' +1h advance button pressed");
        }
        "cancel" => {
            sys.boost = None;
            sys.advance = None;
            info!("MQTT: zone '{zone_id}' cancel override button pressed");
        }
        other => {
            warn!("MQTT: unknown command '{other}' on topic '{topic}'");
            return;
        }
    }

    if let Err(e) = guard.save_to_nvs() {
        warn!("MQTT: failed to persist NVS state after command: {e:?}");
    }
    guard.evaluate_and_actuate_zone(zone_id);
}

fn publish_str(
    client: &mut EspMqttClient<'static>,
    topic: &str,
    payload: &str,
    retain: bool,
) -> Result<()> {
    client.enqueue(topic, QoS::AtLeastOnce, retain, payload.as_bytes())?;
    Ok(())
}

fn publish_json(
    client: &mut EspMqttClient<'static>,
    topic: &str,
    value: &serde_json::Value,
    retain: bool,
) -> Result<()> {
    let bytes = serde_json::to_vec(value)?;
    client.enqueue(topic, QoS::AtLeastOnce, retain, &bytes)?;
    Ok(())
}
