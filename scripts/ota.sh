#!/usr/bin/env bash
set -euo pipefail

# Over-the-Air (OTA) firmware deployment script for central-heating-esp32.
# Usage: ./scripts/ota.sh [ESP32_HOST]

HOST="${1:-192.168.1.168}"
IMAGE_PATH="/tmp/central-heating-esp32-ota.bin"

if [[ -f "$HOME/export-esp.sh" ]]; then
  # shellcheck disable=SC1091
  source "$HOME/export-esp.sh"
fi

if [[ -x "./scripts/build-ui.sh" ]]; then
  ./scripts/build-ui.sh
fi

echo "==> Building firmware..."
cargo build

echo "==> Exporting ESP-IDF OTA app binary..."
espflash save-image \
  --chip esp32 \
  --flash-size 4mb \
  --partition-table partitions.csv \
  target/xtensa-esp32-espidf/debug/central-heating-esp32 \
  "$IMAGE_PATH"

echo "==> Authenticating with http://${HOST}/token/ ..."
read -r AUTH_USER AUTH_PASS < <(python3 - <<'PY'
import tomllib

with open("cfg.toml", "rb") as f:
    cfg = tomllib.load(f)

pw = cfg.get("auth", {}).get("password") or cfg.get("wifi", {}).get("psk")
user = cfg.get("auth", {}).get("username") or "admin"
print(user, pw)
PY
)
TOKEN="$(curl -s --fail -X POST \
  -F "username=${AUTH_USER}" \
  -F "password=${AUTH_PASS}" \
  "http://${HOST}/token/" | python3 -c 'import json,sys; print(json.load(sys.stdin)["access_token"])')"

echo "==> Uploading OTA image ($(wc -c < "$IMAGE_PATH" | tr -d ' ') bytes) to http://${HOST}/api/v3/ota/ ..."
curl --fail -X POST \
  -H "Authorization: Bearer ${TOKEN}" \
  -H "Content-Type: application/octet-stream" \
  --data-binary "@${IMAGE_PATH}" \
  "http://${HOST}/api/v3/ota/"
echo ""

rm -f "$IMAGE_PATH"
echo "==> OTA complete! ESP32 is rebooting into the new slot."
