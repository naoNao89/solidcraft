//! SolidCraft: the text-input decisions of the macOS view, kept free of AppKit so their regression
//! test runs wherever the tests run — this project's CI has no macOS runner, and the vendored crate
//! is not a workspace member (see vendor/README.md).
//!
//! Backport of rust-windowing/winit#4651 (rust-windowing/winit#3925): an input method ends a
//! composition by clearing its marked text and *then* handing over the key that ended it, which
//! `insertText:replacementRange:` used to drop.

// The AppKit view is the only caller, so a Linux or Windows build sees these as unused — they are
// used by the view on macOS and by the tests everywhere.
#![cfg_attr(not(any(target_os = "macos", test)), allow(dead_code))]

/// What `insertText:replacementRange:` does with the string an input method hands over.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct InsertText {
    /// Clear the preedit the string replaces (one `Ime::Preedit("")`).
    pub(crate) clear_preedit: bool,
    /// Commit the string (`Ime::Commit`), where a plain key is left to `KeyboardInput`.
    pub(crate) commit: bool,
}

/// The composition of an input method, as the AppKit view sees it: `setMarkedText:` starts or
/// clears one, `insertText:` hands its text over, allowing IME again after it was disallowed
/// throws a half-finished one away.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct TextInput {
    /// A composition is under way until `insert_text` consumes it.
    pending_commit: bool,
}

impl TextInput {
    /// `setMarkedText:selectedRange:replacementRange:` — the input method set or cleared its
    /// marked text.
    pub(crate) fn set_marked_text(&mut self) {
        self.pending_commit = true;
    }

    /// `insertText:replacementRange:` — the input method handed a string over. `has_marked_text`
    /// is the view's `hasMarkedText`, `ime_enabled` its `is_ime_enabled`, `is_control` whether the
    /// string is a control character.
    pub(crate) fn insert_text(
        &mut self,
        has_marked_text: bool,
        ime_enabled: bool,
        is_control: bool,
    ) -> InsertText {
        let commit = self.pending_commit && has_marked_text && ime_enabled && !is_control;
        if commit {
            self.pending_commit = false;
        }
        InsertText { clear_preedit: has_marked_text, commit }
    }

    /// `set_ime_allowed(false)` — no composition survives it.
    pub(crate) fn ime_disallowed(&mut self) {
        self.pending_commit = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The macOS Vietnamese Telex run this comes from: `tieengs ` typed with the input method,
    /// with every `NSTextInputClient` call winit got logged. Winit saw, in this order:
    ///
    /// ```text
    /// setMarkedText("t") … setMarkedText("tiếng")   one per key of "tieengs"
    /// insertText("tiếng")   marked text still there → the syllable commits
    /// setMarkedText("")     the input method clears its marked text
    /// insertText(" ")       the space itself arrives with no marked text left
    /// ```
    ///
    /// The application has to end up with the space. It only did after a second attempt, because
    /// `insertText(" ")` found no marked text and was dropped: nothing reached the text field, and
    /// the `KeyboardInput` of that key is not forwarded either (the composition had just ended).
    #[test]
    fn the_space_after_a_vietnamese_syllable_is_committed() {
        let mut input = TextInput::default();
        for _ in 0..7 {
            input.set_marked_text();
        }
        assert_eq!(
            input.insert_text(true, true, false),
            InsertText { clear_preedit: true, commit: true },
            "the syllable commits"
        );
        input.set_marked_text();
        assert_eq!(
            input.insert_text(false, true, false),
            InsertText { clear_preedit: false, commit: true },
            "the space that ended the composition is text, marked or not"
        );
        assert_eq!(
            input.insert_text(false, true, false),
            InsertText { clear_preedit: false, commit: false },
            "the composition is over: the next plain key goes through KeyboardInput"
        );
    }

    /// Plain typing: no composition is under way, so nothing is committed here — the key's own
    /// `KeyboardInput` carries the text, and committing it as well would insert it twice.
    #[test]
    fn plain_typing_is_not_committed() {
        let mut input = TextInput::default();
        assert_eq!(
            input.insert_text(false, true, false),
            InsertText { clear_preedit: false, commit: false }
        );
        assert_eq!(
            input.insert_text(true, true, false),
            InsertText { clear_preedit: true, commit: false }
        );
    }

    /// A control character (Return, Tab) is never text, whatever the input method sends while a
    /// composition is under way; the preedit it replaces still goes.
    #[test]
    fn a_control_character_is_never_committed() {
        let mut input = TextInput::default();
        input.set_marked_text();
        assert_eq!(
            input.insert_text(true, true, true),
            InsertText { clear_preedit: true, commit: false }
        );
    }

    /// With IME off, AppKit hands over the key's own text and winit forwards the key event.
    #[test]
    fn a_disabled_ime_never_commits() {
        let mut input = TextInput::default();
        input.set_marked_text();
        assert_eq!(
            input.insert_text(true, false, false),
            InsertText { clear_preedit: true, commit: false }
        );
    }

    /// A field losing focus disallows IME mid-composition; the string that arrives afterwards is
    /// not part of it.
    #[test]
    fn disallowing_ime_throws_the_composition_away() {
        let mut input = TextInput::default();
        input.set_marked_text();
        input.ime_disallowed();
        assert_eq!(
            input.insert_text(false, true, false),
            InsertText { clear_preedit: false, commit: false }
        );
    }
}
