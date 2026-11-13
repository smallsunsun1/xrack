use crate::{
    ControlAction, Length, Node, TextHorizontalAlignment, TextOverflow, TextVerticalAlignment,
    TextWrap, combo_label, decimal_spin_value_after,
    picker::{PickerSession, append_picker_dialog},
    spin_value_after,
};

const FIELD: &str = "__font-picker-field";

/// A font selection expressed in logical pixels and CSS-style font weight.
#[derive(Clone, Debug, PartialEq)]
pub struct Font {
    pub family: String,
    pub size: Length,
    pub weight: i64,
    pub italic: bool,
}

impl Font {
    #[doc(hidden)]
    pub fn finite_size(&self) -> Option<f32> {
        self.size
            .0
            .is_finite()
            .then(|| self.size.0.clamp(-1_000_000.0, 1_000_000.0) as f32)
    }
}

impl Default for Font {
    fn default() -> Self {
        Self {
            family: "sans-serif".into(),
            size: Length(16.0),
            weight: 400,
            italic: false,
        }
    }
}

#[doc(hidden)]
#[derive(Default)]
pub struct FontPopupState(PickerSession<Font>);

impl FontPopupState {
    pub fn close(&self) {
        self.0.close();
    }

    pub fn populate(&self, dialog: &mut Node, initial: impl FnOnce() -> Font) {
        self.try_populate(dialog, || Ok::<_, std::convert::Infallible>(initial()))
            .unwrap_or_else(|error| match error {});
    }

    pub fn try_populate<E>(
        &self,
        dialog: &mut Node,
        initial: impl FnOnce() -> Result<Font, E>,
    ) -> Result<(), E> {
        let Some(font) = self.0.try_begin(dialog, || {
            let mut font = initial()?;
            if font.family.trim().is_empty() {
                font.family = Font::default().family;
            }
            font.size.0 = if font.size.0.is_finite() {
                font.size.0.clamp(1.0, 512.0)
            } else {
                Font::default().size.0
            };
            font.weight = font.weight.clamp(1, 1000);
            Ok(font)
        })?
        else {
            return Ok(());
        };
        dialog
            .properties
            .entry("title")
            .or_insert_with(|| "Select font".to_owned().into());
        let mut body = Node::new("VerticalLayout", None);
        body.properties.insert("spacing", Length(8.0).into());

        let mut family = Node::new("ComboBox", None);
        family
            .properties
            .insert("current-value", font.family.clone().into());
        set_font_family_choices(&mut family, &[]);

        let mut size = Node::new("DoubleSpinBox", None);
        size.properties.insert("value", font.size.0.into());
        size.properties.insert("minimum", 1.0_f64.into());
        size.properties.insert("maximum", 512.0_f64.into());
        size.properties.insert("decimals", 1_i64.into());
        size.properties.insert("editable", true.into());

        let mut weight = Node::new("SpinBox", None);
        weight.properties.insert("value", font.weight.into());
        weight.properties.insert("minimum", 1_i64.into());
        weight.properties.insert("maximum", 1000_i64.into());
        weight.properties.insert("step-size", 100_i64.into());
        weight.properties.insert("editable", true.into());

        let mut style = Node::new("ComboBox", None);
        style.properties.insert(
            "model",
            vec!["Regular".to_owned(), "Italic".to_owned()].into(),
        );
        style
            .properties
            .insert("current-index", i64::from(font.italic).into());

        for (field, label, mut control) in [
            ("family", "Family", family),
            ("size", "Size (px)", size),
            ("weight", "Weight", weight),
            ("style", "Style", style),
        ] {
            self.0.stamp(&mut control);
            control.properties.insert(FIELD, field.to_owned().into());
            control
                .properties
                .insert("accessible-name", label.to_owned().into());
            control
                .properties
                .insert("horizontal-stretch", 1.0_f64.into());
            let mut caption = Node::new("Text", None);
            caption.properties.insert("text", label.to_owned().into());
            caption.properties.insert("width", Length(80.0).into());
            caption
                .properties
                .insert("vertical-alignment", TextVerticalAlignment::Center.into());
            let mut row = Node::new("HorizontalLayout", None);
            row.properties.insert("spacing", Length(8.0).into());
            row.children.extend([caption, control]);
            body.children.push(row);
        }

        let mut preview = Node::new("Text", None);
        let sample = dialog.text("preview-text").unwrap_or("Aa Bb Cc 0123");
        preview.properties.insert("text", sample.to_owned().into());
        preview.properties.insert("font", font.into());
        preview.properties.insert("height", Length(82.0).into());
        preview
            .properties
            .insert("overflow", TextOverflow::Elide.into());
        preview.properties.insert("wrap", TextWrap::NoWrap.into());
        preview.properties.insert("max-lines", 1_i64.into());
        preview.properties.insert(
            "horizontal-alignment",
            TextHorizontalAlignment::Center.into(),
        );
        preview
            .properties
            .insert("vertical-alignment", TextVerticalAlignment::Center.into());
        body.children.push(preview);
        append_picker_dialog(dialog, body);
        Ok(())
    }

    pub fn edit(&self, node: &Node, action: ControlAction) -> bool {
        let Some(mut font) = self.0.current(node) else {
            return false;
        };
        match (node.text(FIELD), action) {
            (Some("family"), ControlAction::Select(index))
                if node.text("current-value") == Some(font.family.as_str()) =>
            {
                let Some(family) =
                    combo_label(node, index).filter(|family| **family != font.family)
                else {
                    return false;
                };
                font.family = family.into_owned();
            }
            (Some("size"), ControlAction::Spin(action))
                if node.number("value") == Some(font.size.0) =>
            {
                let Some(size) = decimal_spin_value_after(node, action) else {
                    return false;
                };
                font.size = Length(size);
            }
            (Some("weight"), ControlAction::Spin(action))
                if node.int("value") == Some(font.weight) =>
            {
                let Some(weight) = spin_value_after(node, action) else {
                    return false;
                };
                font.weight = weight;
            }
            (Some("style"), ControlAction::Select(index @ (0 | 1)))
                if node.int("current-index") == Some(i64::from(font.italic))
                    && font.italic != (index == 1) =>
            {
                font.italic = index == 1;
            }
            _ => return false,
        }
        self.0.set(font);
        true
    }

    pub fn finish(&self, dialog: &Node) -> Option<Font> {
        self.finish_with_role(dialog, crate::DialogRole::Accept)
    }

    pub fn finish_with_role(&self, dialog: &Node, role: crate::DialogRole) -> Option<Font> {
        self.0.finish(dialog, role)
    }
}

pub fn is_font_family_choice(node: &Node) -> bool {
    node.kind == "ComboBox" && node.text(FIELD) == Some("family")
}

pub fn set_font_family_choices(node: &mut Node, available: &[String]) {
    let selected = node.text("current-value").unwrap_or_default();
    let mut families = available.to_vec();
    if let Some(family) = families
        .iter_mut()
        .find(|family| family.eq_ignore_ascii_case(selected))
    {
        if family != selected {
            *family = selected.to_owned();
        }
    } else {
        families.insert(0, selected.to_owned());
    }
    node.properties.insert("model", families.into());
}
