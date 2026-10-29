use tauri::AppHandle;

use super::save;
use crate::state::Persisted;

/// The last weather fetched for the Today line, if any.
#[tauri::command]
pub fn weather() -> Option<crate::weather::Forecast> {
    crate::weather::latest()
}

/// Cities matching what's typed in Settings' weather field.
#[tauri::command]
pub async fn weather_places(text: String) -> Result<Vec<crate::weather::Place>, String> {
    crate::weather::places(&text).await
}

/// Connects to the city search ahead of typing in Settings › Weather.
#[tauri::command]
pub async fn weather_warm() {
    crate::weather::warm().await;
}

/// The weather for a city typed in the bar ("weather tokyo").
#[tauri::command]
pub async fn weather_at(text: String) -> Result<Option<crate::weather::Forecast>, String> {
    crate::weather::forecast_at(&text).await
}

#[tauri::command]
pub async fn save_weather_place(
    app: AppHandle,
    place: Option<crate::weather::Place>,
) -> Result<Persisted, String> {
    let snapshot = save(app, |persisted| {
        persisted.weather_place = place;
        Ok(())
    })
    .await?;
    crate::weather::refresh();
    Ok(snapshot)
}

/// Whether, with no city picked, the weather finds the computer by its IP address.
#[tauri::command]
pub async fn save_weather_by_ip(app: AppHandle, on: bool) -> Result<Persisted, String> {
    let snapshot = save(app, move |persisted| {
        persisted.weather_by_ip = on;
        Ok(())
    })
    .await?;
    crate::weather::refresh();
    Ok(snapshot)
}

/// `c`, `f`, or none for the region's own way.
#[tauri::command]
pub async fn save_weather_unit(app: AppHandle, unit: Option<String>) -> Result<Persisted, String> {
    if unit
        .as_deref()
        .is_some_and(|unit| unit != "c" && unit != "f")
    {
        return Err("Pick Celsius or Fahrenheit".into());
    }
    save(app, |persisted| {
        persisted.weather_unit = unit;
        Ok(())
    })
    .await
}
