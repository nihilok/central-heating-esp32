<script lang="ts">
  import ArcGauge from './ArcGauge.svelte';
  import ScheduleBar from './ScheduleBar.svelte';
  import HistorySparkline from './HistorySparkline.svelte';
  import {
    cancelOverrides,
    describeZoneStatus,
    setAdvance,
    setBoost,
    setProgram,
  } from './api';
  import type { HeatingSystem } from './types';

  interface Props {
    system: HeatingSystem;
    now: Date;
    onUpdated: () => Promise<void>;
    onOpenSchedule: (sys: HeatingSystem) => void;
  }

  let { system, now, onUpdated, onOpenSchedule }: Props = $props();

  let busy = $state(false);
  let status = $derived(describeZoneStatus(system, now));
  let nowSec = $derived(now.getTime() / 1000);
  let hasOverride = $derived(
    Boolean(
      (system.boost && system.boost > nowSec) ||
        (system.advance && system.advance > nowSec),
    ),
  );

  async function handleAction(fn: () => Promise<unknown>) {
    if (busy) return;
    busy = true;
    try {
      await fn();
      await onUpdated();
    } finally {
      busy = false;
    }
  }

  const badgeStyles: Record<typeof status.badgeTone, string> = {
    heating:
      'bg-orange-500/15 text-orange-700 dark:bg-orange-500/20 dark:text-orange-300 border-orange-500/30',
    active:
      'bg-emerald-500/15 text-emerald-700 dark:bg-emerald-500/20 dark:text-emerald-300 border-emerald-500/30',
    override:
      'bg-amber-500/15 text-amber-700 dark:bg-amber-500/20 dark:text-amber-300 border-amber-500/30',
    paused:
      'bg-slate-200/80 text-slate-600 dark:bg-slate-800 dark:text-slate-400 border-slate-300 dark:border-slate-700',
    idle:
      'bg-slate-100 text-slate-600 dark:bg-slate-800/80 dark:text-slate-300 border-slate-200 dark:border-slate-700/80',
    error:
      'bg-red-500/15 text-red-700 dark:bg-red-500/20 dark:text-red-300 border-red-500/30',
  };
</script>

<section
  class={`relative rounded-2xl bg-white dark:bg-slate-900 border transition-all duration-300 p-4 sm:p-5 shadow-xs ${
    system.relay_on
      ? 'border-orange-500/50 dark:border-orange-500/40 ring-1 ring-orange-500/20'
      : 'border-slate-200/90 dark:border-slate-800'
  }`}
