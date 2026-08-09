//! Abstract key input.
//!
//! A logical key, a modifier set and the text the key would commit. No
//! keysyms, no scancodes, no platform event types: translating
//! `IBUS_KEY_BackSpace`, a Win32 `WM_KEYDOWN` or an `NSEvent` into a
//! [`LogicalKey`] is the shell's job, and for IBus it lives in `pinyin-capi`.

use core::fmt;
use core::ops::BitOr;

/// Modifier keys held while a key was pressed.
///
/// A small bitset rather than a dependency: the engine only needs to tell
/// "plain typing" from "a shortcut the shell owns".
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub struct Modifiers(u8);

impl Modifiers {
    /// No modifier held.
    pub const NONE: Self = Self(0);
    /// Shift.
    pub const SHIFT: Self = Self(1 << 0);
    /// Control.
    pub const CONTROL: Self = Self(1 << 1);
    /// Alt, Option or Meta, whichever the platform calls it.
    pub const ALT: Self = Self(1 << 2);
    /// Super, Command or Windows.
    pub const SUPER: Self = Self(1 << 3);

    /// Every bit this type defines.
    const ALL: u8 = 0b1111;

    /// Builds a modifier set from raw bits, rejecting undefined ones.
    ///
    /// Rejecting rather than masking means a shell that invents a bit finds
    /// out instead of being silently misread.
    #[must_use]
    pub const fn from_bits(bits: u8) -> Option<Self> {
        if bits & !Self::ALL == 0 {
            Some(Self(bits))
        } else {
            None
        }
    }

    /// The raw bits.
    #[must_use]
    pub const fn bits(self) -> u8 {
        self.0
    }

    /// Whether no modifier is held.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// Whether every modifier in `other` is held.
    #[must_use]
    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    /// Whether any modifier that suppresses text entry is held.
    ///
    /// Shift is not one: `Shift`+letter is ordinary typing. Control, Alt and
    /// Super mean the shell is invoking a command, so the engine leaves the
    /// key alone.
    #[must_use]
    pub const fn has_command_modifier(self) -> bool {
        self.0 & (Self::CONTROL.0 | Self::ALT.0 | Self::SUPER.0) != 0
    }
}

impl BitOr for Modifiers {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self {
        Self(self.0 | rhs.0)
    }
}

/// A key, named by what it means rather than by how a platform encodes it.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[non_exhaustive]
pub enum LogicalKey {
    /// A key that produces a character.
    Character(char),
    /// Backspace.
    Backspace,
    /// Forward delete.
    Delete,
    /// Enter or Return.
    Enter,
    /// Escape.
    Escape,
    /// Space.
    Space,
    /// Tab.
    Tab,
    /// Left arrow.
    Left,
    /// Right arrow.
    Right,
    /// Up arrow.
    Up,
    /// Down arrow.
    Down,
    /// Home.
    Home,
    /// End.
    End,
    /// Page up.
    PageUp,
    /// Page down.
    PageDown,
    /// A key the shell could not map.
    ///
    /// Present so a shell never has to invent a key it does not understand.
    /// The engine reports such a key as unused and changes nothing.
    Unknown,
}

/// One key press handed to a session.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KeyInput {
    key: LogicalKey,
    modifiers: Modifiers,
    text: String,
}

impl KeyInput {
    /// Builds a key press.
    ///
    /// `text` is what the key would insert if the engine ignored it, and is
    /// empty for keys that produce no text.
    #[must_use]
    pub fn new(key: LogicalKey, modifiers: Modifiers, text: impl Into<String>) -> Self {
        Self {
            key,
            modifiers,
            text: text.into(),
        }
    }

    /// Builds an unmodified character key press.
    #[must_use]
    pub fn character(character: char) -> Self {
        Self::new(
            LogicalKey::Character(character),
            Modifiers::NONE,
            character.to_string(),
        )
    }

    /// Builds an unmodified key press that produces no text.
    #[must_use]
    pub fn plain(key: LogicalKey) -> Self {
        Self::new(key, Modifiers::NONE, "")
    }

    /// The logical key.
    #[must_use]
    pub const fn key(&self) -> LogicalKey {
        self.key
    }

    /// The modifiers held.
    #[must_use]
    pub const fn modifiers(&self) -> Modifiers {
        self.modifiers
    }

    /// The text the key would insert without the engine.
    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }
}

impl fmt::Display for Modifiers {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_empty() {
            return formatter.write_str("none");
        }
        let mut first = true;
        for (modifier, name) in [
            (Self::SHIFT, "shift"),
            (Self::CONTROL, "control"),
            (Self::ALT, "alt"),
            (Self::SUPER, "super"),
        ] {
            if self.contains(modifier) {
                if !first {
                    formatter.write_str("+")?;
                }
                formatter.write_str(name)?;
                first = false;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{KeyInput, LogicalKey, Modifiers};

    #[test]
    fn modifier_sets_compose_and_reject_undefined_bits() {
        let combined = Modifiers::CONTROL | Modifiers::SHIFT;
        assert!(combined.contains(Modifiers::CONTROL));
        assert!(combined.contains(Modifiers::SHIFT));
        assert!(!combined.contains(Modifiers::ALT));
        assert!(combined.has_command_modifier());
        assert!(!Modifiers::SHIFT.has_command_modifier());
        assert!(Modifiers::NONE.is_empty());

        assert_eq!(
            Modifiers::from_bits(0b1111).map(Modifiers::bits),
            Some(0b1111)
        );
        assert_eq!(Modifiers::from_bits(0b1_0000), None);
    }

    #[test]
    fn modifier_display_names_every_held_key() {
        assert_eq!(Modifiers::NONE.to_string(), "none");
        assert_eq!(
            (Modifiers::SHIFT | Modifiers::SUPER).to_string(),
            "shift+super"
        );
    }

    #[test]
    fn character_input_carries_its_own_text() {
        let input = KeyInput::character('n');
        assert_eq!(input.key(), LogicalKey::Character('n'));
        assert_eq!(input.text(), "n");
        assert!(input.modifiers().is_empty());

        let plain = KeyInput::plain(LogicalKey::Enter);
        assert_eq!(plain.key(), LogicalKey::Enter);
        assert_eq!(plain.text(), "");
    }
}
