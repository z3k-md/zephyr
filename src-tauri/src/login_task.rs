//! Starting with Windows. Programs in the registry's Run key are held back for several
//! seconds after sign-in (Explorer staggers them), which left Alt+Space doing nothing right
//! after login. A Task Scheduler task with a logon trigger starts as soon as you sign in. It
//! runs as you, with no admin rights, and creating it needs none.
//!
//! It's opt-in (Settings > General, "Start instantly at sign-in"): Defender's behavior scan
//! flagged an unsigned app registering itself this way as Behavior:Win32/Persistence.A!ml and
//! quarantined it (2.8.0), so the startup list stays the default.

#[cfg(windows)]
const TASK: &str = "Zephyr";

/// Turns starting at sign-in on (pointing at this copy of the app) or off.
#[cfg(windows)]
pub fn set(enabled: bool) -> Result<(), String> {
    if !enabled {
        if exists() {
            schtasks(&["/Delete", "/TN", TASK, "/F"])?;
        }
        return Ok(());
    }
    let exe = std::env::current_exe().map_err(|err| err.to_string())?;
    let user = format!(
        "{}\\{}",
        std::env::var("USERDOMAIN").unwrap_or_default(),
        std::env::var("USERNAME").unwrap_or_default()
    );
    let xml = task_xml(&exe.to_string_lossy(), &user);
    // Task Scheduler reads the definition as UTF-16, as its own exports are.
    let mut bytes = vec![0xFF, 0xFE];
    bytes.extend(xml.encode_utf16().flat_map(u16::to_le_bytes));
    let file = std::env::temp_dir().join("zephyr-login-task.xml");
    std::fs::write(&file, bytes).map_err(|err| err.to_string())?;
    let created = schtasks(&[
        "/Create",
        "/TN",
        TASK,
        "/XML",
        &file.to_string_lossy(),
        "/F",
    ]);
    let _ = std::fs::remove_file(&file);
    created
}

#[cfg(windows)]
fn exists() -> bool {
    schtasks(&["/Query", "/TN", TASK]).is_ok()
}

#[cfg(windows)]
fn schtasks(args: &[&str]) -> Result<(), String> {
    let output = crate::util::hidden_command("schtasks")
        .args(args)
        .output()
        .map_err(|err| format!("Couldn't run schtasks: {err}"))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(format!(
            "schtasks {} failed: {}",
            args.first().copied().unwrap_or_default(),
            String::from_utf8_lossy(&output.stderr).trim()
        ))
    }
}

/// A logon task for `user` that runs `exe`: no delay, no time limit, on battery too, and at
/// normal priority (scheduled tasks otherwise start below normal).
#[cfg_attr(not(windows), allow(dead_code))]
fn task_xml(exe: &str, user: &str) -> String {
    let escape = |text: &str| {
        text.replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('"', "&quot;")
    };
    let (exe, user) = (escape(exe), escape(user));
    format!(
        r#"<?xml version="1.0" encoding="UTF-16"?>
<Task version="1.2" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
  <RegistrationInfo>
    <Description>Starts Zephyr when you sign in, so its shortcut works right away.</Description>
  </RegistrationInfo>
  <Triggers>
    <LogonTrigger>
      <Enabled>true</Enabled>
      <UserId>{user}</UserId>
    </LogonTrigger>
  </Triggers>
  <Principals>
    <Principal id="Author">
      <UserId>{user}</UserId>
      <LogonType>InteractiveToken</LogonType>
      <RunLevel>LeastPrivilege</RunLevel>
    </Principal>
  </Principals>
  <Settings>
    <MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>
    <DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>
    <StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>
    <ExecutionTimeLimit>PT0S</ExecutionTimeLimit>
    <Priority>4</Priority>
  </Settings>
  <Actions Context="Author">
    <Exec>
      <Command>"{exe}"</Command>
    </Exec>
  </Actions>
</Task>
"#
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_task_runs_this_program_at_this_users_logon() {
        let xml = task_xml(
            r"C:\Users\Zach\AppData\Local\Zephyr\zephyr.exe",
            r"ORION\Zach",
        );
        assert!(
            xml.contains(r#"<Command>"C:\Users\Zach\AppData\Local\Zephyr\zephyr.exe"</Command>"#)
        );
        assert_eq!(xml.matches(r"<UserId>ORION\Zach</UserId>").count(), 2);
        assert!(xml.contains("<LogonTrigger>"));
        assert!(xml.contains("<ExecutionTimeLimit>PT0S</ExecutionTimeLimit>"));
        let odd = task_xml(r"C:\A&B\z.exe", "D\\<x>");
        assert!(odd.contains(r"C:\A&amp;B\z.exe") && odd.contains("D\\&lt;x&gt;"));
    }
}
