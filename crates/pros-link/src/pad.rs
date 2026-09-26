//! Controller state, on the wire.
//!
//! Both ends of this format are ours, specified in `docs/VIDEO.md` under Porthole; only the button
//! bits are measured. A record is fixed-size, not a text line, because it goes out at 60 Hz or
//! faster. Each record carries the absolute state and a sequence number, so a receiver that
//! falls behind applies the newest and discards the rest, and a dropped record is wrong for
//! one frame rather than forever. Reserved bytes must be zero, leaving room for gyro, touchpad
//! and rumble in a later version.

/// What a record starts with.
pub const MAGIC: [u8; 4] = *b"PPAD";

/// The version this reads and writes.
pub const VERSION: u16 = 1;

/// How long one record is.
pub const RECORD: usize = 24;

/// How many pads a target accepts.
///
/// The platform's limit, used by the wire check, the slot collection and the panel.
pub const SLOTS: u8 = 4;

/// One button, as a bit in the button word.
///
/// These bits are measured, not chosen: they are the target's own pad structure, and a wrong
/// bit presses a different button. Source: the Ghostpad project, which confirmed each bit on a
/// target and credits shadPS4's `pad.h` for the enum (see `ACKNOWLEDGEMENTS.md`).
///
/// `0x0002_0000` is deliberately unassigned: it is documented as producing an unintended Cross
/// press.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u32)]
pub enum Button {
    /// Left stick, pressed.
    L3 = 0x0000_0002,
    /// Right stick, pressed.
    R3 = 0x0000_0004,
    /// The menu button.
    Options = 0x0000_0008,
    /// Up on the directional pad.
    Up = 0x0000_0010,
    /// Right.
    Right = 0x0000_0020,
    /// Down.
    Down = 0x0000_0040,
    /// Left.
    Left = 0x0000_0080,
    /// Lower left trigger, as a bit.
    ///
    /// The target also reads the analogue byte, so a press sets both; see [`Pad::pull`].
    L2 = 0x0000_0100,
    /// Lower right trigger, as a bit. The same applies.
    R2 = 0x0000_0200,
    /// Upper left shoulder.
    L1 = 0x0000_0400,
    /// Upper right shoulder.
    R1 = 0x0000_0800,
    /// The upper face button.
    Triangle = 0x0000_1000,
    /// The right face button.
    Circle = 0x0000_2000,
    /// The lower face button.
    Cross = 0x0000_4000,
    /// The left face button.
    Square = 0x0000_8000,
    /// The system button.
    ///
    /// Bit sixteen, which the previous generation's headers name differently; confirmed on a
    /// target.
    Home = 0x0001_0000,
    /// The touchpad, pressed.
    Pad = 0x0010_0000,
}

impl Button {
    /// Every button, for anything that has to cover all of them.
    pub const ALL: [Self; 17] = [
        Self::Up,
        Self::Down,
        Self::Left,
        Self::Right,
        Self::Cross,
        Self::Circle,
        Self::Square,
        Self::Triangle,
        Self::L1,
        Self::R1,
        Self::L2,
        Self::R2,
        Self::L3,
        Self::R3,
        Self::Options,
        Self::Home,
        Self::Pad,
    ];

    /// Whether this also needs its analogue byte set.
    #[must_use]
    pub const fn is_a_trigger(self) -> bool {
        matches!(self, Self::L2 | Self::R2)
    }

    /// The button's word name, used in saved layouts, logs and test output.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Up => "up",
            Self::Down => "down",
            Self::Left => "left",
            Self::Right => "right",
            Self::Cross => "cross",
            Self::Circle => "circle",
            Self::Square => "square",
            Self::Triangle => "triangle",
            Self::L1 => "l1",
            Self::R1 => "r1",
            Self::L2 => "l2",
            Self::R2 => "r2",
            Self::L3 => "l3",
            Self::R3 => "r3",
            Self::Options => "options",
            Self::Home => "home",
            Self::Pad => "pad",
        }
    }

    /// What to show on screen: the printed shape or lettering, in plain ASCII.
    ///
    /// Display only; [`Button::name`] stays the word so files remain greppable. The face
    /// shapes are spelled in ASCII because the window font has no symbols for them.
    #[must_use]
    pub const fn glyph(self) -> &'static str {
        match self {
            Self::Up => "up",
            Self::Down => "down",
            Self::Left => "left",
            Self::Right => "right",
            Self::Cross => "X",
            Self::Circle => "O",
            Self::Square => "[]",
            Self::Triangle => "/\\",
            Self::L1 => "L1",
            Self::R1 => "R1",
            Self::L2 => "L2",
            Self::R2 => "R2",
            Self::L3 => "L3",
            Self::R3 => "R3",
            Self::Options => "options",
            Self::Home => "home",
            Self::Pad => "pad",
        }
    }
}

