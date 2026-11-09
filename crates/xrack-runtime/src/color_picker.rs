use crate::{
    Brush, Color, GradientStop, Length, Node, RangeAction, SliderRange, SpinAction,
    TextVerticalAlignment,
    picker::{PickerSession, append_picker_dialog},
    spin_value_after,
};

const CHANNEL: &str = "__color-channel";
const NAMES: [&str; 4] = ["Red", "Green", "Blue", "Opacity"];

#[doc(hidden)]
#[derive(Default)]
pub struct ColorPopupState(PickerSession<Color>);

impl ColorPopupState {
    pub fn close(&self) {
        self.0.close();
    }

    pub fn populate(&self, dialog: &mut Node, initial: impl FnOnce() -> Color) {
        let Some(color) = self.0.begin(dialog, initial) else {
            return;
        };
        dialog
            .properties
            .entry("title")
            .or_insert_with(|| "Select color".to_owned().into());
        let rgba = color.0.to_be_bytes();
        let show_alpha = dialog.boolean("show-alpha", false);
        let mut picker = Node::new("VerticalLayout", None);
        picker.properties.insert("spacing", Length(6.0).into());
        picker.children.push(preview(color, show_alpha));
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
        append_picker_dialog(dialog, picker);
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
        let Some(color) = self.0.current(node) else {
            return false;
        };
        let Some(channel) = node
            .int(CHANNEL)
            .and_then(|channel| usize::try_from(channel).ok())
        else {
            return false;
        };
        let mut rgba = color.0.to_be_bytes();
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
        self.0.set(Color(u32::from_be_bytes(rgba)));
        true
    }

    pub fn finish(&self, dialog: &Node) -> Option<Color> {
        self.0.finish(dialog)
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

fn preview(color: Color, show_alpha: bool) -> Node {
    let mut row = Node::new("HorizontalLayout", None);
    row.properties.insert("spacing", Length(12.0).into());
    row.properties.insert("height", Length(32.0).into());
    let mut swatch = Node::new("ColorPreview", None);
    swatch.properties.insert("color", color.into());
    swatch
        .properties
        .insert("horizontal-stretch", 1.0_f64.into());
    swatch.properties.insert("min-width", Length(80.0).into());
    let mut hex = Node::new("Text", None);
    let label = if show_alpha || color.0 as u8 != 255 {
        format!("#{:08X}", color.0)
    } else {
        format!("#{:06X}", color.0 >> 8)
    };
    hex.properties.insert("text", label.into());
    hex.properties.insert("width", Length(108.0).into());
    hex.properties
        .insert("vertical-alignment", TextVerticalAlignment::Center.into());
    row.children.extend([swatch, hex]);
    row
}
