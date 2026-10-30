/// The quote for a symbol typed in the bar ("TSLA", "eth") over `range` (`1d`, `5d`, `1mo`,
/// `1y`), or None when there's no such one.
#[tauri::command]
pub async fn quote(
    symbol: String,
    crypto: bool,
    range: String,
) -> Result<Option<crate::stocks::Quote>, String> {
    crate::stocks::quote(&symbol, crypto, &range).await
}
