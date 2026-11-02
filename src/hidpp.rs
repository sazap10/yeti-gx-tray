//! HID++ 2.0 framing for the Yeti GX vendor feature `0x8370`.
//!
//! Pure encode/decode only; no I/O. Only the requests listed in `PLAN.md` exist here:
//! root getFeature(0x8370), read of fields 0x00/0x01/0x02, write of fields 0x00/0x01/0x05.

pub const REPORT_SHORT: u8 = 0x10;
pub const REPORT_LONG: u8 = 0x11;
const DEVICE_INDEX: u8 = 0xFF;

/// Vendor feature id carrying mute / gain / Smart Audio Lock.
pub const FEATURE_ID: u16 = 0x8370;

/// Software id stamped on every read request (the device echoes it in the reply).
pub const SW_READ: u8 = 0x0C;
/// Software id stamped on every write request. Distinct from [`SW_READ`] so a late write
/// acknowledgement or error can never be mistaken for the reply to a read.
pub const SW_WRITE: u8 = 0x0D;

pub const FIELD_MUTE: u8 = 0x00;
pub const FIELD_GAIN: u8 = 0x01;
pub const FIELD_SMART_LOCK_READ: u8 = 0x02;
pub const FIELD_SMART_LOCK_WRITE: u8 = 0x05;

pub const SMART_LOCK_WRITE_OFF: u8 = 0x06;
pub const SMART_LOCK_WRITE_ON: u8 = 0x07;

pub const GAIN_MAX: u8 = 100;

/// Notification `type` byte values (`11 ff <idx> 20 00 <type> <value>`).
pub const NOTIFY_MUTE: u8 = 0x00;
pub const NOTIFY_GAIN: u8 = 0x01;
pub const NOTIFY_SMART_LOCK: u8 = 0x05;

/// `10 ff 00 <sw> 83 70 00`: ask the root feature for the index of [`FEATURE_ID`].
pub fn get_feature(sw: u8) -> [u8; 7] {
    let [hi, lo] = FEATURE_ID.to_be_bytes();
    [REPORT_SHORT, DEVICE_INDEX, 0x00, sw & 0x0F, hi, lo, 0x00]
}

/// `10 ff <idx> 2<sw> 00 <field> 00`
pub fn read_field(index: u8, sw: u8, field: u8) -> [u8; 7] {
    [REPORT_SHORT, DEVICE_INDEX, index, 0x20 | (sw & 0x0F), 0x00, field, 0x00]
}

/// `10 ff <idx> 3<sw> 00 <field> <value>`
pub fn write_field(index: u8, sw: u8, field: u8, value: u8) -> [u8; 7] {
    [REPORT_SHORT, DEVICE_INDEX, index, 0x30 | (sw & 0x0F), 0x00, field, value]
}

/// A decoded device-to-host report that is relevant to us.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Msg {
    /// Reply to [`get_feature`]; `0` means the feature is not present.
    FeatureIndex(u8),
    /// Reply to [`read_field`]; value is at byte 7.
    Read { feature: u8, field: u8, value: u8 },
    /// Device-initiated `11 ff <idx> 20 00 <type> <value>`.
    Notify { feature: u8, kind: u8, value: u8 },
    /// `10 ff ff <feat> <func|sw> <code> 00` addressed to our sw id.
    Error { feature: u8, code: u8 },
}

fn is_hidpp(buf: &[u8]) -> bool {
    match buf.first() {
        Some(&REPORT_SHORT) => buf.len() >= 7,
        Some(&REPORT_LONG) => buf.len() >= 20,
        _ => false,
    }
}

