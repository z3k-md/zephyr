/// The quote for a symbol typed in the bar ("TSLA", "eth"), or None when there's no such one.
#[tauri::command]
pub async fn quote(symbol: String, crypto: bool) -> Result<Option<crate::stocks::Quote>, String> {
    crate::stocks::quote(&symbol, crypto).await
}
