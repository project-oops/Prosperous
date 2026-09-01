//! Several pads at once, and what drives each one.
//!
//! # Why a slot is not a device
//!
//! A target accepts four pads. What fills each one is this machine's business: a physical
//! controller, the keyboard, or nothing. **Keeping those apart is what makes a slot
//! reassignable** - unplugging a controller should empty its slot rather than renumber
//! everything after it, which is what happens when the slot *is* the device.
//!
//! # Why the keyboard is a first-class source
//!
//! It costs nothing. The window already receives key state, so a keyboard-driven pad works
//! with no new dependency at all - and a stand-in stream that can be driven the moment it
//! exists is worth more than one waiting on a decision about which gamepad crate to take.
//!
//! Reading a physical controller is a **separate** decision, and it is a real one: this
//! workspace forbids unsafe code, so the platform APIs are out of reach directly and a crate
//! would have to be argued for like every other dependency here. Until then a slot can say it
//! wants a controller and report that none is readable, which is honest and is not nothing.

use std::collections::BTreeMap;

use crate::pad::{Button, CENTRE, Pad, RECORD, SLOTS, Sender};

/// What is driving a slot.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Source {
    /// Nothing. The slot exists and sends nothing.
    #[default]
    Empty,
    /// This machine's keyboard, through the window.
    Keyboard,
    /// A physical controller.
    ///
    /// **Declared but not readable yet.** A slot set to this reports that nothing can read it,
    /// rather than silently behaving like [`Source::Empty`] - the two are different states and
    /// only one of them is somebody's mistake.
    Controller(u8),
}

impl Source {
    /// What to call it.
    #[must_use]
    pub fn describe(self) -> String {
        match self {
            Self::Empty => "nothing".to_owned(),
            Self::Keyboard => "keyboard".to_owned(),
            Self::Controller(which) => format!("controller {which}"),
        }
    }

    /// Whether anything can currently read this.
    ///
    /// **`false` for a declared controller**, because nothing here can read one yet. A source
    /// that claimed to work and sent nothing would be indistinguishable from a pad at rest.
    #[must_use]
    pub const fn is_readable(self) -> bool {
        matches!(self, Self::Keyboard)
    }
}

/// How far a key pushes a stick.
///
/// All the way. A key is held or it is not, so anything less would be inventing an analogue
/// value from a digital source - and a stick that never reaches its edge is one a game reads
/// as a slow walk forever.
///
/// The axis is an unsigned byte centred on [`CENTRE`], matching what the target reads, so the
/// two extremes are the ends of that byte rather than a signed range somebody has to convert.
pub const LOW: u8 = 0;

/// The other end of the same travel.
pub const HIGH: u8 = u8::MAX;

/// Which key does what, for a keyboard-driven pad.
///
/// **Names rather than key codes**, because this crate has no window and must not grow one.
/// The window resolves a name to its own key type, which keeps the mapping testable here and
/// the platform detail there.
///
/// **Keyed by button, not by key.** A button has exactly one key and a key may - wrongly - be
/// on two buttons, so this is the direction that cannot represent the impossible case. The
/// possible-but-wrong one is found by [`Pads::conflicts`] rather than prevented, for reasons
/// given there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Keys {
    /// Which key holds each button.
    pub buttons: BTreeMap<Button, String>,
    /// The four keys that move the left stick: up, down, left, right.
    pub left: [String; 4],
    /// The same for the right stick.
    pub right: [String; 4],
}

