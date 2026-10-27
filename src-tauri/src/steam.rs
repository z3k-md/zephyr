//! Installed Steam games, listed with the apps so the bar launches them by name. They open
//! through Steam (`steam://rungameid/<id>`), which starts itself in the background when it
//! isn't running.

use std::path::{Path, PathBuf};

use base64::Engine;
use base64::engine::general_purpose::STANDARD;

use crate::apps::App;

/// App ids of Steam games: `steam:892970`.
pub const ID_PREFIX: &str = "steam:";

/// Every installed game in every Steam library on this computer.
pub fn games() -> Vec<App> {
    let Some(steam) = steam_dir() else {
        return Vec::new();
    };
    let mut games = Vec::new();
    for library in libraries(&steam) {
        let Ok(entries) = std::fs::read_dir(library.join("steamapps")) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let manifest = path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("appmanifest_") && name.ends_with(".acf"));
            if !manifest {
                continue;
            }
            let Some(game) = std::fs::read_to_string(&path)
                .ok()
                .and_then(|text| parse_manifest(&text))
            else {
                continue;
            };
            if game.installed && !is_tool(&game.id, &game.name) {
                games.push(App {
                    id: format!("{ID_PREFIX}{}", game.id),
                    name: game.name,
                });
            }
        }
    }
    games.sort_by_key(|game| game.name.to_lowercase());
    games.dedup_by(|a, b| a.id == b.id);
    games
}

/// Starts a game through Steam.
pub fn launch(app_id: &str, name: &str) -> Result<(), String> {
    if app_id.is_empty() || !app_id.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(format!("{name} has no Steam id"));
    }
    let link = format!("steam://rungameid/{app_id}");
    #[cfg(windows)]
    let opened = std::process::Command::new("explorer.exe")
        .arg(&link)
        .spawn();
    #[cfg(target_os = "macos")]
    let opened = std::process::Command::new("open").arg(&link).spawn();
    #[cfg(not(any(windows, target_os = "macos")))]
    let opened = std::process::Command::new("xdg-open").arg(&link).spawn();
    opened
        .map(|_| ())
        .map_err(|err| format!("Couldn't open {name} through Steam: {err}"))
}

/// The game's small icon from Steam's art cache, as a `data:` URL.
pub fn icon(app_id: &str) -> Option<String> {
    let cache = steam_dir()?.join("appcache").join("librarycache");
    // Newer Steam keeps each game's art in a folder, the icon named by its hash; older Steam
    // kept `<id>_icon.jpg` beside the rest.
    let hashed = std::fs::read_dir(cache.join(app_id))
        .ok()
        .into_iter()
        .flat_map(|entries| entries.flatten())
        .map(|entry| entry.path())
        .find(|path| {
            path.extension().is_some_and(|ext| ext == "jpg")
                && path
                    .file_stem()
                    .and_then(|stem| stem.to_str())
                    .is_some_and(|stem| {
                        stem.len() == 40 && stem.bytes().all(|b| b.is_ascii_hexdigit())
                    })
        });
    let path = hashed.unwrap_or_else(|| cache.join(format!("{app_id}_icon.jpg")));
    let bytes = std::fs::read(path).ok()?;
    Some(format!("data:image/jpeg;base64,{}", STANDARD.encode(bytes)))
}

#[cfg(windows)]
fn steam_dir() -> Option<PathBuf> {
    use windows::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_SZ, RegGetValueW};
    use windows::core::w;

    let mut buffer = [0u16; 1024];
    let mut size = (buffer.len() * 2) as u32;
    let found = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            w!("Software\\Valve\\Steam"),
            w!("SteamPath"),
            RRF_RT_REG_SZ,
            None,
            Some(buffer.as_mut_ptr().cast()),
            Some(&mut size),
        )
    };
    let from_registry = found.is_ok().then(|| {
        let length = (size as usize / 2).saturating_sub(1).min(buffer.len());
        PathBuf::from(String::from_utf16_lossy(&buffer[..length]))
    });
    from_registry
        .into_iter()
        .chain(Some(PathBuf::from(r"C:\Program Files (x86)\Steam")))
        .find(|dir| dir.join("steamapps").is_dir())
}

#[cfg(target_os = "macos")]
fn steam_dir() -> Option<PathBuf> {
    let dir = PathBuf::from(std::env::var_os("HOME")?)
        .join("Library")
        .join("Application Support")
        .join("Steam");
    dir.join("steamapps").is_dir().then_some(dir)
}

#[cfg(not(any(windows, target_os = "macos")))]
fn steam_dir() -> Option<PathBuf> {
    None
}

