<script lang="ts">
  import { untrack } from 'svelte';
  import { decimalToTime, savePeriods } from './api';
  import type { Days, HeatingSystem, Period } from './types';

  interface Props {
    system: HeatingSystem;
    onClose: () => void;
    onSaved: () => Promise<void>;
  }

  let { system, onClose, onSaved }: Props = $props();

  let draftPeriods = $state<Period[]>(
    untrack(() =>
      $state
        .snapshot(system.periods)
        .sort((a, b) => a.start - b.start || a.end - b.end),
    ),
  );
  let saving = $state(false);
  let errorMsg = $state<string | null>(null);

  const DAY_LIST: { key: keyof Days; short: string; label: string }[] = [
    { key: 'monday', short: 'M', label: 'Mon' },
    { key: 'tuesday', short: 'T', label: 'Tue' },
    { key: 'wednesday', short: 'W', label: 'Wed' },
    { key: 'thursday', short: 'T', label: 'Thu' },
    { key: 'friday', short: 'F', label: 'Fri' },
    { key: 'saturday', short: 'S', label: 'Sat' },
    { key: 'sunday', short: 'S', label: 'Sun' },
  ];

  // Build 15-minute time options from 00:00 to 24:00
  const TIME_OPTIONS: { value: number; label: string }[] = [];
  for (let q = 0; q <= 96; q++) {
    const dec = q * 0.25;
    TIME_OPTIONS.push({ value: dec, label: decimalToTime(dec) });
  }

  function addPeriod() {
    const id =
      typeof crypto !== 'undefined' && 'randomUUID' in crypto
        ? crypto.randomUUID()
        : `p-${Date.now()}-${Math.random().toString(16).slice(2, 8)}`;

    draftPeriods.push({
      id,
      start: 7.0,
      end: 9.0,
      target: 20.5,
      days: {
        monday: true,
        tuesday: true,
        wednesday: true,
        thursday: true,
        friday: true,
        saturday: true,
        sunday: true,
      },
    });
  }

  function removePeriod(id: string) {
    draftPeriods = draftPeriods.filter((p) => p.id !== id);
  }

  function applyDayPreset(p: Period, preset: 'weekdays' | 'weekends' | 'all') {
    p.days = {
      monday: preset !== 'weekends',
      tuesday: preset !== 'weekends',
      wednesday: preset !== 'weekends',
      thursday: preset !== 'weekends',
      friday: preset !== 'weekends',
      saturday: preset !== 'weekdays',
      sunday: preset !== 'weekdays',
    };
  }

  function adjustTarget(p: Period, delta: number) {
    const next = Math.round((p.target + delta) * 2) / 2;
    p.target = Math.max(5.0, Math.min(28.0, next));
  }

  async function handleSave() {
    errorMsg = null;
    for (const p of draftPeriods) {
      if (p.end <= p.start) {
        errorMsg = `End time (${decimalToTime(p.end)}) must be after start time (${decimalToTime(p.start)}).`;
        return;
      }
    }

    saving = true;
    try {
      await savePeriods(system.system_id, draftPeriods);
      await onSaved();
      onClose();
    } catch (e) {
      errorMsg = e instanceof Error ? e.message : 'Failed to save schedule';
    } finally {
      saving = false;
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
    class="w-full max-w-xl max-h-[90vh] flex flex-col rounded-t-3xl sm:rounded-2xl bg-white dark:bg-slate-900 border border-slate-200 dark:border-slate-800 shadow-2xl overflow-hidden"
  >
    <!-- Modal Header -->
    <div class="flex items-center justify-between px-5 py-4 border-b border-slate-100 dark:border-slate-800">
      <div>
        <h3 class="text-base font-bold capitalize text-slate-900 dark:text-white">
          {system.system_id} Schedule
        </h3>
        <p class="text-xs text-slate-500 dark:text-slate-400">
          Configure active heating windows and target temperatures
        </p>
      </div>
      <button
        type="button"
        onclick={onClose}
        aria-label="Close schedule modal"
        class="rounded-lg p-1.5 text-slate-400 hover:text-slate-600 dark:hover:text-slate-200 hover:bg-slate-100 dark:hover:bg-slate-800 cursor-pointer"
      >
        <svg class="w-5 h-5" viewBox="0 0 20 20" fill="currentColor">
          <path
            d="M6.28 5.22a.75.75 0 00-1.06 1.06L8.94 10l-3.72 3.72a.75.75 0 101.06 1.06L10 11.06l3.72 3.72a.75.75 0 101.06-1.06L11.06 10l3.72-3.72a.75.75 0 00-1.06-1.06L10 8.94 6.28 5.22z"
          />
        </svg>
      </button>
    </div>

    <!-- Scrollable Period List -->
    <div class="flex-1 overflow-y-auto p-4 sm:p-5 space-y-3">
      {#if errorMsg}
        <div class="rounded-xl bg-red-500/10 border border-red-500/30 px-3.5 py-2.5 text-xs font-medium text-red-600 dark:text-red-400">
          {errorMsg}
        </div>
      {/if}

      {#if draftPeriods.length === 0}
        <div class="rounded-xl border border-dashed border-slate-200 dark:border-slate-800 p-8 text-center">
          <p class="text-sm font-medium text-slate-600 dark:text-slate-400">
            No scheduled periods configured
          </p>
          <p class="text-xs text-slate-400 dark:text-slate-500 mt-1">
            Add a period below to heat this zone automatically.
          </p>
        </div>
      {/if}

      {#each draftPeriods as period, idx (period.id)}
        <div class="rounded-xl bg-slate-50 dark:bg-slate-800/60 border border-slate-200/80 dark:border-slate-700/70 p-3.5 space-y-3">
          <div class="flex flex-wrap items-center justify-between gap-2">
            <!-- Start & End Time Selectors -->
            <div class="flex items-center gap-1.5">
              <span class="text-xs font-semibold text-slate-400 dark:text-slate-500 mr-1">
                #{idx + 1}
              </span>
              <select
                bind:value={period.start}
                class="rounded-lg bg-white dark:bg-slate-900 border border-slate-200 dark:border-slate-700 px-2.5 py-1.5 text-xs font-semibold tabular-nums text-slate-800 dark:text-slate-100"
              >
                {#each TIME_OPTIONS as opt}
                  <option value={opt.value}>{opt.label}</option>
                {/each}
              </select>
              <span class="text-xs text-slate-400">to</span>
              <select
                bind:value={period.end}
                class="rounded-lg bg-white dark:bg-slate-900 border border-slate-200 dark:border-slate-700 px-2.5 py-1.5 text-xs font-semibold tabular-nums text-slate-800 dark:text-slate-100"
              >
                {#each TIME_OPTIONS as opt}
                  <option value={opt.value}>{opt.label}</option>
                {/each}
              </select>
            </div>

            <!-- Target Temperature Stepper & Delete -->
            <div class="flex items-center gap-2">
              <div class="inline-flex items-center rounded-lg bg-white dark:bg-slate-900 border border-slate-200 dark:border-slate-700 p-0.5">
                <button
                  type="button"
                  onclick={() => adjustTarget(period, -0.5)}
                  class="w-7 h-7 flex items-center justify-center rounded-md text-slate-600 dark:text-slate-300 hover:bg-slate-100 dark:hover:bg-slate-800 font-bold cursor-pointer"
                >
                  −
                </button>
                <span class="px-2 text-xs font-bold tabular-nums text-orange-600 dark:text-orange-400">
                  {period.target.toFixed(1)}°C
                </span>
                <button
                  type="button"
                  onclick={() => adjustTarget(period, 0.5)}
                  class="w-7 h-7 flex items-center justify-center rounded-md text-slate-600 dark:text-slate-300 hover:bg-slate-100 dark:hover:bg-slate-800 font-bold cursor-pointer"
                >
                  +
                </button>
              </div>

              <button
                type="button"
                onclick={() => removePeriod(period.id)}
                title="Remove period"
                class="p-1.5 rounded-lg text-slate-400 hover:text-red-600 dark:hover:text-red-400 hover:bg-red-500/10 cursor-pointer"
              >
                <svg class="w-4 h-4" viewBox="0 0 20 20" fill="currentColor">
                  <path
                    fill-rule="evenodd"
                    d="M8.75 1A2.75 2.75 0 006 3.75v.443c-.795.077-1.584.176-2.365.298a.75.75 0 10.23 1.482l.149-.022.841 10.518A2.75 2.75 0 007.596 19h4.807a2.75 2.75 0 002.742-2.53l.841-10.52.149.023a.75.75 0 00.23-1.482A41.03 41.03 0 0014 4.193V3.75A2.75 2.75 0 0011.25 1h-2.5zM10 4c.84 0 1.673.025 2.5.075V3.75c0-.69-.56-1.25-1.25-1.25h-2.5c-.69 0-1.25.56-1.25 1.25v.325C8.327 4.025 9.16 4 10 4z"
                    clip-rule="evenodd"
                  />
                </svg>
              </button>
            </div>
          </div>

          <!-- Days of week pills + quick presets -->
          <div class="flex flex-wrap items-center justify-between gap-2 pt-1">
            <div class="flex items-center gap-1">
              {#each DAY_LIST as d}
                {@const active = period.days[d.key]}
                <button
                  type="button"
                  title={d.label}
                  onclick={() => (period.days[d.key] = !period.days[d.key])}
                  class={`w-7 h-7 rounded-lg text-xs font-bold transition cursor-pointer ${
                    active
                      ? 'bg-orange-500 text-white shadow-xs'
                      : 'bg-white dark:bg-slate-900 text-slate-400 dark:text-slate-500 border border-slate-200 dark:border-slate-700'
                  }`}
                >
                  {d.short}
                </button>
              {/each}
            </div>

            <div class="flex items-center gap-1 text-[11px]">
              <button
                type="button"
                onclick={() => applyDayPreset(period, 'weekdays')}
                class="rounded-md px-2 py-1 text-slate-500 dark:text-slate-400 hover:bg-slate-200/70 dark:hover:bg-slate-700/60 cursor-pointer"
              >
                Mon–Fri
              </button>
              <button
                type="button"
                onclick={() => applyDayPreset(period, 'weekends')}
                class="rounded-md px-2 py-1 text-slate-500 dark:text-slate-400 hover:bg-slate-200/70 dark:hover:bg-slate-700/60 cursor-pointer"
              >
                Sat–Sun
              </button>
              <button
                type="button"
                onclick={() => applyDayPreset(period, 'all')}
                class="rounded-md px-2 py-1 text-slate-500 dark:text-slate-400 hover:bg-slate-200/70 dark:hover:bg-slate-700/60 cursor-pointer"
              >
                All
              </button>
            </div>
          </div>
        </div>
      {/each}

      <button
        type="button"
        onclick={addPeriod}
        class="w-full flex items-center justify-center gap-1.5 rounded-xl border border-dashed border-slate-300 dark:border-slate-700 hover:border-orange-500 dark:hover:border-orange-500 py-2.5 text-xs font-semibold text-slate-600 dark:text-slate-300 hover:text-orange-600 dark:hover:text-orange-400 transition cursor-pointer"
      >
        <svg class="w-4 h-4" viewBox="0 0 20 20" fill="currentColor">
          <path
            d="M10.75 4.75a.75.75 0 00-1.5 0v4.5h-4.5a.75.75 0 000 1.5h4.5v4.5a.75.75 0 001.5 0v-4.5h4.5a.75.75 0 000-1.5h-4.5v-4.5z"
          />
        </svg>
        <span>Add Period</span>
      </button>
    </div>

    <!-- Modal Footer -->
    <div class="flex items-center justify-end gap-2.5 px-5 py-3.5 bg-slate-50 dark:bg-slate-900/90 border-t border-slate-100 dark:border-slate-800">
      <button
        type="button"
        onclick={onClose}
        class="rounded-xl px-4 py-2 text-xs font-semibold text-slate-600 dark:text-slate-300 hover:bg-slate-200/70 dark:hover:bg-slate-800 cursor-pointer"
      >
        Cancel
      </button>
      <button
        type="button"
        disabled={saving}
        onclick={handleSave}
        class="rounded-xl bg-orange-600 hover:bg-orange-500 text-white px-5 py-2 text-xs font-semibold shadow-xs transition cursor-pointer disabled:opacity-50"
      >
        {saving ? 'Saving…' : 'Save Schedule'}
      </button>
    </div>
  </div>
</div>
