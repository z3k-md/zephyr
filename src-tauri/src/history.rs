use serde::{Deserialize, Serialize};

pub const HISTORY_LIMIT: usize = 200;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryEntry {
    pub query: String,
    pub destination_id: String,
    pub uses: u32,
    pub last_used: i64,
}

pub fn record(history: &mut Vec<HistoryEntry>, query: &str, destination_id: &str, now: i64) {
    let query = query.trim();
    if query.is_empty() {
        return;
    }

    if let Some(entry) = history.iter_mut().find(|entry| {
        entry.destination_id == destination_id && entry.query.eq_ignore_ascii_case(query)
    }) {
        entry.query = query.to_string();
        entry.uses = entry.uses.saturating_add(1);
        entry.last_used = now;
    } else {
        history.push(HistoryEntry {
            query: query.to_string(),
            destination_id: destination_id.to_string(),
            uses: 1,
            last_used: now,
        });
    }

    if history.len() > HISTORY_LIMIT {
        history.sort_by(|left, right| score(right, now).total_cmp(&score(left, now)));
        history.truncate(HISTORY_LIMIT);
    }
}

pub fn matching(
    history: &[HistoryEntry],
    query: &str,
    now: i64,
    limit: usize,
) -> Vec<HistoryEntry> {
    let needle = query.trim().to_lowercase();
    let mut items: Vec<HistoryEntry> = history
        .iter()
        .filter(|entry| needle.is_empty() || entry.query.to_lowercase().starts_with(&needle))
        .cloned()
        .collect();
    items.sort_by(|left, right| score(right, now).total_cmp(&score(left, now)));
    items.truncate(limit);
    items
}

fn score(entry: &HistoryEntry, now: i64) -> f64 {
    let age_hours = (now - entry.last_used).max(0) as f64 / 3600.0;
    // A couple of hours is enough for one fresh search to outrank an older repeat.
    let recency = 1.0 / (1.0 + age_hours / 2.0);
    entry.uses as f64 * recency
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repeats_increment_the_same_entry() {
        let mut history = Vec::new();
        record(&mut history, "Crispr", "pubmed", 10);
        record(&mut history, "crispr", "pubmed", 20);
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].uses, 2);
        assert_eq!(history[0].query, "crispr");
        assert_eq!(history[0].last_used, 20);
    }

    #[test]
    fn recent_use_beats_an_older_repeat() {
        let mut history = Vec::new();
        record(&mut history, "old", "google", 0);
        record(&mut history, "old", "google", 0);
        record(&mut history, "new", "google", 10_000);
        let ranked = matching(&history, "", 10_000, 2);
        assert_eq!(ranked[0].query, "new");
    }

    #[test]
    fn prefix_filter_is_case_insensitive() {
        let mut history = Vec::new();
        record(&mut history, "CRISPR review", "pubmed", 1);
        record(&mut history, "kinase", "pubmed", 1);
        let ranked = matching(&history, "cri", 1, 8);
        assert_eq!(ranked.len(), 1);
        assert_eq!(ranked[0].query, "CRISPR review");
    }
}