/// Steam's own folder and every library folder added in Steam's settings.
fn libraries(steam: &Path) -> Vec<PathBuf> {
    let mut found = vec![steam.to_path_buf()];
    if let Ok(text) = std::fs::read_to_string(steam.join("steamapps").join("libraryfolders.vdf")) {
        for path in parse_libraries(&text) {
            let path = PathBuf::from(path);
            let same = |known: &PathBuf| {
                known.to_string_lossy().to_lowercase().replace('\\', "/")
                    == path.to_string_lossy().to_lowercase().replace('\\', "/")
            };
            if !found.iter().any(same) {
                found.push(path);
            }
        }
    }
    found
}

/// The quoted strings of a Valve KeyValues file (`libraryfolders.vdf`, `appmanifest_*.acf`),
/// in order; braces are skipped, which is enough to read keys and their values.
fn tokens(text: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c != '"' {
            continue;
        }
        let mut token = String::new();
        while let Some(c) = chars.next() {
            match c {
                '"' => break,
                '\\' => {
                    if let Some(escaped) = chars.next() {
                        token.push(escaped);
                    }
                }
                _ => token.push(c),
            }
        }
        tokens.push(token);
    }
    tokens
}

fn parse_libraries(text: &str) -> Vec<String> {
    let tokens = tokens(text);
    tokens
        .windows(2)
        .filter(|pair| pair[0].eq_ignore_ascii_case("path"))
        .map(|pair| pair[1].clone())
        .collect()
}

#[derive(Debug, PartialEq)]
struct Manifest {
    id: String,
    name: String,
    /// Fully installed, not partway through a download.
    installed: bool,
}

fn parse_manifest(text: &str) -> Option<Manifest> {
    let tokens = tokens(text);
    let value = |key: &str| {
        tokens
            .windows(2)
            .find(|pair| pair[0].eq_ignore_ascii_case(key))
            .map(|pair| pair[1].trim().to_string())
    };
    let id = value("appid")?;
    let name = value("name").filter(|name| !name.is_empty())?;
    let flags: u32 = value("StateFlags")
        .and_then(|flags| flags.parse().ok())
        .unwrap_or(0);
    Some(Manifest {
        id,
        name,
        installed: flags & 4 != 0,
    })
}

/// Steam installs runtimes and tools beside games; they aren't things to launch.
fn is_tool(id: &str, name: &str) -> bool {
    // Steamworks Common Redistributables.
    if id == "228980" {
        return true;
    }
    let name = name.to_lowercase();
    [
        "redistributable",
        "steam linux runtime",
        "proton",
        " sdk",
        "dedicated server",
    ]
    .iter()
    .any(|word| name.contains(word))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn library_folders_list_every_path() {
        let vdf = r#""libraryfolders"
{
	"0"
	{
		"path"		"C:\\Program Files (x86)\\Steam"
		"apps"
		{
			"892970"		"1356828390"
		}
	}
	"1"
	{
		"path"		"D:\\SteamLibrary"
	}
}"#;
        assert_eq!(
            parse_libraries(vdf),
            [r"C:\Program Files (x86)\Steam", r"D:\SteamLibrary"]
        );
    }

    #[test]
    fn a_manifest_gives_the_game_and_whether_it_is_fully_installed() {
        let acf = r#""AppState"
{
	"appid"		"892970"
	"name"		"Valheim"
	"StateFlags"		"4"
	"installdir"		"Valheim"
}"#;
        assert_eq!(
            parse_manifest(acf),
            Some(Manifest {
                id: "892970".into(),
                name: "Valheim".into(),
                installed: true,
            })
        );
        // Still downloading (StateFlags 1026 = update required and downloading).
        let downloading = acf.replace("\"4\"", "\"1026\"");
        assert!(!parse_manifest(&downloading).unwrap().installed);
        assert_eq!(parse_manifest("\"AppState\" { \"appid\" \"1\" }"), None);
    }

    #[test]
    fn runtimes_and_tools_are_not_games() {
        assert!(is_tool("228980", "Steamworks Common Redistributables"));
        assert!(is_tool("1628350", "Steam Linux Runtime 3.0 (sniper)"));
        assert!(is_tool("2348590", "Proton 8.0"));
        assert!(is_tool("1", "Valheim Dedicated Server"));
        assert!(!is_tool("892970", "Valheim"));
        assert!(!is_tool("427520", "Factorio"));
    }

    #[test]
    fn only_numeric_ids_launch() {
        assert!(launch("892970; calc", "Valheim").is_err());
        assert!(launch("", "Valheim").is_err());
    }

    /// Lists the games on this computer: `cargo test lists_this_computers_games -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn lists_this_computers_games() {
        for game in games() {
            let id = game.id.trim_start_matches(ID_PREFIX);
            println!("{} ({}) icon: {}", game.name, game.id, icon(id).is_some());
        }
    }
}
