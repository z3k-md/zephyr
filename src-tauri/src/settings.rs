use crate::apps::{self, LaunchEntry, Strength};

/// Bang words that scope the bar to settings.
pub const SCOPE_TRIGGERS: &[&str] = &["set", "settings"];

pub fn is_scope(trigger: &str) -> bool {
    SCOPE_TRIGGERS.contains(&trigger)
}

/// A settings page the bar can open: an OS settings pane or a section of the app's own Settings window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Setting {
    pub id: &'static str,
    pub title: &'static str,
    /// Other words people use for the page ("dark mode" for Colors, "wifi" for Network).
    pub keywords: &'static [&'static str],
    pub target: Target,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    /// A section id in the app's Settings window.
    Zephyr(&'static str),
    /// An `ms-settings:` or `x-apple.systempreferences:` URI, opened by the OS.
    Uri(&'static str),
    /// A Control Panel applet, console or tool in `%SystemRoot%\System32`, opened through the shell
    /// so tools that need elevation get their UAC prompt.
    System(&'static str),
    /// A program run with arguments, for pages the shell can't open by file name alone.
    Command(&'static str, &'static [&'static str]),
}

impl Setting {
    pub fn hint(&self) -> &'static str {
        match self.target {
            Target::Zephyr(_) => "Zephyr",
            Target::System(_) | Target::Command(..) => "Control Panel",
            Target::Uri(_) => "System settings",
        }
    }
}

/// The settings this platform can open: the app's own first, then the OS's.
pub fn catalog() -> &'static [Setting] {
    use std::sync::OnceLock;
    static ALL: OnceLock<Vec<Setting>> = OnceLock::new();
    ALL.get_or_init(|| {
        let system: &[Setting] = if cfg!(windows) {
            WINDOWS
        } else if cfg!(target_os = "macos") {
            MACOS
        } else {
            &[]
        };
        ZEPHYR
            .iter()
            .filter(|setting| cfg!(target_os = "macos") || setting.id != "zephyr.permissions")
            .chain(system.iter().filter(|setting| installed(setting)))
            .copied()
            .collect()
    })
}

/// macOS: whether this Mac has the pane (an older or newer macOS can lack one, like Screen
/// Saver or Passwords). Privacy anchors aren't extensions and always open.
fn installed(setting: &Setting) -> bool {
    if !cfg!(target_os = "macos") {
        return true;
    }
    let Target::Uri(uri) = setting.target else {
        return true;
    };
    let Some(id) = uri.strip_prefix("x-apple.systempreferences:") else {
        return true;
    };
    if id.contains('?') {
        return true;
    }
    let panes = mac_panes();
    // No list at all (the folder moved) means keep everything rather than hide it all.
    panes.is_empty() || panes.iter().any(|pane| pane == id)
}

/// The bundle ids of the System Settings panes installed on this Mac.
fn mac_panes() -> &'static [String] {
    use std::sync::OnceLock;
    static PANES: OnceLock<Vec<String>> = OnceLock::new();
    PANES.get_or_init(|| {
        let Ok(entries) = std::fs::read_dir("/System/Library/ExtensionKit/Extensions") else {
            return Vec::new();
        };
        entries
            .flatten()
            .filter_map(|entry| {
                let plist = entry.path().join("Contents/Info.plist");
                let output = std::process::Command::new("plutil")
                    .args(["-extract", "CFBundleIdentifier", "raw", "-o", "-"])
                    .arg(&plist)
                    .output()
                    .ok()?;
                let id = String::from_utf8_lossy(&output.stdout).trim().to_string();
                (output.status.success() && !id.is_empty()).then_some(id)
            })
            .collect()
    })
}

pub fn find(id: &str) -> Option<&'static Setting> {
    catalog().iter().find(|setting| setting.id == id)
}

/// Words that say the text is about settings ("wifi settings", "settings dark").
const SETTINGS_WORDS: &[&str] = &[
    "settings",
    "setting",
    "preferences",
    "preference",
    "prefs",
    "pref",
];

