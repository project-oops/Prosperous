//! Controller state, on the wire.
//!
//! # Why this format is a decision and not a discovery
//!
//! Nothing here is reverse-engineered. The payload that receives these does not exist yet, so
//! there is nothing to be compatible with - **both ends are ours to write**, and the format is
//! chosen rather than recovered. `docs/VIDEO.md` part three is where it is specified; this
//! implements that side of it.
//!
//! That is worth saying because the sibling projects spend most of their effort in the
//! opposite situation, and the habits that suit a vendor format are the wrong ones here.
//!
//! # Four choices, each with an obvious wrong alternative
//!
//! - **Fixed size, not a text line.** This goes sixty times a second or faster. The rest of
//!   this project prefers text for anything diagnosed by hand with a socket and a keyboard; a
//!   pad is not one of those, and a parser hunting field boundaries at 250 Hz drops inputs.
//! - **A sequence number, and the receiver may skip.** Input is a *state*, not an event. The
//!   newest record supersedes every older one, so a receiver three behind should apply the
//!   last and discard two. A queue that delivered all three would replay stale sticks.
//! - **Absolute state, never deltas.** A dropped delta is wrong forever; a dropped state is
//!   wrong for sixteen milliseconds.
//! - **Reserved bytes are zero, and that is checked.** Gyro, touchpad and rumble are the
//!   obvious additions, and a version bump into room already reserved is cheaper than a
//!   second format beside the first.

/// What a record starts with.
pub const MAGIC: [u8; 4] = *b"PPAD";

/// The version this reads and writes.
pub const VERSION: u16 = 1;

/// How long one record is.
pub const RECORD: usize = 24;

/// How many pads a target accepts.
///
/// Four, which is what the platform supports. **A constant rather than an assumption spread
/// through the code**, because the number appears in a wire check, a collection size and a
/// panel, and three copies of it is how two of them end up disagreeing.
pub const SLOTS: u8 = 4;

/// One button, as a bit in the button word.
///
/// # These are measured, not chosen
///
/// The rest of this format is ours to decide. **This part is not.** The bits are the ones the
/// target's own pad structure uses, and a wrong one does not fail - it presses something else.
///
/// The layout was published by the Ghostpad project, which confirmed each bit empirically
/// against a real target, and it credits shadPS4's `pad.h` for the underlying enum. Recorded
/// in `ACKNOWLEDGEMENTS.md`. **An earlier version of this file invented the numbering**, which
/// would have produced a controller where every button was the wrong one.
///
/// One bit is left out on purpose: `0x0002_0000` is documented as producing an unintended
/// Cross press. A plausible-looking bit that fires a different button is exactly the kind of
/// thing that gets assigned by somebody counting upwards.
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
    /// **Not sufficient on its own.** The target reads the analogue byte as well, so a press
    /// sets both - see [`Pad::pull`].
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
    /// Bit sixteen, which the previous generation's headers name differently. Confirmed on a
    /// target rather than inferred from an enum's order.
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

    /// What to call it.
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

    /// What to show it as.
    ///
    /// # Why the glyph rather than the word
    ///
    /// **It is what is printed on the thing in somebody's hands.** A person rebinding a key
    /// is looking between a screen and a controller, and *triangle* asks them to translate
    /// where the shape does not.
    ///
    /// This is display only - [`Button::name`] stays the word, because that is what goes in a
    /// saved layout, a log line and a test failure. **A glyph in a file is a file somebody
    /// cannot grep**, and a layout that survives a font change matters more than a tidy one.
    ///
    /// The four shapes are the only real glyphs here. The shoulders and sticks have no printed
    /// symbol, so they keep their printed *lettering*, in caps as they are printed on it.
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
/// **Not zero.** The target's structure carries each axis as a single unsigned byte with the
/// centre in the middle, so this format does too - a client that used a signed range would be
/// asking the payload to do arithmetic, and arithmetic in a payload is a thing that can be
/// wrong somewhere nobody is looking.
pub const CENTRE: u8 = 128;

