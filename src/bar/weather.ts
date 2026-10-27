import { onMounted, onUnmounted, ref } from 'vue';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { weather } from '../api';
import type { Forecast } from '../types';

// Weather for the Today line and the weather card. Rust fetches it in the background (every
// 30 minutes) and sends each new one; it's all metric there, and shown in °F and mph only
// where that's the local way.

/** The last forecast, kept current from the `weather` events. */
export function useWeather() {
  const forecast = ref<Forecast | null>(null);
  let unlisten: UnlistenFn | null = null;
  onMounted(async () => {
    forecast.value = await weather().catch(() => null);
    unlisten = await listen<Forecast>('weather', (event) => (forecast.value = event.payload));
  });
  onUnmounted(() => unlisten?.());
  return forecast;
}

const IMPERIAL_REGIONS = ['US', 'LR', 'MM'];

function region(): string {
  try {
    return new Intl.Locale(navigator.language).maximize().region ?? '';
  } catch {
    return '';
  }
}

export const imperial = IMPERIAL_REGIONS.includes(region());

/** "58°" from °C, in the local unit. */
export function degrees(celsius: number): string {
  return `${Math.round(imperial ? (celsius * 9) / 5 + 32 : celsius)}°`;
}

/** "9 mph" or "15 km/h". */
export function windSpeed(kmh: number): string {
  return imperial ? `${Math.round(kmh / 1.609)} mph` : `${Math.round(kmh)} km/h`;
}

export type Wet = 'rain' | 'snow' | null;

interface Kind {
  text: string;
  /** Its glyph by day; night swaps a sun for the moon. */
  glyph: string;
  wet: Wet;
}

/** WMO weather codes, as Open-Meteo gives them. */
function kind(code: number): Kind {
  if (code === 0) return { text: 'Clear', glyph: 'weatherSun', wet: null };
  if (code === 1) return { text: 'Mostly clear', glyph: 'weatherPartly', wet: null };
  if (code === 2) return { text: 'Partly cloudy', glyph: 'weatherPartly', wet: null };
  if (code === 3) return { text: 'Cloudy', glyph: 'weatherCloud', wet: null };
  if (code === 45 || code === 48) return { text: 'Fog', glyph: 'weatherFog', wet: null };
  if (code >= 51 && code <= 57) return { text: 'Drizzle', glyph: 'weatherDrizzle', wet: 'rain' };
  if (code === 61) return { text: 'Light rain', glyph: 'weatherRain', wet: 'rain' };
  if (code === 65) return { text: 'Heavy rain', glyph: 'weatherRain', wet: 'rain' };
  if (code === 66 || code === 67)
    return { text: 'Freezing rain', glyph: 'weatherRain', wet: 'rain' };
  if (code >= 61 && code <= 65) return { text: 'Rain', glyph: 'weatherRain', wet: 'rain' };
  if (code === 71) return { text: 'Light snow', glyph: 'weatherSnow', wet: 'snow' };
  if (code === 75) return { text: 'Heavy snow', glyph: 'weatherSnow', wet: 'snow' };
  if (code >= 71 && code <= 77) return { text: 'Snow', glyph: 'weatherSnow', wet: 'snow' };
  if (code >= 80 && code <= 82) return { text: 'Showers', glyph: 'weatherRain', wet: 'rain' };
  if (code === 85 || code === 86)
    return { text: 'Snow showers', glyph: 'weatherSnow', wet: 'snow' };
  if (code >= 95) return { text: 'Thunderstorms', glyph: 'weatherStorm', wet: 'rain' };
  return { text: 'Cloudy', glyph: 'weatherCloud', wet: null };
}

export function condition(code: number): string {
  return kind(code).text;
}

/** The glyph name for a code; at night a sun is the moon. */
export function weatherGlyph(code: number, isDay = true): string {
  const glyph = kind(code).glyph;
  if (isDay) return glyph;
  if (glyph === 'weatherSun') return 'moon';
  if (glyph === 'weatherPartly') return 'weatherPartlyNight';
  return glyph;
}

/** A color class for the glyph: sun warm, rain and snow cool, the rest plain. */
export function weatherTone(code: number, isDay = true): string {
  const { glyph, wet } = kind(code);
  if (wet) return 'wx-wet';
  return isDay && glyph === 'weatherSun' ? 'wx-sun' : '';
}

/** A time as the place's own clock reads it, for formatting in UTC. */
function placeDate(forecast: Forecast, seconds: number): Date {
  return new Date((seconds + forecast.utcOffsetSeconds) * 1000);
}

const WEEKDAY = new Intl.DateTimeFormat(undefined, { weekday: 'short', timeZone: 'UTC' });
const HOUR = new Intl.DateTimeFormat(undefined, { hour: 'numeric', timeZone: 'UTC' });

export function weekday(forecast: Forecast, seconds: number): string {
  return WEEKDAY.format(placeDate(forecast, seconds));
}

export function hourLabel(forecast: Forecast, seconds: number): string {
  return HOUR.format(placeDate(forecast, seconds));
}

/** Rain or snow likely in the next 48 hours: "Rain now", "Rain from 4 PM", "Snow Mon from
 *  4 AM". A 50% chance or more counts. */
export function wetNote(forecast: Forecast): string | null {
  const now = kind(forecast.current.code).wet;
  if (now) return `${now === 'snow' ? 'Snow' : 'Rain'} now`;
  const hour = forecast.hourly.find((hour) => hour.precipitation >= 50 && kind(hour.code).wet);
  if (!hour) return null;
  const word = kind(hour.code).wet === 'snow' ? 'Snow' : 'Rain';
  const today = placeDate(forecast, Date.now() / 1000).getUTCDate();
  const day =
    placeDate(forecast, hour.time).getUTCDate() === today ? '' : ` ${weekday(forecast, hour.time)}`;
  return `${word}${day} from ${hourLabel(forecast, hour.time)}`;
}

/** "Portland, Oregon". */
export function placeName(forecast: Forecast): string {
  const { name, region } = forecast.place;
  return [name, region].filter(Boolean).join(', ');
}
