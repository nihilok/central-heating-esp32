<script lang="ts">
  import type { HistoryPoint } from './types';

  interface Props {
    history: HistoryPoint[];
    currentTemp: number | null;
    relayOn: boolean;
  }

  let { history, currentTemp, relayOn }: Props = $props();

  const W = 320;
  const H = 64;
  const PAD_Y = 8;

  // Include live reading at the right edge so the sparkline is useful immediately after boot
  let points = $derived.by(() => {
    const nowTs = Math.floor(Date.now() / 1000);
    const list = [...history];
    if (
      currentTemp !== null &&
      (list.length === 0 || nowTs - list[list.length - 1].ts > 30)
    ) {
      list.push({ ts: nowTs, temp: currentTemp, heating: relayOn });
    }
    return list;
  });

  let bounds = $derived.by(() => {
    if (points.length === 0) {
      return { minT: 15, maxT: 22, minTs: 0, maxTs: 1 };
    }
    let minT = Infinity;
    let maxT = -Infinity;
    for (const p of points) {
      if (p.temp < minT) minT = p.temp;
      if (p.temp > maxT) maxT = p.temp;
    }
    // Pad range by at least 2°C so flat lines look calm and centred
    if (maxT - minT < 2.0) {
      const mid = (maxT + minT) / 2;
      minT = mid - 1.0;
      maxT = mid + 1.0;
    } else {
      minT -= 0.4;
      maxT += 0.4;
    }
    const minTs = points[0].ts;
    const maxTs = Math.max(minTs + 60, points[points.length - 1].ts);
    return { minT, maxT, minTs, maxTs };
  });

  function xFor(i: number, ts: number): number {
    if (points.length <= 1) return W / 2;
    const span = bounds.maxTs - bounds.minTs;
    if (span <= 0) return (i / (points.length - 1)) * W;
    return ((ts - bounds.minTs) / span) * W;
  }

  function yFor(temp: number): number {
    const frac = (temp - bounds.minT) / (bounds.maxT - bounds.minT);
    return H - PAD_Y - frac * (H - PAD_Y * 2);
  }

  let linePath = $derived.by(() => {
    if (points.length === 0) return '';
    if (points.length === 1) {
      const y = yFor(points[0].temp).toFixed(1);
      return `M 0 ${y} L ${W} ${y}`;
    }
    return points
      .map((p, i) => `${i === 0 ? 'M' : 'L'} ${xFor(i, p.ts).toFixed(1)} ${yFor(p.temp).toFixed(1)}`)
      .join(' ');
  });

  let areaPath = $derived.by(() => {
    if (!linePath) return '';
    return `${linePath} L ${W} ${H} L 0 ${H} Z`;
  });

  // Compute orange vertical bands where relay was firing
  let heatingBands = $derived.by(() => {
    const bands: { x: number; w: number }[] = [];
    if (points.length === 0) return bands;
    if (points.length === 1) {
      if (points[0].heating) bands.push({ x: W - 8, w: 8 });
      return bands;
    }
    for (let i = 0; i < points.length; i++) {
      if (!points[i].heating) continue;
      const x0 = i === 0 ? 0 : (xFor(i - 1, points[i - 1].ts) + xFor(i, points[i].ts)) / 2;
      const x1 =
        i === points.length - 1
          ? W
          : (xFor(i, points[i].ts) + xFor(i + 1, points[i + 1].ts)) / 2;
      bands.push({ x: x0, w: Math.max(2, x1 - x0) });
    }
    return bands;
  });

  let minRecorded = $derived(
    points.length > 0 ? Math.min(...points.map((p) => p.temp)) : null,
  );
  let maxRecorded = $derived(
    points.length > 0 ? Math.max(...points.map((p) => p.temp)) : null,
  );

  let spanHours = $derived.by(() => {
    if (points.length < 2) return 0;
    return Math.max(0.1, (points[points.length - 1].ts - points[0].ts) / 3600);
  });
</script>

<div class="mt-3 pt-3 border-t border-slate-100 dark:border-slate-800/80">
  <div class="flex items-center justify-between text-[11px] text-slate-500 dark:text-slate-400 mb-1.5">
    <div class="flex items-center gap-2">
      <span class="font-medium">
        {spanHours >= 1 ? `${Math.round(spanHours)}h History` : 'Recent History'}
      </span>
      <span class="inline-flex items-center gap-1 text-[10px] text-orange-600 dark:text-orange-400">
        <span class="w-2 h-2 rounded-xs bg-orange-500/40 border border-orange-500/60"></span>
        boiler on
      </span>
    </div>
    {#if minRecorded !== null && maxRecorded !== null}
      <span class="tabular-nums text-[11px]">
        <span class="text-sky-600 dark:text-sky-400">{minRecorded.toFixed(1)}°</span>
        <span class="mx-0.5 text-slate-300 dark:text-slate-600">–</span>
        <span class="text-orange-600 dark:text-orange-400">{maxRecorded.toFixed(1)}°</span>
      </span>
    {/if}
  </div>

  <div class="relative h-14 w-full rounded-lg bg-slate-50/80 dark:bg-slate-900/60 border border-slate-200/60 dark:border-slate-800 overflow-hidden">
    {#if points.length > 0}
      <svg viewBox={`0 0 ${W} ${H}`} preserveAspectRatio="none" class="w-full h-full">
        <defs>
          <linearGradient id="sparkArea" x1="0" y1="0" x2="0" y2="1">
            <stop offset="0%" stop-color="#38bdf8" stop-opacity="0.24" />
            <stop offset="100%" stop-color="#38bdf8" stop-opacity="0.0" />
          </linearGradient>
        </defs>

        <!-- Shaded orange bands where heating relay was ON -->
        {#each heatingBands as band}
          <rect
            x={band.x}
            y="0"
            width={band.w}
            height={H}
            class="fill-orange-500/20 dark:fill-orange-500/25"
          />
        {/each}

        <!-- Temperature area & line -->
        <path d={areaPath} fill="url(#sparkArea)" />
        <path
          d={linePath}
          fill="none"
          stroke="currentColor"
          stroke-width="2"
          vector-effect="non-scaling-stroke"
          class="text-sky-500 dark:text-sky-400"
        />
      </svg>
    {:else}
      <div class="h-full flex items-center justify-center text-xs text-slate-400 dark:text-slate-600">
        Collecting 5-minute sensor samples…
      </div>
    {/if}
  </div>
</div>