/// Decode one raw report. Replies and errors are returned only when their sw id equals `sw`;
/// notifications (sw id 0) are always returned. Anything else yields `None`.
pub fn decode(buf: &[u8], sw: u8) -> Option<Msg> {
    if !is_hidpp(buf) || buf[1] != DEVICE_INDEX {
        return None;
    }
    let sw = sw & 0x0F;
    if buf[2] == 0xFF {
        // Error frame: byte 3 = feature index, byte 4 = func<<4 | sw, byte 5 = code.
        return (buf[4] & 0x0F == sw).then_some(Msg::Error { feature: buf[3], code: buf[5] });
    }
    let feature = buf[2];
    let (func, frame_sw) = (buf[3] >> 4, buf[3] & 0x0F);
    if func == 2 && frame_sw == 0 {
        return Some(Msg::Notify { feature, kind: buf[5], value: buf[6] });
    }
    if frame_sw != sw {
        return None;
    }
    match func {
        0 if feature == 0 => Some(Msg::FeatureIndex(buf[4])),
        2 if buf.len() > 7 => Some(Msg::Read { feature, field: buf[5], value: buf[7] }),
        _ => None,
    }
}

pub fn error_name(code: u8) -> &'static str {
    match code {
        0x02 => "invalid argument",
        0x03 => "out of range",
        0x04 => "invalid feature index",
        0x05 => "invalid function",
        0x07 => "unsupported",
        _ => "unknown error",
    }
}

pub fn hex(buf: &[u8]) -> String {
    buf.iter().map(|b| format!("{b:02x}")).collect::<Vec<_>>().join(" ")
}

