use crate::config::AppRule;

// `foreground_process_name() -> Option<String>`: basename of the focused
// window's process, lower-cased. `None` when it can't be determined; callers
// must keep the previous profile.
#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
pub use linux::foreground_process_name;
#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use windows::foreground_process_name;

pub fn basename(path: &str) -> &str {
    path.rsplit(['\\', '/'])
        .find(|s| !s.is_empty())
        .unwrap_or(path)
}

/// First rule whose process basename matches. Matching is case-insensitive.
pub fn match_rule<'a>(rules: &'a [AppRule], process: &str) -> Option<&'a AppRule> {
    let process = basename(process);
    rules
        .iter()
        .find(|r| basename(&r.process).eq_ignore_ascii_case(process))
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