/// Lowercase, with spelling variants folded so they match: "Wi-Fi" and "wi fi" are "wifi",
/// "&" is "and", and plurals lose their "s" ("displays" is "display").
pub fn normalize(text: &str) -> String {
    let lower = text
        .to_lowercase()
        .replace("wi-fi", "wifi")
        .replace("wi fi", "wifi")
        .replace('&', " and ");
    lower
        .split_whitespace()
        .map(|word| {
            let keep = word.len() <= 3
                || !word.ends_with('s')
                || ["ss", "us", "is", "os"]
                    .iter()
                    .any(|end| word.ends_with(end));
            if keep { word } else { &word[..word.len() - 1] }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn is_settings_word(word: &str) -> bool {
    SETTINGS_WORDS.contains(&word)
}

/// The text is only a settings word ("settings", "prefs").
pub fn is_bare(query: &str) -> bool {
    let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
    words.len() == 1 && is_settings_word(&words[0])
}

/// The text minus one settings word at its start or end ("wifi settings" is "wifi"), when
/// what's left is long enough to mean something.
fn strip_settings_word(query: &str) -> Option<String> {
    let words: Vec<&str> = query.split_whitespace().collect();
    if words.len() < 2 {
        return None;
    }
    let lower = |word: &str| word.to_lowercase();
    let rest = if is_settings_word(&lower(words[words.len() - 1])) {
        &words[..words.len() - 1]
    } else if is_settings_word(&lower(words[0])) {
        &words[1..]
    } else {
        return None;
    };
    let rest = rest.join(" ");
    (rest.chars().count() >= 2).then_some(rest)
}

/// The words a setting answers to: its own keywords, plus the cross-platform concepts it is
/// this platform's page for ("night shift" finds Night Light on Windows).
fn words(setting: &Setting) -> impl Iterator<Item = &'static str> + '_ {
    let concepts = CONCEPTS
        .iter()
        .filter(move |concept| concept.windows == setting.id || concept.macos == setting.id)
        .flat_map(|concept| concept.words.iter().copied());
    setting.keywords.iter().copied().chain(concepts)
}

/// How well `query` names this setting, and the keyword it matched when that wasn't the
/// title. The title wins ties with a keyword of the same strength.
fn matched(setting: &Setting, query: &str) -> Option<(Strength, Option<&'static str>)> {
    let query = normalize(query);
    let title = apps::strength(&normalize(setting.title), &query).map(|found| (found, None));
    let keyword = words(setting)
        .filter_map(|word| apps::strength(&normalize(word), &query).map(|found| (found, word)))
        .max_by_key(|(found, _)| *found)
        .map(|(found, word)| (found, Some(word)));
    match (title, keyword) {
        (Some(title), Some(keyword)) if keyword.0 > title.0 => Some(keyword),
        (Some(title), _) => Some(title),
        (None, keyword) => keyword,
    }
}

fn strength(setting: &Setting, query: &str) -> Option<(Strength, bool)> {
    matched(setting, query).map(|(found, keyword)| (found, keyword.is_none()))
}

/// A setting with the keyword that found it, shown dimmed beside the title.
#[derive(Debug, Clone, Copy)]
pub struct Match<'a> {
    pub setting: &'a Setting,
    pub keyword: Option<&'static str>,
}

fn is_zephyr(setting: &Setting) -> bool {
    matches!(setting.target, Target::Zephyr(_))
}

/// What ranks a match: naming a whole word of the page ("shell" in "Zephyr Shell", not just
/// the start of a longer word), then match strength, then title over keyword.
type Score = (bool, Strength, bool);

/// Whether `query` is one or more whole words of `text`.
fn whole_words(text: &str, query: &str) -> bool {
    let text = format!(" {} ", normalize(text));
    text.contains(&format!(" {} ", normalize(query)))
}

fn score(setting: &Setting, query: &str, found: Strength, keyword: Option<&str>) -> Score {
    let whole = whole_words(keyword.unwrap_or(setting.title), query);
    (whole, found, keyword.is_none())
}

/// Ranks matches: score, then the OS's page before the app's own (unless the text says
/// "zephyr"), then the most opened, then catalog order.
fn rank<'a>(
    mut found: Vec<(usize, Match<'a>, Score)>,
    query: &str,
    opens: &[LaunchEntry],
    now: i64,
) -> Vec<Match<'a>> {
    let zephyr_first = query.to_lowercase().contains("zephyr");
    let uses = |item: &Match| apps::frecency(opens, item.setting.id, now);
    found.sort_by(|left, right| {
        right
            .2
            .cmp(&left.2)
            .then_with(|| {
                let later = |item: &Match| is_zephyr(item.setting) != zephyr_first;
                later(&left.1).cmp(&later(&right.1))
            })
            .then_with(|| uses(&right.1).total_cmp(&uses(&left.1)))
            .then(left.0.cmp(&right.0))
    });
    found.into_iter().map(|(_, item, _)| item).collect()
}

/// "wifi settings", "settings dark", "display prefs": a settings word at either end, and
/// the rest naming a page by its title or a keyword (word prefix or better). Up to `limit`
/// pages, best first; empty when the text isn't asking for settings.
pub fn intent<'a>(
    settings: &'a [Setting],
    query: &str,
    opens: &[LaunchEntry],
    now: i64,
    limit: usize,
) -> Vec<Match<'a>> {
    let Some(rest) = strip_settings_word(query) else {
        return Vec::new();
    };
    // A page whose own title has the settings word ("Zephyr Settings") matches whole.
    let whole = normalize(query);
    if let Some(setting) = settings
        .iter()
        .find(|setting| normalize(setting.title) == whole)
    {
        return vec![Match {
            setting,
            keyword: None,
        }];
    }
    let found = settings
        .iter()
        .enumerate()
        .filter_map(|(order, setting)| {
            let (found, keyword) = matched(setting, &rest)?;
            (found >= Strength::WordPrefix).then_some((
                order,
                Match { setting, keyword },
                score(setting, &rest, found, keyword),
            ))
        })
        .collect();
    let mut ranked = rank(found, query, opens, now);
    ranked.truncate(limit);
    ranked
}

/// A page named exactly, with no settings word ("bluetooth", "dark mode"): shown under the
/// web row, never taking Enter.
pub fn exact<'a>(
    settings: &'a [Setting],
    query: &str,
    opens: &[LaunchEntry],
    now: i64,
) -> Option<Match<'a>> {
    let found = settings
        .iter()
        .enumerate()
        .filter_map(|(order, setting)| {
            let (found, keyword) = matched(setting, query)?;
            (found == Strength::Exact).then_some((
                order,
                Match { setting, keyword },
                score(setting, query, found, keyword),
            ))
        })
        .collect();
    rank(found, query, opens, now).into_iter().next()
}

/// The pages for a bare "settings": the most opened, then Wi-Fi, Bluetooth, Displays and
/// Sound. The app's own pages aren't in this list; they have their own row.
pub fn favorites<'a>(
    settings: &'a [Setting],
    opens: &[LaunchEntry],
    now: i64,
    limit: usize,
) -> Vec<&'a Setting> {
    let mut opened: Vec<(&Setting, f64)> = settings
        .iter()
        .filter(|setting| !is_zephyr(setting))
        .map(|setting| (setting, apps::frecency(opens, setting.id, now)))
        .filter(|(_, score)| *score > 0.0)
        .collect();
    opened.sort_by(|left, right| right.1.total_cmp(&left.1));
    let mut chosen: Vec<&Setting> = opened.into_iter().map(|(setting, _)| setting).collect();
    let defaults: &[&str] = if cfg!(windows) {
        &["win.wifi", "win.bluetooth", "win.display", "win.sound"]
    } else {
        &["mac.wifi", "mac.bluetooth", "mac.displays", "mac.sound"]
    };
    for id in defaults {
        if let Some(setting) = settings.iter().find(|setting| setting.id == *id)
            && !chosen.iter().any(|picked| picked.id == setting.id)
        {
            chosen.push(setting);
        }
    }
    chosen.truncate(limit);
    chosen
}

