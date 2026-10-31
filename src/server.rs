//! Embedded HTTP server (`EspHttpServer`) exposing the Central Heating v3 REST API
//! and serving the pre-compressed React/Vite web UI bundle.

use anyhow::{Context, Result};
use esp_idf_svc::http::server::{Configuration, EspHttpConnection, EspHttpServer, Request};
use esp_idf_svc::http::Method;
use esp_idf_svc::io::Write;
use esp_idf_svc::ota::EspOta;
use log::{error, info, warn};
use serde::Serialize;
use serde_json::json;

use crate::control::SharedController;
use crate::model::{
    AdvanceBody, HistoryPoint, PeriodsBody, SensorReceiveBody, SystemOut, SystemUpdate,
    DEFAULT_MINIMUM_TARGET,
};
use crate::sensor::fetch_sensor_temperature;
use crate::time;

const MAX_BODY_LEN: usize = 16_384;
const HTTP_STACK_SIZE: usize = 10_240;
const MAX_URI_HANDLERS: usize = 28;

// Embedded frontend assets from `web/` (Svelte 5 + Tailwind v4 build)
const INDEX_HTML: &[u8] = include_bytes!("../web/index.html");
const MANIFEST_JSON: &[u8] = include_bytes!("../web/manifest.webmanifest");
const REGISTER_SW_JS: &[u8] = include_bytes!("../web/registerSW.js");
const SW_JS: &[u8] = include_bytes!("../web/sw.js");
const FLAME_512_PNG: &[u8] = include_bytes!("../web/flame-512.png");
const FLAME_192_PNG: &[u8] = include_bytes!("../web/assets/flame-Bd7yHd1u.png");
const INDEX_CSS_GZ: &[u8] = include_bytes!("../web/assets/index.css.gz");
const INDEX_JS_GZ: &[u8] = include_bytes!("../web/assets/index.js.gz");

const CORS_HEADERS: &[(&str, &str)] = &[
    ("Content-Type", "application/json"),
    ("Access-Control-Allow-Origin", "*"),
    (
        "Access-Control-Allow-Headers",
        "Content-Type, Authorization, X-Sensor-Token",
    ),
    ("Access-Control-Allow-Methods", "GET, POST, OPTIONS"),
];

#[derive(Serialize)]
struct DiagnosticsPayload {
    uptime_secs: u64,
    free_heap_bytes: u32,
    min_free_heap_bytes: u32,
    wifi_rssi: Option<i8>,
    ota_slot: String,
    firmware_version: &'static str,
}

#[derive(Serialize)]
struct StatusPayload<'a> {
    systems: &'a [SystemOut],
    loop_enabled: bool,
    server_epoch: Option<f64>,
    diagnostics: DiagnosticsPayload,
}

