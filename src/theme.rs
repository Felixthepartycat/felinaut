//! Shared color tokens: themes change browser chrome, never website styling.

use gpui::{App, Global};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Theme {
    #[default]
    MidnightParty,
    ConfettiSunset,
    NeonWool,
    AlpinePop,
    ForestCarnival,
}

#[derive(Clone, Copy)]
pub struct Palette {
    pub theme: Theme,
    pub background: u32,
    pub surface: u32,
    pub elevated: u32,
    pub input: u32,
    pub text: u32,
    pub muted: u32,
    pub border: u32,
    pub primary: u32,
    pub secondary: u32,
    pub accent: u32,
    pub highlight: u32,
    pub dark: bool,
}

impl Theme {
    pub const ALL: [Self; 5] = [
        Self::MidnightParty,
        Self::ConfettiSunset,
        Self::NeonWool,
        Self::AlpinePop,
        Self::ForestCarnival,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::MidnightParty => "Midnight Party",
            Self::ConfettiSunset => "Confetti Sunset",
            Self::NeonWool => "Neon Wool",
            Self::AlpinePop => "Alpine Pop",
            Self::ForestCarnival => "Forest Carnival",
        }
    }

    pub fn palette(self) -> Palette {
        match self {
            Self::MidnightParty => Palette {
                theme: self,
                background: 0x2b0a54,
                surface: 0x3c126f,
                elevated: 0x512287,
                input: 0x23083f,
                text: 0xf8f7ff,
                muted: 0xc6b5de,
                border: 0x70499a,
                primary: 0xff3ea5,
                secondary: 0x8a4dff,
                accent: 0xff8a00,
                highlight: 0x00d4ff,
                dark: true,
            },
            Self::ConfettiSunset => Palette {
                theme: self,
                background: 0xfff8ed,
                surface: 0xffffff,
                elevated: 0xffe6ee,
                input: 0xfffaf5,
                text: 0x2b0a54,
                muted: 0x77566e,
                border: 0xd9b7d5,
                primary: 0xba165e,
                secondary: 0xff8a00,
                accent: 0xffd83d,
                highlight: 0x008da9,
                dark: false,
            },
            Self::NeonWool => Palette {
                theme: self,
                background: 0x1b0638,
                surface: 0x341364,
                elevated: 0x4c2383,
                input: 0x220b43,
                text: 0xf8f7ff,
                muted: 0xc4b4df,
                border: 0x6b4694,
                primary: 0xae80ff,
                secondary: 0x00d4ff,
                accent: 0xff3ea5,
                highlight: 0xffd83d,
                dark: true,
            },
            Self::AlpinePop => Palette {
                theme: self,
                background: 0xf2faff,
                surface: 0xffffff,
                elevated: 0xdff3ff,
                input: 0xf6fbff,
                text: 0x2b0a54,
                muted: 0x52657c,
                border: 0xafcadf,
                primary: 0x007c9e,
                secondary: 0x8a4dff,
                accent: 0xffb400,
                highlight: 0xd4207f,
                dark: false,
            },
            Self::ForestCarnival => Palette {
                theme: self,
                background: 0xf3fff2,
                surface: 0xffffff,
                elevated: 0xe0f5df,
                input: 0xf8fff7,
                text: 0x1b0638,
                muted: 0x536b57,
                border: 0xaccfae,
                primary: 0x007e38,
                secondary: 0x8a4dff,
                accent: 0xffd83d,
                highlight: 0xd4207f,
                dark: false,
            },
        }
    }
}

#[derive(Default)]
pub struct ThemeSettings {
    pub theme: Theme,
    /// Session preference, additionally constrained by macOS Reduce Motion.
    pub animate: bool,
}
impl Global for ThemeSettings {}

pub fn palette(cx: &App) -> Palette {
    cx.global::<ThemeSettings>().theme.palette()
}

pub fn system_reduced_motion() -> bool {
    #[cfg(target_os = "macos")]
    {
        objc2_app_kit::NSWorkspace::sharedWorkspace().accessibilityDisplayShouldReduceMotion()
    }
    #[cfg(not(target_os = "macos"))]
    {
        true
    }
}

pub fn motion_allowed(cx: &App) -> bool {
    cx.global::<ThemeSettings>().animate && !system_reduced_motion()
}

#[cfg(test)]
mod tests {
    use super::*;
    fn luminance(color: u32) -> f64 {
        let channel = |shift: u32| {
            let c = ((color >> shift) & 255u32) as f64 / 255.;
            if c <= 0.04045 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * channel(16) + 0.7152 * channel(8) + 0.0722 * channel(0)
    }
    fn contrast(a: u32, b: u32) -> f64 {
        let a = luminance(a);
        let b = luminance(b);
        (a.max(b) + 0.05) / (a.min(b) + 0.05)
    }
    #[test]
    fn mountain_theme_is_named_alpine_pop() {
        assert_eq!(Theme::AlpinePop.name(), "Alpine Pop");
        assert!(Theme::ALL.contains(&Theme::AlpinePop));
    }

    #[test]
    fn text_is_readable_in_every_theme() {
        for theme in Theme::ALL {
            let p = theme.palette();
            for background in [p.background, p.surface, p.elevated, p.input] {
                assert!(
                    contrast(p.text, background) >= 4.5,
                    "{}: primary text",
                    theme.name()
                );
                assert!(
                    contrast(p.muted, background) >= 4.5,
                    "{}: muted text",
                    theme.name()
                );
            }
            assert!(
                contrast(p.primary, p.surface) >= 3.,
                "{}: accent",
                theme.name()
            );
        }
    }
}
