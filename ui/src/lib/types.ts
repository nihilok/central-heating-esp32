export interface Days {
  monday: boolean;
  tuesday: boolean;
  wednesday: boolean;
  thursday: boolean;
  friday: boolean;
  saturday: boolean;
  sunday: boolean;
}

export interface Period {
  id: string;
  start: number;
  end: number;
  target: number;
  days: Days;
}

export interface SensorNode {
  url: string;
  adjustment: number | null;
}

export interface HistoryPoint {
  ts: number;
  temp: number;
  heating: boolean;
}

export interface HeatingSystem {
  system_id: string;
  sensor: SensorNode;
  periods: Period[];
  program: boolean;
  advance: number | null;
  boost: number | null;
  is_within_period: boolean;
  temperature: number | null;
  current_target: number;
  relay_on: boolean;
  disabled: boolean;
  error_count: number;
  history?: HistoryPoint[];
}

export type HistoryResponse = Record<string, HistoryPoint[]>;

export interface Diagnostics {
  uptime_secs: number;
  free_heap_bytes: number;
  min_free_heap_bytes: number;
  wifi_rssi: number | null;
  ota_slot: string;
  firmware_version: string;
}

export interface StatusResponse {
  systems: HeatingSystem[];
  loop_enabled: boolean;
  server_epoch: number | null;
  diagnostics: Diagnostics;
}

export interface WeatherLocation {
  query: string;
  name: string;
  latitude: number;
  longitude: number;
}

export interface WeatherData {
  locationName: string;
  temperature: number;
  apparentTemperature: number;
  weatherCode: number;
  description: string;
  icon: 'sun' | 'moon' | 'cloud-sun' | 'cloud-moon' | 'cloud' | 'rain' | 'snow' | 'storm' | 'fog';
  high: number;
  low: number;
  windSpeed: number;
  fetchedAt: number;
}

export type ThemeMode = 'system' | 'light' | 'dark';
