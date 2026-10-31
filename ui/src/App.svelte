<script lang="ts">
  import { onMount } from 'svelte';
  import ZoneCard from './lib/ZoneCard.svelte';
  import ScheduleModal from './lib/ScheduleModal.svelte';
  import SettingsModal from './lib/SettingsModal.svelte';
  import {
    checkToken,
    fetchHistory,
    fetchStatus,
    getSavedToken,
    login,
    setSavedToken,
  } from './lib/api';
  import {
    fetchOutdoorWeather,
    getSavedWeatherLocation,
  } from './lib/weather';
  import type {
    Diagnostics,
    HeatingSystem,
    HistoryPoint,
    ThemeMode,
    WeatherData,
    WeatherLocation,
  } from './lib/types';

  const THEME_KEY = 'heating_theme_mode';

  let authenticated = $state<boolean>(Boolean(getSavedToken()));
  let authChecking = $state<boolean>(true);
  let username = $state('');
  let password = $state('');
  let loginError = $state<string | null>(null);
  let loginBusy = $state(false);

  let systems = $state<HeatingSystem[]>([]);
  let historyData = $state<Record<string, HistoryPoint[]>>({});
  let loopEnabled = $state<boolean>(true);
  let diagnostics = $state<Diagnostics | null>(null);
  let loadingStatus = $state<boolean>(true);
  let pollError = $state<string | null>(null);
  let now = $state<Date>(new Date());

  // Modals
  let scheduleModalSystemId = $state<string | null>(null);
  let settingsOpen = $state<boolean>(false);

  // Weather
  let weatherLocation = $state<WeatherLocation>(getSavedWeatherLocation());
  let weather = $state<WeatherData | null>(null);

  // Theme (system / light / dark)
  let themeMode = $state<ThemeMode>(
    (localStorage.getItem(THEME_KEY) as ThemeMode) || 'system',
  );

  function applyTheme(mode: ThemeMode) {
    const prefersDark = window.matchMedia('(prefers-color-scheme: dark)').matches;
    const isDark = mode === 'dark' || (mode === 'system' && prefersDark);
    document.documentElement.classList.toggle('dark', isDark);
  }

  function setTheme(mode: ThemeMode) {
    themeMode = mode;
    localStorage.setItem(THEME_KEY, mode);
    applyTheme(mode);
  }

  async function refreshStatus() {
    try {
      const data = await fetchStatus();
      // Order downstairs first, upstairs second, preserving cached history
      systems = data.systems
        .map((s) => ({
          ...s,
          history: historyData[s.system_id] ?? [],
        }))
        .sort((a, b) => a.system_id.localeCompare(b.system_id));
      loopEnabled = data.loop_enabled;
      diagnostics = data.diagnostics;
      pollError = null;
    } catch (err) {
      if (err instanceof Error && err.message === 'UNAUTHORIZED') {
        authenticated = false;
      } else {
        pollError = err instanceof Error ? err.message : 'Connection lost';
      }
    } finally {
      loadingStatus = false;
    }
  }

  async function refreshHistory() {
    try {
      const data = await fetchHistory();
      historyData = data;
      systems = systems.map((s) => ({
        ...s,
        history: data[s.system_id] ?? s.history ?? [],
      }));
    } catch {
      // Non-fatal if offline
    }
  }

  async function refreshWeather(loc = weatherLocation) {
    try {
      weather = await fetchOutdoorWeather(loc);
    } catch {
      // Non-fatal if offline
    }
  }

  async function handleLogin(e: Event) {
    e.preventDefault();
    loginError = null;
    loginBusy = true;
    try {
      await login(username, password);
      authenticated = true;
      password = '';
      refreshHistory();
      await refreshStatus();
    } catch (err) {
      loginError = err instanceof Error ? err.message : 'Login failed';
    } finally {
      loginBusy = false;
    }
  }

  function handleLogout() {
    setSavedToken(null);
    authenticated = false;
    settingsOpen = false;
  }

  let activeScheduleSystem = $derived(
    scheduleModalSystemId
      ? systems.find((s) => s.system_id === scheduleModalSystemId) ?? null
      : null,
  );

  onMount(() => {
    applyTheme(themeMode);
    const mediaQuery = window.matchMedia('(prefers-color-scheme: dark)');
    const onSystemThemeChange = () => {
      if (themeMode === 'system') applyTheme('system');
    };
    mediaQuery.addEventListener('change', onSystemThemeChange);

    // Verify existing token & start polling
    (async () => {
      refreshWeather();
      refreshHistory();
      await refreshStatus();
      if (getSavedToken()) {
        const valid = await checkToken();
        authenticated = valid;
      }
      authChecking = false;
    })();

    const statusTimer = setInterval(() => {
      now = new Date();
      if (authenticated) {
        refreshStatus();
      }
    }, 5000);

    const historyTimer = setInterval(() => {
      if (authenticated) {
        refreshHistory();
      }
    }, 2 * 60 * 1000);

    const weatherTimer = setInterval(() => {
      refreshWeather();
    }, 10 * 60 * 1000);

    return () => {
      mediaQuery.removeEventListener('change', onSystemThemeChange);
      clearInterval(statusTimer);
      clearInterval(historyTimer);
      clearInterval(weatherTimer);
    };
  });