/// Where a stick rests.
///
/// Not zero: the target carries each axis as an unsigned byte centred in the middle, and this
/// format matches so the payload does no conversion.
pub const CENTRE: u8 = 128;

/// Everything a pad is doing at one moment.
///
/// Absolute state, shaped like the structure the target reads: sticks are unsigned bytes
/// centred on [`CENTRE`], triggers rest at zero. [`Pad::rest`] is the neutral pad; a zeroed
/// one has both sticks hard left and up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pad {
    /// One bit per [`Button`].
    pub buttons: u32,
    /// Left stick, horizontal. [`CENTRE`] is the middle, less is left.
    pub left_x: u8,
    /// Left stick, vertical. Less is up.
    pub left_y: u8,
    /// Right stick, horizontal.
    pub right_x: u8,
    /// Right stick, vertical.
    pub right_y: u8,
    /// Left trigger, resting at zero.
    pub l2: u8,
    /// Right trigger.
    pub r2: u8,
    /// Which update this is, counted per slot.
    ///
    /// A receiver that is behind applies the newest record and discards the rest.
    pub sequence: u32,
    /// Which pad this is, counted from zero.
    ///
    /// In the record rather than implied by the connection, so all pads share one socket and
    /// a reconnect in a different order cannot move input between slots.
    pub slot: u8,
}

impl Pad {
    /// A pad at rest: sticks at [`CENTRE`], nothing held.
    #[must_use]
    pub const fn rest() -> Self {
        Self {
            buttons: 0,
            left_x: CENTRE,
            left_y: CENTRE,
            right_x: CENTRE,
            right_y: CENTRE,
            l2: 0,
            r2: 0,
            slot: 0,
            sequence: 0,
        }
    }

    /// Whether a button is held.
    #[must_use]
    pub const fn holds(&self, button: Button) -> bool {
        self.buttons & (button as u32) != 0
    }

    /// Holds a button, or lets it go.
    ///
    /// A trigger sets its analogue byte to full as well, since the target does not register
    /// the bit alone. Use [`Pad::pull`] for a partial press.
    pub const fn hold(&mut self, button: Button, down: bool) {
        if down {
            self.buttons |= button as u32;
        } else {
            self.buttons &= !(button as u32);
        }
        match button {
            Button::L2 => self.l2 = if down { u8::MAX } else { 0 },
            Button::R2 => self.r2 = if down { u8::MAX } else { 0 },
            _ => {}
        }
    }

    /// Pulls a trigger part way.
    ///
    /// Sets the bit once there is any travel, because the target does not see pressure
    /// without the bit. Other buttons are unaffected.
    pub const fn pull(&mut self, button: Button, amount: u8) {
        match button {
            Button::L2 => self.l2 = amount,
            Button::R2 => self.r2 = amount,
            _ => {}
        }
        if button.is_a_trigger() {
            if amount > 0 {
                self.buttons |= button as u32;
            } else {
                self.buttons &= !(button as u32);
            }
        }
    }

    /// Whether the pad is at rest: nothing held, sticks centred, triggers released.
    #[must_use]
    pub const fn is_at_rest(&self) -> bool {
        self.buttons == 0
            && self.left_x == CENTRE
            && self.left_y == CENTRE
            && self.right_x == CENTRE
            && self.right_y == CENTRE
            && self.l2 == 0
            && self.r2 == 0
    }

    /// Writes one record.
    #[must_use]
    pub fn to_wire(&self) -> [u8; RECORD] {
        let mut out = [0_u8; RECORD];
        out[0..4].copy_from_slice(&MAGIC);
        out[4..6].copy_from_slice(&VERSION.to_le_bytes());
        out[6] = self.slot;
        // 7 reserved.
        out[8..12].copy_from_slice(&self.buttons.to_le_bytes());
        out[12] = self.left_x;
        out[13] = self.left_y;
        out[14] = self.right_x;
        out[15] = self.right_y;
        out[16] = self.l2;
        out[17] = self.r2;
        // 18..20 reserved.
        out[20..24].copy_from_slice(&self.sequence.to_le_bytes());
        out
    }