impl Default for Keys {
    /// A layout somebody can use without reading anything.
    ///
    /// The face buttons sit under the right hand where a pad's would be, movement is on the
    /// left, and nothing is bound to a key a window needs for itself.
    fn default() -> Self {
        let named = |text: &str| text.to_owned();
        Self {
            buttons: [
                (Button::Square, "J"),
                (Button::Cross, "K"),
                (Button::Circle, "L"),
                (Button::Triangle, "I"),
                (Button::L1, "U"),
                (Button::R1, "O"),
                (Button::L2, "Q"),
                (Button::R2, "E"),
                (Button::L3, "Z"),
                (Button::R3, "X"),
                (Button::Options, "Enter"),
                (Button::Home, "H"),
                (Button::Pad, "B"),
                (Button::Up, "ArrowUp"),
                (Button::Down, "ArrowDown"),
                (Button::Left, "ArrowLeft"),
                (Button::Right, "ArrowRight"),
            ]
            .into_iter()
            .map(|(button, key)| (button, named(key)))
            .collect(),
            left: [named("W"), named("S"), named("A"), named("D")],
            right: [named("P"), named(";"), named("["), named("]")],
        }
    }
}

impl Keys {
    /// Nothing bound at all.
    ///
    /// **What a second keyboard slot starts from is a real question**, and this is only half
    /// the answer - see [`Keys::shifted`], which is the other half. An empty map is right when
    /// somebody wants to build a layout; it is the wrong default for somebody who wanted a
    /// second player and now has seventeen buttons to bind by hand.
    #[must_use]
    pub fn none() -> Self {
        Self {
            buttons: BTreeMap::new(),
            left: [String::new(), String::new(), String::new(), String::new()],
            right: [String::new(), String::new(), String::new(), String::new()],
        }
    }

    /// A second layout that shares no key with the default one.
    ///
    /// # Why this exists rather than a copy of the default
    ///
    /// Two keyboard players is an ordinary thing to want, and the two obvious routes are both
    /// bad: copying the default collides on every key, and starting empty means binding
    /// seventeen buttons before anything happens.
    ///
    /// So the second player gets the other side of the keyboard. It will not suit everybody
    /// and it is rebindable - **what matters is that it works immediately and conflicts with
    /// nothing**, so the first thing somebody does is play rather than configure.
    #[must_use]
    pub fn shifted() -> Self {
        let named = |text: &str| text.to_owned();
        Self {
            buttons: [
                (Button::Square, "F"),
                (Button::Cross, "G"),
                (Button::Circle, "V"),
                (Button::Triangle, "R"),
                (Button::L1, "T"),
                (Button::R1, "Y"),
                (Button::L2, "1"),
                (Button::R2, "2"),
                (Button::L3, "N"),
                (Button::R3, "M"),
                (Button::Options, "Tab"),
                (Button::Home, "Backspace"),
                (Button::Pad, "Backslash"),
                (Button::Up, "Num8"),
                (Button::Down, "Num5"),
                (Button::Left, "Num4"),
                (Button::Right, "Num6"),
            ]
            .into_iter()
            .map(|(button, key)| (button, named(key)))
            .collect(),
            left: [named("Num7"), named("Num1"), named("Num9"), named("Num3")],
            right: [
                named("Home"),
                named("End"),
                named("Delete"),
                named("PageDown"),
            ],
        }
    }

    /// Builds a pad from whichever of these keys are held.
    ///
    /// `held` answers whether a named key is down. **Opposite directions cancel** rather than
    /// one winning, because a keyboard can hold both and a stick cannot be in two places - and
    /// picking a winner would make left-plus-right mean something a pad can never say.
    #[must_use]
    pub fn read(&self, held: &dyn Fn(&str) -> bool) -> Pad {
        // Rest, not zero: a zeroed pad holds both sticks hard left and up.
        let mut pad = Pad::rest();
        for (button, key) in &self.buttons {
            if held(key) {
                // `hold` sets a trigger's analogue byte as well, which the target needs.
                pad.hold(*button, true);
            }
        }
        let axis = |minus: &str, plus: &str| -> u8 {
            match (held(minus), held(plus)) {
                (true, false) => LOW,
                (false, true) => HIGH,
                // Both or neither is centred. A keyboard can hold both directions; a stick
                // cannot be in two places.
                _ => CENTRE,
            }
        };
        pad.left_y = axis(&self.left[0], &self.left[1]);
        pad.left_x = axis(&self.left[2], &self.left[3]);
        pad.right_y = axis(&self.right[0], &self.right[1]);
        pad.right_x = axis(&self.right[2], &self.right[3]);
        pad
    }