/// Starts the HTTP server on port 80 and registers all REST API and static UI routes.
pub fn start_http_server(controller: SharedController) -> Result<EspHttpServer<'static>> {
    let config = Configuration {
        stack_size: HTTP_STACK_SIZE,
        max_uri_handlers: MAX_URI_HANDLERS,
        max_open_sockets: 7,
        lru_purge_enable: true,
        uri_match_wildcard: true,
        ..Default::default()
    };

    let mut server = EspHttpServer::new(&config).context("Failed to start EspHttpServer")?;

    // -------------------------------------------------------------------------
    // Authentication endpoints
    // -------------------------------------------------------------------------

    // POST /token/
    {
        let ctrl = controller.clone();
        server.fn_handler("/token/", Method::Post, move |mut req| {
            let content_type = req
                .header("Content-Type")
                .or_else(|| req.header("content-type"))
                .map(|s| s.to_string());
            let body = read_body(&mut req)?;

            let Some((username, password)) =
                parse_login_credentials(content_type.as_deref(), &body)
            else {
                return send_json(
                    req,
                    400,
                    &json!({"detail": "Missing username or password"}),
                );
            };

            let token_opt = {
                let Ok(mut guard) = ctrl.lock() else {
                    return send_json(req, 500, &json!({"detail": "Internal lock error"}));
                };
                guard.authenticate(&username, &password)
            };

            match token_opt {
                Some(token) => {
                    info!("Auth: user '{}' logged in", username);
                    send_json(
                        req,
                        200,
                        &json!({
                            "access_token": token,
                            "token_type": "bearer"
                        }),
                    )
                }
                None => {
                    warn!("Auth: failed login attempt for user '{}'", username);
                    send_json(
                        req,
                        401,
                        &json!({"detail": "Incorrect username or password"}),
                    )
                }
            }
        })?;
    }

    // GET /check_token/
    {
        let ctrl = controller.clone();
        server.fn_handler("/check_token/", Method::Get, move |req| {
            let Some(token) = check_bearer_auth(&req, &ctrl) else {
                return send_json(
                    req,
                    401,
                    &json!({"detail": "Could not validate credentials"}),
                );
            };
            send_json(
                req,
                200,
                &json!({
                    "access_token": token,
                    "token_type": "bearer"
                }),
            )
        })?;
    }

    // -------------------------------------------------------------------------
    // Heating API v3 endpoints
    // -------------------------------------------------------------------------

    // GET /api/v3/systems/ (must be registered BEFORE /api/v3/systems/*)
    {
        let ctrl = controller.clone();
        server.fn_handler("/api/v3/systems/", Method::Get, move |req| {
            let wall = time::now();
            let now_epoch = time::now_epoch();

            let Ok(mut guard) = ctrl.lock() else {
                return send_json(req, 500, &json!({"detail": "Internal lock error"}));
            };

            let out: Vec<_> = guard
                .systems
                .iter_mut()
                .map(|s| s.as_system_out(wall.as_ref(), now_epoch))
                .collect();

            send_json(req, 200, &out)
        })?;
    }

    // POST /api/v3/systems/
    {
        let ctrl = controller.clone();
        server.fn_handler("/api/v3/systems/", Method::Post, move |mut req| {
            if check_bearer_auth(&req, &ctrl).is_none() {
                return send_json(
                    req,
                    401,
                    &json!({"detail": "Could not validate credentials"}),
                );
            }

            let body = read_body(&mut req)?;
            let Ok(update) = serde_json::from_slice::<SystemUpdate>(&body) else {
                return send_json(req, 422, &json!({"detail": "Unprocessable Entity"}));
            };

            let Ok(mut guard) = ctrl.lock() else {
                return send_json(req, 500, &json!({"detail": "Internal lock error"}));
            };

            let Some(sys) = guard
                .systems
                .iter_mut()
                .find(|s| s.system_id == update.system_id)
            else {
                return send_json(req, 404, &json!({"detail": "System not found"}));
            };

            if let Some(sensor) = update.sensor {
                if sensor.url == sys.sensor.url {
                    let old_adj = sys.sensor.adjustment.unwrap_or(0.0);
                    let new_adj = sensor.adjustment.unwrap_or(0.0);
                    if let Some(t) = sys.temperature {
                        let raw = t - old_adj;
                        sys.temperature = Some(((raw + new_adj) * 10.0).round() / 10.0);
                    }
                } else {
                    sys.temperature_expiry = None;
                }
                sys.sensor = sensor;
            }
            if let Some(periods) = update.periods {
                sys.periods = periods;
            }
            if let Some(program) = update.program {
                sys.program = program;
            }

            if let Err(e) = guard.save_to_nvs() {
                warn!("Failed to save system update to NVS: {e:?}");
            }
            guard.evaluate_and_actuate_zone(&update.system_id);

            send_json(req, 200, &json!({}))
        })?;
    }

    // GET /api/v3/systems/{system_id}/
    {
        let ctrl = controller.clone();
        server.fn_handler("/api/v3/systems/*", Method::Get, move |req| {
            let Some(system_id) = extract_single_param(req.uri(), "/api/v3/systems/").map(str::to_string) else {
                return send_json(req, 404, &json!({"detail": "System not found"}));
            };

            let wall = time::now();
            let now_epoch = time::now_epoch();

            let Ok(mut guard) = ctrl.lock() else {
                return send_json(req, 500, &json!({"detail": "Internal lock error"}));
            };

            let Some(sys) = guard.systems.iter_mut().find(|s| s.system_id == system_id) else {
                return send_json(req, 404, &json!({"detail": "System not found"}));
            };

            let out = sys.as_system_out(wall.as_ref(), now_epoch);
            send_json(req, 200, &out)
        })?;
    }

    // GET /api/v3/temperature/{system_id}/
    {
        let ctrl = controller.clone();
        server.fn_handler("/api/v3/temperature/*", Method::Get, move |req| {
            let Some(system_id) =
                extract_single_param(req.uri(), "/api/v3/temperature/").map(str::to_string)
            else {
                return send_json(req, 404, &json!({"detail": "System not found"}));
            };

            let now_epoch = time::now_epoch().unwrap_or(0.0);

            // Fast path: return cached temperature if still valid (or if recently polled)
            let poll_target = {
                let Ok(guard) = ctrl.lock() else {
                    return send_json(req, 500, &json!({"detail": "Internal lock error"}));
                };
                let Some(sys) = guard.systems.iter().find(|s| s.system_id == system_id) else {
                    return send_json(req, 404, &json!({"detail": "System not found"}));
                };

                if let Some(temp) = sys.valid_cached_temperature(now_epoch) {
                    return send_json(req, 200, &json!({"temperature": temp}));
                }
                if sys.disabled {
                    if let Some(temp) = sys.temperature {
                        return send_json(req, 200, &json!({"temperature": temp}));
                    }
                    return send_json(req, 502, &json!({"detail": "Sensor temporarily disabled"}));
                }
                (sys.sensor.url.clone(), sys.temperature)
            };

            // Slow path (cache miss): poll sensor outside lock
            match fetch_sensor_temperature(&poll_target.0) {
                Ok(raw_temp) => {
                    let Ok(mut guard) = ctrl.lock() else {
                        return send_json(req, 500, &json!({"detail": "Internal lock error"}));
                    };
                    let rounded = if let Some(sys) =
                        guard.systems.iter_mut().find(|s| s.system_id == system_id)
                    {
                        sys.update_temperature(raw_temp, now_epoch);
                        sys.temperature.unwrap_or(raw_temp)
                    } else {
                        raw_temp
                    };
                    send_json(req, 200, &json!({"temperature": rounded}))
                }
                Err(e) => {
                    if let Some(last_known) = poll_target.1 {
                        send_json(req, 200, &json!({"temperature": last_known}))
                    } else {
                        send_json(req, 502, &json!({"detail": e.to_string()}))
                    }
                }
            }
        })?;
    }

    // GET /api/v3/target/{system_id}/
    {
        let ctrl = controller.clone();
        server.fn_handler("/api/v3/target/*", Method::Get, move |req| {
            let Some(system_id) =
                extract_single_param(req.uri(), "/api/v3/target/").map(str::to_string)
            else {
                return send_json(req, 404, &json!({"detail": "System not found"}));
            };

            let wall = time::now();
            let now_epoch = time::now_epoch();

            let Ok(mut guard) = ctrl.lock() else {
                return send_json(req, 500, &json!({"detail": "Internal lock error"}));
            };

            let Some(sys) = guard.systems.iter_mut().find(|s| s.system_id == system_id) else {
                return send_json(req, 404, &json!({"detail": "System not found"}));
            };

            if let Some(epoch) = now_epoch {
                sys.clear_expired_overrides(epoch);
            }

            let current_target = wall
                .as_ref()
                .map(|w| sys.current_target(w))
                .unwrap_or(DEFAULT_MINIMUM_TARGET);

            send_json(
                req,
                200,
                &json!({
                    "current_target": current_target,
                    "relay_on": sys.relay_on
                }),
            )
        })?;
    }

    // POST /api/v3/periods/{system_id}/
    {
        let ctrl = controller.clone();
        server.fn_handler("/api/v3/periods/*", Method::Post, move |mut req| {
            if check_bearer_auth(&req, &ctrl).is_none() {
                return send_json(
                    req,
                    401,
                    &json!({"detail": "Could not validate credentials"}),
                );
            }

            let Some(system_id) =
                extract_single_param(req.uri(), "/api/v3/periods/").map(str::to_string)
            else {
                return send_json(req, 404, &json!({"detail": "System not found"}));
            };

            let body = read_body(&mut req)?;
            let Ok(parsed) = serde_json::from_slice::<PeriodsBody>(&body) else {
                return send_json(req, 422, &json!({"detail": "Invalid periods payload"}));
            };

            let wall = time::now();
            let now_epoch = time::now_epoch();

            let Ok(mut guard) = ctrl.lock() else {
                return send_json(req, 500, &json!({"detail": "Internal lock error"}));
            };

            let out = {
                let Some(sys) = guard.systems.iter_mut().find(|s| s.system_id == system_id) else {
                    return send_json(req, 404, &json!({"detail": "System not found"}));
                };
                sys.periods = parsed.periods;
                sys.as_system_out(wall.as_ref(), now_epoch)
            };

            if let Err(e) = guard.save_to_nvs() {
                warn!("Failed to persist updated periods to NVS: {e:?}");
            }
            guard.evaluate_and_actuate_zone(&system_id);
            info!("API: updated schedule periods for '{system_id}'");

            send_json(req, 200, &out)
        })?;
    }

    // POST /api/v3/advance/{system_id}/
    {
        let ctrl = controller.clone();
        server.fn_handler("/api/v3/advance/*", Method::Post, move |mut req| {
            if check_bearer_auth(&req, &ctrl).is_none() {
                return send_json(
                    req,
                    401,
                    &json!({"detail": "Could not validate credentials"}),
                );
            }

            let Some(system_id) =
                extract_single_param(req.uri(), "/api/v3/advance/").map(str::to_string)
            else {
                return send_json(req, 404, &json!({"detail": "System not found"}));
            };

            let body = read_body(&mut req)?;
            let Ok(parsed) = serde_json::from_slice::<AdvanceBody>(&body) else {
                return send_json(req, 422, &json!({"detail": "Invalid advance payload"}));
            };

            let wall = time::now();
            let now_epoch = time::now_epoch();

            let Ok(mut guard) = ctrl.lock() else {
                return send_json(req, 500, &json!({"detail": "Internal lock error"}));
            };

            let out = {
                let Some(sys) = guard.systems.iter_mut().find(|s| s.system_id == system_id) else {
                    return send_json(req, 404, &json!({"detail": "System not found"}));
                };
                sys.advance = Some(parsed.end_time);
                sys.as_system_out(wall.as_ref(), now_epoch)
            };

            if let Err(e) = guard.save_to_nvs() {
                warn!("Failed to persist advance override to NVS: {e:?}");
            }
            guard.evaluate_and_actuate_zone(&system_id);
            info!("API: enabled advance for '{system_id}' until {}", parsed.end_time);

            send_json(req, 200, &out)
        })?;
    }

    // POST /api/v3/boost/{system_id}/
    {
        let ctrl = controller.clone();
        server.fn_handler("/api/v3/boost/*", Method::Post, move |mut req| {
            if check_bearer_auth(&req, &ctrl).is_none() {
                return send_json(
                    req,
                    401,
                    &json!({"detail": "Could not validate credentials"}),
                );
            }

            let Some(system_id) =
                extract_single_param(req.uri(), "/api/v3/boost/").map(str::to_string)
            else {
                return send_json(req, 404, &json!({"detail": "System not found"}));
            };

            let body = read_body(&mut req)?;
            let Ok(parsed) = serde_json::from_slice::<AdvanceBody>(&body) else {
                return send_json(req, 422, &json!({"detail": "Invalid boost payload"}));
            };

            let wall = time::now();
            let now_epoch = time::now_epoch();

            let Ok(mut guard) = ctrl.lock() else {
                return send_json(req, 500, &json!({"detail": "Internal lock error"}));
            };

            let out = {
                let Some(sys) = guard.systems.iter_mut().find(|s| s.system_id == system_id) else {
                    return send_json(req, 404, &json!({"detail": "System not found"}));
                };
                sys.boost = Some(parsed.end_time);
                sys.as_system_out(wall.as_ref(), now_epoch)
            };

            if let Err(e) = guard.save_to_nvs() {
                warn!("Failed to persist boost override to NVS: {e:?}");
            }
            guard.evaluate_and_actuate_zone(&system_id);
            info!("API: enabled boost for '{system_id}' until {}", parsed.end_time);

            send_json(req, 200, &out)
        })?;
    }

    // POST /api/v3/cancel_all/{system_id}/
    {
        let ctrl = controller.clone();
        server.fn_handler("/api/v3/cancel_all/*", Method::Post, move |req| {
            if check_bearer_auth(&req, &ctrl).is_none() {
                return send_json(
                    req,
                    401,
                    &json!({"detail": "Could not validate credentials"}),
                );
            }

            let Some(system_id) =
                extract_single_param(req.uri(), "/api/v3/cancel_all/").map(str::to_string)
            else {
                return send_json(req, 404, &json!({"detail": "System not found"}));
            };

            let wall = time::now();
            let now_epoch = time::now_epoch();

            let Ok(mut guard) = ctrl.lock() else {
                return send_json(req, 500, &json!({"detail": "Internal lock error"}));
            };

            let out = {
                let Some(sys) = guard.systems.iter_mut().find(|s| s.system_id == system_id) else {
                    return send_json(req, 404, &json!({"detail": "System not found"}));
                };
                sys.boost = None;
                sys.advance = None;
                sys.as_system_out(wall.as_ref(), now_epoch)
            };

            if let Err(e) = guard.save_to_nvs() {
                warn!("Failed to persist cancel_all to NVS: {e:?}");
            }
            guard.evaluate_and_actuate_zone(&system_id);
            info!("API: cancelled overrides for '{system_id}'");

            send_json(req, 200, &out)
        })?;
    }

    // POST /api/v3/program/{system_id}/{on}/
    {
        let ctrl = controller.clone();
        server.fn_handler("/api/v3/program/*", Method::Post, move |req| {
            if check_bearer_auth(&req, &ctrl).is_none() {
                return send_json(
                    req,
                    401,
                    &json!({"detail": "Could not validate credentials"}),
                );
            }

            let Some((sys_raw, on_raw)) = extract_two_params(req.uri(), "/api/v3/program/") else {
                return send_json(req, 404, &json!({"detail": "NOT FOUND"}));
            };
            let system_id = sys_raw.to_string();
            let program_on = match on_raw {
                "on" => true,
                "off" => false,
                _ => return send_json(req, 404, &json!({"detail": "NOT FOUND"})),
            };

            let Ok(mut guard) = ctrl.lock() else {
                return send_json(req, 500, &json!({"detail": "Internal lock error"}));
            };

            {
                let Some(sys) = guard.systems.iter_mut().find(|s| s.system_id == system_id) else {
                    return send_json(req, 404, &json!({"detail": "System not found"}));
                };
                sys.program = program_on;
            }

            if let Err(e) = guard.save_to_nvs() {
                warn!("Failed to persist program toggle to NVS: {e:?}");
            }
            guard.evaluate_and_actuate_zone(&system_id);
            info!("API: set program={} for '{system_id}'", program_on);

            send_json(req, 200, &json!({"program_on": program_on}))
        })?;
    }

    // GET /api/v3/all_data/
    {
        let ctrl = controller.clone();
        server.fn_handler("/api/v3/all_data/", Method::Get, move |req| {
            let wall = time::now();
            let now_epoch = time::now_epoch();

            let Ok(mut guard) = ctrl.lock() else {
                return send_json(req, 500, &json!({"detail": "Internal lock error"}));
            };

            let mut data: Vec<_> = guard
                .systems
                .iter_mut()
                .map(|sys| {
                    if let Some(epoch) = now_epoch {
                        sys.clear_expired_overrides(epoch);
                    }
                    let target = wall
                        .as_ref()
                        .map(|w| sys.current_target(w))
                        .unwrap_or(DEFAULT_MINIMUM_TARGET);
                    json!({
                        "id": sys.system_id,
                        "temperature": sys.temperature,
                        "target": target,
                        "relay_on": sys.relay_on,
                    })
                })
                .collect();

            data.sort_by(|a, b| {
                let id_a = a.get("id").and_then(|v| v.as_str()).unwrap_or("");
                let id_b = b.get("id").and_then(|v| v.as_str()).unwrap_or("");
                id_b.cmp(id_a)
            });

            send_json(req, 200, &json!({"systems": data}))
        })?;
    }

    // GET /api/v3/status/ — Consolidated dashboard payload (all zones + diagnostics)
    {
        let ctrl = controller.clone();
        server.fn_handler("/api/v3/status/", Method::Get, move |req| {
            let wall = time::now();
            let now_epoch = time::now_epoch();

            let (systems, loop_enabled) = {
                let Ok(mut guard) = ctrl.lock() else {
                    return send_json(req, 500, &json!({"detail": "Internal lock error"}));
                };
                let out: Vec<_> = guard
                    .systems
                    .iter_mut()
                    .map(|s| s.as_system_out(wall.as_ref(), now_epoch))
                    .collect();
                (out, guard.loop_enabled)
            };

            let diagnostics = collect_diagnostics();

            send_json(
                req,
                200,
                &StatusPayload {
                    systems: &systems,
                    loop_enabled,
                    server_epoch: now_epoch,
                    diagnostics,
                },
            )
        })?;
    }

    // GET /api/v3/history/ — Historical 5-minute temperature & heating samples
    {
        let ctrl = controller.clone();
        server.fn_handler("/api/v3/history/", Method::Get, move |req| {
            let histories: Vec<(String, Vec<HistoryPoint>)> = {
                let Ok(guard) = ctrl.lock() else {
                    return send_json(req, 500, &json!({"detail": "Internal lock error"}));
                };
                guard
                    .systems
                    .iter()
                    .map(|s| (s.system_id.clone(), s.get_history()))
                    .collect()
            };

            let mut resp = req.into_response(200, None, CORS_HEADERS)?;
            resp.write_all(b"{")?;
            for (i, (system_id, points)) in histories.into_iter().enumerate() {
                if i > 0 {
                    resp.write_all(b",")?;
                }
                let key_json = serde_json::to_string(&system_id)?;
                resp.write_all(key_json.as_bytes())?;
                resp.write_all(b":")?;
                let points_json = serde_json::to_vec(&points)?;
                resp.write_all(&points_json)?;
            }
            resp.write_all(b"}")?;
            Ok(())
        })?;
    }

    // GET /api/v3/start_loop/
    {
        let ctrl = controller.clone();
        server.fn_handler("/api/v3/start_loop/", Method::Get, move |req| {
            if check_bearer_auth(&req, &ctrl).is_none() {
                return send_json(
                    req,
                    401,
                    &json!({"detail": "Could not validate credentials"}),
                );
            }

            if let Ok(mut guard) = ctrl.lock() {
                guard.loop_enabled = true;
                let ids: Vec<String> = guard.systems.iter().map(|s| s.system_id.clone()).collect();
                for id in ids {
                    guard.evaluate_and_actuate_zone(&id);
                }
            }
            info!("API: control loop enabled");
            send_json(req, 200, &json!({"detail": "all systems go!"}))
        })?;
    }

    // GET /api/v3/stop_loop/
    {
        let ctrl = controller.clone();
        server.fn_handler("/api/v3/stop_loop/", Method::Get, move |req| {
            if check_bearer_auth(&req, &ctrl).is_none() {
                return send_json(
                    req,
                    401,
                    &json!({"detail": "Could not validate credentials"}),
                );
            }

            if let Ok(mut guard) = ctrl.lock() {
                guard.loop_enabled = false;
                guard.switch_all_off();
            }
            warn!("API: control loop stopped — all relays switched OFF");
            send_json(req, 200, &json!({"detail": "stopped"}))
        })?;
    }

    // POST /api/v3/reboot_system/
    {
        let ctrl = controller.clone();
        server.fn_handler("/api/v3/reboot_system/", Method::Post, move |req| {
            if check_bearer_auth(&req, &ctrl).is_none() {
                return send_json(
                    req,
                    401,
                    &json!({"detail": "Could not validate credentials"}),
                );
            }
            send_json(req, 410, &json!({"detail": "API-triggered reboot is disabled"}))
        })?;
    }

    // POST /api/v3/ota/ — Over-the-Air firmware update (streams raw espflash app image into standby OTA slot)
    {
        let ctrl = controller.clone();
        server.fn_handler("/api/v3/ota/", Method::Post, move |mut req| {
            if check_bearer_auth(&req, &ctrl).is_none() {
                return send_json(
                    req,
                    401,
                    &json!({"detail": "Could not validate credentials"}),
                );
            }

            // Fail-safe: pause control loop and switch all relays OFF before writing flash
            if let Ok(mut guard) = ctrl.lock() {
                guard.loop_enabled = false;
                guard.switch_all_off();
                if let Err(e) = guard.save_history_to_nvs() {
                    warn!("Failed to save history to NVS before OTA: {e:?}");
                }
            }

            info!("OTA: initiating firmware update...");
            let mut ota = match EspOta::new() {
                Ok(o) => o,
                Err(e) => {
                    error!("OTA: EspOta::new failed: {e:?}");
                    return send_json(req, 500, &json!({"detail": format!("OTA init failed: {e}")}));
                }
            };

            let mut update = match ota.initiate_update() {
                Ok(u) => u,
                Err(e) => {
                    error!("OTA: initiate_update failed: {e:?}");
                    return send_json(
                        req,
                        500,
                        &json!({"detail": format!("OTA start failed: {e}")}),
                    );
                }
            };

            let mut buf = [0u8; 4096];
            let mut total_bytes: usize = 0;

            loop {
                match req.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        if let Err(e) = update.write(&buf[..n]) {
                            error!("OTA: flash write failed after {total_bytes} bytes: {e:?}");
                            let _ = update.abort();
                            return send_json(
                                req,
                                500,
                                &json!({"detail": format!("OTA write failed: {e}")}),
                            );
                        }
                        total_bytes += n;
                    }
                    Err(e) => {
                        error!("OTA: network read error after {total_bytes} bytes: {e:?}");
                        let _ = update.abort();
                        return send_json(
                            req,
                            400,
                            &json!({"detail": format!("OTA read failed: {e}")}),
                        );
                    }
                }
            }

            if total_bytes < 65_536 {
                warn!("OTA: image too small ({total_bytes} bytes), aborting");
                let _ = update.abort();
                return send_json(req, 400, &json!({"detail": "Image too small"}));
            }

            if let Err(e) = update.complete() {
                error!("OTA: image validation / completion failed: {e:?}");
                return send_json(
                    req,
                    400,
                    &json!({"detail": format!("OTA validation failed: {e}")}),
                );
            }

            info!("OTA: successfully wrote {total_bytes} bytes! Rebooting into new firmware...");
            let res = send_json(
                req,
                200,
                &json!({
                    "detail": "OTA update complete, rebooting",
                    "bytes": total_bytes
                }),
            );

            std::thread::spawn(|| {
                std::thread::sleep(std::time::Duration::from_millis(600));
                unsafe {
                    esp_idf_svc::sys::esp_restart();
                }
            });

            res
        })?;
    }

    // POST /api/v3/receive/{sensor_id}/
    {
        let ctrl = controller.clone();
        server.fn_handler("/api/v3/receive/*", Method::Post, move |mut req| {
            let sensor_token = req
                .header("X-Sensor-Token")
                .or_else(|| req.header("x-sensor-token"))
                .map(|s| s.to_string());

            {
                let Ok(guard) = ctrl.lock() else {
                    return send_json(req, 500, &json!({"detail": "Internal lock error"}));
                };
                if let Err((status, detail)) = guard.verify_sensor_token(sensor_token.as_deref()) {
                    return send_json(req, status, &json!({"detail": detail}));
                }
            }

            let Some(sensor_id) =
                extract_single_param(req.uri(), "/api/v3/receive/").map(str::to_string)
            else {
                return send_json(req, 404, &json!({"detail": "System not found"}));
            };

            let body = read_body(&mut req)?;
            let Ok(parsed) = serde_json::from_slice::<SensorReceiveBody>(&body) else {
                return send_json(req, 422, &json!({"detail": "Invalid sensor payload"}));
            };

            let now_epoch = time::now_epoch().unwrap_or(0.0);

            let Ok(mut guard) = ctrl.lock() else {
                return send_json(req, 500, &json!({"detail": "Internal lock error"}));
            };

            let Some(sys) = guard.systems.iter_mut().find(|s| s.system_id == sensor_id) else {
                return send_json(req, 404, &json!({"detail": "System not found"}));
            };

            sys.update_temperature(parsed.temperature, now_epoch);
            guard.evaluate_and_actuate_zone(&sensor_id);

            send_json(req, 200, &json!({}))
        })?;
    }

    // -------------------------------------------------------------------------
    // Frontend Static Assets & SPA Fallback (`GET /` and `GET /*`)
    // -------------------------------------------------------------------------
    server.fn_handler("/", Method::Get, serve_frontend)?;
    server.fn_handler("/*", Method::Get, serve_frontend)?;

    info!("HTTP server listening on http://0.0.0.0:80");
    Ok(server)
}

