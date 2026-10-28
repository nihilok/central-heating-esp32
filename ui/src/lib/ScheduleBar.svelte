<script lang="ts">
  import { decimalToTime, getNowDecimalHours, getTodaysPeriods } from './api';
  import type { HeatingSystem } from './types';

  interface Props {
    system: HeatingSystem;
    now: Date;
    onOpenSchedule: () => void;
  }

  let { system, now, onOpenSchedule }: Props = $props();

  let todaysPeriods = $derived(getTodaysPeriods(system, now));
  let nowDec = $derived(getNowDecimalHours(now));
  let nowPct = $derived(Math.max(0, Math.min(100, (nowDec / 24) * 100)));
</script>

<div class="mt-4">
  <div class="flex items-center justify-between mb-1.5">
    <span class="text-xs font-medium text-slate-500 dark:text-slate-400">
      Today's Schedule
    </span>
    <button
      type="button"
      onclick={onOpenSchedule}
      class="inline-flex items-center gap-1 rounded-lg px-2.5 py-1.5 -my-1 -mr-1.5 text-xs font-semibold text-orange-600 dark:text-orange-400 hover:bg-orange-500/10 active:bg-orange-500/20 cursor-pointer touch-manipulation"
    >
      <span>Edit periods</span>
      <svg class="w-3.5 h-3.5 pointer-events-none" viewBox="0 0 20 20" fill="currentColor">
        <path
          fill-rule="evenodd"
          d="M7.21 14.77a.75.75 0 01.02-1.06L11.168 10 7.23 6.29a.75.75 0 111.04-1.08l4.5 4.25a.75.75 0 010 1.08l-4.5 4.25a.75.75 0 01-1.06-.02z"
          clip-rule="evenodd"
        />
      </svg>
    </button>
  </div>

  <!-- Interactive 24h bar -->
  <button
    type="button"
    onclick={onOpenSchedule}
    title="Tap to edit schedule periods"
    class="group relative block w-full h-8 rounded-lg bg-slate-100 dark:bg-slate-800/90 border border-slate-200/80 dark:border-slate-700/70 overflow-hidden cursor-pointer touch-manipulation focus:outline-none focus:ring-2 focus:ring-orange-500/50 transition"
  >
    <!-- 6-hour grid lines -->
    {#each [6, 12, 18] as hour}
      <div
        class="absolute top-0 bottom-0 w-px bg-slate-200/90 dark:bg-slate-700/60"
        style={`left: ${(hour / 24) * 100}%`}
      ></div>
    {/each}

    <!-- Active periods for today -->
    {#each todaysPeriods as period (period.id)}
      {@const leftPct = Math.max(0, (period.start / 24) * 100)}
      {@const widthPct = Math.max(1.5, ((period.end - period.start) / 24) * 100)}
      {@const isCurrent = system.program && period.start <= nowDec && nowDec < period.end}
      <div
        class={`absolute top-1 bottom-1 rounded-md flex items-center justify-center overflow-hidden text-[10px] font-bold transition-colors ${
          !system.program
            ? 'bg-slate-300/70 dark:bg-slate-700 text-slate-600 dark:text-slate-400'
            : isCurrent
              ? 'bg-gradient-to-r from-amber-500 to-orange-500 text-white shadow-sm'
              : 'bg-orange-500/25 dark:bg-orange-500/30 text-orange-800 dark:text-orange-200 border border-orange-500/30'
        }`}
        style={`left: ${leftPct}%; width: ${widthPct}%`}
        title={`${decimalToTime(period.start)}–${decimalToTime(period.end)} (${period.target.toFixed(1)}°C)`}
      >
        {#if widthPct >= 10}
          <span class="truncate px-1">{period.target.toFixed(0)}°</span>
        {/if}
      </div>
    {/each}

    <!-- Current time needle -->
    <div
      class="absolute top-0 bottom-0 w-0.5 bg-slate-900 dark:bg-white z-10 shadow"
      style={`left: ${nowPct}%`}
    >
      <div
        class="absolute -top-0.5 -left-1 w-2.5 h-2 rounded-b-full bg-slate-900 dark:bg-white"
      ></div>
    </div>
  </button>

  <!-- Hour axis labels -->
  <div class="mt-1 flex justify-between text-[10px] font-medium text-slate-400 dark:text-slate-500 tabular-nums px-0.5">
    <span>00:00</span>
    <span>06:00</span>
    <span>12:00</span>
    <span>18:00</span>
    <span>24:00</span>
  </div>
</div>