    /// Which key holds a button, if any does.
    #[must_use]
    pub fn key_for(&self, button: Button) -> Option<&str> {
        self.buttons.get(&button).map(String::as_str)
    }

    /// Binds a key to a button.
    ///
    /// **Does not take the key from whatever else had it.** An earlier version did, and that
    /// is the same failure it was meant to prevent seen from the other side: the button it
    /// silently unbound is now dead, and nothing said so. A key on two buttons is reported by
    /// [`Pads::conflicts`] instead, where somebody can see it and decide.
    pub fn bind(&mut self, key: &str, button: Button) {
        self.buttons.insert(button, key.to_owned());
    }

    /// Every key bound to more than one button here.
    fn collisions(&self) -> Vec<(String, [Button; 2])> {
        let mut seen: BTreeMap<&str, Button> = BTreeMap::new();
        let mut found = Vec::new();
        for (button, key) in &self.buttons {
            if let Some(already) = seen.insert(key.as_str(), *button) {
                found.push((key.clone(), [already, *button]));
            }
        }
        found
    }

    /// Every key this layout uses, including the stick and trigger keys.
    fn every_key(&self) -> Vec<&str> {
        self.buttons
            .values()
            .map(String::as_str)
            .chain(self.left.iter().map(String::as_str))
            .chain(self.right.iter().map(String::as_str))
            .filter(|key| !key.is_empty())
            .collect()
    }
}

/// One key doing two jobs.
///
/// **Reported rather than resolved**, because resolving it means silently undoing a binding
/// somebody made, and a binding that half works with nothing saying so is the harder bug.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Conflict {
    /// One key bound to two buttons on the same slot.
    Doubled {
        /// Which slot.
        slot: u8,
        /// The key.
        key: String,
        /// The two buttons it presses.
        buttons: [Button; 2],
    },
    /// One key driving two slots at once.
    ///
    /// **The two-players-one-keyboard case**, and the one that looks like it works: both pads
    /// move together, which reads as one person controlling two characters rather than as a
    /// mapping mistake.
    Shared {
        /// The two slots.
        slots: [u8; 2],
        /// The key they both use.
        key: String,
    },
}

impl Conflict {
    /// How to put it to somebody.
    #[must_use]
    pub fn describe(&self) -> String {
        match self {
            Self::Doubled { slot, key, buttons } => format!(
                "pad {}: {key} presses both {} and {}",
                slot + 1,
                buttons[0].name(),
                buttons[1].name()
            ),
            Self::Shared { slots, key } => format!(
                "{key} drives pad {} and pad {} together",
                slots[0] + 1,
                slots[1] + 1
            ),
        }
    }
}

/// One slot: what drives it, how it is bound, and what it last sent.
#[derive(Debug, Clone)]
pub struct Slot {
    /// What is driving it.
    pub source: Source,
    /// Which key holds which button, when the source is the keyboard.
    ///
    /// **Kept even when the source is not the keyboard**, so switching a slot to a controller
    /// and back does not throw away a layout somebody spent time on.
    pub keys: Keys,
    /// The state it last read.
    pub state: Pad,
    sender: Sender,
}

impl Slot {
    /// A slot with a layout but nothing driving it.
    ///
    /// The first two get real layouts and the rest start empty - not from meanness, but
    /// because two players share one keyboard and four cannot: past the second there are no
    /// keys left that anybody would find comfortable, and inventing a third layout out of
    /// whatever remains would be worse than an honest blank.
    #[must_use]
    pub fn new(slot: u8) -> Self {
        let keys = match slot {
            0 => Keys::default(),
            1 => Keys::shifted(),
            _ => Keys::none(),
        };
        Self {
            source: Source::Empty,
            keys,
            state: Pad {
                slot,
                ..Pad::rest()
            },
            sender: Sender::new(slot),
        }
    }

