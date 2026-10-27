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
TOKEN="$(python3 - "$HOST" <<'PY'
import json, sys, tomllib, urllib.request

host = sys.argv[1]
with open("cfg.toml", "rb") as f:
    cfg = tomllib.load(f)

pw = cfg.get("auth", {}).get("password") or cfg.get("wifi", {}).get("psk")
user = cfg.get("auth", {}).get("username") or "admin"

boundary = "----ESP32OTABoundary"
body = (
    f"--{boundary}\r\n"
    f'Content-Disposition: form-data; name="username"\r\n\r\n{user}\r\n'
    f"--{boundary}\r\n"
    f'Content-Disposition: form-data; name="password"\r\n\r\n{pw}\r\n'
    f"--{boundary}--\r\n"
).encode()

req = urllib.request.Request(
    f"http://{host}/token/",
    data=body,
    headers={"Content-Type": f"multipart/form-data; boundary={boundary}"},
    method="POST",
)
with urllib.request.urlopen(req, timeout=10) as resp:
    data = json.loads(resp.read())
    print(data["access_token"])
PY
)"

echo "==> Uploading OTA image ($(wc -c < "$IMAGE_PATH" | tr -d ' ') bytes) to http://${HOST}/api/v3/ota/ ..."
curl --fail -X POST \
  -H "Authorization: Bearer ${TOKEN}" \
  -H "Content-Type: application/octet-stream" \
  --data-binary "@${IMAGE_PATH}" \
  "http://${HOST}/api/v3/ota/"
echo ""

rm -f "$IMAGE_PATH"
echo "==> OTA complete! ESP32 is rebooting into the new slot."