    /// Reads one record.
    ///
    /// # Errors
    ///
    /// [`NotAPad`] for anything that is not one of ours. Reserved bytes must be zero, so a
    /// later version that uses them can tell an old sender from a new one.
    pub fn from_wire(raw: &[u8]) -> Result<Self, NotAPad> {
        let Some(record) = raw.get(..RECORD) else {
            return Err(NotAPad::Short(raw.len()));
        };
        if record.get(..4) != Some(&MAGIC) {
            return Err(NotAPad::NotOurs);
        }
        let version = u16::from_le_bytes([record[4], record[5]]);
        if version != VERSION {
            return Err(NotAPad::Version(version));
        }
        if record[7] != 0 || record[18] != 0 || record[19] != 0 {
            return Err(NotAPad::Reserved);
        }
        let slot = record[6];
        if slot >= SLOTS {
            return Err(NotAPad::Slot(slot));
        }
        Ok(Self {
            buttons: u32::from_le_bytes([record[8], record[9], record[10], record[11]]),
            left_x: record[12],
            left_y: record[13],
            right_x: record[14],
            right_y: record[15],
            l2: record[16],
            r2: record[17],
            slot,
            sequence: u32::from_le_bytes([record[20], record[21], record[22], record[23]]),
        })
    }
}

impl Default for Pad {
    fn default() -> Self {
        Self::rest()
    }
}

/// Why a record was not read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotAPad {
    /// Fewer bytes than a record.
    Short(usize),
    /// Something else on the socket.
    NotOurs,
    /// A version this does not read.
    Version(u16),
    /// The reserved byte carried something.
    Reserved,
    /// A slot beyond what the target has.
    ///
    /// Refused rather than clamped, so one player's input never arrives on another's pad.
    Slot(u8),
}

impl std::fmt::Display for NotAPad {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Short(got) => write!(out, "{got} bytes, and a record is {RECORD}"),
            Self::NotOurs => write!(out, "not a controller record"),
            Self::Version(saw) => {
                write!(out, "a record of version {saw}, and this reads {VERSION}")
            }
            Self::Reserved => write!(
                out,
                "the reserved byte is not zero - a later version puts something there"
            ),
            Self::Slot(slot) => write!(out, "slot {slot}, and a target has {SLOTS}"),
        }
    }
}

impl std::error::Error for NotAPad {}

/// Keeps the newest state and says whether it is worth sending.
///
/// A pad that stays at rest sends nothing, but the first rest after activity is always sent,
/// or the target keeps holding whatever moved last.
#[derive(Debug, Clone, Copy, Default)]
pub struct Sender {
    last: Pad,
    sent_anything: bool,
    sequence: u32,
    slot: u8,
}

impl Sender {
    /// A sender for one slot, which has sent nothing.
    #[must_use]
    pub const fn new(slot: u8) -> Self {
        Self {
            last: Pad {
                slot,
                ..Pad::rest()
            },
            sent_anything: false,
            sequence: 0,
            slot,
        }
    }

    /// Which pad this sends for.
    #[must_use]
    pub const fn slot(&self) -> u8 {
        self.slot
    }

    /// Takes a new state and returns the record to send, if one should be.
    ///
    /// `None` means nothing changed and the pad is at rest.
    pub fn update(&mut self, now: Pad) -> Option<[u8; RECORD]> {
        let same = now.buttons == self.last.buttons
            && now.left_x == self.last.left_x
            && now.left_y == self.last.left_y
            && now.right_x == self.last.right_x
            && now.right_y == self.last.right_y
            && now.l2 == self.last.l2
            && now.r2 == self.last.r2;
        if same && now.is_at_rest() && self.sent_anything {
            return None;
        }
        self.last = now;
        self.sent_anything = true;
        self.sequence = self.sequence.wrapping_add(1);
        let mut record = now;
        record.sequence = self.sequence;
        // The sender owns sequence and slot, so a caller cannot duplicate an update or write
        // into another slot.
        record.slot = self.slot;
        Some(record.to_wire())
    }
}

#[cfg(test)]
mod tests {
    use super::{Button, NotAPad, Pad, RECORD, Sender};

    fn busy() -> Pad {
        let mut pad = Pad {
            left_x: 12,
            left_y: 200,
            right_x: 3,
            right_y: 254,
            sequence: 41,
            ..Pad::rest()
        };
        pad.pull(Button::L2, 255);
        pad.pull(Button::R2, 7);
        pad.hold(Button::Cross, true);
        pad.hold(Button::L3, true);
        pad.hold(Button::Home, true);
        pad
    }

