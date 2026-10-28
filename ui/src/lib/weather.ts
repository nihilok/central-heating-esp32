import type { WeatherData, WeatherLocation } from './types';

const WEATHER_LOC_KEY = 'heating_weather_location';

const DEFAULT_LOCATION: WeatherLocation = {
  query: 'London',
  name: 'London, UK',
  latitude: 51.5072,
  longitude: -0.1276,
};

export function getSavedWeatherLocation(): WeatherLocation {
  try {
    const raw = localStorage.getItem(WEATHER_LOC_KEY);
    if (raw) {
      const parsed = JSON.parse(raw) as WeatherLocation;
      if (typeof parsed.latitude === 'number' && typeof parsed.longitude === 'number') {
        return parsed;
      }
    }
  } catch {
    // ignore
  }
  return DEFAULT_LOCATION;
}

export function saveWeatherLocation(loc: WeatherLocation): void {
  localStorage.setItem(WEATHER_LOC_KEY, JSON.stringify(loc));
}

// Matches full or partial UK postcode (e.g. "SW1A 1AA", "M1 1AE", "BS8")
const UK_POSTCODE_REGEX = /^[A-Z]{1,2}\d[A-Z\d]?\s*\d?[A-Z]{0,2}$/i;

export async function geocodeLocation(input: string): Promise<WeatherLocation> {
  const trimmed = input.trim();
  if (!trimmed) {
    throw new Error('Please enter a town or UK postcode');
  }

  // 1. Try UK Postcode lookup via postcodes.io if it looks like a postcode
  if (UK_POSTCODE_REGEX.test(trimmed)) {
    const compact = trimmed.replace(/\s+/g, '');
    const pcRes = await fetch(`https://api.postcodes.io/postcodes/${encodeURIComponent(compact)}`);
    if (pcRes.ok) {
      const pcData = await pcRes.json();
      if (pcData?.result?.latitude && pcData?.result?.longitude) {
        const r = pcData.result;
        const area = r.admin_district || r.parish || r.outcode || r.postcode;
        return {
          query: trimmed.toUpperCase(),
          name: `${area} (${r.postcode})`,
          latitude: r.latitude,
          longitude: r.longitude,
        };
      }
    }
    // Also try outcode (e.g. "SW1A" or "BS8")
    const outRes = await fetch(`https://api.postcodes.io/outcodes/${encodeURIComponent(compact)}`);
    if (outRes.ok) {
      const outData = await outRes.json();
      if (outData?.result?.latitude && outData?.result?.longitude) {
        const r = outData.result;
        const district = Array.isArray(r.admin_district) && r.admin_district[0] ? r.admin_district[0] : r.outcode;
        return {
          query: trimmed.toUpperCase(),
          name: `${district} (${r.outcode})`,
          latitude: r.latitude,
          longitude: r.longitude,
        };
      }
    }
  }

  // 2. Fallback / standard town & city lookup via Open-Meteo Geocoding API
  const geoUrl = `https://geocoding-api.open-meteo.com/v1/search?name=${encodeURIComponent(trimmed)}&count=5&language=en&format=json`;
  const res = await fetch(geoUrl);
  if (!res.ok) {
    throw new Error('Failed to look up location');
  }
  const data = await res.json();
  const results = data?.results;
  if (!Array.isArray(results) || results.length === 0) {
    throw new Error(`Could not find "${trimmed}"`);
  }

  // Prefer UK match if available, otherwise first result
  const best = results.find((r: { country_code?: string }) => r.country_code === 'GB') ?? results[0];
  const labelParts = [best.name, best.admin1, best.country_code].filter(Boolean);

  return {
    query: trimmed,
    name: labelParts.slice(0, 2).join(', '),
    latitude: best.latitude,
    longitude: best.longitude,
  };
}

export async function fetchOutdoorWeather(loc: WeatherLocation): Promise<WeatherData> {
  const url =
    `https://api.open-meteo.com/v1/forecast?latitude=${loc.latitude.toFixed(4)}&longitude=${loc.longitude.toFixed(4)}` +
    `&current=temperature_2m,apparent_temperature,weather_code,wind_speed_10m,is_day` +
    `&daily=temperature_2m_max,temperature_2m_min&timezone=auto&forecast_days=1`;

  const res = await fetch(url);
  if (!res.ok) {
    throw new Error('Weather service unavailable');
  }
  const data = await res.json();
  const cur = data.current;
  const daily = data.daily;
  const code = Number(cur?.weather_code ?? 0);
  const isDay = Boolean(cur?.is_day ?? 1);
  const { description, icon } = describeWmoCode(code, isDay);

  return {
    locationName: loc.name,
    temperature: Math.round((cur?.temperature_2m ?? 0) * 10) / 10,
    apparentTemperature: Math.round((cur?.apparent_temperature ?? 0) * 10) / 10,
    weatherCode: code,
    description,
    icon,
    high: Math.round((daily?.temperature_2m_max?.[0] ?? cur?.temperature_2m ?? 0) * 10) / 10,
    low: Math.round((daily?.temperature_2m_min?.[0] ?? cur?.temperature_2m ?? 0) * 10) / 10,
    windSpeed: Math.round(cur?.wind_speed_10m ?? 0),
    fetchedAt: Date.now(),
  };
}

function describeWmoCode(
  code: number,
  isDay: boolean,
): {
  description: string;
  icon: WeatherData['icon'];
} {
  if (code === 0) {
    return isDay
      ? { description: 'Clear sky', icon: 'sun' }
      : { description: 'Clear night', icon: 'moon' };
  }
  if (code === 1 || code === 2) {
    return isDay
      ? { description: 'Partly cloudy', icon: 'cloud-sun' }
      : { description: 'Partly cloudy', icon: 'cloud-moon' };
  }
  if (code === 3) return { description: 'Overcast', icon: 'cloud' };
  if (code === 45 || code === 48) return { description: 'Foggy', icon: 'fog' };
  if (code >= 51 && code <= 67) return { description: 'Rain', icon: 'rain' };
  if (code >= 71 && code <= 77) return { description: 'Snow', icon: 'snow' };
  if (code >= 80 && code <= 82) return { description: 'Showers', icon: 'rain' };
  if (code >= 85 && code <= 86) return { description: 'Snow showers', icon: 'snow' };
  if (code >= 95) return { description: 'Thunderstorm', icon: 'storm' };
  return { description: 'Cloudy', icon: 'cloud' };
}
