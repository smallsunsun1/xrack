use crate::{Rect, Size, place_in_direction};
use xrack_runtime::LayoutDirection;

/// Space reserved around a control's replaceable content and its minimum
/// outer size. The same insets are used when measuring and placing content.
pub(super) struct ControlContentLayout {
    leading: f32,
    trailing: f32,
    vertical: f32,
    minimum: Size,
}

impl ControlContentLayout {
    pub(super) fn uniform(padding: f32, minimum: Size) -> Self {
        Self {
            leading: padding,
            trailing: padding,
            vertical: padding,
            minimum,
        }
    }

    pub(super) fn horizontal(leading: f32, trailing: f32, minimum: Size) -> Self {
        Self {
            leading,
            trailing,
            vertical: 0.0,
            minimum,
        }
    }

    fn insets(&self) -> Size {
        Size::new(self.leading + self.trailing, 2.0 * self.vertical)
    }

    pub(super) fn available(&self, outer: Size) -> Size {
        let insets = self.insets();
        Size::new(
            (outer.width - insets.width).max(0.0),
            (outer.height - insets.height).max(0.0),
        )
    }

    pub(super) fn preferred(&self, content: Size) -> Size {
        let insets = self.insets();
        Size::new(
            (content.width + insets.width).max(self.minimum.width),
            (content.height + insets.height).max(self.minimum.height),
        )
    }

    pub(super) fn bounds(&self, outer: Rect, direction: LayoutDirection) -> Rect {
        let insets = self.insets();
        let leading = if insets.width > outer.width {
            self.leading * (outer.width / insets.width)
        } else {
            self.leading
        };
        let vertical = self.vertical.min(outer.height * 0.5);
        let rect = Rect::new(
            outer.x + leading,
            outer.y + vertical,
            (outer.width - insets.width).max(0.0),
            (outer.height - 2.0 * vertical).max(0.0),
        );
        place_in_direction(rect, outer, direction)
    }
}
