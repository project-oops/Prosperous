//! Several pads at once, and what drives each one.
//!
//! A target accepts [`crate::pad::SLOTS`] pads. A slot is not a device: what fills it (the keyboard, a
//! physical controller, or nothing) is assigned per slot, so unplugging a controller empties
//! its slot instead of renumbering the rest.
//!
//! The keyboard is a first-class source because the window already receives key state, so it
//! needs no dependency. Reading a physical controller would need a crate (the workspace forbids
//! unsafe code), so a slot set to a controller reports that nothing can read it.

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
    /// Declared but not readable: it reports as unreadable rather than behaving like
    /// [`Source::Empty`], since only one of the two is a mistake.
    Controller(u8),
}

impl Source {
    /// The source's display name.
    #[must_use]
    pub fn describe(self) -> String {
        match self {
            Self::Empty => "nothing".to_owned(),
            Self::Keyboard => "keyboard".to_owned(),
            Self::Controller(which) => format!("controller {which}"),
        }
    }

    /// Whether anything can read this.
    ///
    /// `false` for a controller, which nothing here reads; claiming otherwise would make it
    /// look like a pad at rest.
    #[must_use]
    pub const fn is_readable(self) -> bool {
        matches!(self, Self::Keyboard)
    }
}

/// How far a key pushes a stick: all the way.
///
/// A key is digital, and a stick that never reaches its edge reads as a slow walk. The axis is
/// an unsigned byte centred on [`CENTRE`], as the target reads it.
pub const LOW: u8 = 0;

/// The other end of the same travel.
pub const HIGH: u8 = u8::MAX;

/// Which key does what, for a keyboard-driven pad.
///
/// Keys are names, not key codes, because this crate has no window; the window resolves a name
/// to its own key type. Keyed by button, so a button has exactly one key; a key on two buttons
/// is found by [`Pads::conflicts`].
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
    /// The first player's layout: face buttons under the right hand, movement on the left, and
    /// no key the window needs for itself.
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
    /// Nothing bound at all, for building a layout from scratch.
    #[must_use]
    pub fn none() -> Self {
        Self {
            buttons: BTreeMap::new(),
            left: [String::new(), String::new(), String::new(), String::new()],
            right: [String::new(), String::new(), String::new(), String::new()],
        }
    }

    /// A second player's layout that shares no key with [`Keys::default`].
    ///
    /// Uses the other side of the keyboard, so a second keyboard player works at once with no
    /// conflict and no binding by hand.
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
    /// `held` answers whether a named key is down. Opposite directions cancel to centre, since
    /// a keyboard can hold both and a stick cannot be in two places.
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
    /// Does not take the key from another button, which would leave that button silently
    /// dead; a key on two buttons is reported by [`Pads::conflicts`] instead.
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
/// Reported rather than resolved, because resolving it would silently undo a binding.
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
    /// Both pads move together, which looks like working input rather than a mapping mistake.
    Shared {
        /// The two slots.
        slots: [u8; 2],
        /// The key they both use.
        key: String,
    },
}

impl Conflict {
    /// The conflict as a sentence for display.
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
    /// Kept when the source changes, so switching away and back keeps the layout.
    pub keys: Keys,
    /// The state it last read.
    pub state: Pad,
    sender: Sender,
}

impl Slot {
    /// A slot with a layout but nothing driving it.
    ///
    /// Slots 0 and 1 get [`Keys::default`] and [`Keys::shifted`]; the rest start empty, since
    /// one keyboard has no comfortable room for a third layout.
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
    /// A slot with no bindings looks exactly like one where the keyboard is not working.
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
    /// Fixed, so emptying one slot never renumbers the others.
    pub slots: Vec<Slot>,
}

impl Default for Pads {
    fn default() -> Self {
        Self::new()
    }
}

impl Pads {
    /// All slots, the first driven by the keyboard so a fresh window can press something.
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
    /// Reported, not prevented: preventing a doubled key would silently unbind a button, and
    /// preventing a shared one would refuse a layout that may be intended. Shared keys are
    /// checked only between slots the keyboard drives.
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

        // An overlap with a slot on a controller has no effect.
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
                // An empty slot has nothing to read and a controller cannot be read.
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

    /// Opposite directions cancel to centre rather than one winning.
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

    /// Binding a key already in use keeps both bindings and reports the collision.
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

    /// Two keyboard slots sharing a key are reported as a shared conflict.
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

    /// An overlap with a slot the keyboard does not drive is not a conflict.
    #[test]
    fn an_overlap_on_a_slot_the_keyboard_does_not_drive_is_not_a_conflict() {
        let mut pads = Pads::new();
        pads.slots[1].source = Source::Controller(0);
        pads.slots[1].keys.bind("K", Button::Cross);
        assert!(pads.conflicts().is_empty(), "{:?}", pads.conflicts());
    }

    /// A second keyboard slot is bound, conflict-free and announces itself when switched on.
    #[test]
    fn the_second_slot_is_playable_the_moment_it_is_switched_on() {
        let mut pads = Pads::new();
        pads.slots[1].source = Source::Keyboard;

        assert!(pads.slots[1].is_bound(), "it starts with a layout");
        assert!(pads.conflicts().is_empty(), "and it collides with nothing");

        // A newly active slot sends once at rest, so the target learns the pad exists.
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

    /// A slot keeps its layout when its source changes and changes back.
    #[test]
    fn a_layout_survives_the_slot_changing_hands() {
        let mut pads = Pads::new();
        pads.slots[0].keys.bind("Y", Button::Cross);
        pads.slots[0].source = Source::Controller(0);
        pads.slots[0].source = Source::Keyboard;
        assert_eq!(pads.slots[0].keys.key_for(Button::Cross), Some("Y"));
    }

    /// A target has all its slots whether or not anything drives them.
    #[test]
    fn there_are_always_four_slots() {
        let pads = Pads::new();
        assert_eq!(pads.slots.len(), SLOTS as usize);
        assert_eq!(pads.filled(), 1, "the first is on the keyboard");
        for (at, slot) in (0..).zip(pads.slots.iter()) {
            assert_eq!(slot.number(), at);
        }
    }

    /// Each slot's records carry its own number, so a payload can tell them apart.
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

    /// A controller slot counts as filled, is unreadable, and sends nothing.
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

    /// Each slot goes quiet at rest independently, and letting go still sends.
    #[test]
    fn each_slot_goes_quiet_on_its_own() {
        let mut pads = Pads::new();
        pads.slots[1].source = Source::Keyboard;

        assert_eq!(pads.poll(&holding(&["K", "G"])).len(), 2, "both press");
        assert_eq!(pads.poll(&holding(&[])).len(), 2, "both let go");
        assert!(pads.poll(&holding(&[])).is_empty(), "then both are quiet");
    }
}
