import type { Days, HeatingSystem, Period, SensorNode, StatusResponse } from './types';

const TOKEN_STORAGE_KEY = 'heating_auth_token';

export function getSavedToken(): string | null {
  return localStorage.getItem(TOKEN_STORAGE_KEY);
}

export function setSavedToken(token: string | null): void {
  if (token) {
    localStorage.setItem(TOKEN_STORAGE_KEY, token);
  } else {
    localStorage.removeItem(TOKEN_STORAGE_KEY);
  }
}

async function apiFetch<T>(path: string, init: RequestInit = {}): Promise<T> {
  const token = getSavedToken();
  const headers = new Headers(init.headers ?? {});
  if (token && !headers.has('Authorization')) {
    headers.set('Authorization', `Bearer ${token}`);
  }
  if (init.body && typeof init.body === 'string' && !headers.has('Content-Type')) {
    headers.set('Content-Type', 'application/json');
  }

  const res = await fetch(path, { ...init, headers });
  if (res.status === 401) {
    setSavedToken(null);
    throw new Error('UNAUTHORIZED');
  }
  if (!res.ok) {
    let detail = `HTTP ${res.status}`;
    try {
      const errJson = await res.json();
      if (errJson?.detail) detail = errJson.detail;
    } catch {
      // ignore
    }
    throw new Error(detail);
  }
  return res.json() as Promise<T>;
}

export async function login(username: string, password: string): Promise<string> {
  const res = await fetch('/token/', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ username, password }),
  });
  if (!res.ok) {
    throw new Error('Incorrect username or password');
  }
  const data = (await res.json()) as { access_token: string };
  setSavedToken(data.access_token);
  return data.access_token;
}

export async function checkToken(): Promise<boolean> {
  const token = getSavedToken();
  if (!token) return false;
  try {
    await apiFetch('/check_token/');
    return true;
  } catch {
    return false;
  }
}

export async function fetchStatus(): Promise<StatusResponse> {
  return apiFetch<StatusResponse>('/api/v3/status/');
}

export async function savePeriods(systemId: string, periods: Period[]): Promise<HeatingSystem> {
  return apiFetch<HeatingSystem>(`/api/v3/periods/${encodeURIComponent(systemId)}/`, {
    method: 'POST',
    body: JSON.stringify({ periods }),
  });
}

export async function setAdvance(systemId: string, durationMinutes = 60): Promise<HeatingSystem> {
  const endTime = Date.now() / 1000 + durationMinutes * 60;
  return apiFetch<HeatingSystem>(`/api/v3/advance/${encodeURIComponent(systemId)}/`, {
    method: 'POST',
    body: JSON.stringify({ end_time: endTime }),
  });
}

export async function setBoost(systemId: string, durationMinutes = 15): Promise<HeatingSystem> {
  const endTime = Date.now() / 1000 + durationMinutes * 60;
  return apiFetch<HeatingSystem>(`/api/v3/boost/${encodeURIComponent(systemId)}/`, {
    method: 'POST',
    body: JSON.stringify({ end_time: endTime }),
  });
}

export async function cancelOverrides(systemId: string): Promise<HeatingSystem> {
  return apiFetch<HeatingSystem>(`/api/v3/cancel_all/${encodeURIComponent(systemId)}/`, {
    method: 'POST',
  });
}

export async function setProgram(systemId: string, enabled: boolean): Promise<void> {
  await apiFetch(`/api/v3/program/${encodeURIComponent(systemId)}/${enabled ? 'on' : 'off'}/`, {
    method: 'POST',
  });
}

export async function updateSystemConfig(
  systemId: string,
  sensor: SensorNode,
): Promise<void> {
  await apiFetch('/api/v3/systems/', {
    method: 'POST',
    body: JSON.stringify({
      system_id: systemId,
      sensor,
    }),
  });
}

export async function setControlLoop(enabled: boolean): Promise<void> {
  await apiFetch(enabled ? '/api/v3/start_loop/' : '/api/v3/stop_loop/');
}

// ---------------------------------------------------------------------------
// Schedule & Time Helpers
// ---------------------------------------------------------------------------

export function decimalToTime(dec: number): string {
  const clamped = Math.max(0, Math.min(24, dec));
  const totalMins = Math.round(clamped * 60);
  const h = Math.floor(totalMins / 60);
  const m = totalMins % 60;
  return `${String(h).padStart(2, '0')}:${String(m).padStart(2, '0')}`;
}

export function timeToDecimal(timeStr: string): number {
  const [hStr, mStr] = timeStr.split(':');
  const h = parseInt(hStr || '0', 10);
  const m = parseInt(mStr || '0', 10);
  return Math.round(((h + m / 60) + Number.EPSILON) * 100) / 100;
}

