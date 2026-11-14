use crate::{
    Brush, Color, DialogRole, GradientStop, InputRoute, Length, Node, Orientation, RangeAction,
    SliderRange, SpinAction, TextVerticalAlignment,
    picker::{PickerSession, append_picker_dialog},
    spin_value_after,
};

const CHANNEL: &str = "__color-channel";
const HUE: &str = "__color-hue";
const HEX: &str = "__color-hex";
const NAMES: [&str; 4] = ["Red", "Green", "Blue", "Opacity"];

#[derive(Clone, Copy, PartialEq)]
struct Hsv {
    hue: f64,
    saturation: f64,
    value: f64,
}

impl Hsv {
    fn from_color(color: Color, previous: Option<Self>) -> Self {
        let [red, green, blue, _] = color.0.to_be_bytes().map(|value| f64::from(value) / 255.0);
        let value = red.max(green).max(blue);
        let chroma = value - red.min(green).min(blue);
        let hue = if chroma == 0.0 {
            previous.map_or(0.0, |hsv| hsv.hue)
        } else {
            let sector = if value == red {
                (green - blue) / chroma
            } else if value == green {
                (blue - red) / chroma + 2.0
            } else {
                (red - green) / chroma + 4.0
            };
            sector.rem_euclid(6.0) * 60.0
        };
        let saturation = if value > 0.0 {
            chroma / value
        } else {
            previous.map_or(0.0, |hsv| hsv.saturation)
        };
        Self {
            hue,
            saturation,
            value,
        }
    }

    fn to_color(self, alpha: u8) -> Color {
        let sector = self.hue.rem_euclid(360.0) / 60.0;
        let chroma = self.saturation * self.value;
        let second = chroma * (1.0 - (sector.rem_euclid(2.0) - 1.0).abs());
        let [red, green, blue] = match sector as u8 {
            0 => [chroma, second, 0.0],
            1 => [second, chroma, 0.0],
            2 => [0.0, chroma, second],
            3 => [0.0, second, chroma],
            4 => [second, 0.0, chroma],
            _ => [chroma, 0.0, second],
        }
        .map(|channel| ((channel + self.value - chroma) * 255.0).round() as u8);
        Color(u32::from_be_bytes([red, green, blue, alpha]))
    }
}

#[derive(Clone)]
struct HexDraft {
    text: String,
    show_alpha: bool,
    valid: bool,
}

#[derive(Clone)]
struct ColorDraft {
    color: Color,
    hsv: Hsv,
    hex: Option<HexDraft>,
}

impl ColorDraft {
    fn new(color: Color) -> Self {
        Self {
            color,
            hsv: Hsv::from_color(color, None),
            hex: None,
        }
    }

    fn set_color(&mut self, color: Color) {
        if color.0 >> 8 != self.color.0 >> 8 {
            self.hsv = Hsv::from_color(color, Some(self.hsv));
        }
        self.color = color;
        self.hex = None;
    }

    fn active_hex(&self, show_alpha: bool) -> Option<&HexDraft> {
        self.hex.as_ref().filter(|hex| hex.show_alpha == show_alpha)
    }

    fn hex_valid(&self, show_alpha: bool) -> bool {
        self.active_hex(show_alpha).is_none_or(|hex| hex.valid)
    }

    fn hex_text(&self, show_alpha: bool) -> String {
        self.active_hex(show_alpha)
            .map_or_else(|| hex_text(self.color, show_alpha), |hex| hex.text.clone())
    }
}

#[doc(hidden)]
#[derive(Default)]
pub struct ColorPopupState(PickerSession<ColorDraft>);

impl ColorPopupState {
    #[doc(hidden)]
    pub fn accepts_input(&self, node: &Node, input: InputRoute<'_>) -> bool {
        self.0.accepts_input(node, input)
    }

    pub fn close(&self) {
        self.0.close();
    }

    // Used by older xrackc output, which has no hex edit route.
    pub fn populate(&self, dialog: &mut Node, initial: impl FnOnce() -> Color) {
        self.try_populate_inner(
            dialog,
            || Ok::<_, std::convert::Infallible>(initial()),
            false,
        )
        .unwrap_or_else(|error| match error {});
    }

    pub fn populate_editor(&self, dialog: &mut Node, initial: impl FnOnce() -> Color) {
        self.try_populate_editor(dialog, || Ok::<_, std::convert::Infallible>(initial()))
            .unwrap_or_else(|error| match error {});
    }

