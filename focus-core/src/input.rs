#[derive(Clone, Debug)]
pub enum InputEvent<'a> {
    CloseRequested,
    ModifiersChanged(ModifiersState),
    Key {
        state: ElementState,
        logical_key: Key<'a>,
    },
    MouseWheel {
        y_offset: f32,
    },
    MouseButton {
        state: ElementState,
        position: [f32; 2],
    },
    FocusChanged {
        focused: bool,
    },
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ElementState {
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
