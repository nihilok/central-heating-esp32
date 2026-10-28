<script lang="ts">
  import { untrack } from 'svelte';
  import { formatUptime, setControlLoop, updateSystemConfig } from './api';
  import { geocodeLocation, saveWeatherLocation } from './weather';
  import type {
    Diagnostics,
    HeatingSystem,
    ThemeMode,
    WeatherLocation,
  } from './types';

  interface Props {
    systems: HeatingSystem[];
    loopEnabled: boolean;
    diagnostics: Diagnostics | null;
    weatherLocation: WeatherLocation;
    themeMode: ThemeMode;
    onThemeChange: (mode: ThemeMode) => void;
    onClose: () => void;
    onUpdated: () => Promise<void>;
    onWeatherLocationChanged: (loc: WeatherLocation) => void;
    onLogout: () => void;
  }

  let {
    systems,
    loopEnabled,
    diagnostics,
    weatherLocation,
    themeMode,
    onThemeChange,
    onClose,
    onUpdated,
    onWeatherLocationChanged,
    onLogout,
  }: Props = $props();

  // Weather location state
  let locationInput = $state(untrack(() => weatherLocation.query));
  let locationStatus = $state<string | null>(null);
  let lookingUpLocation = $state(false);

  // Per-zone sensor drafts
  let sensorDrafts = $state(
    untrack(() =>
      systems.map((s) => ({
        system_id: s.system_id,
        url: s.sensor.url,
        adjustment: s.sensor.adjustment ?? 0,
      })),
    ),
  );
  let savingSensorId = $state<string | null>(null);
  let sensorMsg = $state<string | null>(null);
  let loopBusy = $state(false);

  async function handleLookupLocation(e: Event) {
    e.preventDefault();
    lookingUpLocation = true;
    locationStatus = null;
    try {
      const resolved = await geocodeLocation(locationInput);
      saveWeatherLocation(resolved);
      onWeatherLocationChanged(resolved);
      locationStatus = `Saved: ${resolved.name}`;
    } catch (err) {
      locationStatus = err instanceof Error ? err.message : 'Lookup failed';
    } finally {
      lookingUpLocation = false;
    }
  }

  async function handleSaveSensor(draft: {
    system_id: string;
    url: string;
    adjustment: number;
  }) {
    savingSensorId = draft.system_id;
    sensorMsg = null;
    try {
      const roundedAdj = Math.round(draft.adjustment * 10) / 10;
      await updateSystemConfig(draft.system_id, {
        url: draft.url.trim(),
        adjustment: roundedAdj === 0 ? null : roundedAdj,
      });
      await onUpdated();
      sensorMsg = `Saved ${draft.system_id} sensor settings to NVS`;
    } catch (err) {
      sensorMsg = err instanceof Error ? err.message : 'Failed to save sensor';
    } finally {
      savingSensorId = null;
    }
  }

  async function handleToggleLoop() {
    loopBusy = true;
    try {
      await setControlLoop(!loopEnabled);
      await onUpdated();
    } finally {
      loopBusy = false;
    }
  }
</script>

<!-- Backdrop -->
<div
  class="fixed inset-0 z-50 flex items-end sm:items-center justify-center bg-slate-950/60 backdrop-blur-xs p-0 sm:p-4"
  role="dialog"
  aria-modal="true"