/// Settings matching `query`, best first; equal matches keep catalog order, which is curated by
/// how often people look for each page.
pub fn ranked<'a>(settings: &'a [Setting], query: &str, limit: usize) -> Vec<&'a Setting> {
    let mut matches: Vec<(usize, &Setting, (Strength, bool))> = settings
        .iter()
        .enumerate()
        .filter_map(|(order, setting)| {
            strength(setting, query).map(|found| (order, setting, found))
        })
        .collect();
    matches.sort_by(|left, right| right.2.cmp(&left.2).then(left.0.cmp(&right.0)));
    matches
        .into_iter()
        .take(limit)
        .map(|(_, setting, _)| setting)
        .collect()
}

/// Settings worth showing below unscoped results: only clear name matches, so ordinary searches
/// don't sprout settings rows.
pub fn incidental<'a>(settings: &'a [Setting], query: &str, limit: usize) -> Vec<&'a Setting> {
    if query.trim().chars().count() < 3 {
        return Vec::new();
    }
    let mut matches: Vec<(usize, &Setting, (Strength, bool))> = settings
        .iter()
        .enumerate()
        .filter_map(|(order, setting)| {
            strength(setting, query)
                .filter(|(found, _)| *found >= Strength::WordPrefix)
                .map(|found| (order, setting, found))
        })
        .collect();
    matches.sort_by(|left, right| right.2.cmp(&left.2).then(left.0.cmp(&right.0)));
    matches
        .into_iter()
        .take(limit)
        .map(|(_, setting, _)| setting)
        .collect()
}

#[cfg(windows)]
pub fn open_system(
    target: Target,
    open_uri: impl Fn(&str) -> Result<(), String>,
) -> Result<(), String> {
    match target {
        Target::Uri(uri) => open_uri(uri),
        // Through Explorer, like launching an app: it shows the UAC prompt itself for tools
        // that need elevation (regedit), and a declined prompt is just nothing happening.
        // Opening them directly from the bar failed with "cancelled by the user" (1223).
        Target::System(file) => {
            let root = std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into());
            let path = std::path::PathBuf::from(root).join("System32").join(file);
            std::process::Command::new("explorer.exe")
                .arg(path)
                .spawn()
                .map(|_| ())
                .map_err(|err| err.to_string())
        }
        Target::Command(program, args) => std::process::Command::new(program)
            .args(args)
            .spawn()
            .map(|_| ())
            .map_err(|err| err.to_string()),
        Target::Zephyr(_) => unreachable!("Zephyr settings open in Zephyr's window"),
    }
}

#[cfg(not(windows))]
pub fn open_system(
    target: Target,
    open_uri: impl Fn(&str) -> Result<(), String>,
) -> Result<(), String> {
    match target {
        Target::Uri(uri) => open_uri(uri),
        Target::System(_) | Target::Command(..) => Err("That page only exists on Windows.".into()),
        Target::Zephyr(_) => unreachable!("Zephyr settings open in Zephyr's window"),
    }
}

const fn zephyr(
    id: &'static str,
    title: &'static str,
    section: &'static str,
    keywords: &'static [&'static str],
) -> Setting {
    Setting {
        id,
        title,
        keywords,
        target: Target::Zephyr(section),
    }
}

const fn uri(
    id: &'static str,
    title: &'static str,
    uri: &'static str,
    keywords: &'static [&'static str],
) -> Setting {
    Setting {
        id,
        title,
        keywords,
        target: Target::Uri(uri),
    }
}

const fn system(
    id: &'static str,
    title: &'static str,
    file: &'static str,
    keywords: &'static [&'static str],
) -> Setting {
    Setting {
        id,
        title,
        keywords,
        target: Target::System(file),
    }
}

/// The app's own settings: one page per section of its Settings window (kept in step with
/// SECTIONS in SettingsView.vue by a test), plus rows for single controls, which open the
/// section and flash the control ("general:summon").
const ZEPHYR: &[Setting] = &[
    zephyr(
        "zephyr.settings",
        "Zephyr Settings",
        "general",
        &["preferences", "options"],
    ),
    zephyr(
        "zephyr.general",
        "Zephyr General",
        "general",
        &["return to mode", "keep search text"],
    ),
    zephyr(
        "zephyr.theme",
        "Zephyr Theme",
        "theme",
        &["appearance", "color", "tint"],
    ),
    zephyr(
        "zephyr.about",
        "About Zephyr",
        "about",
        &["zephyr version", "zephyr updates"],
    ),
    zephyr(
        "zephyr.dev",
        "Zephyr Dev",
        "dev",
        &["bug report", "captures", "snapshot", "developer"],
    ),
    zephyr(
        "zephyr.terminal",
        "Zephyr Terminal",
        "terminal",
        &["shell", "bash", "powershell", "command", "wsl"],
    ),
    zephyr(
        "zephyr.resources",
        "Zephyr Resources",
        "resources",
        &[
            "footer stats",
            "cpu meter",
            "gpu",
            "system resources",
            "cpu temperature",
        ],
    ),
    zephyr(
        "zephyr.sync",
        "Zephyr Sync",
        "sync",
        &["sign in", "devices", "recovery key"],
    ),
    zephyr(
        "zephyr.clipboard",
        "Zephyr Clipboard",
        "clipboard",
        &[
            "clipboard history",
            "paste history",
            "copied",
            "ignored apps",
        ],
    ),
    zephyr(
        "zephyr.notes",
        "Zephyr Notes",
        "notes",
        &["notes shortcut", "quick notes", "notes folder"],
    ),
    zephyr(
        "zephyr.shortcuts",
        "Zephyr Shortcuts",
        "shortcuts",
        &["aliases", "hotkeys", "keyboard shortcuts"],
    ),
    zephyr(
        "zephyr.weather",
        "Zephyr Weather",
        "weather",
        &[
            "weather location",
            "celsius",
            "fahrenheit",
            "temperature units",
        ],
    ),
    zephyr(
        "zephyr.network",
        "Zephyr Network",
        "network",
        &[
            "connection monitor",
            "watch connection",
            "ping",
            "speed test",
        ],
    ),
    zephyr(
        "zephyr.launchgroups",
        "Zephyr Launch Groups",
        "launchgroups",
        &["launch group", "open several apps"],
    ),
    zephyr(
        "zephyr.files",
        "Zephyr Files",
        "files",
        &[
            "file search folders",
            "indexed folders",
            "exclude folders",
            "file index",
        ],
    ),
    zephyr(
        "zephyr.permissions",
        "Zephyr Permissions",
        "permissions",
        &["accessibility", "automation", "allow"],
    ),
    zephyr(
        "zephyr.shortcut",
        "Zephyr Summon Shortcut",
        "general:summon",
        &["hotkey", "zephyr shortcut"],
    ),
    zephyr(
        "zephyr.startup",
        "Launch Zephyr at Startup",
        "general:startup",
        &["autostart", "login", "zephyr startup"],
    ),
    zephyr(
        "zephyr.updates",
        "Check for Zephyr Updates",
        "about:updates",
        &["update zephyr", "zephyr version", "about zephyr"],
    ),
];