    pub fn try_populate_editor<E>(
        &self,
        dialog: &mut Node,
        initial: impl FnOnce() -> Result<Color, E>,
    ) -> Result<(), E> {
        self.try_populate_inner(dialog, initial, true)
    }

    fn try_populate_inner<E>(
        &self,
        dialog: &mut Node,
        initial: impl FnOnce() -> Result<Color, E>,
        editable_hex: bool,
    ) -> Result<(), E> {
        let Some(draft) = self
            .0
            .try_begin(dialog, || initial().map(ColorDraft::new))?
        else {
            return Ok(());
        };
        dialog
            .properties
            .entry("title")
            .or_insert_with(|| "Select color".to_owned().into());
        let rgba = draft.color.0.to_be_bytes();
        let show_alpha = dialog.boolean("show-alpha", false);
        let mut picker = Node::new("VerticalLayout", None);
        picker.properties.insert("spacing", Length(6.0).into());
        let label = if editable_hex {
            draft.hex_text(show_alpha)
        } else {
            hex_text(draft.color, show_alpha || draft.color.0 as u8 != 255)
        };
        let mut preview = preview(draft.color, label, editable_hex);
        if editable_hex {
            let hex = &mut preview.children[1];
            self.0.stamp(hex);
            hex.properties.insert(HEX, show_alpha.into());
            hex.properties.insert("maximum-length", 9_i64.into());
            if !draft.hex_valid(show_alpha) {
                hex.properties.insert("border-width", Length(2.0).into());
                hex.properties
                    .insert("border-color", Color(0xe45e60ff).into());
                hex.properties.insert(
                    "accessible-description",
                    if show_alpha {
                        "Enter six or eight hexadecimal digits"
                    } else {
                        "Enter six hexadecimal digits"
                    }
                    .to_owned()
                    .into(),
                );
            }
        }
        picker.children.push(preview);
        for (channel, label) in NAMES.iter().enumerate().take(channel_count(show_alpha)) {
            let mut row = Node::new("HorizontalLayout", None);
            row.properties.insert("spacing", Length(8.0).into());

            let mut caption = Node::new("Text", None);
            caption
                .properties
                .insert("text", (*label).to_owned().into());
            caption.properties.insert("width", Length(62.0).into());
            caption
                .properties
                .insert("vertical-alignment", TextVerticalAlignment::Center.into());

            let value = rgba[channel];
            let mut slider = Node::new("Slider", None);
            slider.properties.insert("value", f64::from(value).into());
            slider.properties.insert("maximum", 255.0_f64.into());
            slider
                .properties
                .insert("horizontal-stretch", 1.0_f64.into());
            slider
                .properties
                .insert("__slider-track", channel_gradient(rgba, channel).into());
            slider
                .properties
                .insert("accessible-name", format!("{label} slider").into());

            let mut spin = Node::new("SpinBox", None);
            spin.properties.insert("value", i64::from(value).into());
            spin.properties.insert("maximum", 255_i64.into());
            spin.properties.insert("width", Length(82.0).into());
            spin.properties
                .insert("accessible-name", (*label).to_owned().into());

            for control in [&mut slider, &mut spin] {
                self.0.stamp(control);
                control.properties.insert(CHANNEL, (channel as i64).into());
            }
            row.children.extend([caption, slider, spin]);
            picker.children.push(row);
        }
        if dialog.boolean("show-color-plane", false) {
            if !show_alpha {
                picker.children.push(Node::hidden());
            }
            picker.children.push(self.color_plane(draft.hsv));
        }
        append_picker_dialog(dialog, picker);
        if !draft.hex_valid(show_alpha) {
            dialog.children.last_mut().expect("picker footer").children[1]
                .properties
                .insert("enabled", false.into());
        }
        Ok(())
    }