fn serve_frontend(req: Request<&mut EspHttpConnection<'_>>) -> Result<()> {
    let path = strip_query(req.uri());

    match path {
        "/" | "/index.html" | "/login" | "/dashboard" | "/settings" | "/settings/periods" => {
            send_bytes(
                req,
                200,
                &[
                    ("Content-Type", "text/html; charset=utf-8"),
                    ("Cache-Control", "no-cache"),
                ],
                INDEX_HTML,
            )
        }
        "/assets/index.js" => send_bytes(
            req,
            200,
            &[
                ("Content-Type", "application/javascript; charset=utf-8"),
                ("Content-Encoding", "gzip"),
                ("Cache-Control", "no-cache"),
            ],
            INDEX_JS_GZ,
        ),
        "/assets/index.css" => send_bytes(
            req,
            200,
            &[
                ("Content-Type", "text/css; charset=utf-8"),
                ("Content-Encoding", "gzip"),
                ("Cache-Control", "no-cache"),
            ],
            INDEX_CSS_GZ,
        ),
        "/assets/flame-Bd7yHd1u.png" | "/flame-192.png" => send_bytes(
            req,
            200,
            &[
                ("Content-Type", "image/png"),
                ("Cache-Control", "public, max-age=86400"),
            ],
            FLAME_192_PNG,
        ),
        "/flame-512.png" | "/favicon.ico" => send_bytes(
            req,
            200,
            &[
                ("Content-Type", "image/png"),
                ("Cache-Control", "public, max-age=86400"),
            ],
            FLAME_512_PNG,
        ),
        "/manifest.webmanifest" => send_bytes(
            req,
            200,
            &[("Content-Type", "application/manifest+json")],
            MANIFEST_JSON,
        ),
        "/registerSW.js" => send_bytes(
            req,
            200,
            &[("Content-Type", "application/javascript; charset=utf-8")],
            REGISTER_SW_JS,
        ),
        "/sw.js" => send_bytes(
            req,
            200,
            &[
                ("Content-Type", "application/javascript; charset=utf-8"),
                ("Cache-Control", "no-cache"),
            ],
            SW_JS,
        ),
        other if other.starts_with("/api/") => {
            send_json(req, 404, &json!({"detail": "Not Found"}))
        }
        _ => send_bytes(
            req,
            200,
            &[
                ("Content-Type", "text/html; charset=utf-8"),
                ("Cache-Control", "no-cache"),
            ],
            INDEX_HTML,
        ),
    }
}

