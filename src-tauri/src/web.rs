//! Where free text goes when it isn't an app, a settings page or a link: a Google search.

/// The Google search for `query`.
pub fn search_url(query: &str) -> String {
    format!(
        "https://www.google.com/search?q={}",
        urlencoding::encode(query)
    )
}

/// Google's autocomplete for `query`: a JSON list whose second item is the suggestions.
pub fn suggest_url(query: &str) -> String {
    format!(
        "https://suggestqueries.google.com/complete/search?client=firefox&q={}",
        urlencoding::encode(query)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_the_query() {
        assert_eq!(
            search_url("a b&c!"),
            "https://www.google.com/search?q=a%20b%26c%21"
        );
    }
}
