/// Hardware state of the microphone as last read back from the device.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mic {
    pub muted: bool,
    /// 0..=100 preamp gain.
    pub gain: u8,
    pub smart_lock: bool,
}

/// Tooltip/title text. `None` means the device is not connected.
pub fn summary(mic: Option<&Mic>) -> String {
    match mic {
        None => "Yeti GX — disconnected".into(),
        Some(m) => format!(
            "Yeti GX — gain {}, {}, Smart Lock {}",
            m.gain,
            if m.muted { "muted" } else { "unmuted" },
            if m.smart_lock { "on" } else { "off" }
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summary_matches_the_plan_format() {
        let m = Mic { muted: false, gain: 100, smart_lock: false };
        assert_eq!(summary(Some(&m)), "Yeti GX — gain 100, unmuted, Smart Lock off");
        let m = Mic { muted: true, gain: 5, smart_lock: true };
        assert_eq!(summary(Some(&m)), "Yeti GX — gain 5, muted, Smart Lock on");
        assert_eq!(summary(None), "Yeti GX — disconnected");
    }
}