/// Human-readable meaning of any report, for `--listen`. `index` is the discovered index of
/// [`FEATURE_ID`], used only to annotate.
pub fn describe(buf: &[u8], index: Option<u8>) -> String {
    if !is_hidpp(buf) || buf[1] != DEVICE_INDEX {
        return "not a HID++ report".into();
    }
    if buf[2] == 0xFF {
        return format!(
            "error: {} (0x{:02x}) for feature idx {} func {} sw {}",
            error_name(buf[5]),
            buf[5],
            buf[3],
            buf[4] >> 4,
            buf[4] & 0x0F
        );
    }
    let feature = buf[2];
    let (func, sw) = (buf[3] >> 4, buf[3] & 0x0F);
    let feat = if Some(feature) == index {
        format!("0x{FEATURE_ID:04x}")
    } else {
        format!("idx {feature}")
    };
    if func == 2 && sw == 0 {
        let what = match buf[5] {
            NOTIFY_MUTE => "mute",
            NOTIFY_GAIN => "gain",
            NOTIFY_SMART_LOCK => "smart lock",
            _ => "unknown",
        };
        return format!("NOTIFICATION {feat}: {what} (type 0x{:02x}) value {}", buf[5], buf[6]);
    }
    match func {
        0 if feature == 0 => format!("getFeature reply: 0x{FEATURE_ID:04x} is at idx {}", buf[4]),
        2 if buf.len() > 7 => {
            format!("read reply {feat} sw {sw}: field 0x{:02x} = {}", buf[5], buf[7])
        }
        3 => format!("write reply {feat} sw {sw}: field 0x{:02x}", buf[5]),
        _ => format!("reply {feat} func {func} sw {sw}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn long(head: &[u8]) -> Vec<u8> {
        let mut v = head.to_vec();
        v.resize(20, 0);
        v
    }

    #[test]
    fn encodes_get_feature_like_the_plan() {
        assert_eq!(get_feature(SW_READ), [0x10, 0xff, 0x00, 0x0c, 0x83, 0x70, 0x00]);
    }

    #[test]
    fn encodes_read_and_write() {
        assert_eq!(
            read_field(13, SW_READ, FIELD_GAIN),
            [0x10, 0xff, 13, 0x2c, 0x00, 0x01, 0x00]
        );
        assert_eq!(
            write_field(13, SW_WRITE, FIELD_GAIN, 55),
            [0x10, 0xff, 13, 0x3d, 0x00, 0x01, 55]
        );
        assert_eq!(
            write_field(13, SW_WRITE, FIELD_SMART_LOCK_WRITE, SMART_LOCK_WRITE_ON),
            [0x10, 0xff, 13, 0x3d, 0x00, 0x05, 0x07]
        );
    }

    #[test]
    fn sw_id_is_masked_to_a_nibble() {
        assert_eq!(read_field(1, 0xFC, 0)[3], 0x2c);
    }

    #[test]
    fn decodes_feature_index_reply() {
        let r = long(&[0x11, 0xff, 0x00, 0x0c, 13, 0x00, 0x01]);
        assert_eq!(decode(&r, SW_READ), Some(Msg::FeatureIndex(13)));
        let short = [0x10, 0xff, 0x00, 0x0c, 13, 0, 0];
        assert_eq!(decode(&short, SW_READ), Some(Msg::FeatureIndex(13)));
    }

    #[test]
    fn decodes_read_reply_value_at_byte_7() {
        let r = long(&[0x11, 0xff, 13, 0x2c, 0x00, 0x01, 0x00, 100]);
        assert_eq!(decode(&r, SW_READ), Some(Msg::Read { feature: 13, field: 1, value: 100 }));
    }

    #[test]
    fn ignores_replies_with_other_sw_id() {
        let r = long(&[0x11, 0xff, 13, 0x2e, 0x00, 0x01, 0x00, 100]);
        assert_eq!(decode(&r, SW_READ), None);
        let root = long(&[0x11, 0xff, 0x00, 0x0e, 13]);
        assert_eq!(decode(&root, SW_READ), None);
        // A write ack carries SW_WRITE and is never a read reply.
        let ack = long(&[0x11, 0xff, 13, 0x3d, 0x00, 0x01, 0x00, 55]);
        assert_eq!(decode(&ack, SW_READ), None);
    }

    #[test]
    fn decodes_notifications_regardless_of_sw() {
        let n = long(&[0x11, 0xff, 13, 0x20, 0x00, NOTIFY_GAIN, 42]);
        assert_eq!(
            decode(&n, SW_READ),
            Some(Msg::Notify { feature: 13, kind: NOTIFY_GAIN, value: 42 })
        );
        assert_eq!(
            decode(&n, SW_WRITE),
            Some(Msg::Notify { feature: 13, kind: NOTIFY_GAIN, value: 42 })
        );
    }

    #[test]
    fn decodes_error_replies_only_for_our_sw_id() {
        let e = [0x10, 0xff, 0xff, 13, 0x3d, 0x03, 0x00];
        assert_eq!(decode(&e, SW_WRITE), Some(Msg::Error { feature: 13, code: 0x03 }));
        assert_eq!(decode(&e, SW_READ), None);
    }

    #[test]
    fn rejects_malformed_reports() {
        assert_eq!(decode(&[], SW_READ), None);
        assert_eq!(decode(&[0x10, 0xff, 0x00], SW_READ), None);
        // long report id with short length
        assert_eq!(decode(&[0x11, 0xff, 13, 0x2c, 0, 1, 0, 100], SW_READ), None);
        // wrong device index
        assert_eq!(decode(&long(&[0x11, 0x01, 13, 0x2c, 0, 1, 0, 100]), SW_READ), None);
        // unrelated report id
        assert_eq!(decode(&long(&[0x20, 0xff, 13, 0x2c, 0, 1, 0, 100]), SW_READ), None);
        // short-form read reply has no byte 7
        assert_eq!(decode(&[0x10, 0xff, 13, 0x2c, 0, 1, 0], SW_READ), None);
    }

    #[test]
    fn describe_names_notifications_and_errors() {
        let n = long(&[0x11, 0xff, 13, 0x20, 0x00, NOTIFY_MUTE, 0]);
        let s = describe(&n, Some(13));
        assert!(s.contains("NOTIFICATION") && s.contains("mute") && s.contains("0x8370"), "{s}");
        let e = [0x10, 0xff, 0xff, 13, 0x3d, 0x03, 0x00];
        assert!(describe(&e, Some(13)).contains("out of range"));
        assert_eq!(describe(&[0x01, 0x02], None), "not a HID++ report");
    }
}