/// One idea, two platforms: the words people use for it and the page that has it on each.
struct Concept {
    words: &'static [&'static str],
    windows: &'static str,
    macos: &'static str,
}

const fn concept(
    words: &'static [&'static str],
    windows: &'static str,
    macos: &'static str,
) -> Concept {
    Concept {
        words,
        windows,
        macos,
    }
}

/// Shared vocabulary, so "night shift" finds Night Light on Windows and "startup apps"
/// finds Login Items on a Mac. Words only one platform uses stay on that page.
#[rustfmt::skip]
const CONCEPTS: &[Concept] = &[
    concept(&["wifi", "wireless"], "win.wifi", "mac.wifi"),
    concept(&["bluetooth", "pair", "headphones", "airpods"], "win.bluetooth", "mac.bluetooth"),
    concept(&["display", "screen", "monitor", "resolution", "brightness"], "win.display", "mac.displays"),
    concept(&["sound", "audio", "volume", "speakers"], "win.sound", "mac.sound"),
    concept(&["dark mode", "light mode", "accent color"], "win.colors", "mac.appearance"),
    concept(&["wallpaper", "background", "desktop picture"], "win.background", "mac.wallpaper"),
    concept(&["night light", "night shift", "blue light"], "win.nightlight", "mac.displays"),
    concept(&["trackpad", "touchpad", "gestures"], "win.touchpad", "mac.trackpad"),
    concept(&["do not disturb", "focus"], "win.focus", "mac.focus"),
    concept(&["startup apps", "login items", "launch at login"], "win.startupapps", "mac.loginitems"),
    concept(&["default browser"], "win.defaultapps", "mac.dock"),
    concept(&["camera", "webcam"], "win.camera", "mac.privacy.camera"),
    concept(&["microphone", "mic"], "win.microphone", "mac.privacy.microphone"),
    concept(&["location"], "win.location", "mac.privacy.location"),
    concept(&["battery", "power", "energy saver", "low power"], "win.battery", "mac.battery"),
    concept(&["storage", "disk space", "free up space"], "win.storage", "mac.storage"),
    concept(&["update", "software update"], "win.update", "mac.update"),
    concept(&["printer", "scanner"], "win.printers", "mac.printers"),
    concept(&["mouse", "pointer speed"], "win.mouse", "mac.mouse"),
    concept(&["keyboard", "typing"], "win.typing", "mac.keyboard"),
    concept(&["date", "time", "time zone", "clock"], "win.datetime", "mac.datetime"),
    concept(&["language", "region"], "win.language", "mac.language"),
    concept(&["users", "user accounts", "add user"], "win.users", "mac.users"),
    concept(&["vpn"], "win.vpn", "mac.vpn"),
    concept(&["hotspot", "tethering", "internet sharing"], "win.hotspot", "mac.sharing"),
    concept(&["notifications", "alerts"], "win.notifications", "mac.notifications"),
    concept(&["privacy", "security", "permissions"], "win.privacy", "mac.privacy"),
    concept(&["network", "internet", "ip address"], "win.network", "mac.network"),
    concept(&["about", "system info", "specs"], "win.about", "mac.about"),
    concept(&["accessibility", "text size"], "win.textsize", "mac.accessibility"),
    concept(&["lock screen"], "win.lockscreen", "mac.lockscreen"),
    concept(&["backup"], "win.backup", "mac.timemachine"),
    concept(&["sleep", "screen timeout"], "win.power", "mac.lockscreen"),
];

