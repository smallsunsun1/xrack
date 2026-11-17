use xrack_testing::{ActivationKey, Size, TargetError, UiDriver};

xrack::include_modules!();

#[test]
fn increments_from_mouse_and_keyboard() {
    let mut ui = UiDriver::headless(Counter::new(), Size::new(240.0, 120.0));
    assert!(matches!(
        ui.find_path("missing"),
        Err(TargetError::Missing(_))
    ));
    assert_eq!(ui.component().get_count(), 0);

    ui.click("increment");
    assert_eq!(ui.component().get_count(), 1);
    assert_eq!(ui.focused(), Some("increment"));

    assert!(ui.activate(ActivationKey::Enter));
    assert_eq!(ui.component().get_doubled(), 4);
}
