//! Turning a Moonlight controller packet into a [`Pad`] the target understands.
//!
//! **The packet layout is adapted from Moonshine** (Hans Gaiser, BSD-2-Clause; see
//! `THIRD-PARTY-LICENSES.md`), whose `control/input/gamepad.rs` reads the same multi-controller
//! packet. The mapping from Moonlight's XInput-style button bits to the target's own buttons is
//! this project's, because the target's bitmap is not Moonlight's - `pros_link::pad::Button` uses
//! the values measured on the console, so each Moonlight bit is *translated* through
//! [`Pad::hold`] rather than copied.
//!
//! Sticks are the other translation: Moonlight sends signed 16-bit axes centred on zero with up
//! positive; the target reads unsigned bytes centred on 128 with up *low* (`pros_link::pad`), so
//! the vertical axes are inverted as well as rescaled. Getting either wrong is the class of bug
//! that reads as "a stick that rests off-centre", so it is done in one place and tested.

use pros_link::pad::{Button, Pad};

/// The offset of the controller number in the packet.
const SLOT_AT: usize = 2;
/// The offset of the low 16 button bits.
const BUTTONS_LOW_AT: usize = 8;
/// The offset of the left trigger byte.
const LEFT_TRIGGER_AT: usize = 10;
/// The offset of the right trigger byte.
const RIGHT_TRIGGER_AT: usize = 11;
/// The offset of the left stick X (signed 16-bit, little-endian); the axes follow in order.
const LEFT_X_AT: usize = 12;
/// The offset of the high 16 button bits.
const BUTTONS_HIGH_AT: usize = 22;
/// The smallest packet this can read: through the high button bits.
const MIN_LEN: usize = BUTTONS_HIGH_AT + 2;

/// How each Moonlight button bit maps to a target button.
///
/// Moonlight's values are the XInput-style ones every client sends; the target's are
/// [`pros_link::pad::Button`]'s. Select maps to the touchpad click, the target pad's nearest
/// equivalent, and the guide button to the system button.
const BUTTONS: [(u32, Button); 15] = [
    (0x0001, Button::Up),
    (0x0002, Button::Down),
    (0x0004, Button::Left),
    (0x0008, Button::Right),
    (0x0010, Button::Options), // Play / Start
    (0x0020, Button::Pad),     // Back / Select -> touchpad click
    (0x0040, Button::L3),
    (0x0080, Button::R3),
    (0x0100, Button::L1),
    (0x0200, Button::R1),
    (0x0400, Button::Home), // Special / Guide
    (0x1000, Button::Cross), // A
    (0x2000, Button::Circle), // B
    (0x4000, Button::Square), // X
    (0x8000, Button::Triangle), // Y
];

/// A decoded controller update: which pad, and its whole state.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Update {
    /// Which pad this is, 0-3.
    pub(crate) slot: u8,
    /// The pad's state, ready to send to the target (its sequence still to be set by the caller).
    pub(crate) pad: Pad,
}

/// Decode a Moonlight multi-controller packet payload into a target pad update.
///
/// Returns `None` if the payload is too short to be one, so a stray control message is ignored
/// rather than turned into a pad resting hard in a corner.
pub(crate) fn decode(payload: &[u8]) -> Option<Update> {
    if payload.len() < MIN_LEN {
        return None;
    }
    let slot = payload[SLOT_AT];
    let low = u32::from(u16::from_le_bytes([payload[BUTTONS_LOW_AT], payload[BUTTONS_LOW_AT + 1]]));
    let high =
        u32::from(u16::from_le_bytes([payload[BUTTONS_HIGH_AT], payload[BUTTONS_HIGH_AT + 1]]));
    let buttons = low | (high << 16);

    let mut pad = Pad::rest();
    pad.slot = slot;
    for (bit, button) in BUTTONS {
        if buttons & bit != 0 {
            pad.hold(button, true);
        }
    }
    // Triggers are analogue: set the pressure, which also sets the bit (see `Pad::pull`).
    pad.pull(Button::L2, payload[LEFT_TRIGGER_AT]);
    pad.pull(Button::R2, payload[RIGHT_TRIGGER_AT]);

    let axis = |at: usize| i16::from_le_bytes([payload[at], payload[at + 1]]);
    pad.left_x = to_byte(axis(LEFT_X_AT));
    pad.left_y = flip(to_byte(axis(LEFT_X_AT + 2)));
    pad.right_x = to_byte(axis(LEFT_X_AT + 4));
    pad.right_y = flip(to_byte(axis(LEFT_X_AT + 6)));

    Some(Update { slot, pad })
}

