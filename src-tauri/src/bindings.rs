//! Hotkeys and aliases the user gives commands, apps and bookmarks. A hotkey
//! runs its target from anywhere; an alias typed in the bar goes straight to it. There are
//! none until the user sets one, so the bar behaves as before.

use crate::state::Binding;

/// What a binding can point at.
const KINDS: &[&str] = &["view:", "scope:", "app:", "bookmark:", "group:", "setting:"];

/// Checks a binding against the others and the shortcuts already in use, and returns
/// the list with it in place (or removed, when it has neither a hotkey nor an alias).
/// `taken` pairs each reserved shortcut with what it does, for the error message.
pub fn upsert(
    bindings: &[Binding],
    candidate: Binding,
    taken: &[(&str, &str)],
) -> Result<Vec<Binding>, String> {
    if !KINDS.iter().any(|kind| candidate.target.starts_with(kind)) {
        return Err("That can't have a shortcut".into());
    }
    let others: Vec<&Binding> = bindings
        .iter()
        .filter(|binding| binding.target != candidate.target)
        .collect();

    let hotkey = candidate.hotkey.trim().to_string();
    if !hotkey.is_empty() {
        crate::shortcut::canonical_shortcut(&hotkey)?;
        if let Some((_, what)) = taken
            .iter()
            .find(|(used, _)| crate::shortcut::clashes(used, &hotkey))
        {
            return Err(format!("That shortcut already opens {what}"));
        }
        if let Some(other) = others
            .iter()
            .find(|other| crate::shortcut::clashes(&other.hotkey, &hotkey))
        {
            return Err(format!("That shortcut already runs {}", other.label));
        }
    }

    let alias = candidate.alias.trim().to_lowercase();
    if !alias.is_empty() {
        if alias.chars().any(char::is_whitespace) {
            return Err("An alias is one word, with no spaces".into());
        }
        if alias.starts_with('!') || alias.starts_with('>') {
            return Err("An alias can't start with ! or >, which the bar already uses".into());
        }
        if alias.chars().count() > 16 {
            return Err("Keep an alias to 16 characters or fewer".into());
        }
        if let Some(other) = others.iter().find(|other| other.alias == alias) {
            return Err(format!("“{alias}” already goes to {}", other.label));
        }
    }

    let mut updated: Vec<Binding> = others.into_iter().cloned().collect();
    if !hotkey.is_empty() || !alias.is_empty() {
        let position = bindings
            .iter()
            .position(|binding| binding.target == candidate.target)
            .unwrap_or(updated.len())
            .min(updated.len());
        updated.insert(
            position,
            Binding {
                hotkey,
                direct: candidate.direct && !alias.is_empty(),
                alias,
                ..candidate
            },
        );
    }
    Ok(updated)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn binding(target: &str, hotkey: &str, alias: &str) -> Binding {
        Binding {
            target: target.into(),
            label: target.into(),
            hotkey: hotkey.into(),
            alias: alias.into(),
            direct: false,
        }
    }

    const TAKEN: &[(&str, &str)] = &[
        ("alt+space", "the bar"),
        ("ctrl+alt+r", "bug recording"),
        ("", "clipboard history"),
    ];

    #[test]
    fn adds_replaces_and_removes() {
        let added = upsert(&[], binding("view:clip", "ctrl+alt+a", ""), TAKEN).unwrap();
        assert_eq!(added.len(), 1);
        let replaced = upsert(&added, binding("view:clip", "", "c"), TAKEN).unwrap();
        assert_eq!(
            (replaced[0].hotkey.as_str(), replaced[0].alias.as_str()),
            ("", "c")
        );
        let removed = upsert(&replaced, binding("view:clip", "", ""), TAKEN).unwrap();
        assert!(removed.is_empty());
    }

    #[test]
    fn refuses_shortcuts_already_in_use() {
        let error = upsert(&[], binding("view:clip", "Alt+Space", ""), TAKEN).unwrap_err();
        assert!(error.contains("the bar"), "{error}");
        assert!(upsert(&[], binding("view:clip", "ctrl+alt+r", ""), TAKEN).is_err());
        let existing = [binding("app:spotify", "ctrl+alt+m", "")];
        let error = upsert(&existing, binding("view:clip", "ctrl+alt+m", ""), TAKEN).unwrap_err();
        assert!(error.contains("app:spotify"), "{error}");
        assert!(upsert(&[], binding("view:clip", "not a key", ""), TAKEN).is_err());
    }

    #[test]
    fn aliases_are_one_unique_word() {
        assert!(upsert(&[], binding("app:g", "", "two words"), TAKEN).is_err());
        assert!(upsert(&[], binding("app:g", "", "!g"), TAKEN).is_err());
        let existing = [binding("app:g", "", "g")];
        assert!(upsert(&existing, binding("app:w", "", "G"), TAKEN).is_err());
        let saved = upsert(&existing, binding("app:w", "", " W "), TAKEN).unwrap();
        assert_eq!(saved[1].alias, "w");
        assert!(upsert(&[], binding("nothing:x", "", "x"), TAKEN).is_err());
    }
}