    /// Everything written is read back, unchanged.
    #[test]
    fn a_record_survives_the_wire() {
        let before = busy();
        let after = Pad::from_wire(&before.to_wire()).expect("it reads");
        assert_eq!(before, after);
        assert!(after.holds(Button::Cross));
        assert!(after.holds(Button::Home));
        assert!(!after.holds(Button::Triangle));
    }

    /// A record is exactly the specified size, since a payload reads it as a fixed struct.
    #[test]
    fn a_record_is_the_size_it_says_it_is() {
        assert_eq!(busy().to_wire().len(), RECORD);
        assert_eq!(RECORD, 24);
    }

    /// A non-zero reserved byte is refused.
    #[test]
    fn a_record_with_something_in_the_reserved_byte_is_refused() {
        for at in [7_usize, 18, 19] {
            let mut raw = busy().to_wire();
            raw[at] = 1;
            assert_eq!(
                Pad::from_wire(&raw),
                Err(NotAPad::Reserved),
                "byte {at} is reserved"
            );
        }
    }

    /// The slot survives the wire, so a payload can tell pads apart.
    #[test]
    fn a_record_carries_which_pad_it_is() {
        for slot in 0..super::SLOTS {
            let pad = Pad { slot, ..busy() };
            let read = Pad::from_wire(&pad.to_wire()).expect("reads");
            assert_eq!(read.slot, slot);
        }
    }

    /// A slot beyond what a target has is refused, not clamped.
    #[test]
    fn a_slot_the_target_does_not_have_is_refused() {
        let mut raw = busy().to_wire();
        raw[6] = super::SLOTS;
        assert_eq!(Pad::from_wire(&raw), Err(NotAPad::Slot(super::SLOTS)));
    }

    /// A sender puts its own slot on every record, whatever the caller filled in.
    #[test]
    fn the_sender_owns_the_slot_as_well_as_the_sequence() {
        let mut sender = Sender::new(2);
        let mut pad = busy();
        pad.slot = 0;
        let record = sender.update(pad).expect("busy is worth sending");
        assert_eq!(Pad::from_wire(&record).expect("reads").slot, 2);
    }

    /// Short, foreign and newer-version records are distinct errors.
    #[test]
    fn the_ways_of_not_being_a_record_stay_distinct() {
        assert_eq!(Pad::from_wire(&[0_u8; 8]), Err(NotAPad::Short(8)));

        let mut wrong = busy().to_wire();
        wrong[0] = b'X';
        assert_eq!(Pad::from_wire(&wrong), Err(NotAPad::NotOurs));

        let mut newer = busy().to_wire();
        newer[4..6].copy_from_slice(&9_u16.to_le_bytes());
        assert_eq!(Pad::from_wire(&newer), Err(NotAPad::Version(9)));
    }

    /// The default pad is at rest and survives the wire.
    #[test]
    fn a_pad_at_rest_is_zero_everywhere() {
        let rest = Pad::default();
        assert!(rest.is_at_rest());
        assert!(!busy().is_at_rest());

        let after = Pad::from_wire(&rest.to_wire()).expect("reads");
        assert_eq!(after, rest);
    }

    /// Releasing a button is sent once, then rest goes quiet.
    #[test]
    fn letting_go_is_sent_and_then_silence_follows() {
        let mut sender = Sender::new(0);
        let mut pad = Pad::default();
        pad.hold(Button::Cross, true);

        assert!(sender.update(pad).is_some(), "pressing is worth sending");

        pad.hold(Button::Cross, false);
        let released = sender.update(pad).expect("letting go must be sent");
        let read = Pad::from_wire(&released).expect("reads");
        assert!(!read.holds(Button::Cross));

        assert!(sender.update(pad).is_none(), "rest after rest says nothing");
        assert!(sender.update(pad).is_none());
    }

    /// The sequence number advances on every record sent.
    #[test]
    fn the_sequence_advances_so_a_receiver_can_skip() {
        let mut sender = Sender::new(0);
        let mut pad = Pad::default();

        let mut seen = Vec::new();
        for at in 0..4_u8 {
            pad.left_x = at.wrapping_mul(20).wrapping_add(1);
            let record = sender.update(pad).expect("moving is worth sending");
            seen.push(Pad::from_wire(&record).expect("reads").sequence);
        }
        assert_eq!(seen, [1, 2, 3, 4]);
    }