#[rustfmt::skip]
const WINDOWS: &[Setting] = &[
    uri("win.display", "Display", "ms-settings:display", &["screen", "resolution", "monitor", "scale", "brightness", "orientation"]),
    uri("win.nightlight", "Night Light", "ms-settings:nightlight", &["blue light", "warm screen"]),
    uri("win.sound", "Sound", "ms-settings:sound", &["audio", "speakers", "volume", "microphone input", "output device"]),
    uri("win.volume", "Volume Mixer", "ms-settings:apps-volume", &["app volume", "per app audio"]),
    uri("win.bluetooth", "Bluetooth & Devices", "ms-settings:bluetooth", &["bluetooth", "pair", "headphones", "devices"]),
    uri("win.wifi", "Wi-Fi", "ms-settings:network-wifi", &["wifi", "wireless", "network"]),
    uri("win.network", "Network & Internet", "ms-settings:network-status", &["internet", "connection", "ip address"]),
    uri("win.ethernet", "Ethernet", "ms-settings:network-ethernet", &["wired", "lan"]),
    uri("win.vpn", "VPN", "ms-settings:network-vpn", &["virtual private network"]),
    uri("win.hotspot", "Mobile Hotspot", "ms-settings:network-mobilehotspot", &["tethering", "share internet"]),
    uri("win.airplane", "Airplane Mode", "ms-settings:network-airplanemode", &["flight mode"]),
    uri("win.proxy", "Proxy", "ms-settings:network-proxy", &["proxy server"]),
    uri("win.colors", "Colors", "ms-settings:colors", &["dark mode", "light mode", "theme color", "accent color"]),
    uri("win.background", "Background", "ms-settings:personalization-background", &["wallpaper", "desktop background"]),
    uri("win.themes", "Themes", "ms-settings:themes", &["theme", "desktop icons"]),
    uri("win.lockscreen", "Lock Screen", "ms-settings:lockscreen", &["screen saver", "lock screen picture"]),
    uri("win.start", "Start Menu", "ms-settings:personalization-start", &["start"]),
    uri("win.taskbar", "Taskbar", "ms-settings:taskbar", &["system tray", "notification area"]),
    uri("win.fonts", "Fonts", "ms-settings:fonts", &["typeface", "install font"]),
    uri("win.notifications", "Notifications", "ms-settings:notifications", &["alerts", "toasts"]),
    uri("win.focus", "Focus", "ms-settings:quiethours", &["do not disturb", "focus assist", "quiet hours"]),
    uri("win.power", "Power & Sleep", "ms-settings:powersleep", &["sleep", "power", "screen timeout", "shutdown", "power plan"]),
    uri("win.battery", "Battery Saver", "ms-settings:batterysaver", &["battery", "energy saver"]),
    uri("win.storage", "Storage", "ms-settings:storagesense", &["disk space", "storage sense", "free up space", "cleanup"]),
    uri("win.multitasking", "Multitasking", "ms-settings:multitasking", &["snap windows", "alt tab", "virtual desktops"]),
    uri("win.clipboard", "Clipboard", "ms-settings:clipboard", &["clipboard history"]),
    uri("win.remotedesktop", "Remote Desktop", "ms-settings:remotedesktop", &["rdp"]),
    uri("win.about", "About", "ms-settings:about", &["system info", "pc name", "rename pc", "windows version", "specs"]),
    uri("win.printers", "Printers & Scanners", "ms-settings:printers", &["printer", "scanner", "print"]),
    uri("win.mouse", "Mouse", "ms-settings:mousetouchpad", &["scroll", "pointer speed", "mouse buttons"]),
    uri("win.touchpad", "Touchpad", "ms-settings:devices-touchpad", &["trackpad", "gestures"]),
    uri("win.typing", "Typing", "ms-settings:typing", &["autocorrect", "spell check", "touch keyboard"]),
    uri("win.pen", "Pen & Windows Ink", "ms-settings:pen", &["stylus"]),
    uri("win.autoplay", "AutoPlay", "ms-settings:autoplay", &["usb autoplay"]),
    uri("win.usb", "USB", "ms-settings:usb", &["usb devices"]),
    uri("win.apps", "Installed Apps", "ms-settings:appsfeatures", &["uninstall", "remove app", "apps and features", "programs"]),
    uri("win.defaultapps", "Default Apps", "ms-settings:defaultapps", &["default browser", "file associations", "open with"]),
    uri("win.optionalfeatures", "Optional Features", "ms-settings:optionalfeatures", &["windows features"]),
    uri("win.startupapps", "Startup Apps", "ms-settings:startupapps", &["startup", "login items", "run at startup"]),
    uri("win.yourinfo", "Your Info", "ms-settings:yourinfo", &["account", "microsoft account", "profile picture"]),
    uri("win.signin", "Sign-in Options", "ms-settings:signinoptions", &["password", "pin", "windows hello", "fingerprint", "face recognition"]),
    uri("win.email", "Email & Accounts", "ms-settings:emailandaccounts", &["email accounts"]),
    uri("win.work", "Access Work or School", "ms-settings:workplace", &["work account", "mdm", "school account"]),
    uri("win.users", "Other Users", "ms-settings:otherusers", &["family", "add user", "user accounts"]),
    uri("win.datetime", "Date & Time", "ms-settings:dateandtime", &["time zone", "clock", "date", "time"]),
    uri("win.language", "Language & Region", "ms-settings:regionlanguage", &["language", "region", "keyboard layout", "input method", "locale"]),
    uri("win.speech", "Speech", "ms-settings:speech", &["voice", "dictation"]),
    uri("win.gamemode", "Game Mode", "ms-settings:gaming-gamemode", &["gaming"]),
    uri("win.gamebar", "Game Bar", "ms-settings:gaming-gamebar", &["xbox game bar", "screen recording"]),
    uri("win.graphics", "Graphics", "ms-settings:display-advancedgraphics", &["gpu", "graphics card", "hdr"]),
    uri("win.textsize", "Text Size", "ms-settings:easeofaccess-display", &["font size", "bigger text", "accessibility"]),
    uri("win.colorfilters", "Color Filters", "ms-settings:easeofaccess-colorfilter", &["color blind", "grayscale", "invert colors"]),
    uri("win.contrast", "Contrast Themes", "ms-settings:easeofaccess-highcontrast", &["high contrast"]),
    uri("win.narrator", "Narrator", "ms-settings:easeofaccess-narrator", &["screen reader"]),
    uri("win.magnifier", "Magnifier", "ms-settings:easeofaccess-magnifier", &["zoom", "magnify"]),
    uri("win.keyboard", "Keyboard Accessibility", "ms-settings:easeofaccess-keyboard", &["sticky keys", "filter keys", "on-screen keyboard"]),
    uri("win.pointer", "Mouse Pointer", "ms-settings:easeofaccess-mousepointer", &["cursor size", "pointer size", "cursor color"]),
    uri("win.privacy", "Privacy & Security", "ms-settings:privacy", &["privacy", "permissions", "security"]),
    uri("win.location", "Location", "ms-settings:privacy-location", &["location services", "gps"]),
    uri("win.camera", "Camera Privacy", "ms-settings:privacy-webcam", &["camera", "webcam"]),
    uri("win.microphone", "Microphone Privacy", "ms-settings:privacy-microphone", &["microphone", "mic"]),
    uri("win.update", "Windows Update", "ms-settings:windowsupdate", &["updates", "patch", "upgrade"]),
    uri("win.security", "Windows Security", "ms-settings:windowsdefender", &["defender", "antivirus", "virus protection", "firewall"]),
    uri("win.recovery", "Recovery", "ms-settings:recovery", &["reset pc", "advanced startup", "restore"]),
    uri("win.activation", "Activation", "ms-settings:activation", &["product key", "license"]),
    uri("win.backup", "Windows Backup", "ms-settings:backup", &["backup", "onedrive backup"]),
    uri("win.troubleshoot", "Troubleshoot", "ms-settings:troubleshoot", &["troubleshooter", "fix problems"]),
    uri("win.developers", "For Developers", "ms-settings:developers", &["developer mode", "sudo"]),
    Setting {
        id: "win.envvars",
        title: "Environment Variables",
        keywords: &["path", "env vars", "system variables"],
        target: Target::Command("rundll32.exe", &["sysdm.cpl,EditEnvironmentVariables"]),
    },
    system("win.controlpanel", "Control Panel", "control.exe", &["classic settings"]),
    system("win.programs", "Programs and Features", "appwiz.cpl", &["uninstall a program", "add remove programs"]),
    system("win.connections", "Network Connections", "ncpa.cpl", &["adapter settings", "network adapters", "dns"]),
    system("win.system", "System Properties", "sysdm.cpl", &["computer name", "advanced system settings", "performance options"]),
    system("win.soundpanel", "Sound Control Panel", "mmsys.cpl", &["playback devices", "recording devices"]),
    system("win.poweroptions", "Power Options", "powercfg.cpl", &["power plan", "lid close"]),
    system("win.internetoptions", "Internet Options", "inetcpl.cpl", &["internet properties"]),
    system("win.firewall", "Windows Defender Firewall", "firewall.cpl", &["firewall rules"]),
    system("win.devices", "Device Manager", "devmgmt.msc", &["drivers", "hardware"]),
    system("win.disks", "Disk Management", "diskmgmt.msc", &["partitions", "format drive", "volumes"]),
    system("win.services", "Services", "services.msc", &["windows services"]),
    system("win.tasks", "Task Scheduler", "taskschd.msc", &["scheduled tasks"]),
    system("win.events", "Event Viewer", "eventvwr.msc", &["logs", "event log"]),
    system("win.computer", "Computer Management", "compmgmt.msc", &["manage"]),
    system("win.taskmanager", "Task Manager", "taskmgr.exe", &["processes", "kill process", "performance"]),
    system("win.registry", "Registry Editor", "regedit.exe", &["regedit", "registry"]),
];

