use super::*;

#[test]
fn editable_spin_text_supports_touch_selection_without_claiming_its_arrows() {
    for kind in ["SpinBox", "DoubleSpinBox"] {
        let mut spin = node(kind, "spin");
        length(&mut spin, "width", 180.0);
        length(&mut spin, "height", 40.0);
        boolean(&mut spin, "editable", true);
        let size = Size::new(180.0, 40.0);
        let input = Interaction::default();
        let frame = layout(&spin, size, &mut Text);
        assert!(
            input
                .touch_text_target(&spin, &frame, Point { x: 20.0, y: 20.0 })
                .is_some()
        );
        for y in [10.0, 30.0] {
            assert!(
                input
                    .touch_text_target(&spin, &frame, Point { x: 170.0, y })
                    .is_none()
            );
        }
        boolean(&mut spin, "read-only", true);
        let frame = layout(&spin, size, &mut Text);
        assert!(
            input
                .touch_text_target(&spin, &frame, Point { x: 20.0, y: 20.0 })
                .is_none()
        );
        boolean(&mut spin, "read-only", false);
        boolean(&mut spin, "editable", false);
        let frame = layout(&spin, size, &mut Text);
        assert!(
            input
                .touch_text_target(&spin, &frame, Point { x: 20.0, y: 20.0 })
                .is_none()
        );
    }
}
