<script lang="ts">
  interface Props {
    temperature: number | null;
    target: number;
    relayOn: boolean;
    disabled?: boolean;
  }

  let { temperature, target, relayOn, disabled = false }: Props = $props();

  const MIN_TEMP = 5;
  const MAX_TEMP = 28;
  const START_ANGLE = -210; // degrees (bottom-left)
  const END_ANGLE = 30;     // degrees (bottom-right, 240° total sweep)
  const SWEEP = END_ANGLE - START_ANGLE;
  const CX = 110;
  const CY = 104;
  const R = 82;

  function tempToFraction(t: number): number {
    return Math.max(0, Math.min(1, (t - MIN_TEMP) / (MAX_TEMP - MIN_TEMP)));
  }

  function polarToCartesian(cx: number, cy: number, r: number, angleDeg: number) {
    const rad = (angleDeg * Math.PI) / 180;
    return {
      x: cx + r * Math.cos(rad),
      y: cy + r * Math.sin(rad),
    };
  }

  function describeArc(startDeg: number, endDeg: number): string {
    const start = polarToCartesian(CX, CY, R, startDeg);
    const end = polarToCartesian(CX, CY, R, endDeg);
    const largeArcFlag = endDeg - startDeg <= 180 ? '0' : '1';
    return `M ${start.x.toFixed(2)} ${start.y.toFixed(2)} A ${R} ${R} 0 ${largeArcFlag} 1 ${end.x.toFixed(2)} ${end.y.toFixed(2)}`;
  }

  let isBoost = $derived(target >= 900);
  let effectiveTarget = $derived(isBoost ? 26.0 : target);
  let tempFraction = $derived(temperature !== null ? tempToFraction(temperature) : 0);
  let targetFraction = $derived(tempToFraction(effectiveTarget));

  let bgArcPath = $derived(describeArc(START_ANGLE, END_ANGLE));
  let valueEndAngle = $derived(START_ANGLE + Math.max(0.01, tempFraction) * SWEEP);
  let valueArcPath = $derived(describeArc(START_ANGLE, valueEndAngle));

  let targetAngle = $derived(START_ANGLE + targetFraction * SWEEP);
  let targetInner = $derived(polarToCartesian(CX, CY, R - 11, targetAngle));
  let targetOuter = $derived(polarToCartesian(CX, CY, R + 11, targetAngle));
  let dotPos = $derived(polarToCartesian(CX, CY, R, valueEndAngle));
</script>

<div class="relative flex flex-col items-center justify-center select-none">
  <svg viewBox="0 0 220 172" class="w-full max-w-[235px] overflow-visible">
    <defs>
      <linearGradient id="heatGrad" x1="0%" y1="100%" x2="100%" y2="0%">
        <stop offset="0%" stop-color="#f59e0b" />
        <stop offset="100%" stop-color="#ea580c" />
      </linearGradient>
      <linearGradient id="idleGrad" x1="0%" y1="100%" x2="100%" y2="0%">
        <stop offset="0%" stop-color="#38bdf8" />
        <stop offset="100%" stop-color="#10b981" />
      </linearGradient>
      <filter id="arcGlow" x="-20%" y="-20%" width="140%" height="140%">
        <feGaussianBlur stdDeviation="3.5" result="blur" />
        <feComposite in="SourceGraphic" in2="blur" operator="over" />
      </filter>
    </defs>

    <!-- Background track arc -->
    <path
      d={bgArcPath}
      fill="none"
      stroke="currentColor"
      class="text-slate-200 dark:text-slate-800"
      stroke-width="13"
      stroke-linecap="round"
    />

    <!-- Active temperature fill arc -->
    {#if temperature !== null && !disabled}
      <path
        d={valueArcPath}
        fill="none"
        stroke={relayOn ? 'url(#heatGrad)' : 'url(#idleGrad)'}
        stroke-width="13"
        stroke-linecap="round"
        filter={relayOn ? 'url(#arcGlow)' : undefined}
        class="transition-all duration-500 ease-out"
      />
    {/if}

    <!-- Target notch marker on the arc -->
    {#if !disabled}
      <line
        x1={targetInner.x}
        y1={targetInner.y}
        x2={targetOuter.x}
        y2={targetOuter.y}
        stroke="currentColor"
        stroke-width="3"
        stroke-linecap="round"
        class={relayOn
          ? 'text-orange-600 dark:text-orange-400'
          : 'text-slate-500 dark:text-slate-400'}
      />
    {/if}

    <!-- Current temperature thumb dot -->
    {#if temperature !== null && !disabled}
      <circle
        cx={dotPos.x}
        cy={dotPos.y}
        r="6"
        class={relayOn
          ? 'fill-white stroke-orange-600 dark:stroke-orange-400'
          : 'fill-white stroke-emerald-500 dark:stroke-sky-400'}
        stroke-width="3"
      />
    {/if}

    <!-- Min / Max scale labels -->
    <text
      x="34"
      y="158"
      text-anchor="middle"
      class="fill-slate-400 dark:fill-slate-500 text-[10px] font-medium"
    >
      {MIN_TEMP}°
    </text>
    <text
      x="186"
      y="158"
      text-anchor="middle"
      class="fill-slate-400 dark:fill-slate-500 text-[10px] font-medium"
    >
      {MAX_TEMP}°
    </text>
  </svg>

  <!-- Center readout overlay -->
  <div class="absolute inset-0 flex flex-col items-center justify-center pt-3 pointer-events-none">
    {#if relayOn}
      <div
        class="mb-0.5 inline-flex items-center gap-1 rounded-full bg-orange-500/15 dark:bg-orange-500/20 px-2.5 py-0.5 text-[11px] font-semibold text-orange-600 dark:text-orange-400 animate-flame"
      >
        <svg class="w-3.5 h-3.5 fill-current" viewBox="0 0 24 24">
          <path
            d="M12 2c1.5 3.5 4.5 5.5 4.5 9.5 0 2.5-1.5 4.5-3.5 5.5.5-1.5.5-3-.5-4.5-1 2-2.5 3-2.5 5 0 1.2.5 2.2 1.2 2.9C8 19.8 5.5 17 5.5 13.2 5.5 8.5 9.5 5.5 12 2z"
          />
        </svg>
        <span>HEATING</span>
      </div>
    {:else}
      <div class="h-5"></div>
    {/if}

    <div class="flex items-baseline tracking-tight">
      {#if temperature !== null}
        <span class="text-4xl font-bold tabular-nums text-slate-900 dark:text-white">
          {temperature.toFixed(1)}
        </span>
        <span class="text-lg font-semibold text-slate-400 dark:text-slate-500 ml-0.5">°C</span>
      {:else}
        <span class="text-3xl font-bold text-slate-400 dark:text-slate-600">—.-°C</span>
      {/if}
    </div>

    <div class="mt-1 flex items-center gap-1.5 text-xs font-medium text-slate-500 dark:text-slate-400">
      <span>Target</span>
      {#if isBoost}
        <span class="rounded bg-orange-500/15 px-1.5 py-0.5 font-bold text-orange-600 dark:text-orange-400">
          BOOST (26°C)
        </span>
      {:else}
        <span class="font-semibold tabular-nums text-slate-700 dark:text-slate-200">
          {target.toFixed(1)}°C
        </span>
      {/if}
    </div>
  </div>
</div>