    /// Which pad this is.
    #[must_use]
    pub const fn number(&self) -> u8 {
        self.sender.slot()
    }

    /// Whether anything at all is bound here.
    ///
    /// **Worth asking before a slot is switched to the keyboard**, because a slot with no
    /// bindings looks exactly like one where the keyboard is not working.
    #[must_use]
    pub fn is_bound(&self) -> bool {
        !self.keys.buttons.is_empty()
    }

    /// Takes a freshly read state and returns the record to send, if one is worth sending.
    pub fn offer(&mut self, now: Pad) -> Option<[u8; RECORD]> {
        self.state = now;
        self.sender.update(now)
    }
}

/// Every slot a target has.
#[derive(Debug, Clone)]
pub struct Pads {
    /// One per slot, always [`SLOTS`] of them.
    ///
    /// **Fixed rather than a list that grows.** A target has four whether or not anything
    /// drives them, and a collection that only held the filled ones would renumber the rest
    /// when one emptied.
    pub slots: Vec<Slot>,
}

impl Default for Pads {
    fn default() -> Self {
        Self::new()
    }
}

impl Pads {
    /// Four slots, the first driven by the keyboard.
    ///
    /// One rather than none, because a fresh window with nothing bound has no way to press
    /// anything; one rather than four, because three empty pads in front of somebody with one
    /// keyboard is three questions they did not ask.
    #[must_use]
    pub fn new() -> Self {
        let mut slots: Vec<Slot> = (0..SLOTS).map(Slot::new).collect();
        if let Some(first) = slots.first_mut() {
            first.source = Source::Keyboard;
        }
        Self { slots }
    }

    /// How many slots something is driving.
    #[must_use]
    pub fn filled(&self) -> usize {
        self.slots
            .iter()
            .filter(|slot| slot.source != Source::Empty)
            .count()
    }

    /// Every key doing two jobs.
    ///
    /// # Why these are reported and not prevented
    ///
    /// Both kinds are almost always a mistake made while rebinding, and both alternatives are
    /// worse than saying so. Preventing a doubled key means silently unbinding whatever had
    /// it - a dead button nobody was told about. Preventing a shared one means refusing a
    /// layout somebody may have meant.
    ///
    /// **The shared case is the one that hides.** Two slots on the same keys move together,
    /// which reads as one person driving two characters rather than as a mapping fault, and
    /// nothing about it looks broken.
    #[must_use]
    pub fn conflicts(&self) -> Vec<Conflict> {
        let mut found = Vec::new();
        for slot in &self.slots {
            for (key, buttons) in slot.keys.collisions() {
                found.push(Conflict::Doubled {
                    slot: slot.number(),
                    key,
                    buttons,
                });
            }
        }

        // Only between slots the keyboard actually drives. Two layouts that overlap while one
        // of them is on a controller is a collision nobody can feel.
        let driven: Vec<&Slot> = self
            .slots
            .iter()
            .filter(|slot| slot.source == Source::Keyboard)
            .collect();
        for (at, one) in driven.iter().enumerate() {
            for other in &driven[at.saturating_add(1)..] {
                let theirs = other.keys.every_key();
                let mut said: Vec<&str> = Vec::new();
                for key in one.keys.every_key() {
                    if theirs.contains(&key) && !said.contains(&key) {
                        said.push(key);
                        found.push(Conflict::Shared {
                            slots: [one.number(), other.number()],
                            key: key.to_owned(),
                        });
                    }
                }
            }
        }
        found
    }

