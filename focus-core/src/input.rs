#[derive(Clone, Debug)]
pub enum InputEvent<'a> {
    CloseRequested,
    ModifiersChanged(ModifiersState),
    Key {
        state: ButtonState,
        logical_key: Key<'a>,
    },
    /// A mouse wheel notch.
    MouseWheel {
        y_lines: f32,
    },
    /// One step of a touchpad scroll gesture: `y_pixels` of finger
    /// movement, and where in the gesture we are.
    TouchpadScroll {
        y_pixels: f32,
        phase: ScrollPhase,
    },
    MouseButton {
        state: ButtonState,
        position: [f32; 2],
    },
    MouseMoved {
        position: [f32; 2],
    },
    FocusChanged {
        focused: bool,
    },
}

/// A touchpad scroll gesture runs Started, Moved.., Ended as the fingers
/// touch, move and leave the pad. Only Ended is worth acting on beyond
/// scrolling: it is where the momentum starts.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ScrollPhase {
    Started,
    Moved,
    Ended,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ButtonState {
    Pressed,
    Released,
}

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

#[derive(Clone, Copy, Debug)]
pub enum Key<'a> {
    Character(&'a str),
    Named(NamedKey),
}

#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub struct ModifiersState {
    pub control: bool,
    pub alt: bool,
    pub shift: bool,
    pub super_: bool,
}
