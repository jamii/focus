// Input vocabulary for the core, independent of any windowing backend.
// The platform layer (the `chrome`/`render` modules in the `focus` crate)
// translates winit events into these types; tests and the fuzzer build
// them directly.

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ElementState {
    Pressed,
    Released,
}

// The named (non-text) keys the editor distinguishes. Anything else the
// platform layer can't map is simply not forwarded.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NamedKey {
    Enter,
    Space,
    Backspace,
    Delete,
    ArrowLeft,
    ArrowRight,
    ArrowUp,
    ArrowDown,
}

// A logical key press. Events are consumed where they're produced, so the
// text payload is borrowed rather than owned — match arms compare against
// string literals directly:
//
//   match key {
//       Key::Character("s") => ...
//       Key::Named(NamedKey::Enter) => ...
//   }
#[derive(Clone, Copy, Debug)]
pub enum Key<'a> {
    Character(&'a str),
    Named(NamedKey),
}

// Which modifier keys are currently held.
#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub struct ModifiersState {
    pub control: bool,
    pub alt: bool,
    pub shift: bool,
    pub super_: bool,
}

// bot: remove these
impl ModifiersState {
    pub const CONTROL: Self = ModifiersState {
        control: true,
        alt: false,
        shift: false,
        super_: false,
    };
    pub const ALT: Self = ModifiersState {
        control: false,
        alt: true,
        shift: false,
        super_: false,
    };
}