/// Ventura+ System Settings extension ids; privacy pages keep the legacy anchor ids, which
/// macOS still routes to the right pane.
#[rustfmt::skip]
const MACOS: &[Setting] = &[
    uri("mac.wifi", "Wi-Fi", "x-apple.systempreferences:com.apple.wifi-settings-extension", &["wifi", "wireless", "network"]),
    uri("mac.bluetooth", "Bluetooth", "x-apple.systempreferences:com.apple.BluetoothSettings", &["pair", "headphones", "airpods"]),
    uri("mac.network", "Network", "x-apple.systempreferences:com.apple.Network-Settings.extension", &["internet", "ethernet", "ip address", "dns", "proxy"]),
    uri("mac.vpn", "VPN", "x-apple.systempreferences:com.apple.NetworkExtensionSettingsUI.NESettingsUIExtension", &["virtual private network"]),
    uri("mac.displays", "Displays", "x-apple.systempreferences:com.apple.Displays-Settings.extension", &["display", "screen", "resolution", "monitor", "brightness", "night shift"]),
    uri("mac.sound", "Sound", "x-apple.systempreferences:com.apple.Sound-Settings.extension", &["audio", "volume", "speakers", "microphone input", "output device"]),
    uri("mac.appearance", "Appearance", "x-apple.systempreferences:com.apple.Appearance-Settings.extension", &["dark mode", "light mode", "accent color"]),
    uri("mac.wallpaper", "Wallpaper", "x-apple.systempreferences:com.apple.Wallpaper-Settings.extension", &["background", "desktop picture"]),
    uri("mac.screensaver", "Screen Saver", "x-apple.systempreferences:com.apple.ScreenSaver-Settings.extension", &["screensaver"]),
    uri("mac.dock", "Desktop & Dock", "x-apple.systempreferences:com.apple.Desktop-Settings.extension", &["dock", "mission control", "hot corners", "stage manager", "default browser"]),
    uri("mac.controlcenter", "Control Center", "x-apple.systempreferences:com.apple.ControlCenter-Settings.extension", &["menu bar"]),
    uri("mac.notifications", "Notifications", "x-apple.systempreferences:com.apple.Notifications-Settings.extension", &["alerts", "banners"]),
    uri("mac.focus", "Focus", "x-apple.systempreferences:com.apple.Focus-Settings.extension", &["do not disturb"]),
    uri("mac.screentime", "Screen Time", "x-apple.systempreferences:com.apple.Screen-Time-Settings.extension", &["parental controls", "app limits"]),
    uri("mac.battery", "Battery", "x-apple.systempreferences:com.apple.Battery-Settings.extension", &["energy saver", "power", "low power mode", "sleep"]),
    uri("mac.lockscreen", "Lock Screen", "x-apple.systempreferences:com.apple.Lock-Screen-Settings.extension", &["screen timeout", "require password"]),
    uri("mac.touchid", "Touch ID & Password", "x-apple.systempreferences:com.apple.Touch-ID-Settings.extension", &["fingerprint", "password", "apple watch unlock"]),
    uri("mac.users", "Users & Groups", "x-apple.systempreferences:com.apple.Users-Groups-Settings.extension", &["user accounts", "add user", "guest"]),
    uri("mac.passwords", "Passwords", "x-apple.systempreferences:com.apple.Passwords-Settings.extension", &["keychain", "saved passwords"]),
    uri("mac.accounts", "Internet Accounts", "x-apple.systempreferences:com.apple.Internet-Accounts-Settings.extension", &["email accounts", "google account"]),
    uri("mac.keyboard", "Keyboard", "x-apple.systempreferences:com.apple.Keyboard-Settings.extension", &["key repeat", "keyboard shortcuts", "input sources", "dictation", "keyboard layout"]),
    uri("mac.trackpad", "Trackpad", "x-apple.systempreferences:com.apple.Trackpad-Settings.extension", &["gestures", "tap to click", "scroll direction"]),
    uri("mac.mouse", "Mouse", "x-apple.systempreferences:com.apple.Mouse-Settings.extension", &["pointer speed", "scroll direction"]),
    uri("mac.printers", "Printers & Scanners", "x-apple.systempreferences:com.apple.Print-Scan-Settings.extension", &["printer", "scanner", "print"]),
    uri("mac.general", "General", "x-apple.systempreferences:com.apple.systempreferences.GeneralSettings", &["general settings"]),
    uri("mac.about", "About This Mac", "x-apple.systempreferences:com.apple.SystemProfiler.AboutExtension", &["about", "system info", "serial number", "macos version", "specs"]),
    uri("mac.update", "Software Update", "x-apple.systempreferences:com.apple.Software-Update-Settings.extension", &["updates", "upgrade macos"]),
    uri("mac.storage", "Storage", "x-apple.systempreferences:com.apple.settings.Storage", &["disk space", "free up space"]),
    uri("mac.airdrop", "AirDrop & Handoff", "x-apple.systempreferences:com.apple.AirDrop-Handoff-Settings.extension", &["airdrop", "handoff", "airplay receiver"]),
    uri("mac.loginitems", "Login Items", "x-apple.systempreferences:com.apple.LoginItems-Settings.extension", &["startup apps", "launch at login", "background items"]),
    uri("mac.language", "Language & Region", "x-apple.systempreferences:com.apple.Localization-Settings.extension", &["language", "region", "locale", "date format"]),
    uri("mac.datetime", "Date & Time", "x-apple.systempreferences:com.apple.Date-Time-Settings.extension", &["time zone", "clock", "date", "time"]),
    uri("mac.sharing", "Sharing", "x-apple.systempreferences:com.apple.Sharing-Settings.extension", &["file sharing", "screen sharing", "remote login", "ssh", "computer name"]),
    uri("mac.timemachine", "Time Machine", "x-apple.systempreferences:com.apple.Time-Machine-Settings.extension", &["backup"]),
    uri("mac.reset", "Transfer or Reset", "x-apple.systempreferences:com.apple.Transfer-Reset-Settings.extension", &["erase all content", "factory reset"]),
    uri("mac.startupdisk", "Startup Disk", "x-apple.systempreferences:com.apple.Startup-Disk-Settings.extension", &["boot disk"]),
    uri("mac.accessibility", "Accessibility", "x-apple.systempreferences:com.apple.Accessibility-Settings.extension", &["voiceover", "zoom", "text size", "reduce motion", "screen reader"]),
    uri("mac.siri", "Siri", "x-apple.systempreferences:com.apple.Siri-Settings.extension", &["apple intelligence", "voice assistant"]),
    uri("mac.gamecenter", "Game Center", "x-apple.systempreferences:com.apple.Game-Center-Settings.extension", &["gaming"]),
    uri("mac.privacy", "Privacy & Security", "x-apple.systempreferences:com.apple.settings.PrivacySecurity.extension", &["privacy", "security", "permissions", "filevault", "gatekeeper", "firewall"]),
    uri("mac.privacy.accessibility", "Accessibility Permissions", "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility", &["allow accessibility", "control computer"]),
    uri("mac.privacy.screen", "Screen Recording Permissions", "x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture", &["screen recording", "screen capture", "screen and audio recording"]),
    uri("mac.privacy.disk", "Full Disk Access", "x-apple.systempreferences:com.apple.preference.security?Privacy_AllFiles", &["disk access", "file access"]),
    uri("mac.privacy.camera", "Camera Permissions", "x-apple.systempreferences:com.apple.preference.security?Privacy_Camera", &["camera", "webcam"]),
    uri("mac.privacy.microphone", "Microphone Permissions", "x-apple.systempreferences:com.apple.preference.security?Privacy_Microphone", &["microphone", "mic"]),
    uri("mac.privacy.location", "Location Services", "x-apple.systempreferences:com.apple.preference.security?Privacy_LocationServices", &["location", "gps"]),
    uri("mac.privacy.automation", "Automation Permissions", "x-apple.systempreferences:com.apple.preference.security?Privacy_Automation", &["apple events", "applescript"]),
];

