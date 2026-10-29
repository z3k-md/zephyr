//! Weather for the empty bar's Today line and the "weather" card, from Open-Meteo, which
//! needs no key. A background thread fetches it at start and every 30 minutes after, never
//! on summon, and the last reply is kept in the app's data folder so the bar has it at once
//! after a restart. The place is the one set in Settings, or else where the computer's IP
//! address says it is (asked once a day, unless Settings turns that off).
//!
//! Everything is metric here; the bar converts for the locales that use °F.

use std::path::PathBuf;
use std::sync::{Condvar, Mutex, OnceLock};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};

use crate::state::AppState;
use crate::util::now_ms;

const REFRESH_MS: u64 = 30 * 60 * 1000;
/// After a failed fetch (offline, asleep), the next try.
const RETRY_MS: u64 = 5 * 60 * 1000;
const IP_PLACE_MS: u64 = 24 * 60 * 60 * 1000;

/// A place weather is for: a city picked in Settings, or where the IP address says.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct Place {
    pub name: String,
    /// State or province, when there is one.
    pub region: String,
    pub country: String,
    pub latitude: f64,
    pub longitude: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct Forecast {
    pub place: Place,
    /// When it was fetched, in ms since the epoch.
    pub fetched_at: u64,
    /// The place's offset from UTC, so days and hours read in its own time.
    pub utc_offset_seconds: i64,
    pub current: Current,
    /// The next 48 hours, from the current one.
    pub hourly: Vec<Hour>,
    /// Today and the next six days.
    pub daily: Vec<Day>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct Current {
    /// °C.
    pub temperature: f64,
    pub feels_like: f64,
    /// A WMO weather code.
    pub code: u8,
    /// km/h.
    pub wind: f64,
    pub is_day: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct Hour {
    /// Seconds since the epoch.
    pub time: i64,
    pub temperature: f64,
    pub code: u8,
    /// Chance of rain or snow, 0–100.
    pub precipitation: u8,
    pub is_day: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct Day {
    /// Midnight at the place, in seconds since the epoch.
    pub time: i64,
    pub code: u8,
    pub high: f64,
    pub low: f64,
    pub precipitation: u8,
    pub uv: f64,
}

/// What's kept between runs.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct Cache {
    forecast: Option<Forecast>,
    /// Where the IP address put the computer, and when that was asked.
    ip_place: Option<Place>,
    ip_place_at: u64,
}

struct Shared {
    cache: Cache,
    file: PathBuf,
    /// Set when the place changes, so the thread fetches at once.
    stale: bool,
}

static SHARED: OnceLock<(Mutex<Shared>, Condvar)> = OnceLock::new();

/// Loads the last forecast and starts the thread that keeps it fresh.
pub fn start(app: &AppHandle, dir: PathBuf) {
    let file = dir.join("weather.json");
    let cache = std::fs::read(&file)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default();
    let shared = Shared {
        cache,
        file,
        stale: false,
    };
    if SHARED.set((Mutex::new(shared), Condvar::new())).is_err() {
        return;
    }
    let app = app.clone();
    std::thread::spawn(move || run(app));
}

/// The last forecast, if there is one.
pub fn latest() -> Option<Forecast> {
    let (shared, _) = SHARED.get()?;
    shared.lock().ok()?.cache.forecast.clone()
}

/// The place changed in Settings: fetch for it now.
pub fn refresh() {
    if let Some((shared, wake)) = SHARED.get()
        && let Ok(mut shared) = shared.lock()
    {
        shared.stale = true;
        wake.notify_one();
    }
}

fn run(app: AppHandle) {
    let Some((shared, wake)) = SHARED.get() else {
        return;
    };
    // Settings are managed later in setup than this starts.
    while app.try_state::<AppState>().is_none() {
        std::thread::sleep(Duration::from_millis(200));
    }
    let mut last_try = 0u64;
    loop {
        let (chosen, by_ip) = app
            .state::<AppState>()
            .snapshot()
            .map(|persisted| (persisted.weather_place.clone(), persisted.weather_by_ip))
            .unwrap_or((None, true));
        // No city picked and locating by IP turned off: nothing to fetch, and the place the
        // IP address gave is forgotten. It waits for Settings to change that.
        if chosen.is_none() && !by_ip {
            let Ok(mut guard) = shared.lock() else { return };
            guard.stale = false;
            if guard.cache.forecast.is_some() || guard.cache.ip_place.is_some() {
                guard.cache = Cache::default();
                save(&guard);
                let _ = app.emit("weather", None::<Forecast>);
            }
            drop(wake.wait_while(guard, |shared| !shared.stale));
            continue;
        }
        let (due, cache) = {
            let Ok(guard) = shared.lock() else { return };
            let fetched = guard
                .cache
                .forecast
                .as_ref()
                .map(|forecast| forecast.fetched_at)
                .unwrap_or(0);
            let now = now_ms();
            let due = guard.stale
                || (now.saturating_sub(fetched) >= REFRESH_MS
                    && now.saturating_sub(last_try) >= RETRY_MS);
            (due, guard.cache.clone())
        };
        if due {
            last_try = now_ms();
            let result = tauri::async_runtime::block_on(fetch_for(chosen, &cache));
            if let Ok(mut guard) = shared.lock() {
                guard.stale = false;
                match result {
                    Ok((forecast, ip_place)) => {
                        if let Some(place) = ip_place {
                            guard.cache.ip_place = Some(place);
                            guard.cache.ip_place_at = now_ms();
                        }
                        guard.cache.forecast = Some(forecast.clone());
                        save(&guard);
                        let _ = app.emit("weather", &forecast);
                    }
                    Err(err) => log::warn!("weather: {err}"),
                }
            }
        }
        // A minute at a time, so waking from sleep catches up soon after.
        if let Ok(guard) = shared.lock() {
            let _ = wake.wait_timeout_while(guard, Duration::from_secs(60), |shared| !shared.stale);
        }
    }
}

fn save(shared: &Shared) {
    match serde_json::to_vec(&shared.cache) {
        Ok(bytes) => {
            if let Err(err) = crate::util::write_atomic(&shared.file, &bytes) {
                log::warn!("weather: couldn't save: {err}");
            }
        }
        Err(err) => log::warn!("weather: {err}"),
    }
}

/// The forecast for the chosen place, or for where the IP address says, and that place when
/// it was newly asked.
async fn fetch_for(
    chosen: Option<Place>,
    cache: &Cache,
) -> Result<(Forecast, Option<Place>), String> {
    let client = client();
    if let Some(place) = chosen {
        return Ok((forecast(client, place).await?, None));
    }
    let fresh = now_ms().saturating_sub(cache.ip_place_at) < IP_PLACE_MS;
    let (place, asked) = match &cache.ip_place {
        Some(place) if fresh => (place.clone(), None),
        _ => {
            let place = ip_place(client).await?;
            (place.clone(), Some(place))
        }
    };
    Ok((forecast(client, place).await?, asked))
}

/// One client for every request, so its connections (and their TLS) are reused: a city search
/// that set up a new one each keystroke spent most of its time on the handshake.
fn client() -> &'static reqwest::Client {
    crate::util::http()
}

/// How long a weather request may take.
const TIMEOUT: Duration = Duration::from_secs(10);

/// The forecast for one place: the weather now, the next 48 hours and seven days.
async fn forecast(client: &reqwest::Client, place: Place) -> Result<Forecast, String> {
    let url = format!(
        "https://api.open-meteo.com/v1/forecast?latitude={}&longitude={}\
         &current=temperature_2m,apparent_temperature,weather_code,wind_speed_10m,is_day\
         &hourly=temperature_2m,weather_code,precipitation_probability,is_day\
         &daily=weather_code,temperature_2m_max,temperature_2m_min,precipitation_probability_max,uv_index_max\
         &forecast_days=7&forecast_hours=48&timezone=auto&timeformat=unixtime",
        place.latitude, place.longitude
    );
    let raw: Raw = client
        .get(url)
        .timeout(TIMEOUT)
        .send()
        .await
        .and_then(|response| response.error_for_status())
        .map_err(|err| err.to_string())?
        .json()
        .await
        .map_err(|err| err.to_string())?;
    Ok(raw.into_forecast(place, now_ms()))
}

/// Where the computer's IP address says it is.
async fn ip_place(client: &reqwest::Client) -> Result<Place, String> {
    #[derive(Deserialize)]
    struct Located {
        city: Option<String>,
        region: Option<String>,
        country_name: Option<String>,
        latitude: Option<f64>,
        longitude: Option<f64>,
    }
    let located: Located = client
        .get("https://ipapi.co/json/")
        .timeout(TIMEOUT)
        .send()
        .await
        .and_then(|response| response.error_for_status())
        .map_err(|err| err.to_string())?
        .json()
        .await
        .map_err(|err| err.to_string())?;
    let (Some(latitude), Some(longitude)) = (located.latitude, located.longitude) else {
        return Err("the IP lookup gave no location".into());
    };
    Ok(Place {
        name: located.city.unwrap_or_default(),
        region: located.region.unwrap_or_default(),
        country: located.country_name.unwrap_or_default(),
        latitude,
        longitude,
    })
}

/// Opens the connection to the city search when its field is focused, so the first letters
/// typed don't wait on DNS and TLS (most of a cold search's second or so).
pub async fn warm() {
    let _ = client()
        .head("https://geocoding-api.open-meteo.com/v1/search")
        .timeout(TIMEOUT)
        .send()
        .await;
}

/// Cities whose name starts with `text`, for the Settings field.
pub async fn places(text: &str) -> Result<Vec<Place>, String> {
    #[derive(Deserialize)]
    struct Found {
        #[serde(default)]
        results: Vec<City>,
    }
    #[derive(Deserialize)]
    struct City {
        name: String,
        #[serde(default)]
        admin1: String,
        #[serde(default)]
        country: String,
        latitude: f64,
        longitude: f64,
        #[serde(default)]
        population: u64,
    }
    let text = text.trim();
    if text.chars().count() < 2 {
        return Ok(Vec::new());
    }
    // Backspacing and retyping asks for the same text again; the answer doesn't change.
    static SEEN: Mutex<Vec<(String, Vec<Place>)>> = Mutex::new(Vec::new());
    let key = text.to_lowercase();
    if let Some((_, places)) = SEEN
        .lock()
        .ok()
        .and_then(|seen| seen.iter().find(|(seen, _)| *seen == key).cloned())
    {
        return Ok(places);
    }
    let found: Found = client()
        .get(format!(
            "https://geocoding-api.open-meteo.com/v1/search?name={}&count=20&language=en",
            urlencoding::encode(text)
        ))
        .timeout(TIMEOUT)
        .send()
        .await
        .and_then(|response| response.error_for_status())
        .map_err(|err| err.to_string())?
        .json()
        .await
        .map_err(|err| err.to_string())?;
    // The geocoder puts exact names first, so a few villages called "Billi" beat Billings;
    // the biggest places are the likely ones.
    let mut results = found.results;
    results.sort_by_key(|city| std::cmp::Reverse(city.population));
    let places = results
        .into_iter()
        .take(6)
        .map(|found| Place {
            name: found.name,
            region: found.admin1,
            country: found.country,
            latitude: found.latitude,
            longitude: found.longitude,
        })
        .collect::<Vec<_>>();
    if let Ok(mut seen) = SEEN.lock() {
        if seen.len() >= 64 {
            seen.remove(0);
        }
        seen.push((key, places.clone()));
    }
    Ok(places)
}

/// The forecast for a city typed in the bar ("weather tokyo"), kept for 30 minutes.
pub async fn forecast_at(text: &str) -> Result<Option<Forecast>, String> {
    static TYPED: Mutex<Vec<(String, Forecast)>> = Mutex::new(Vec::new());
    let key = text.trim().to_lowercase();
    if key.is_empty() {
        return Ok(None);
    }
    if let Ok(typed) = TYPED.lock()
        && let Some((_, forecast)) = typed.iter().find(|(name, forecast)| {
            *name == key && now_ms().saturating_sub(forecast.fetched_at) < REFRESH_MS
        })
    {
        return Ok(Some(forecast.clone()));
    }
    let Some(place) = places(&key).await?.into_iter().next() else {
        return Ok(None);
    };
    let found = forecast(client(), place).await?;
    if let Ok(mut typed) = TYPED.lock() {
        typed.retain(|(name, _)| *name != key);
        typed.push((key, found.clone()));
        if typed.len() > 8 {
            typed.remove(0);
        }
    }
    Ok(Some(found))
}

/// Open-Meteo's reply: each series is a column, and any value can be missing.
#[derive(Deserialize, Default)]
#[serde(default)]
struct Raw {
    utc_offset_seconds: i64,
    current: RawCurrent,
    hourly: RawHourly,
    daily: RawDaily,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct RawCurrent {
    temperature_2m: Option<f64>,
    apparent_temperature: Option<f64>,
    weather_code: Option<f64>,
    wind_speed_10m: Option<f64>,
    is_day: Option<f64>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct RawHourly {
    time: Vec<i64>,
    temperature_2m: Vec<Option<f64>>,
    weather_code: Vec<Option<f64>>,
    precipitation_probability: Vec<Option<f64>>,
    is_day: Vec<Option<f64>>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct RawDaily {
    time: Vec<i64>,
    weather_code: Vec<Option<f64>>,
    temperature_2m_max: Vec<Option<f64>>,
    temperature_2m_min: Vec<Option<f64>>,
    precipitation_probability_max: Vec<Option<f64>>,
    uv_index_max: Vec<Option<f64>>,
}

fn at(column: &[Option<f64>], index: usize) -> f64 {
    column.get(index).copied().flatten().unwrap_or(0.0)
}

fn code(value: f64) -> u8 {
    value.clamp(0.0, 99.0) as u8
}

fn percent(value: f64) -> u8 {
    value.clamp(0.0, 100.0).round() as u8
}

impl Raw {
    fn into_forecast(self, place: Place, now: u64) -> Forecast {
        let current = Current {
            temperature: self.current.temperature_2m.unwrap_or(0.0),
            feels_like: self.current.apparent_temperature.unwrap_or(0.0),
            code: code(self.current.weather_code.unwrap_or(0.0)),
            wind: self.current.wind_speed_10m.unwrap_or(0.0),
            is_day: self.current.is_day.unwrap_or(1.0) > 0.0,
        };
        let hours = &self.hourly;
        let hourly = (0..hours.time.len())
            .map(|index| Hour {
                time: hours.time[index],
                temperature: at(&hours.temperature_2m, index),
                code: code(at(&hours.weather_code, index)),
                precipitation: percent(at(&hours.precipitation_probability, index)),
                is_day: at(&hours.is_day, index) > 0.0,
            })
            .collect();
        let days = &self.daily;
        let daily = (0..days.time.len())
            .map(|index| Day {
                time: days.time[index],
                code: code(at(&days.weather_code, index)),
                high: at(&days.temperature_2m_max, index),
                low: at(&days.temperature_2m_min, index),
                precipitation: percent(at(&days.precipitation_probability_max, index)),
                uv: at(&days.uv_index_max, index),
            })
            .collect();
        Forecast {
            place,
            fetched_at: now,
            utc_offset_seconds: self.utc_offset_seconds,
            current,
            hourly,
            daily,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_open_meteo_columns_and_tolerates_gaps() {
        let raw: Raw = serde_json::from_str(
            r#"{
                "utc_offset_seconds": -25200,
                "current": {"temperature_2m": 14.2, "apparent_temperature": 12.9,
                            "weather_code": 3, "wind_speed_10m": 11.5, "is_day": 0},
                "hourly": {"time": [1761534000, 1761537600],
                           "temperature_2m": [14.0, null],
                           "weather_code": [3, 61],
                           "precipitation_probability": [10, 70],
                           "is_day": [0, 1]},
                "daily": {"time": [1761462000],
                          "weather_code": [61],
                          "temperature_2m_max": [16.1],
                          "temperature_2m_min": [8.3],
                          "precipitation_probability_max": [80],
                          "uv_index_max": [2.4]}
            }"#,
        )
        .unwrap();
        let forecast = raw.into_forecast(Place::default(), 5);
        assert_eq!(forecast.utc_offset_seconds, -25200);
        assert_eq!(forecast.current.code, 3);
        assert!(!forecast.current.is_day);
        assert_eq!(forecast.hourly.len(), 2);
        assert_eq!(forecast.hourly[1].temperature, 0.0);
        assert_eq!(forecast.hourly[1].precipitation, 70);
        assert_eq!(forecast.daily[0].code, 61);
        assert_eq!(forecast.daily[0].high, 16.1);
    }

    /// Asks Open-Meteo for real. `cargo test -- --ignored weather_live --nocapture`
    #[test]
    #[ignore]
    fn weather_live() {
        let found = tauri::async_runtime::block_on(forecast_at("Portland")).unwrap();
        println!("{found:#?}");
        assert!(found.is_some());
    }
}
