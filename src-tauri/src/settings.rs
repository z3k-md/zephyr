use crate::apps::{self, Strength};

/// Bang words that scope the bar to settings instead of naming a destination.
pub const SCOPE_TRIGGERS: &[&str] = &["set", "settings"];

pub fn is_scope(trigger: &str) -> bool {
    SCOPE_TRIGGERS.contains(&trigger)
}

/// A settings page the bar can open: an OS settings pane or a section of Zephyr's own window.
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
    /// A section id in Zephyr's settings window.
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
            Target::Zephyr(_) => "Zephyr settings",
            Target::System(_) | Target::Command(..) => "Control Panel",
            Target::Uri(_) => "System settings",
        }
    }
}

/// The settings this platform can open: Zephyr's own first, then the OS's.
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
        ZEPHYR.iter().chain(system).copied().collect()
    })
}

pub fn find(id: &str) -> Option<&'static Setting> {
    catalog().iter().find(|setting| setting.id == id)
}

/// How well `query` names this setting. The title wins ties with a keyword of the same strength.
fn strength(setting: &Setting, query: &str) -> Option<(Strength, bool)> {
    let title = apps::strength(setting.title, query).map(|found| (found, true));
    let keyword = setting
        .keywords
        .iter()
        .filter_map(|keyword| apps::strength(keyword, query))
        .max()
        .map(|found| (found, false));
    title.max(keyword)
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
        Target::System(file) => {
            let root = std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into());
            let path = std::path::PathBuf::from(root).join("System32").join(file);
            open_uri(&path.to_string_lossy())
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

const ZEPHYR: &[Setting] = &[
    zephyr(
        "zephyr.settings",
        "Zephyr Settings",
        "general",
        &["preferences", "options"],
    ),
    zephyr(
        "zephyr.shortcut",
        "Zephyr Summon Shortcut",
        "general",
        &["hotkey", "keyboard shortcut", "zephyr shortcut"],
    ),
    zephyr(
        "zephyr.startup",
        "Launch Zephyr at Startup",
        "general",
        &["autostart", "login", "zephyr startup"],
    ),
    zephyr(
        "zephyr.default",
        "Zephyr Default Destination",
        "general",
        &["default search engine", "default destination"],
    ),
    zephyr(
        "zephyr.updates",
        "Check for Zephyr Updates",
        "general",
        &["update zephyr", "zephyr version"],
    ),
    zephyr(
        "zephyr.destinations",
        "Zephyr Destinations",
        "destinations",
        &[
            "add destination",
            "search engines",
            "bangs",
            "triggers",
            "edit destination",
        ],
    ),
    zephyr(
        "zephyr.claude",
        "Zephyr Claude Projects",
        "claude",
        &["claude code", "claude projects", "background agent"],
    ),
    zephyr(
        "zephyr.shell",
        "Zephyr Shell",
        "shell",
        &["terminal", "bash", "powershell", "command", "wsl"],
    ),
    zephyr(
        "zephyr.notes",
        "Zephyr Notes",
        "notes",
        &["notes shortcut", "quick notes", "notes folder"],
    ),
    zephyr(
        "zephyr.clipboard",
        "Zephyr Clipboard History",
        "clipboard",
        &["clipboard", "paste history", "copied", "ignored apps"],
    ),
    zephyr(
        "zephyr.ai",
        "Zephyr AI",
        "ai",
        &[
            "ai provider",
            "api key",
            "ollama",
            "lm studio",
            "anthropic",
            "openai",
            "model",
        ],
    ),
    zephyr(
        "zephyr.history",
        "Zephyr Search History",
        "history",
        &["clear history", "recent searches"],
    ),
    zephyr(
        "zephyr.files",
        "Zephyr File Search",
        "files",
        &[
            "file search folders",
            "indexed folders",
            "exclude folders",
            "file index",
        ],
    ),
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
            assert!(matches!(
                setting.target,
                Target::Zephyr(
                    "general"
                        | "ai"
                        | "claude"
                        | "clipboard"
                        | "shell"
                        | "notes"
                        | "destinations"
                        | "files"
                        | "history"
                )
            ));
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
            titles(ranked(ZEPHYR, "add dest", 1)),
            ["Zephyr Destinations"]
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
}