    fn color_plane(&self, hsv: Hsv) -> Node {
        let mut row = Node::new("HorizontalLayout", None);
        row.properties.insert("height", Length(132.0).into());
        row.properties.insert("spacing", Length(6.0).into());

        let mut plane = Node::new("ColorPlane", None);
        self.0.stamp(&mut plane);
        plane.properties.insert("height", Length(132.0).into());
        plane
            .properties
            .insert("horizontal-stretch", 1.0_f64.into());
        plane.properties.insert("hue", hsv.hue.into());
        plane.properties.insert("saturation", hsv.saturation.into());
        plane.properties.insert("brightness", hsv.value.into());
        let hue_color = Hsv {
            hue: hsv.hue,
            saturation: 1.0,
            value: 1.0,
        }
        .to_color(255);
        plane.properties.insert("hue-color", hue_color.into());
        plane.properties.insert(
            "accessible-name",
            "Color saturation and brightness".to_owned().into(),
        );
        plane.properties.insert("accessible-description", format!(
            "Saturation {:.0}%, brightness {:.0}%. Use Left and Right for saturation and Up and Down for brightness.",
            hsv.saturation * 100.0, hsv.value * 100.0,
        ).into());

        let mut hue = Node::new("Slider", None);
        self.0.stamp(&mut hue);
        hue.properties.insert("height", Length(132.0).into());
        hue.properties.insert(HUE, true.into());
        hue.properties.insert("width", Length(30.0).into());
        hue.properties
            .insert("orientation", Orientation::Vertical.into());
        hue.properties.insert("value", hsv.hue.into());
        hue.properties.insert("maximum", 360.0_f64.into());
        hue.properties
            .insert("accessible-name", "Hue".to_owned().into());
        let stops: Vec<_> = (0..=6)
            .map(|sector| GradientStop {
                color: Hsv {
                    hue: f64::from(sector) * 60.0,
                    saturation: 1.0,
                    value: 1.0,
                }
                .to_color(255),
                offset: f64::from(sector) / 6.0,
            })
            .collect();
        hue.properties.insert(
            "__slider-track",
            Brush::Linear {
                angle_degrees: 0.0,
                stops: stops.into(),
            }
            .into(),
        );
        row.children.extend([plane, hue]);
        row
    }

    pub fn adjust_plane(&self, node: &Node, action: RangeAction) -> bool {
        let Some(mut draft) = self.0.current(node) else {
            return false;
        };
        let hsv = draft.hsv;
        let next = match (node.kind, action) {
            ("ColorPlane", RangeAction::SetPoint { x, y })
                if x.is_finite()
                    && y.is_finite()
                    && node.number("hue") == Some(hsv.hue)
                    && node.number("saturation") == Some(hsv.saturation)
                    && node.number("brightness") == Some(hsv.value) =>
            {
                Hsv {
                    saturation: x.clamp(0.0, 1.0),
                    value: y.clamp(0.0, 1.0),
                    ..hsv
                }
            }
            ("Slider", action)
                if node.boolean(HUE, false) && node.number("value") == Some(hsv.hue) =>
            {
                let Some(hue) = SliderRange::from_node(node).apply(action) else {
                    return false;
                };
                Hsv { hue, ..hsv }
            }
            _ => return false,
        };
        if next == hsv {
            return false;
        }
        draft.color = next.to_color(draft.color.0 as u8);
        draft.hsv = next;
        draft.hex = None;
        self.0.set(draft);
        true
    }

    pub fn edit_hex(&self, show_alpha: bool, node: &Node, text: String) -> bool {
        let Some(mut draft) = self.0.current(node) else {
            return false;
        };
        if node.kind != "TextInput"
            || node.properties.get(HEX) != Some(&show_alpha.into())
            || node.text("text") != Some(draft.hex_text(show_alpha).as_str())
        {
            return false;
        }
        let parsed = parse_hex(&text, show_alpha, draft.color.0 as u8);
        if let Some(color) = parsed {
            draft.set_color(color);
        }
        draft.hex = Some(HexDraft {
            text,
            show_alpha,
            valid: parsed.is_some(),
        });
        self.0.set(draft);
        true
    }

    pub fn try_finish_input<E>(
        &self,
        dialog: &Node,
        role: DialogRole,
        input: InputRoute<'_>,
        show_alpha: impl FnOnce() -> Result<bool, E>,
    ) -> Result<Option<Color>, E> {
        let draft = self.0.try_finish_input(dialog, role, input, |draft| {
            let show_alpha = show_alpha()?;
            Ok(Self::can_accept(dialog, show_alpha, &draft).then_some(draft))
        })?;
        Ok(draft.map(|draft| draft.color))
    }

    pub fn finish_input(
        &self,
        dialog: &Node,
        role: DialogRole,
        input: InputRoute<'_>,
        show_alpha: impl FnOnce() -> bool,
    ) -> Option<Color> {
        self.try_finish_input(dialog, role, input, || {
            Ok::<_, std::convert::Infallible>(show_alpha())
        })
        .unwrap_or_else(|error| match error {})
    }

    fn can_accept(dialog: &Node, show_alpha: bool, draft: &ColorDraft) -> bool {
        dialog.boolean("show-alpha", false) == show_alpha && draft.hex_valid(show_alpha)
    }