    /// The sender numbers records, ignoring the caller's sequence field.
    #[test]
    fn the_sender_numbers_the_records_rather_than_the_caller() {
        let mut sender = Sender::new(0);
        let mut pad = busy();
        pad.sequence = 9_999;
        let record = sender.update(pad).expect("busy is worth sending");
        assert_eq!(Pad::from_wire(&record).expect("reads").sequence, 1);
    }

    /// Holding a trigger sets both the bit and full pressure, and releasing clears both.
    #[test]
    fn a_trigger_press_carries_its_pressure() {
        let mut pad = Pad::rest();
        pad.hold(Button::L2, true);
        assert!(pad.holds(Button::L2), "the bit");
        assert_eq!(pad.l2, u8::MAX, "and the pressure");

        pad.hold(Button::L2, false);
        assert!(!pad.holds(Button::L2));
        assert_eq!(pad.l2, 0, "letting go clears both");
    }

    /// A partial pull sets the bit; a pull of zero clears it.
    #[test]
    fn a_partial_pull_still_counts_as_a_press() {
        let mut pad = Pad::rest();
        pad.pull(Button::R2, 40);
        assert!(pad.holds(Button::R2), "any travel is a press");
        assert_eq!(pad.r2, 40);

        pad.pull(Button::R2, 0);
        assert!(!pad.holds(Button::R2), "and none is not");
    }

    /// The default pad is rest with centred sticks, not zero.
    #[test]
    fn a_zeroed_pad_would_be_holding_both_sticks() {
        let rest = Pad::rest();
        assert!(rest.is_at_rest());
        assert_eq!(rest.left_x, super::CENTRE);
        assert_eq!(Pad::default(), rest, "the default is rest, not zero");

        let zeroed = Pad {
            left_x: 0,
            left_y: 0,
            right_x: 0,
            right_y: 0,
            ..Pad::rest()
        };
        assert!(!zeroed.is_at_rest(), "hard left and up is not rest");
    }

    /// No button maps to `0x0002_0000`, which the target reads as an unintended Cross press.
    #[test]
    fn the_bit_that_fires_the_wrong_button_is_unassigned() {
        const UNSAFE_BIT: u32 = 0x0002_0000;
        for button in Button::ALL {
            assert_ne!(
                button as u32,
                UNSAFE_BIT,
                "{} claims a bit the target reads as Cross",
                button.name()
            );
        }
    }

    /// `name` is a plain ASCII word and `glyph` is an ASCII shape distinct from it.
    #[test]
    fn the_name_is_the_word_and_the_glyph_is_the_shape() {
        for button in Button::ALL {
            let name = button.name();
            assert!(
                name.is_ascii() && !name.is_empty(),
                "{name} must survive a grep, a log and a config file"
            );
            assert!(!button.glyph().is_empty(), "{name} has nothing to show");
        }

        // The face shapes use characters any font has; the window font has no symbols for
        // them, and a missing-glyph box is indistinguishable from Square's own shape.
        assert_eq!(Button::Triangle.glyph(), "/\\");
        assert_eq!(Button::Cross.glyph(), "X");
        assert_eq!(Button::Circle.glyph(), "O");
        assert_eq!(Button::Square.glyph(), "[]");
        for button in Button::ALL {
            assert!(
                button.glyph().is_ascii(),
                "{} is not something every font can draw",
                button.name()
            );
        }
        assert_ne!(Button::Triangle.glyph(), Button::Triangle.name());

        // The shoulders keep their printed lettering.
        assert_eq!(Button::L1.glyph(), "L1");
        assert_eq!(Button::R2.glyph(), "R2");
    }

    /// No two buttons show the same glyph.
    #[test]
    fn no_two_buttons_look_alike() {
        for (at, one) in Button::ALL.iter().enumerate() {
            for other in &Button::ALL[at + 1..] {
                assert_ne!(
                    one.glyph(),
                    other.glyph(),
                    "{} and {} show the same thing",
                    one.name(),
                    other.name()
                );
            }
        }
    }

    /// Every button has its own bit.
    #[test]
    fn no_two_buttons_share_a_bit() {
        for (at, one) in Button::ALL.iter().enumerate() {
            for other in &Button::ALL[at + 1..] {
                assert_ne!(
                    *one as u32,
                    *other as u32,
                    "{} and {} are the same bit",
                    one.name(),
                    other.name()
                );
            }
        }
    }
}
