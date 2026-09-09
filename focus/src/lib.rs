// Distinct title + app_id in debug builds so a niri window-rule can
// match only the dev instance (e.g. `open-focused false`), and so a
// debug daemon never takes over the release one's socket.
pub const APP_ID: &str = if cfg!(debug_assertions) {
    "focus-debug"
} else {
    "focus"
};

pub mod atlas;
pub mod chrome;
pub mod daemon;
pub mod render;