    /// Reads every slot and returns what should go on the wire.
    ///
    /// `held` answers whether a named key is down, for whichever slots are on the keyboard.
    pub fn poll(&mut self, held: &dyn Fn(&str) -> bool) -> Vec<[u8; RECORD]> {
        let mut out = Vec::new();
        for slot in &mut self.slots {
            let read = match slot.source {
                Source::Keyboard => slot.keys.read(held),
                // Nothing can read a controller yet, and an empty slot has nothing to read.
                // Neither sends, and neither pretends the other's state.
                Source::Empty | Source::Controller(_) => continue,
            };
            let numbered = Pad {
                slot: slot.number(),
                ..read
            };
            if let Some(record) = slot.offer(numbered) {
                out.push(record);
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::{Conflict, Keys, Pads, Source};
    use crate::pad::{Button, CENTRE, Pad, SLOTS};

    /// Answers true for the keys named.
    fn holding<'a>(down: &'a [&'a str]) -> impl Fn(&str) -> bool + 'a {
        move |key: &str| down.contains(&key)
    }

    /// The default layout reads into the buttons it names.
    #[test]
    fn a_held_key_holds_its_button() {
        let keys = Keys::default();
        let pad = keys.read(&holding(&["K", "U"]));
        assert!(pad.holds(Button::Cross));
        assert!(pad.holds(Button::L1));
        assert!(!pad.holds(Button::Circle));
    }

    /// **Opposite directions cancel rather than one winning.**
    ///
    /// A keyboard can hold both; a stick cannot be in two places, and picking a winner would
    /// make left-plus-right mean something no pad can say.
    #[test]
    fn holding_both_directions_centres_the_stick() {
        let keys = Keys::default();
        assert_eq!(keys.read(&holding(&["A"])).left_x, super::LOW);
        assert_eq!(keys.read(&holding(&["D"])).left_x, super::HIGH);
        assert_eq!(keys.read(&holding(&["A", "D"])).left_x, CENTRE);
        assert_eq!(keys.read(&holding(&[])).left_x, CENTRE);
    }

    /// A trigger key sets the bit and the pressure, because the target reads both.
    #[test]
    fn a_trigger_key_presses_and_pulls() {
        let pad = Keys::default().read(&holding(&["Q"]));
        assert!(pad.holds(Button::L2));
        assert_eq!(pad.l2, u8::MAX);
    }

    /// **Binding no longer steals the key from another button.**
    ///
    /// It used to, which was the same failure it meant to prevent seen from the other side:
    /// the button it silently unbound was dead and nothing said so. The collision is reported
    /// instead.
    #[test]
    fn binding_a_key_twice_is_reported_rather_than_resolved() {
        let mut pads = Pads::new();
        pads.slots[0].keys.bind("K", Button::Triangle);

        assert_eq!(
            pads.slots[0].keys.key_for(Button::Cross),
            Some("K"),
            "the button that had it still has it"
        );
        assert_eq!(pads.slots[0].keys.key_for(Button::Triangle), Some("K"));

        let said = pads.conflicts();
        assert_eq!(said.len(), 1, "{said:?}");
        assert!(matches!(said[0], Conflict::Doubled { slot: 0, .. }));
        assert!(said[0].describe().contains('K'));
    }

    /// **Two keyboard slots sharing a key is the conflict that hides.**
    ///
    /// Both pads move together, which reads as one person driving two characters rather than
    /// as a mapping fault.
    #[test]
    fn two_slots_on_the_same_key_are_reported() {
        let mut pads = Pads::new();
        pads.slots[1].source = Source::Keyboard;
        assert!(
            pads.conflicts().is_empty(),
            "the second layout shares nothing with the first"
        );

        pads.slots[1].keys.bind("K", Button::Cross);
        let said = pads.conflicts();
        assert!(
            said.iter().any(|one| matches!(
                one,
                Conflict::Shared { slots: [0, 1], key } if key == "K"
            )),
            "{said:?}"
        );
    }

    /// An overlap only counts when the keyboard actually drives both - a layout that overlaps
    /// while its slot is on a controller is a collision nobody can feel.
    #[test]
    fn an_overlap_on_a_slot_the_keyboard_does_not_drive_is_not_a_conflict() {
        let mut pads = Pads::new();
        pads.slots[1].source = Source::Controller(0);
        pads.slots[1].keys.bind("K", Button::Cross);
        assert!(pads.conflicts().is_empty(), "{:?}", pads.conflicts());
    }

    /// **A second keyboard player works immediately.**
    ///
    /// The two obvious alternatives are both bad: copying the first layout collides on every
    /// key, and an empty one means binding seventeen buttons before anything happens.
    #[test]
    fn the_second_slot_is_playable_the_moment_it_is_switched_on() {
        let mut pads = Pads::new();
        pads.slots[1].source = Source::Keyboard;

        assert!(pads.slots[1].is_bound(), "it starts with a layout");
        assert!(pads.conflicts().is_empty(), "and it collides with nothing");

        // **A newly active slot announces itself**, even at rest, so the target learns the
        // pad exists rather than only hearing about it when somebody presses something.
        let sent = pads.poll(&holding(&["K"]));
        assert_eq!(sent.len(), 2, "one press and one introduction");

        let mut read: Vec<Pad> = sent
            .iter()
            .map(|record| Pad::from_wire(record).expect("reads"))
            .collect();
        read.sort_by_key(|pad| pad.slot);

        // The first player's key moves only the first pad.
        assert_eq!(read[0].slot, 0);
        assert!(read[0].holds(Button::Cross));
        assert_eq!(read[1].slot, 1);
        assert!(
            read[1].is_at_rest(),
            "the second pad is present and holding nothing"
        );
    }

    /// A slot keeps its layout when it stops being on the keyboard, so switching back does
    /// not lose work.
    #[test]
    fn a_layout_survives_the_slot_changing_hands() {
        let mut pads = Pads::new();
        pads.slots[0].keys.bind("Y", Button::Cross);
        pads.slots[0].source = Source::Controller(0);
        pads.slots[0].source = Source::Keyboard;
        assert_eq!(pads.slots[0].keys.key_for(Button::Cross), Some("Y"));
    }

    /// A target has four slots whether or not anything drives them.
    #[test]
    fn there_are_always_four_slots() {
        let pads = Pads::new();
        assert_eq!(pads.slots.len(), SLOTS as usize);
        assert_eq!(pads.filled(), 1, "the first is on the keyboard");
        for (at, slot) in (0..).zip(pads.slots.iter()) {
            assert_eq!(slot.number(), at);
        }
    }

    /// **Each slot's records carry its own number**, so a payload can tell them apart.
    #[test]
    fn every_slot_sends_under_its_own_number() {
        let mut pads = Pads::new();
        pads.slots[2].source = Source::Keyboard;
        pads.slots[2].keys = Keys::default();

        let sent = pads.poll(&holding(&["K"]));
        let slots: Vec<u8> = sent
            .iter()
            .map(|record| Pad::from_wire(record).expect("reads").slot)
            .collect();
        assert_eq!(slots, [0, 2], "and nothing from the empty ones");
    }

    /// **An unreadable source sends nothing and says so**, rather than behaving like an empty
    /// slot - a declared controller nothing can read is somebody's mistake, and an empty slot
    /// is not.
    #[test]
    fn a_controller_is_declared_but_not_readable() {
        let mut pads = Pads::new();
        pads.slots[0].source = Source::Empty;
        pads.slots[1].source = Source::Controller(0);

        assert_eq!(pads.filled(), 1, "the slot is filled");
        assert!(!Source::Controller(0).is_readable(), "and cannot be read");
        assert!(
            pads.poll(&holding(&["K"])).is_empty(),
            "so nothing goes on the wire"
        );
    }

    /// A slot at rest goes quiet, and letting go still sends - the same rule as one pad, kept
    /// per slot so one player's silence does not stop another's input.
    #[test]
    fn each_slot_goes_quiet_on_its_own() {
        let mut pads = Pads::new();
        pads.slots[1].source = Source::Keyboard;

        assert_eq!(pads.poll(&holding(&["K", "G"])).len(), 2, "both press");
        assert_eq!(pads.poll(&holding(&[])).len(), 2, "both let go");
        assert!(pads.poll(&holding(&[])).is_empty(), "then both are quiet");
    }
}