fn read_body(req: &mut Request<&mut EspHttpConnection<'_>>) -> Result<Vec<u8>> {
    let mut body = Vec::new();
    let mut chunk = [0u8; 1024];
    loop {
        let n = req.read(&mut chunk)?;
        if n == 0 {
            break;
        }
        if body.len() + n > MAX_BODY_LEN {
            anyhow::bail!("Request body exceeds maximum size ({MAX_BODY_LEN} bytes)");
        }
        body.extend_from_slice(&chunk[..n]);
    }
    Ok(body)
}

fn send_json<T: Serialize>(
    req: Request<&mut EspHttpConnection<'_>>,
    status: u16,
    value: &T,
) -> Result<()> {
    let payload = serde_json::to_vec(value)?;
    let mut resp = req.into_response(status, None, CORS_HEADERS)?;
    resp.write_all(&payload)?;
    Ok(())
}

fn send_bytes(
    req: Request<&mut EspHttpConnection<'_>>,
    status: u16,
    headers: &[(&str, &str)],
    bytes: &[u8],
) -> Result<()> {
    let mut resp = req.into_response(status, None, headers)?;
    resp.write_all(bytes)?;
    Ok(())
}

fn strip_query(uri: &str) -> &str {
    uri.split('?').next().unwrap_or(uri)
}