/// Everything a pad is doing at one moment.
///
/// **Absolute, never a change**, and shaped like the structure the target actually reads:
/// sticks are unsigned bytes centred on [`CENTRE`], triggers rest at zero. A more precise
/// representation here would be precision the wire throws away, and the conversion would have
/// to happen in the payload where a mistake is hardest to see.
///
/// Use [`Pad::rest`] rather than `default()` for a neutral pad - a zeroed one has both sticks
/// hard left and up.
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
    /// Which update this is.
    ///
    /// **The receiver is allowed to skip.** Being three behind means applying the newest and
    /// discarding two, because every record is the whole state.
    pub sequence: u32,
    /// Which pad this is, counted from zero.
    ///
    /// # Why this is in the record rather than in a connection
    ///
    /// Four pads over one socket, each identifying itself, rather than four sockets that a
    /// receiver has to associate with slots. **A slot is a property of the input, not of the
    /// route it took** - and a payload that inferred the slot from which connection carried it
    /// would put pad two's input on pad one the first time a socket reconnected in a different
    /// order.
    ///
    /// It also makes the sequence number per-slot, which is the only way it means anything:
    /// one counter shared by four pads advances on somebody else's input, so *behind by two*
    /// would stop being answerable.
    pub slot: u8,
}

impl Pad {
    /// A pad at rest.
    ///
    /// **Not `default()`**, which zeroes every field and so holds both sticks hard left and
    /// up. Neutral is a value here rather than the absence of one, which is the cost of
    /// matching the target's own representation - and it is cheaper than the arithmetic the
    /// alternative would put in a payload.
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
    /// **A trigger sets its analogue byte too.** The target reads both, and the bit alone does
    /// not register - which is the kind of thing that looks like a dead button and gets
    /// diagnosed as a network problem. Use [`Pad::pull`] for a partial press.
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
    /// Sets the bit once there is any travel at all, because the two are read together and a
    /// pressure with no bit is a press the target does not see.
    pub const fn pull(&mut self, button: Button, amount: u8) {
        match button {
            Button::L2 => self.l2 = amount,
            Button::R2 => self.r2 = amount,
            // Anything else has no analogue half, so this is the same as holding it.
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

    /// Whether anything at all is being done.
    ///
    /// Useful for not sending: a pad at rest that keeps sending its rest state spends a
    /// network on nothing. **The first rest after activity still has to go**, or the target
    /// holds the last thing that moved.
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
    /// [`NotAPad`] for anything that is not one of ours. **Reserved bytes must be zero**: they
    /// are where a later version puts gyro or the touchpad, and accepting them now means a
    /// newer payload cannot tell an old sender from a new one.
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
    /// **Refused rather than clamped.** A record meant for a fifth pad is a sender that
    /// believes something untrue, and quietly delivering it to the fourth would make one
    /// person's input arrive as another's.
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
/// # Why sending is conditional
///
/// A pad at rest sending its rest state sixty times a second spends a network on nothing. But
/// **the first rest after activity must go**, or the target keeps holding whatever moved last -
/// which is the difference between a stick that returns to centre and one that sticks.
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
    /// `None` means nothing changed and the pad is at rest, so there is nothing worth saying.
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
        // The sender owns both, so a caller cannot make two records claiming to be the same
        // update, nor put its input in somebody else's slot.
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

    /// A record is exactly the size the specification says, because a payload will read it
    /// with a fixed-size struct at the other end.
    #[test]
    fn a_record_is_the_size_it_says_it_is() {
        assert_eq!(busy().to_wire().len(), RECORD);
        assert_eq!(RECORD, 24);
    }

    /// **The reserved byte is checked, not ignored.**
    ///
    /// It is where the next version puts gyro or rumble. Accepting anything there now means a
    /// later payload cannot tell an old sender from a new one, and would read whatever
    /// happened to be present as a real value.
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

    /// The slot survives the wire, because a payload has to tell four pads apart.
    #[test]
    fn a_record_carries_which_pad_it_is() {
        for slot in 0..super::SLOTS {
            let pad = Pad { slot, ..busy() };
            let read = Pad::from_wire(&pad.to_wire()).expect("reads");
            assert_eq!(read.slot, slot);
        }
    }

    /// **A slot beyond what a target has is refused, not clamped.**
    ///
    /// A record meant for a fifth pad is a sender believing something untrue, and quietly
    /// delivering it to the fourth would make one person's input arrive as another's.
    #[test]
    fn a_slot_the_target_does_not_have_is_refused() {
        let mut raw = busy().to_wire();
        raw[6] = super::SLOTS;
        assert_eq!(Pad::from_wire(&raw), Err(NotAPad::Slot(super::SLOTS)));
    }

    /// A sender puts its own slot on every record, so a caller cannot send into another
    /// player's pad by filling the field in.
    #[test]
    fn the_sender_owns_the_slot_as_well_as_the_sequence() {
        let mut sender = Sender::new(2);
        let mut pad = busy();
        pad.slot = 0;
        let record = sender.update(pad).expect("busy is worth sending");
        assert_eq!(Pad::from_wire(&record).expect("reads").slot, 2);
    }

    /// Three ways of not being a record, kept apart because they need different work.
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

    /// Neutral is zero in every field, so a rest state needs nothing remembered.
    #[test]
    fn a_pad_at_rest_is_zero_everywhere() {
        let rest = Pad::default();
        assert!(rest.is_at_rest());
        assert!(!busy().is_at_rest());

        let after = Pad::from_wire(&rest.to_wire()).expect("reads");
        assert_eq!(after, rest);
    }

    /// **A held button that stops being held still sends.**
    ///
    /// The one that matters: without it the target keeps holding whatever moved last, which
    /// is the difference between a stick returning to centre and a stick that sticks.
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

        // And only then does it go quiet.
        assert!(sender.update(pad).is_none(), "rest after rest says nothing");
        assert!(sender.update(pad).is_none());
    }

    /// **The sequence number advances on every record sent**, so a receiver can tell how far
    /// behind it is and discard everything but the newest.
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

    /// The caller's own sequence field is not what goes on the wire - the sender owns it, so
    /// two callers cannot produce two records claiming to be the same update.
    #[test]
    fn the_sender_numbers_the_records_rather_than_the_caller() {
        let mut sender = Sender::new(0);
        let mut pad = busy();
        pad.sequence = 9_999;
        let record = sender.update(pad).expect("busy is worth sending");
        assert_eq!(Pad::from_wire(&record).expect("reads").sequence, 1);
    }

    /// **A trigger sets both halves, because the target reads both.**
    ///
    /// The bit alone does not register. That failure looks like a dead button and gets
    /// diagnosed as a network problem, which is why it is pinned rather than commented.
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

    /// And a partial pull sets the bit, because pressure with no press is not seen.
    #[test]
    fn a_partial_pull_still_counts_as_a_press() {
        let mut pad = Pad::rest();
        pad.pull(Button::R2, 40);
        assert!(pad.holds(Button::R2), "any travel is a press");
        assert_eq!(pad.r2, 40);

        pad.pull(Button::R2, 0);
        assert!(!pad.holds(Button::R2), "and none is not");
    }

    /// **Neutral is not zero.** A zeroed pad holds both sticks hard left and up, which is why
    /// `rest()` exists and `default()` defers to it.
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

    /// **The bit that presses the wrong button is not reachable.**
    ///
    /// `0x0002_0000` is documented as producing an unintended Cross press on the target. No
    /// button maps to it, and this is what stops one being added by somebody counting upwards.
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

    /// **The name stays greppable and the glyph stays for the screen.**
    ///
    /// The temptation is to make `name` the shape and be done with it. That would put a glyph
    /// into every saved layout, log line and test failure - and a file somebody cannot grep,
    /// type or read over the phone is worse than one that says `triangle`.
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

        // **The four shapes, spelled in characters any font has.**
        //
        // They were the real symbols, and the window font has none of them - every one drew
        // as a replacement box. Square was the worst of it: its symbol IS a box, so a Square
        // button and a glyph the font could not draw were the same picture.
        assert_eq!(Button::Triangle.glyph(), "/\\");
        assert_eq!(Button::Cross.glyph(), "X");
        assert_eq!(Button::Circle.glyph(), "O");
        assert_eq!(Button::Square.glyph(), "[]");
        // And nothing here is outside plain ASCII, which is the property that keeps it true.
        for button in Button::ALL {
            assert!(
                button.glyph().is_ascii(),
                "{} is not something every font can draw",
                button.name()
            );
        }
        assert_ne!(Button::Triangle.glyph(), Button::Triangle.name());

        // The shoulders have no printed symbol, so they keep their printed lettering rather
        // than being given an invented one.
        assert_eq!(Button::L1.glyph(), "L1");
        assert_eq!(Button::R2.glyph(), "R2");
    }

    /// No two buttons show the same thing, or a mapping table would have rows nobody can
    /// tell apart.
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

    /// Every button has its own bit, which a hand-written table is exactly the place to get
    /// wrong.
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
