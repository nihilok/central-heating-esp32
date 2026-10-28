#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
UI_DIR="$REPO_ROOT/ui"
WEB_DIR="$REPO_ROOT/web"

echo "==> Building Svelte 5 frontend in $UI_DIR..."
cd "$UI_DIR"
npm run build

echo "==> Packaging compressed assets into $WEB_DIR..."
mkdir -p "$WEB_DIR/assets"
rm -f "$WEB_DIR/assets/"*.js.gz "$WEB_DIR/assets/"*.css.gz "$WEB_DIR/"workbox-*.js.gz

cp "$UI_DIR/dist/index.html" "$WEB_DIR/index.html"
gzip -9 -n -c "$UI_DIR/dist/assets/index.js" > "$WEB_DIR/assets/index.js.gz"
gzip -9 -n -c "$UI_DIR/dist/assets/index.css" > "$WEB_DIR/assets/index.css.gz"

# Self-unregistering service worker to evict any old Workbox cache from previous React build
cat > "$WEB_DIR/sw.js" <<'EOF'
self.addEventListener('install', () => {
  self.skipWaiting();
});
self.addEventListener('activate', (event) => {
  event.waitUntil(
    caches.keys()
      .then((keys) => Promise.all(keys.map((k) => caches.delete(k))))
      .then(() => self.registration.unregister())
      .then(() => self.clients.matchAll())
      .then((clients) => clients.forEach((client) => client.navigate(client.url)))
  );
});
EOF

ls -lh "$WEB_DIR/index.html" "$WEB_DIR/assets/index.js.gz" "$WEB_DIR/assets/index.css.gz"