#[cfg(test)]
mod tests {
    use super::*;

    fn titles(found: Vec<&Setting>) -> Vec<&'static str> {
        found.into_iter().map(|setting| setting.title).collect()
    }

    #[test]
    fn ids_are_unique_and_targets_fit_their_platform() {
        let mut seen = std::collections::HashSet::new();
        for setting in ZEPHYR.iter().chain(WINDOWS).chain(MACOS) {
            assert!(seen.insert(setting.id), "duplicate id {}", setting.id);
            assert!(!setting.title.is_empty());
        }
        for setting in ZEPHYR {
            assert!(matches!(setting.target, Target::Zephyr(_)));
        }
        for concept in CONCEPTS {
            assert!(
                WINDOWS.iter().any(|s| s.id == concept.windows),
                "{}",
                concept.windows
            );
            assert!(
                MACOS.iter().any(|s| s.id == concept.macos),
                "{}",
                concept.macos
            );
        }
        for setting in WINDOWS {
            if let Target::Uri(uri) = setting.target {
                assert!(uri.starts_with("ms-settings:"), "{uri}");
            }
            assert!(!matches!(setting.target, Target::Zephyr(_)));
        }
        for setting in MACOS {
            let Target::Uri(uri) = setting.target else {
                panic!("{} should be a URI", setting.id);
            };
            assert!(
                uri.starts_with("x-apple.systempreferences:com.apple."),
                "{uri}"
            );
        }
    }

    #[test]
    fn synonyms_find_the_page() {
        assert_eq!(titles(ranked(WINDOWS, "dark mode", 1)), ["Colors"]);
        assert_eq!(titles(ranked(MACOS, "dark mode", 1)), ["Appearance"]);
        assert_eq!(titles(ranked(WINDOWS, "wifi", 1)), ["Wi-Fi"]);
        assert_eq!(titles(ranked(WINDOWS, "env", 1)), ["Environment Variables"]);
        assert_eq!(
            titles(ranked(MACOS, "screen rec", 1)),
            ["Screen Recording Permissions"]
        );
        assert_eq!(
            titles(ranked(ZEPHYR, "launch group", 1)),
            ["Zephyr Launch Groups"]
        );
    }

    #[test]
    fn a_title_match_beats_a_keyword_match_of_the_same_strength() {
        // "Sound" is a title on one page and only a keyword prefix elsewhere.
        assert_eq!(
            titles(ranked(WINDOWS, "sound", 2)),
            ["Sound", "Sound Control Panel"]
        );
        assert_eq!(titles(ranked(WINDOWS, "display", 1)), ["Display"]);
    }

    #[test]
    fn unscoped_text_only_shows_clear_name_matches() {
        assert_eq!(
            titles(incidental(WINDOWS, "bluetooth", 2)),
            ["Bluetooth & Devices"]
        );
        assert!(incidental(WINDOWS, "bluetooth headphones review", 2).is_empty());
        assert!(incidental(WINDOWS, "dis", 2).len() <= 2);
        assert!(incidental(WINDOWS, "di", 2).is_empty());
        // A loose subsequence is fine in the scope but not as an unasked-for row.
        assert!(incidental(WINDOWS, "dsply", 2).is_empty());
        assert_eq!(titles(ranked(WINDOWS, "dsply", 1)), ["Display"]);
    }

    #[test]
    fn scope_words_are_recognized() {
        assert!(is_scope("set"));
        assert!(is_scope("settings"));
        assert!(!is_scope("s"));
    }

    fn intent_titles(settings: &[Setting], query: &str) -> Vec<&'static str> {
        intent(settings, query, &[], 0, 3)
            .into_iter()
            .map(|found| found.setting.title)
            .collect()
    }

    fn all(system: &[Setting]) -> Vec<Setting> {
        ZEPHYR.iter().chain(system).copied().collect()
    }

    #[test]
    fn spelling_variants_and_plurals_fold() {
        assert_eq!(normalize("Wi-Fi"), "wifi");
        assert_eq!(normalize("wi fi settings"), "wifi setting");
        assert_eq!(normalize("Printers & Scanners"), "printer and scanner");
        assert_eq!(normalize("Displays"), "display");
        assert_eq!(normalize("access"), "access");
    }

    #[test]
    fn a_settings_word_at_either_end_finds_the_page() {
        for system in [WINDOWS, MACOS] {
            let catalog = all(system);
            let wifi = intent_titles(&catalog, "wifi settings");
            assert_eq!(wifi.first(), Some(&"Wi-Fi"), "{wifi:?}");
            assert_eq!(
                intent_titles(&catalog, "settings wifi").first(),
                Some(&"Wi-Fi")
            );
            assert!(intent_titles(&catalog, "display prefs")[0].starts_with("Display"));
            let dark = intent_titles(&catalog, "settings dark");
            assert!(["Colors", "Appearance"].contains(&dark[0]), "{dark:?}");
            assert_eq!(
                intent_titles(&catalog, "shell settings")[0],
                "Zephyr Terminal"
            );
            assert!(intent_titles(&catalog, "iphone wifi settings not working").is_empty());
            assert!(intent_titles(&catalog, "wifi").is_empty());
        }
    }

    #[test]
    fn the_os_page_comes_before_zephyrs_unless_the_text_says_zephyr() {
        let catalog = all(MACOS);
        assert_eq!(
            intent_titles(&catalog, "appearance settings")[..2],
            ["Appearance", "Zephyr Theme"]
        );
        assert_eq!(
            intent_titles(&catalog, "zephyr theme settings")[0],
            "Zephyr Theme"
        );
        assert_eq!(
            intent_titles(&catalog, "zephyr settings"),
            ["Zephyr Settings"]
        );
    }

    #[test]
    fn an_exact_page_name_without_a_settings_word_is_found_exactly() {
        let mac = all(MACOS);
        assert_eq!(
            exact(&mac, "bluetooth", &[], 0).unwrap().setting.title,
            "Bluetooth"
        );
        let dark = exact(&mac, "dark mode", &[], 0).unwrap();
        assert_eq!(
            (dark.setting.title, dark.keyword),
            ("Appearance", Some("dark mode"))
        );
        let windows = all(WINDOWS);
        assert_eq!(
            exact(&windows, "power options", &[], 0)
                .unwrap()
                .setting
                .title,
            "Power Options"
        );
        assert!(exact(&windows, "blue", &[], 0).is_none());
    }

    #[test]
    fn a_bare_settings_word_lists_the_usual_pages() {
        assert!(is_bare("Settings"));
        assert!(is_bare("prefs"));
        assert!(!is_bare("wifi settings"));
        // The usual pages are this platform's.
        let (catalog, usual, keyboard) = if cfg!(windows) {
            (
                all(WINDOWS),
                ["Wi-Fi", "Bluetooth & Devices", "Display", "Sound"],
                ("win.keyboard", "Keyboard Accessibility"),
            )
        } else {
            (
                all(MACOS),
                ["Wi-Fi", "Bluetooth", "Displays", "Sound"],
                ("mac.keyboard", "Keyboard"),
            )
        };
        let titles: Vec<_> = favorites(&catalog, &[], 0, 4)
            .iter()
            .map(|s| s.title)
            .collect();
        assert_eq!(titles, usual);
        let opened = [LaunchEntry {
            app_id: keyboard.0.into(),
            uses: 3,
            last_used: 0,
        }];
        assert_eq!(favorites(&catalog, &opened, 0, 4)[0].title, keyboard.1);
    }

    #[test]
    fn zephyr_rows_cover_every_settings_section() {
        // SECTIONS in the Settings window is the source of truth.
        let view = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../src/settings/SettingsView.vue"
        ))
        .unwrap();
        let start = view.find("const SECTIONS = [").unwrap();
        let block = &view[start..start + view[start..].find("];").unwrap()];
        for line in block.lines() {
            let Some(id) = line
                .split("id: '")
                .nth(1)
                .and_then(|rest| rest.split('\'').next())
            else {
                continue;
            };
            assert!(
                ZEPHYR.iter().any(
                    |setting| matches!(setting.target, Target::Zephyr(section) if section == id)
                ),
                "no Zephyr row opens the {id} section"
            );
        }
    }
}
