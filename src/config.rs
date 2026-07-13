/* rent configuration -- edit and rebuild, suckless style.
 * Mirrors sent's config.def.h. */

use winit::event::MouseButton;
use winit::keyboard::{Key, NamedKey};

/* Font families tried in order; every family found on the system is loaded.
 * Per-character fallback walks this list, then a generic sans-serif face,
 * then EMOJI_FONTS. */
pub const FONT_FALLBACKS: &[&str] = &[
    "Helvetica Neue",
    "DejaVu Sans",
    "Roboto",
    "Ubuntu",
];

/* Color emoji faces, tried last so text symbols keep the text font. */
pub const EMOJI_FONTS: &[&str] = &[
    "Apple Color Emoji",
    "Noto Color Emoji",
    "Segoe UI Emoji",
];

pub const NUMFONTSCALES: usize = 42;

/* x in [0, NUMFONTSCALES-1] */
pub fn fontsz(x: usize) -> f32 {
    (10.0 * 1.1288f32.powi(x as i32)) as i32 as f32
}

/* 0xRRGGBB */
pub const FOREGROUND: u32 = 0x000000;
pub const BACKGROUND: u32 = 0xFFFFFF;

pub const LINESPACING: f32 = 1.4;

/* how much screen estate is to be used at max for the content */
pub const USABLEWIDTH: f32 = 0.75;
pub const USABLEHEIGHT: f32 = 0.75;

/* raster resolution of pdf export pages (-o); page aspect follows it */
pub const EXPORTWIDTH: u32 = 1920;
pub const EXPORTHEIGHT: u32 = 1080;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Advance(i32),
    Quit,
    Reload,
}

pub fn key_action(key: &Key) -> Option<Action> {
    use NamedKey::*;
    match key {
        Key::Named(k) => match k {
            Escape => Some(Action::Quit),
            ArrowRight | ArrowDown | Enter | Space | PageDown => Some(Action::Advance(1)),
            ArrowLeft | ArrowUp | Backspace | PageUp => Some(Action::Advance(-1)),
            _ => None,
        },
        Key::Character(s) => match s.to_lowercase().as_str() {
            "q" => Some(Action::Quit),
            " " | "l" | "j" | "n" => Some(Action::Advance(1)),
            "h" | "k" | "p" => Some(Action::Advance(-1)),
            "r" => Some(Action::Reload),
            _ => None,
        },
        _ => None,
    }
}

pub fn button_action(b: MouseButton) -> Option<Action> {
    match b {
        MouseButton::Left => Some(Action::Advance(1)),
        MouseButton::Right => Some(Action::Advance(-1)),
        _ => None,
    }
}
