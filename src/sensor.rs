//! HTTP client for polling remote BME280 temperature sensor nodes.

use std::time::Duration;

use anyhow::{bail, Context, Result};
use esp_idf_svc::http::client::{Configuration as HttpConfiguration, EspHttpConnection};
use esp_idf_svc::http::Method;
use serde::Deserialize;

const SENSOR_HTTP_TIMEOUT: Duration = Duration::from_secs(4);

#[derive(Debug, Deserialize)]
struct SensorResponse {
    temperature: f64,
}

/// Fetches the current temperature (°C) from a remote sensor node HTTP endpoint.
///
/// Expects a JSON payload containing `{"temperature": <number>, ...}`.
pub fn fetch_sensor_temperature(url: &str) -> Result<f64> {
    let config = HttpConfiguration {
        timeout: Some(SENSOR_HTTP_TIMEOUT),
        buffer_size: Some(512),
        buffer_size_tx: Some(256),
        ..Default::default()
    };

    let mut conn = EspHttpConnection::new(&config)
        .context("Failed to initialise EspHttpConnection")?;

    let headers = [("Accept", "application/json")];
    conn.initiate_request(Method::Get, url, &headers)
        .with_context(|| format!("Failed to initiate GET request to {url}"))?;

    conn.initiate_response()
        .with_context(|| format!("Failed waiting for response from {url}"))?;

    let status = conn.status();
    if !(200..300).contains(&status) {
        bail!("Sensor at {url} returned HTTP status {status}");
    }

    let mut buf = [0u8; 512];
    let mut total_read = 0usize;
    while total_read < buf.len() {
        let n = conn
            .read(&mut buf[total_read..])
            .with_context(|| format!("Error reading response body from {url}"))?;
        if n == 0 {
            break;
        }
        total_read += n;
    }

    let body = core::str::from_utf8(&buf[..total_read])
        .context("Sensor response is not valid UTF-8")?;

    let parsed: SensorResponse = serde_json::from_str(body.trim())
        .with_context(|| format!("Invalid sensor JSON from {url}: {body:?}"))?;

    let temp = parsed.temperature;
    if !temp.is_finite() || !(-40.0..=85.0).contains(&temp) {
        bail!("Out-of-range sensor temperature reading ({temp}°C) from {url}");
    }

    Ok(temp)
}