fn extract_single_param<'a>(uri: &'a str, prefix: &str) -> Option<&'a str> {
    let path = strip_query(uri);
    let rest = path.strip_prefix(prefix)?.trim_end_matches('/');
    if rest.is_empty() || rest.contains('/') {
        None
    } else {
        Some(rest)
    }
}

fn extract_two_params<'a>(uri: &'a str, prefix: &str) -> Option<(&'a str, &'a str)> {
    let path = strip_query(uri);
    let rest = path.strip_prefix(prefix)?.trim_end_matches('/');
    let (first, second) = rest.split_once('/')?;
    if first.is_empty() || second.is_empty() || second.contains('/') {
        None
    } else {
        Some((first, second))
    }
}

fn check_bearer_auth(
    req: &Request<&mut EspHttpConnection<'_>>,
    ctrl: &SharedController,
) -> Option<String> {
    let header = req
        .header("Authorization")
        .or_else(|| req.header("authorization"))?;
    let token = header
        .strip_prefix("Bearer ")
        .or_else(|| header.strip_prefix("bearer "))?
        .trim();
    let guard = ctrl.lock().ok()?;
    if guard.verify_token(token) {
        Some(token.to_string())
    } else {
        None
    }
}

/// Parses `username` and `password` from `multipart/form-data`, `application/x-www-form-urlencoded`,
/// or `application/json` request bodies.
fn parse_login_credentials(content_type: Option<&str>, body: &[u8]) -> Option<(String, String)> {
    let text = core::str::from_utf8(body).ok()?;
    let ct = content_type.unwrap_or("").to_ascii_lowercase();

    // 1. JSON body: {"username": "...", "password": "..."}
    if ct.contains("application/json") || text.trim_start().starts_with('{') {
        #[derive(serde::Deserialize)]
        struct LoginJson {
            username: String,
            password: String,
        }
        if let Ok(parsed) = serde_json::from_slice::<LoginJson>(body) {
            return Some((parsed.username, parsed.password));
        }
    }

    // 2. multipart/form-data (used by browser `new FormData()` in AuthContext.tsx)
    if ct.contains("multipart/form-data") || text.contains("Content-Disposition: form-data") {
        let mut username = None;
        let mut password = None;

        for part in text.split("Content-Disposition: form-data;") {
            let Some((header_block, rest)) = part.split_once("\r\n\r\n").or_else(|| part.split_once("\n\n")) else {
                continue;
            };
            // Value ends right before the next `\r\n--` boundary marker
            let value = rest
                .split("\r\n--")
                .next()
                .unwrap_or(rest)
                .split("\n--")
                .next()
                .unwrap_or(rest)
                .trim_end_matches('\r')
                .trim_end_matches('\n');

            if header_block.contains("name=\"username\"") {
                username = Some(value.to_string());
            } else if header_block.contains("name=\"password\"") {
                password = Some(value.to_string());
            }
        }

        if let (Some(u), Some(p)) = (username, password) {
            return Some((u, p));
        }
    }

    // 3. application/x-www-form-urlencoded (e.g., `username=foo&password=bar`)
    let mut username = None;
    let mut password = None;
    for pair in text.trim().split('&') {
        if let Some((k, v)) = pair.split_once('=') {
            match k.trim() {
                "username" => username = Some(url_decode(v)),
                "password" => password = Some(url_decode(v)),
                _ => {}
            }
        }
    }

    match (username, password) {
        (Some(u), Some(p)) => Some((u, p)),
        _ => None,
    }
}