const DAY_KEYS: (keyof Days)[] = [
  'sunday',
  'monday',
  'tuesday',
  'wednesday',
  'thursday',
  'friday',
  'saturday',
];

export function getTodayDayKey(now = new Date()): keyof Days {
  return DAY_KEYS[now.getDay()];
}

export function getTomorrowDayKey(now = new Date()): keyof Days {
  return DAY_KEYS[(now.getDay() + 1) % 7];
}

export function getNowDecimalHours(now = new Date()): number {
  return now.getHours() + now.getMinutes() / 60 + now.getSeconds() / 3600;
}

export function getTodaysPeriods(system: HeatingSystem, now = new Date()): Period[] {
  const todayKey = getTodayDayKey(now);
  return system.periods
    .filter((p) => p.days[todayKey] && p.end > p.start)
    .sort((a, b) => a.start - b.start || a.end - b.end);
}

export interface ZoneStatusSummary {
  headline: string;
  subline: string;
  badgeTone: 'heating' | 'active' | 'override' | 'paused' | 'idle' | 'error';
}

export function describeZoneStatus(system: HeatingSystem, now = new Date()): ZoneStatusSummary {
  const nowSec = now.getTime() / 1000;
  const nowDec = getNowDecimalHours(now);

  if (system.disabled) {
    return {
      headline: 'Sensor unreachable',
      subline: `Safety hold (${system.error_count} failed polls)`,
      badgeTone: 'error',
    };
  }

  if (system.boost && system.boost > nowSec) {
    const minsLeft = Math.max(1, Math.ceil((system.boost - nowSec) / 60));
    return {
      headline: `Boost active · ${minsLeft}m left`,
      subline: system.relay_on ? 'Heating to max (26.0°C)' : 'Target reached (26.0°C)',
      badgeTone: system.relay_on ? 'heating' : 'override',
    };
  }

  if (system.advance && system.advance > nowSec) {
    const minsLeft = Math.max(1, Math.ceil((system.advance - nowSec) / 60));
    return {
      headline: `Advance +1h · ${minsLeft}m left`,
      subline: `${system.relay_on ? 'Heating to' : 'Holding'} ${system.current_target.toFixed(1)}°C`,
      badgeTone: system.relay_on ? 'heating' : 'override',
    };
  }

  if (!system.program) {
    return {
      headline: 'Schedule paused',
      subline: `Frost protection (${system.current_target.toFixed(1)}°C)`,
      badgeTone: 'paused',
    };
  }

  const todaysPeriods = getTodaysPeriods(system, now);
  const activePeriod = todaysPeriods.find((p) => p.start <= nowDec && nowDec < p.end);

  if (activePeriod) {
    const untilStr = decimalToTime(activePeriod.end);
    return {
      headline: system.relay_on
        ? `Heating to ${activePeriod.target.toFixed(1)}°C`
        : `In period · ${activePeriod.target.toFixed(1)}°C`,
      subline: `Scheduled until ${untilStr}`,
      badgeTone: system.relay_on ? 'heating' : 'active',
    };
  }

  const nextToday = todaysPeriods.find((p) => p.start > nowDec);
  if (nextToday) {
    return {
      headline: 'Outside scheduled period',
      subline: `Next at ${decimalToTime(nextToday.start)} (${nextToday.target.toFixed(1)}°C)`,
      badgeTone: 'idle',
    };
  }

  const tomorrowKey = getTomorrowDayKey(now);
  const tomorrowFirst = system.periods
    .filter((p) => p.days[tomorrowKey] && p.end > p.start)
    .sort((a, b) => a.start - b.start)[0];

  if (tomorrowFirst) {
    return {
      headline: 'Done for today',
      subline: `Tomorrow ${decimalToTime(tomorrowFirst.start)} (${tomorrowFirst.target.toFixed(1)}°C)`,
      badgeTone: 'idle',
    };
  }

  return {
    headline: 'Standby',
    subline: `Frost protection (${system.current_target.toFixed(1)}°C)`,
    badgeTone: 'idle',
  };
}

export function formatUptime(seconds: number): string {
  const days = Math.floor(seconds / 86400);
  const hours = Math.floor((seconds % 86400) / 3600);
  const mins = Math.floor((seconds % 3600) / 60);
  if (days > 0) return `${days}d ${hours}h ${mins}m`;
  if (hours > 0) return `${hours}h ${mins}m`;
  return `${mins}m ${seconds % 60}s`;
}