    pub fn finish_editor(
        &self,
        show_alpha: bool,
        dialog: &Node,
        role: DialogRole,
    ) -> Option<Color> {
        if role == DialogRole::Accept
            && !Self::can_accept(dialog, show_alpha, &self.0.current(dialog)?)
        {
            return None;
        }
        self.0.finish(dialog, role).map(|draft| draft.color)
    }

    pub fn slide(&self, show_alpha: bool, node: &Node, action: RangeAction) -> bool {
        node.kind == "Slider"
            && self.edit(show_alpha, node, |node| {
                SliderRange::from_node(node)
                    .apply(action)
                    .map(|value| value.round() as u8)
            })
    }

    pub fn spin(&self, show_alpha: bool, node: &Node, action: SpinAction) -> bool {
        node.kind == "SpinBox"
            && self.edit(show_alpha, node, |node| {
                spin_value_after(node, action).and_then(|value| u8::try_from(value).ok())
            })
    }

    fn edit(
        &self,
        show_alpha: bool,
        node: &Node,
        change: impl FnOnce(&Node) -> Option<u8>,
    ) -> bool {
        let Some(mut draft) = self.0.current(node) else {
            return false;
        };
        let Some(channel) = node
            .int(CHANNEL)
            .and_then(|channel| usize::try_from(channel).ok())
        else {
            return false;
        };
        let mut rgba = draft.color.0.to_be_bytes();
        let displayed = node
            .number("value")
            .or_else(|| node.int("value").map(|value| value as f64));
        if channel >= channel_count(show_alpha) || displayed != Some(f64::from(rgba[channel])) {
            return false;
        }
        let Some(value) = change(node) else {
            return false;
        };
        if rgba[channel] == value {
            return false;
        }
        rgba[channel] = value;
        draft.set_color(Color(u32::from_be_bytes(rgba)));
        self.0.set(draft);
        true
    }

    pub fn finish(&self, dialog: &Node) -> Option<Color> {
        self.0
            .finish(dialog, DialogRole::Accept)
            .map(|draft| draft.color)
    }
}

fn channel_count(show_alpha: bool) -> usize {
    if show_alpha { 4 } else { 3 }
}

fn channel_gradient(mut rgba: [u8; 4], channel: usize) -> Brush {
    rgba[3] = 255;
    let stops = [0, 255].map(|value| {
        rgba[channel] = value;
        GradientStop {
            color: Color(u32::from_be_bytes(rgba)),
            offset: f64::from(value) / 255.0,
        }
    });
    Brush::Linear {
        angle_degrees: 90.0,
        stops: stops.into(),
    }
}

fn hex_text(color: Color, show_alpha: bool) -> String {
    if show_alpha {
        format!("#{:08X}", color.0)
    } else {
        format!("#{:06X}", color.0 >> 8)
    }
}

fn parse_hex(text: &str, show_alpha: bool, alpha: u8) -> Option<Color> {
    let digits = text.strip_prefix('#').unwrap_or(text);
    if !digits.bytes().all(|digit| digit.is_ascii_hexdigit()) {
        return None;
    }
    match digits.len() {
        6 => Some(Color(
            (u32::from_str_radix(digits, 16).ok()? << 8) | u32::from(alpha),
        )),
        8 if show_alpha => Some(Color(u32::from_str_radix(digits, 16).ok()?)),
        _ => None,
    }
}

fn preview(color: Color, label: String, editable: bool) -> Node {
    let mut row = Node::new("HorizontalLayout", None);
    row.properties.insert("spacing", Length(12.0).into());
    row.properties.insert("height", Length(32.0).into());
    let mut swatch = Node::new("ColorPreview", None);
    swatch.properties.insert("color", color.into());
    swatch
        .properties
        .insert("horizontal-stretch", 1.0_f64.into());
    swatch.properties.insert("min-width", Length(80.0).into());
    let mut hex = Node::new(if editable { "TextInput" } else { "Text" }, None);
    hex.properties.insert("text", label.into());
    hex.properties.insert("width", Length(118.0).into());
    if editable {
        hex.properties
            .insert("accessible-name", "Hex color".to_owned().into());
        hex.properties
            .insert("font-family", "monospace".to_owned().into());
    }
    hex.properties
        .insert("vertical-alignment", TextVerticalAlignment::Center.into());
    row.children.extend([swatch, hex]);
    row
}