fn url_decode(input: &str) -> String {
    let mut out = Vec::with_capacity(input.len());
    let bytes = input.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b'%' if i + 2 < bytes.len() => {
                if let (Some(hi), Some(lo)) = (hex_val(bytes[i + 1]), hex_val(bytes[i + 2])) {
                    out.push((hi << 4) | lo);
                    i += 3;
                } else {
                    out.push(bytes[i]);
                    i += 1;
                }
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

fn collect_diagnostics() -> DiagnosticsPayload {
    let uptime_secs = unsafe { (esp_idf_svc::sys::esp_timer_get_time() / 1_000_000).max(0) as u64 };
    let free_heap_bytes = unsafe { esp_idf_svc::sys::esp_get_free_heap_size() };
    let min_free_heap_bytes = unsafe { esp_idf_svc::sys::esp_get_minimum_free_heap_size() };

    let wifi_rssi = unsafe {
        let mut ap_info: esp_idf_svc::sys::wifi_ap_record_t = core::mem::zeroed();
        if esp_idf_svc::sys::esp_wifi_sta_get_ap_info(&mut ap_info) == esp_idf_svc::sys::ESP_OK {
            Some(ap_info.rssi)
        } else {
            None
        }
    };

    let ota_slot = EspOta::new()
        .ok()
        .and_then(|ota| ota.get_running_slot().ok().map(|s| s.label.to_string()))
        .unwrap_or_else(|| "unknown".to_string());

    DiagnosticsPayload {
        uptime_secs,
        free_heap_bytes,
        min_free_heap_bytes,
        wifi_rssi,
        ota_slot,
        firmware_version: env!("CARGO_PKG_VERSION"),
    }
}