/// Rescale a signed 16-bit axis, centred on zero, to an unsigned byte centred on 128.
fn to_byte(value: i16) -> u8 {
    // (value + 32768) >> 8 maps [-32768, 32767] onto [0, 255], with 0 -> 128.
    u8::try_from(((i32::from(value) + 32768) >> 8).clamp(0, 255)).unwrap_or(pros_link::pad::CENTRE)
}

/// Flip a vertical axis, because Moonlight has up positive and the target has up low.
fn flip(byte: u8) -> u8 {
    255 - byte
}

#[cfg(test)]
mod tests {
    use super::decode;
    use pros_link::pad::{Button, CENTRE};

    /// Build a minimal controller packet with the given buttons, triggers and sticks.
    fn packet(slot: u8, buttons: u32, lt: u8, rt: u8, lx: i16, ly: i16) -> Vec<u8> {
        let mut p = vec![0_u8; 26];
        p[2] = slot;
        p[8..10].copy_from_slice(&((buttons & 0xffff) as u16).to_le_bytes());
        p[22..24].copy_from_slice(&(((buttons >> 16) & 0xffff) as u16).to_le_bytes());
        p[10] = lt;
        p[11] = rt;
        p[12..14].copy_from_slice(&lx.to_le_bytes());
        p[14..16].copy_from_slice(&ly.to_le_bytes());
        p
    }

    #[test]
    fn a_short_packet_is_ignored() {
        assert!(decode(&[0, 1, 2, 3]).is_none());
    }

    #[test]
    fn the_face_buttons_map_to_the_target_layout() {
        // Moonlight A (0x1000) is the target's Cross; B (0x2000) is Circle.
        let update = decode(&packet(1, 0x1000 | 0x2000, 0, 0, 0, 0)).unwrap();
        assert_eq!(update.slot, 1);
        assert!(update.pad.holds(Button::Cross));
        assert!(update.pad.holds(Button::Circle));
        assert!(!update.pad.holds(Button::Square));
    }

    #[test]
    fn a_trigger_sets_its_pressure() {
        let update = decode(&packet(0, 0, 200, 0, 0, 0)).unwrap();
        assert_eq!(update.pad.l2, 200);
        assert!(update.pad.holds(Button::L2), "pressure implies the bit");
        assert_eq!(update.pad.r2, 0);
    }

    #[test]
    fn a_centred_stick_reads_as_centre_and_up_is_low() {
        let centred = decode(&packet(0, 0, 0, 0, 0, 0)).unwrap();
        assert!((i16::from(centred.pad.left_x) - i16::from(CENTRE)).abs() <= 1);
        assert!((i16::from(centred.pad.left_y) - i16::from(CENTRE)).abs() <= 1);
        // Full up on Moonlight (positive Y) must read as a low byte on the target.
        let up = decode(&packet(0, 0, 0, 0, 0, 32767)).unwrap();
        assert!(up.pad.left_y < 8, "up is low, got {}", up.pad.left_y);
        // Full right (positive X) reads high.
        let right = decode(&packet(0, 0, 0, 0, 32767, 0)).unwrap();
        assert!(right.pad.left_x > 247, "right is high, got {}", right.pad.left_x);
    }
}
