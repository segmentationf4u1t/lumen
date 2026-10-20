use crate::config::AppRule;
use windows_sys::Win32::Foundation::CloseHandle;
use windows_sys::Win32::System::Threading::{
    GetCurrentProcessId, OpenProcess, QueryFullProcessImageNameW, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};

/// Basename of the foreground process, lower-cased. `None` for protected,
/// elevated, or transient windows; callers must keep the previous profile.
pub fn foreground_process_name() -> Option<String> {
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.is_null() {
            return None;
        }
        let mut pid = 0u32;
        if GetWindowThreadProcessId(hwnd, &mut pid) == 0 || pid == 0 {
            return None;
        }
        // Our own hidden tray window must never become an "application".
        if pid == GetCurrentProcessId() {
            return None;
        }
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if process.is_null() {
            return None;
        }
        let mut buf = [0u16; 1024];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(process, 0, buf.as_mut_ptr(), &mut len);
        CloseHandle(process);
        if ok == 0 || len == 0 {
            return None;
        }
        let path = String::from_utf16_lossy(&buf[..len as usize]);
        Some(basename(&path).to_ascii_lowercase())
    }
}

pub fn basename(path: &str) -> &str {
    path.rsplit(['\\', '/'])
        .find(|s| !s.is_empty())
        .unwrap_or(path)
}

/// First rule whose process basename matches. Matching is case-insensitive.
pub fn match_rule<'a>(rules: &'a [AppRule], process: &str) -> Option<&'a AppRule> {
    let process = basename(process).to_ascii_lowercase();
    rules
        .iter()
        .find(|r| basename(&r.process).to_ascii_lowercase() == process)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basename_handles_windows_and_unix_paths() {
        assert_eq!(basename(r"C:\Games\cs2.exe"), "cs2.exe");
        assert_eq!(basename("/usr/bin/game"), "game");
        assert_eq!(basename("game.exe"), "game.exe");
    }

    #[test]
    fn rule_matching_is_case_insensitive_basename() {
        let rules = vec![AppRule {
            process: r"C:\Games\CS2.EXE".into(),
            profile: "fps".into(),
        }];
        assert_eq!(
            match_rule(&rules, "cs2.exe").map(|r| r.profile.as_str()),
            Some("fps")
        );
        assert!(match_rule(&rules, "other.exe").is_none());
    }
}