</script>

<div class="min-h-screen flex flex-col bg-slate-50 dark:bg-slate-950 text-slate-900 dark:text-slate-100">
  <!-- Sticky Top App Bar -->
  <header class="sticky top-0 z-30 bg-white/85 dark:bg-slate-900/85 backdrop-blur-md border-b border-slate-200/80 dark:border-slate-800">
    <div class="max-w-5xl mx-auto px-4 py-2.5 flex items-center justify-between gap-2">
      <!-- Brand Title -->
      <div class="flex items-center gap-2.5">
        <div class="w-8 h-8 rounded-xl bg-gradient-to-br from-amber-500 to-orange-600 flex items-center justify-center text-white shadow-xs">
          <svg class="w-4.5 h-4.5 fill-current" viewBox="0 0 24 24">
            <path
              d="M12 2c1.5 3.5 4.5 5.5 4.5 9.5 0 2.5-1.5 4.5-3.5 5.5.5-1.5.5-3-.5-4.5-1 2-2.5 3-2.5 5 0 1.2.5 2.2 1.2 2.9C8 19.8 5.5 17 5.5 13.2 5.5 8.5 9.5 5.5 12 2z"
            />
          </svg>
        </div>
        <div>
          <h1 class="text-sm sm:text-base font-bold tracking-tight leading-none">
            Central Heating
          </h1>
          <span class="text-[10px] font-medium text-slate-400 dark:text-slate-500">
            ESP32 Controller
          </span>
        </div>
      </div>

      <!-- Right cluster: Outdoor Weather Pill + Theme Toggle + Settings Button -->
      <div class="flex items-center gap-1.5 sm:gap-2">
        {#if weather}
          <button
            type="button"
            onclick={() => {
              if (authenticated) settingsOpen = true;
            }}
            title={`${weather.locationName}: ${weather.description} (High ${weather.high}° / Low ${weather.low}°)`}
            class="inline-flex items-center gap-1.5 rounded-full bg-slate-100 hover:bg-slate-200/70 dark:bg-slate-800 dark:hover:bg-slate-700/70 border border-slate-200/80 dark:border-slate-700/80 px-2.5 py-1 text-xs font-medium transition cursor-pointer"
          >
            <!-- Weather Icon -->
            {#if weather.icon === 'sun'}
              <span class="text-amber-500">☀</span>
            {:else if weather.icon === 'moon'}
              <svg class="w-3.5 h-3.5 text-indigo-400" viewBox="0 0 20 20" fill="currentColor">
                <path
                  fill-rule="evenodd"
                  d="M7.455 2.004a.75.75 0 01.26.77 7 7 0 009.958 7.967.75.75 0 011.067.853A8.5 8.5 0 116.647 1.921a.75.75 0 01.808.083z"
                  clip-rule="evenodd"
                />
              </svg>
            {:else if weather.icon === 'cloud-sun'}
              <span class="text-amber-400">⛅</span>
            {:else if weather.icon === 'cloud-moon'}
              <svg class="w-3.5 h-3.5 text-indigo-300" viewBox="0 0 20 20" fill="currentColor">
                <path
                  fill-rule="evenodd"
                  d="M7.455 2.004a.75.75 0 01.26.77 7 7 0 009.958 7.967.75.75 0 011.067.853A8.5 8.5 0 116.647 1.921a.75.75 0 01.808.083z"
                  clip-rule="evenodd"
                />
              </svg>
            {:else if weather.icon === 'rain'}
              <span class="text-sky-400">🌧</span>
            {:else if weather.icon === 'snow'}
              <span class="text-sky-300">❄</span>
            {:else if weather.icon === 'storm'}
              <span class="text-amber-500">⚡</span>
            {:else}
              <span class="text-slate-400">☁</span>
            {/if}

            <span class="font-bold tabular-nums text-slate-800 dark:text-slate-100">
              {weather.temperature.toFixed(1)}°C
            </span>
            <span class="hidden md:inline text-[11px] text-slate-500 dark:text-slate-400">
              {weather.locationName.split(',')[0]} · H:{weather.high.toFixed(0)}° L:{weather.low.toFixed(0)}°
            </span>
          </button>
        {/if}

        <!-- Settings Modal Trigger -->
        {#if authenticated}
          <button
            type="button"
            onclick={() => (settingsOpen = true)}
            aria-label="Open settings"
            class="rounded-xl bg-slate-100 hover:bg-slate-200/70 dark:bg-slate-800 dark:hover:bg-slate-700/70 border border-slate-200/80 dark:border-slate-700/80 p-1.5 text-slate-700 dark:text-slate-200 transition cursor-pointer"
          >
            <svg class="w-4 h-4" viewBox="0 0 20 20" fill="currentColor">
              <path
                fill-rule="evenodd"
                d="M7.84 1.804A1 1 0 018.82 1h2.36a1 1 0 01.98.804l.331 1.652a6.993 6.993 0 011.929 1.115l1.598-.54a1 1 0 011.186.447l1.18 2.044a1 1 0 01-.205 1.251l-1.267 1.113a7.047 7.047 0 010 2.228l1.267 1.113a1 1 0 01.206 1.25l-1.18 2.045a1 1 0 01-1.187.447l-1.598-.54a6.993 6.993 0 01-1.929 1.115l-.33 1.652a1 1 0 01-.98.804H8.82a1 1 0 01-.98-.804l-.331-1.652a6.993 6.993 0 01-1.929-1.115l-1.598.54a1 1 0 01-1.186-.447l-1.18-2.044a1 1 0 01.205-1.251l1.267-1.114a7.05 7.05 0 010-2.227L1.821 7.773a1 1 0 01-.206-1.25l1.18-2.045a1 1 0 011.187-.447l1.598.54A6.993 6.993 0 017.51 3.456l.33-1.652zM10 13a3 3 0 100-6 3 3 0 000 6z"
                clip-rule="evenodd"
              />
            </svg>
          </button>
        {/if}
      </div>
    </div>
  </header>

  <!-- Main Content -->
  <main class="flex-1 max-w-5xl w-full mx-auto px-3.5 sm:px-4 py-4 sm:py-6">
    {#if !authenticated && !authChecking}
      <!-- Login Card -->
      <div class="max-w-sm mx-auto mt-10 rounded-2xl bg-white dark:bg-slate-900 border border-slate-200 dark:border-slate-800 p-6 shadow-sm">
        <h2 class="text-lg font-bold text-slate-900 dark:text-white">
          Sign in to Central Heating
        </h2>
        <p class="text-xs text-slate-500 dark:text-slate-400 mt-1">
          Enter your controller credentials to manage schedules and overrides.
        </p>

        {#if loginError}
          <div class="mt-3 rounded-xl bg-red-500/10 border border-red-500/30 px-3 py-2 text-xs font-medium text-red-600 dark:text-red-400">
            {loginError}
          </div>
        {/if}

        <form onsubmit={handleLogin} class="mt-4 space-y-3">
          <div>
            <label class="block text-xs font-semibold text-slate-600 dark:text-slate-300 mb-1">
              Username
              <input
                type="text"
                required
                autocomplete="username"
                bind:value={username}
                class="mt-1 w-full rounded-xl bg-slate-50 dark:bg-slate-800 border border-slate-200 dark:border-slate-700 px-3.5 py-2.5 text-sm text-slate-900 dark:text-white focus:outline-none focus:ring-2 focus:ring-orange-500/50"
              />
            </label>
          </div>
          <div>
            <label class="block text-xs font-semibold text-slate-600 dark:text-slate-300 mb-1">
              Password
              <input
                type="password"
                required
                autocomplete="current-password"
                bind:value={password}
                class="mt-1 w-full rounded-xl bg-slate-50 dark:bg-slate-800 border border-slate-200 dark:border-slate-700 px-3.5 py-2.5 text-sm text-slate-900 dark:text-white focus:outline-none focus:ring-2 focus:ring-orange-500/50"
              />
            </label>
          </div>
          <button
            type="submit"
            disabled={loginBusy}
            class="w-full rounded-xl bg-orange-600 hover:bg-orange-500 text-white py-2.5 text-sm font-semibold shadow-xs transition cursor-pointer disabled:opacity-50"
          >
            {loginBusy ? 'Signing in…' : 'Sign In'}
          </button>
        </form>
      </div>
    {:else}
      <!-- Master Loop Paused Warning Banner -->
      {#if !loopEnabled}
        <div class="mb-4 rounded-xl bg-red-500/15 border border-red-500/30 px-4 py-3 flex items-center justify-between gap-3">
          <div class="text-xs font-semibold text-red-700 dark:text-red-300">
            Master control loop is currently STOPPED — all heating relays are held OFF.
          </div>
          <button
            type="button"
            onclick={() => (settingsOpen = true)}
            class="rounded-lg bg-red-600 text-white px-3 py-1.5 text-xs font-semibold cursor-pointer"
          >
            Manage
          </button>
        </div>
      {/if}

      {#if pollError}
        <div class="mb-4 rounded-xl bg-amber-500/15 border border-amber-500/30 px-4 py-2.5 text-xs font-medium text-amber-700 dark:text-amber-300">
          Reconnecting to ESP32 controller ({pollError})…
        </div>
      {/if}

      <!-- Mobile-first Weather Sub-banner (visible on small screens) -->
      {#if weather}
        <div class="md:hidden mb-3.5 flex items-center justify-between rounded-xl bg-white dark:bg-slate-900 border border-slate-200/80 dark:border-slate-800 px-3.5 py-2 text-xs">
          <div class="flex items-center gap-2">
            <span class="font-semibold text-slate-700 dark:text-slate-200">
              {weather.locationName}
            </span>
            <span class="text-slate-400">·</span>
            <span class="text-slate-500 dark:text-slate-400">{weather.description}</span>
          </div>
          <div class="tabular-nums font-medium text-slate-500 dark:text-slate-400">
            H:{weather.high.toFixed(0)}° L:{weather.low.toFixed(0)}°
          </div>
        </div>
      {/if}

      <!-- Zone Cards Grid: Stacked on mobile, 2 columns on desktop -->
      {#if loadingStatus && systems.length === 0}
        <div class="grid grid-cols-1 lg:grid-cols-2 gap-4">
          {#each [1, 2] as _}
            <div class="h-96 rounded-2xl bg-white dark:bg-slate-900 border border-slate-200 dark:border-slate-800 animate-pulse p-5"></div>
          {/each}
        </div>
      {:else}
        <div class="grid grid-cols-1 lg:grid-cols-2 gap-4">
          {#each systems as system (system.system_id)}
            <ZoneCard
              {system}
              {now}
              onUpdated={refreshStatus}
              onOpenSchedule={(sys) => (scheduleModalSystemId = sys.system_id)}
            />
          {/each}
        </div>
      {/if}
    {/if}
  </main>

  <!-- Schedule Editor Modal -->
  {#if activeScheduleSystem}
    <ScheduleModal
      system={activeScheduleSystem}
      onClose={() => (scheduleModalSystemId = null)}
      onSaved={refreshStatus}
    />
  {/if}

  <!-- Settings & Diagnostics Modal -->
  {#if settingsOpen}
    <SettingsModal
      {systems}
      {loopEnabled}
      {diagnostics}
      {weatherLocation}
      {themeMode}
      onThemeChange={setTheme}
      onClose={() => (settingsOpen = false)}
      onUpdated={refreshStatus}
      onWeatherLocationChanged={(loc) => {
        weatherLocation = loc;
        refreshWeather(loc);
      }}
      onLogout={handleLogout}
    />
  {/if}
</div>
