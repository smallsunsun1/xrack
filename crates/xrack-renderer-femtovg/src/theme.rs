use std::ops::Deref;
use xrack_runtime::{Color, ColorScheme, Node};

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Theme {
    pub window: Color,
    pub text: Color,
    pub button: Color,
    pub disabled: Color,
    pub disabled_text: Color,
    pub focus: Color,
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            window: Color(0x18202bff),
            text: Color(0xf2f5faff),
            button: Color(0x536dfeff),
            disabled: Color(0x3a4350ff),
            disabled_text: Color(0x85909fff),
            focus: Color(0xadc2ffff),
        }
    }
}

impl Theme {
    fn light() -> Self {
        Self {
            window: Color(0xf3f6fbff),
            text: Color(0x16223aff),
            button: Color(0x3456caff),
            disabled: Color(0xd7dce6ff),
            disabled_text: Color(0x637089ff),
            focus: Color(0x3154b8ff),
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) enum PaletteRole {
    Surface,
    InsetSurface,
    Control,
    SecondaryButton,
    Danger,
    Border,
    RaisedBorder,
    Divider,
    StrongDivider,
    Track,
    DialArc,
    ScrollTrack,
    ScrollThumb,
    Grip,
    HoverSurface,
    SelectedSurface,
    InactiveSelection,
    TableHeader,
    SelectedColumn,
    ActiveColumn,
    TextSelection,
    RowSelection,
    SelectionOutline,
    RowHover,
    Disclosure,
    DisabledDisclosure,
    DelayProgress,
    Scrim,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct Palette {
    theme: Theme,
    light: bool,
    accent: Option<Color>,
    on_accent: Option<Color>,
}

impl Deref for Palette {
    type Target = Theme;

    fn deref(&self) -> &Theme {
        &self.theme
    }
}

impl Palette {
    pub(crate) fn resolve(fallback: Theme, scene: &Node, system: ColorScheme) -> Self {
        let scheme = scene.color_scheme().map(|scheme| match scheme {
            ColorScheme::System => system,
            chosen => chosen,
        });
        let light = scheme == Some(ColorScheme::Light);
        let mut theme = match scheme {
            Some(ColorScheme::Light) => Theme::light(),
            Some(ColorScheme::Dark) => Theme::default(),
            Some(ColorScheme::System) | None => fallback,
        };
        let accent = scene
            .color("accent-color")
            .map(|color| with_alpha(color, 255));
        if let Some(accent) = accent {
            theme.button = accent;
            theme.focus = mix(accent, theme.text, 0.5);
        }
        let on_accent = (accent.is_some()
            || matches!(scheme, Some(ColorScheme::Light | ColorScheme::Dark)))
        .then(|| contrasting_text(theme.button));
        Self {
            theme,
            light,
            accent,
            on_accent,
        }
    }

    pub(crate) fn on_accent(self, fallback: Color) -> Color {
        self.on_accent.unwrap_or(fallback)
    }

    pub(crate) fn is_light(self) -> bool {
        self.light
    }

    pub(crate) fn role(self, role: PaletteRole) -> Color {
        use PaletteRole::*;
        let (dark, light) = match role {
            Surface => (0x27354dff, 0xffffffff),
            InsetSurface => (0x25334bff, 0xeaf0f9ff),
            Control => (0x384761ff, 0xd5deedff),
            SecondaryButton => (0x384966ff, 0xe1e9f6ff),
            Danger => (0xa63843ff, 0xb02b39ff),
            Border => (0x75859aff, 0x7183a2ff),
            RaisedBorder => (0x627596ff, 0x889ab7ff),
            Divider => (0x637ca4ff, 0x8598b7ff),
            StrongDivider => (0x506483ff, 0x9babbeff),
            Track => (0x35415bff, 0xd3deefff),
            DialArc => (0x586582ff, 0x9cabbeff),
            ScrollTrack => (0x202c42d9, 0xdbe5f2d9),
            ScrollThumb => (0x9bb4e3d9, 0x5b7399d9),
            Grip => (0x9bb4e3a0, 0x5b7399a0),
            HoverSurface => (0x405582ff, 0xdbe6f9ff),
            SelectedSurface => (0x435e99ff, 0xcbdcfaff),
            InactiveSelection => (0x35466dff, 0xe1eafaff),
            TableHeader => (0x314363ff, 0xe6eefaff),
            SelectedColumn => (0x455a89ff, 0xcbdcfaff),
            ActiveColumn => (0x526899ff, 0xbacef6ff),
            TextSelection => (0x4e71c09b, 0x87acf69b),
            RowSelection => (0x768dff38, 0x3456ca26),
            SelectionOutline => (0x9aadffff, 0x3154b8ff),
            RowHover => (0xb3c3ff24, 0x5278c21f),
            Disclosure => (0xc0d1eaff, 0x3f5072ff),
            DisabledDisclosure => (0x687991ff, 0x7d899eff),
            DelayProgress => (0x366483ff, 0x25449dff),
            Scrim => (0x080d15a8, 0x19274566),
        };
        if let Some(accent) = self.accent {
            let surface = Color(if self.light { 0xffffffff } else { 0x27354dff });
            match role {
                SelectedSurface | SelectedColumn => return mix(surface, accent, 0.28),
                HoverSurface | InactiveSelection => return mix(surface, accent, 0.16),
                ActiveColumn => return mix(surface, accent, 0.36),
                TextSelection => return with_alpha(mix(surface, accent, 0.6), 155),
                RowSelection => return with_alpha(accent, 56),
                SelectionOutline => return self.focus,
                DelayProgress => return mix(accent, Color(0x000000ff), 0.22),
                _ => {}
            }
        }
        Color(if self.light { light } else { dark })
    }
}

fn with_alpha(color: Color, alpha: u8) -> Color {
    Color((color.0 & 0xffffff00) | u32::from(alpha))
}

fn mix(from: Color, to: Color, amount: f32) -> Color {
    let a = from.0.to_be_bytes();
    let b = to.0.to_be_bytes();
    let bytes = std::array::from_fn(|index| {
        (f32::from(a[index]) * (1.0 - amount) + f32::from(b[index]) * amount).round() as u8
    });
    Color(u32::from_be_bytes(bytes))
}

fn contrasting_text(color: Color) -> Color {
    let [r, g, b, _] = color.0.to_be_bytes();
    let linear = |channel: u8| {
        let s = f64::from(channel) / 255.0;
        if s <= 0.04045 {
            s / 12.92
        } else {
            ((s + 0.055) / 1.055).powf(2.4)
        }
    };
    let luminance = 0.2126 * linear(r) + 0.7152 * linear(g) + 0.0722 * linear(b);
    // The contrast of black and white ink is equal at this luminance.
    if luminance > 0.179 {
        Color(0x000000ff)
    } else {
        Color(0xffffffff)
    }
}