>
  <!-- Card Header -->
  <div class="flex items-start justify-between gap-3">
    <div>
      <div class="flex items-center gap-2">
        <h2 class="text-lg font-bold capitalize tracking-tight text-slate-900 dark:text-white">
          {system.system_id}
        </h2>
        {#if system.sensor.adjustment}
          <span
            title="Sensor calibration offset configured in Settings"
            class="rounded-md bg-slate-100 dark:bg-slate-800 px-1.5 py-0.5 text-[11px] font-medium text-slate-500 dark:text-slate-400 tabular-nums"
          >
            {system.sensor.adjustment > 0 ? `+${system.sensor.adjustment.toFixed(1)}` : system.sensor.adjustment.toFixed(1)}°C cal
          </span>
        {/if}
      </div>
      <p class="text-xs text-slate-500 dark:text-slate-400 mt-0.5">
        {status.subline}
      </p>
    </div>

    <!-- Plain-English Status Pill -->
    <div
      class={`inline-flex items-center gap-1.5 rounded-full border px-2.5 py-1 text-xs font-semibold ${badgeStyles[status.badgeTone]}`}
    >
      <span
        class={`w-2 h-2 rounded-full ${
          system.relay_on
            ? 'bg-orange-500 animate-ping'
            : status.badgeTone === 'active'
              ? 'bg-emerald-500'
              : status.badgeTone === 'error'
                ? 'bg-red-500'
                : 'bg-slate-400 dark:bg-slate-500'
        }`}
      ></span>
      <span>{status.headline}</span>
    </div>
  </div>

  <!-- Custom SVG Arc Gauge -->
  <div class="my-2">
    <ArcGauge
      temperature={system.temperature}
      target={system.current_target}
      relayOn={system.relay_on}
      disabled={system.disabled}
    />
  </div>

  <!-- Quick Action Controls (Mobile-first 2x2 or 3-col grid) -->
  <div class="grid grid-cols-3 gap-2 mt-1">
    {#if hasOverride}
      <button
        type="button"
        disabled={busy}
        onclick={() => handleAction(() => cancelOverrides(system.system_id))}
        class="col-span-2 inline-flex items-center justify-center gap-1.5 rounded-xl bg-red-500/15 hover:bg-red-500/25 dark:bg-red-500/20 dark:hover:bg-red-500/30 border border-red-500/30 px-3 py-2.5 text-xs font-semibold text-red-700 dark:text-red-300 transition cursor-pointer disabled:opacity-50"
      >
        <svg class="w-4 h-4" viewBox="0 0 20 20" fill="currentColor">
          <path
            fill-rule="evenodd"
            d="M10 18a8 8 0 100-16 8 8 0 000 16zM8.28 7.22a.75.75 0 00-1.06 1.06L8.94 10l-1.72 1.72a.75.75 0 101.06 1.06L10 11.06l1.72 1.72a.75.75 0 101.06-1.06L11.06 10l1.72-1.72a.75.75 0 00-1.06-1.06L10 8.94 8.28 7.22z"
            clip-rule="evenodd"
          />
        </svg>
        <span>Cancel Override</span>
      </button>
    {:else}
      <button
        type="button"
        disabled={busy}
        onclick={() => handleAction(() => setAdvance(system.system_id, 60))}
        class="inline-flex items-center justify-center gap-1.5 rounded-xl bg-slate-100 hover:bg-slate-200/80 dark:bg-slate-800 dark:hover:bg-slate-700/80 border border-slate-200/80 dark:border-slate-700 px-3 py-2.5 text-xs font-semibold text-slate-800 dark:text-slate-100 transition cursor-pointer disabled:opacity-50"
      >
        <svg class="w-4 h-4 text-amber-500" viewBox="0 0 20 20" fill="currentColor">
          <path
            d="M6.3 2.841A1.5 1.5 0 004 4.11V15.89a1.5 1.5 0 002.3 1.269l9.344-5.89a1.5 1.5 0 000-2.538L6.3 2.84z"
          />
        </svg>
        <span>Adv +1h</span>
      </button>

      <button
        type="button"
        disabled={busy}
        onclick={() => handleAction(() => setBoost(system.system_id, 15))}
        class="inline-flex items-center justify-center gap-1.5 rounded-xl bg-orange-500/15 hover:bg-orange-500/25 dark:bg-orange-500/20 dark:hover:bg-orange-500/30 border border-orange-500/30 px-3 py-2.5 text-xs font-semibold text-orange-700 dark:text-orange-300 transition cursor-pointer disabled:opacity-50"
      >
        <svg class="w-4 h-4 text-orange-500" viewBox="0 0 20 20" fill="currentColor">
          <path
            d="M11.983 1.907a.75.75 0 00-1.292-.657l-8.5 9.5A.75.75 0 002.75 12h6.572l-1.305 6.093a.75.75 0 001.292.657l8.5-9.5A.75.75 0 0017.25 8h-6.572l1.305-6.093z"
          />
        </svg>
        <span>Boost 15m</span>
      </button>
    {/if}

    <button
      type="button"
      disabled={busy}
      onclick={() => handleAction(() => setProgram(system.system_id, !system.program))}
      class={`inline-flex items-center justify-center gap-1.5 rounded-xl border px-3 py-2.5 text-xs font-semibold transition cursor-pointer disabled:opacity-50 ${
        system.program
          ? 'bg-emerald-500/10 hover:bg-emerald-500/20 dark:bg-emerald-500/15 border-emerald-500/30 text-emerald-700 dark:text-emerald-300'
          : 'bg-slate-100 hover:bg-slate-200 dark:bg-slate-800 border-slate-300 dark:border-slate-700 text-slate-600 dark:text-slate-400'
      }`}
    >
      <span
        class={`w-2 h-2 rounded-full ${
          system.program ? 'bg-emerald-500' : 'bg-slate-400'
        }`}
      ></span>
      <span>{system.program ? 'Auto On' : 'Paused'}</span>
    </button>
  </div>

  <!-- 24h Schedule Strip -->
  <ScheduleBar
    {system}
    {now}
    onOpenSchedule={() => onOpenSchedule(system)}
  />

  <!-- 24h Temperature + Boiler History Sparkline -->
  <HistorySparkline
    history={system.history ?? []}
    currentTemp={system.temperature}
    relayOn={system.relay_on}
  />
</section>
