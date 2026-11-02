//! Plasma colour scheme lookup, so the resting icon uses the panel's own text colour.
//!
//! Plasma rewrites `~/.config/kdeglobals` when the colour scheme changes; the `[Colors:Window]`
//! `ForegroundNormal` entry is the text colour the default panel uses.

use std::fs;
use std::path::PathBuf;
use std::time::SystemTime;

/// Used when no scheme can be read. Breeze Dark's text colour: legible on a dark panel.
pub const FALLBACK_FG: [u8; 3] = [0xef, 0xf0, 0xf1];

fn config_path() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(base.join("kdeglobals"))
}

/// Modification time of the scheme file, to detect a scheme change cheaply.
pub fn modified() -> Option<SystemTime> {
    fs::metadata(config_path()?).ok()?.modified().ok()
}

pub fn foreground() -> [u8; 3] {
    config_path()
        .and_then(|p| fs::read_to_string(p).ok())
        .and_then(|s| parse_foreground(&s))
        .unwrap_or(FALLBACK_FG)
}

/// `ForegroundNormal=r,g,b` from the `[Colors:Window]` section.
pub fn parse_foreground(ini: &str) -> Option<[u8; 3]> {
    let mut in_window = false;
    for line in ini.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_window = line == "[Colors:Window]";
        } else if in_window && let Some(v) = line.strip_prefix("ForegroundNormal=") {
            let mut it = v.split(',').map(|c| c.trim().parse::<u8>());
            let rgb = [it.next()?.ok()?, it.next()?.ok()?, it.next()?.ok()?];
            return it.next().is_none().then_some(rgb);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_window_foreground_not_other_sections() {
        let ini = "[Colors:Button]\nForegroundNormal=1,2,3\n\
                   [Colors:Window]\nBackgroundNormal=230, 233, 239\nForegroundNormal=76, 79, 105\n\
                   [Colors:View]\nForegroundNormal=9,9,9\n";
        assert_eq!(parse_foreground(ini), Some([76, 79, 105]));
        assert_eq!(parse_foreground("[Colors:Window]\nForegroundNormal=35,38,41"), Some([35, 38, 41]));
    }

    #[test]
    fn rejects_missing_or_malformed_values() {
        assert_eq!(parse_foreground(""), None);
        assert_eq!(parse_foreground("[Colors:Window]\nBackgroundNormal=1,2,3\n"), None);
        assert_eq!(parse_foreground("[Colors:Window]\nForegroundNormal=1,2\n"), None);
        assert_eq!(parse_foreground("[Colors:Window]\nForegroundNormal=1,2,300\n"), None);
        assert_eq!(parse_foreground("[Colors:Window]\nForegroundNormal=1,2,3,4\n"), None);
        assert_eq!(parse_foreground("ForegroundNormal=1,2,3\n"), None);
    }
}
