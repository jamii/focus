#[derive(Clone, Debug)]
pub enum InputEvent<'a> {
    CloseRequested,
    ModifiersChanged(ModifiersState),
    Key {
        state: ButtonState,
        logical_key: Key<'a>,
    },
    MouseWheel {
        y_offset: f32,
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