>
  <div
    class="w-full max-w-lg max-h-[90vh] flex flex-col rounded-t-3xl sm:rounded-2xl bg-white dark:bg-slate-900 border border-slate-200 dark:border-slate-800 shadow-2xl overflow-hidden"
  >
    <!-- Header -->
    <div class="flex items-center justify-between px-5 py-4 border-b border-slate-100 dark:border-slate-800">
      <div>
        <h3 class="text-base font-bold text-slate-900 dark:text-white">
          Settings & Diagnostics
        </h3>
        <p class="text-xs text-slate-500 dark:text-slate-400">
          Weather location, sensor calibration & ESP32 system status
        </p>
      </div>
      <button
        type="button"
        onclick={onClose}
        aria-label="Close settings modal"
        class="rounded-lg p-1.5 text-slate-400 hover:text-slate-600 dark:hover:text-slate-200 hover:bg-slate-100 dark:hover:bg-slate-800 cursor-pointer"
      >
        <svg class="w-5 h-5" viewBox="0 0 20 20" fill="currentColor">
          <path
            d="M6.28 5.22a.75.75 0 00-1.06 1.06L8.94 10l-3.72 3.72a.75.75 0 101.06 1.06L10 11.06l3.72 3.72a.75.75 0 101.06-1.06L11.06 10l3.72-3.72a.75.75 0 00-1.06-1.06L10 8.94 6.28 5.22z"
          />
        </svg>
      </button>
    </div>

    <!-- Body -->
    <div class="flex-1 overflow-y-auto p-5 space-y-6">
      <!-- 1. Appearance (System / Light / Dark) -->
      <section>
        <h4 class="text-xs font-bold uppercase tracking-wider text-slate-400 dark:text-slate-500 mb-2">
          Appearance
        </h4>
        <div class="grid grid-cols-3 gap-1.5 rounded-xl bg-slate-100 dark:bg-slate-800 p-1 border border-slate-200/80 dark:border-slate-700/70">
          {#each (['system', 'light', 'dark'] as const) as mode}
            <button
              type="button"
              onclick={() => onThemeChange(mode)}
              class={`rounded-lg py-2 text-xs font-semibold capitalize transition cursor-pointer ${
                themeMode === mode
                  ? 'bg-white dark:bg-slate-900 text-slate-900 dark:text-white shadow-xs'
                  : 'text-slate-500 dark:text-slate-400 hover:text-slate-800 dark:hover:text-slate-200'
              }`}
            >
              {mode}
            </button>
          {/each}
        </div>
      </section>

      <!-- 2. Outdoor Weather Location -->
      <section>
        <h4 class="text-xs font-bold uppercase tracking-wider text-slate-400 dark:text-slate-500 mb-2">
          Outdoor Weather Location
        </h4>
        <form onsubmit={handleLookupLocation} class="flex gap-2">
          <input
            type="text"
            bind:value={locationInput}
            placeholder="Town or UK Postcode (e.g. SW1A 1AA)"
            class="flex-1 rounded-xl bg-slate-50 dark:bg-slate-800 border border-slate-200 dark:border-slate-700 px-3.5 py-2 text-xs font-medium text-slate-900 dark:text-white focus:outline-none focus:ring-2 focus:ring-orange-500/50"
          />
          <button
            type="submit"
            disabled={lookingUpLocation}
            class="rounded-xl bg-slate-900 hover:bg-slate-800 dark:bg-slate-100 dark:hover:bg-white text-white dark:text-slate-900 px-4 py-2 text-xs font-semibold transition cursor-pointer disabled:opacity-50"
          >
            {lookingUpLocation ? 'Searching…' : 'Update'}
          </button>
        </form>
        <p class="mt-1.5 text-[11px] text-slate-500 dark:text-slate-400">
          {#if locationStatus}
            <span class="font-medium text-emerald-600 dark:text-emerald-400">{locationStatus}</span>
          {:else}
            Currently showing weather for <span class="font-semibold">{weatherLocation.name}</span>
          {/if}
        </p>
      </section>

      <!-- 2. Sensor Calibration & Endpoints -->
      <section>
        <div class="flex items-center justify-between mb-2">
          <h4 class="text-xs font-bold uppercase tracking-wider text-slate-400 dark:text-slate-500">
            Zone Sensors & Calibration
          </h4>
          {#if sensorMsg}
            <span class="text-[11px] font-medium text-emerald-600 dark:text-emerald-400">
              {sensorMsg}
            </span>
          {/if}
        </div>

        <div class="space-y-3">
          {#each sensorDrafts as draft (draft.system_id)}
            <div class="rounded-xl bg-slate-50 dark:bg-slate-800/60 border border-slate-200/80 dark:border-slate-700/70 p-3.5 space-y-2.5">
              <div class="flex items-center justify-between">
                <span class="text-xs font-bold capitalize text-slate-800 dark:text-slate-100">
                  {draft.system_id}
                </span>
                <button
                  type="button"
                  disabled={savingSensorId === draft.system_id}
                  onclick={() => handleSaveSensor(draft)}
                  class="rounded-lg bg-orange-600 hover:bg-orange-500 text-white px-3 py-1 text-[11px] font-semibold transition cursor-pointer disabled:opacity-50"
                >
                  {savingSensorId === draft.system_id ? 'Saving…' : 'Save to NVS'}
                </button>
              </div>

              <div class="grid grid-cols-1 sm:grid-cols-3 gap-2">
                <div class="sm:col-span-2">
                  <label class="block text-[10px] font-medium text-slate-400 mb-1">
                    Sensor HTTP Endpoint
                    <input
                      type="url"
                      bind:value={draft.url}
                      class="mt-1 w-full rounded-lg bg-white dark:bg-slate-900 border border-slate-200 dark:border-slate-700 px-2.5 py-1.5 text-xs font-mono text-slate-800 dark:text-slate-100"
                    />
                  </label>
                </div>

                <div>
                  <span class="block text-[10px] font-medium text-slate-400 mb-1">
                    Temp Offset (°C)
                  </span>
                  <div class="flex items-center justify-between rounded-lg bg-white dark:bg-slate-900 border border-slate-200 dark:border-slate-700 p-0.5">
                    <button
                      type="button"
                      onclick={() =>
                        (draft.adjustment = Math.round((draft.adjustment - 0.1) * 10) / 10)}
                      class="w-7 h-7 flex items-center justify-center rounded-md text-slate-600 dark:text-slate-300 hover:bg-slate-100 dark:hover:bg-slate-800 font-bold cursor-pointer"
                    >
                      −
                    </button>
                    <span class="text-xs font-bold tabular-nums text-slate-800 dark:text-slate-100">
                      {draft.adjustment > 0
                        ? `+${draft.adjustment.toFixed(1)}`
                        : draft.adjustment.toFixed(1)}°
                    </span>
                    <button
                      type="button"
                      onclick={() =>
                        (draft.adjustment = Math.round((draft.adjustment + 0.1) * 10) / 10)}
                      class="w-7 h-7 flex items-center justify-center rounded-md text-slate-600 dark:text-slate-300 hover:bg-slate-100 dark:hover:bg-slate-800 font-bold cursor-pointer"
                    >
                      +
                    </button>
                  </div>
                </div>
              </div>
            </div>
          {/each}
        </div>
      </section>

      <!-- 3. Master Control Loop & ESP32 Diagnostics -->
      <section>
        <h4 class="text-xs font-bold uppercase tracking-wider text-slate-400 dark:text-slate-500 mb-2">
          ESP32 Controller & Diagnostics
        </h4>

        <div class="rounded-xl bg-slate-50 dark:bg-slate-800/60 border border-slate-200/80 dark:border-slate-700/70 p-3.5 space-y-3">
          <div class="flex items-center justify-between">
            <div>
              <div class="text-xs font-bold text-slate-800 dark:text-slate-100">
                Background Heating Control Loop
              </div>
              <div class="text-[11px] text-slate-500 dark:text-slate-400">
                {loopEnabled
                  ? 'Actively polling sensors every 10s'
                  : 'STOPPED — all relays held Fail-Safe OFF'}
              </div>
            </div>
            <button
              type="button"
              disabled={loopBusy}
              onclick={handleToggleLoop}
              class={`rounded-xl px-3.5 py-1.5 text-xs font-semibold transition cursor-pointer ${
                loopEnabled
                  ? 'bg-red-500/15 text-red-700 dark:text-red-300 border border-red-500/30 hover:bg-red-500/25'
                  : 'bg-emerald-600 text-white hover:bg-emerald-500'
              }`}
            >
              {loopEnabled ? 'Stop Loop' : 'Start Loop'}
            </button>
          </div>

          {#if diagnostics}
            <div class="grid grid-cols-2 sm:grid-cols-4 gap-2 pt-2 border-t border-slate-200/70 dark:border-slate-700/60 text-center">
              <div class="rounded-lg bg-white dark:bg-slate-900 p-2 border border-slate-200/60 dark:border-slate-800">
                <div class="text-[10px] text-slate-400">Uptime</div>
                <div class="text-xs font-bold tabular-nums text-slate-800 dark:text-slate-100 mt-0.5">
                  {formatUptime(diagnostics.uptime_secs)}
                </div>
              </div>
              <div class="rounded-lg bg-white dark:bg-slate-900 p-2 border border-slate-200/60 dark:border-slate-800">
                <div class="text-[10px] text-slate-400">Wi-Fi RSSI</div>
                <div class="text-xs font-bold tabular-nums text-slate-800 dark:text-slate-100 mt-0.5">
                  {diagnostics.wifi_rssi !== null ? `${diagnostics.wifi_rssi} dBm` : '—'}
                </div>
              </div>
              <div class="rounded-lg bg-white dark:bg-slate-900 p-2 border border-slate-200/60 dark:border-slate-800">
                <div class="text-[10px] text-slate-400">Free Heap</div>
                <div class="text-xs font-bold tabular-nums text-slate-800 dark:text-slate-100 mt-0.5">
                  {Math.round(diagnostics.free_heap_bytes / 1024)} KB
                </div>
              </div>
              <div class="rounded-lg bg-white dark:bg-slate-900 p-2 border border-slate-200/60 dark:border-slate-800">
                <div class="text-[10px] text-slate-400">OTA Partition</div>
                <div class="text-xs font-mono font-bold text-slate-800 dark:text-slate-100 mt-0.5">
                  {diagnostics.ota_slot}
                </div>
              </div>
            </div>
          {/if}
        </div>
      </section>
    </div>

    <!-- Footer -->
    <div class="flex items-center justify-between px-5 py-3.5 bg-slate-50 dark:bg-slate-900/90 border-t border-slate-100 dark:border-slate-800">
      <button
        type="button"
        onclick={onLogout}
        class="rounded-xl px-3.5 py-2 text-xs font-semibold text-red-600 dark:text-red-400 hover:bg-red-500/10 cursor-pointer"
      >
        Sign Out
      </button>
      <button
        type="button"
        onclick={onClose}
        class="rounded-xl bg-slate-900 dark:bg-slate-100 text-white dark:text-slate-900 px-5 py-2 text-xs font-semibold cursor-pointer"
      >
        Done
      </button>
    </div>
  </div>
</div>
