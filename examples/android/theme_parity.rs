#![cfg(test)]

mod navigation_fixture;

use mobile_navigation_ui::MobileNavigation;
use navigation_fixture::interpreted_navigation;
use std::time::{Duration, Instant};
use xrack_layout::{ChoiceKey, WindowEffects};
use xrack_runtime::{
    Color, ColorScheme, Component, ControlAction, Font, ImageFit, ImagePlaybackQueue, Length, Node,
    RangeAction, SpinAction, SpinDecimal,
};
use xrack_testing::{FixedText, Point, Size, TextMeasurer, UiDriver};

struct CatalogText(Vec<String>);
impl TextMeasurer for CatalogText {
    fn measure(&mut self, text: &str, size: f32, width: f32) -> Size {
        FixedText.measure(text, size, width)
    }
    fn font_families(&self) -> &[String] {
        &self.0
    }
}

fn exercise_system_theme(component: impl Component) {
    let mut ui = UiDriver::headless(component, Size::new(390.0, 750.0));
    ui.click("settings");
    assert_eq!(ui.scene().color_scheme(), Some(ColorScheme::Light));
    ui.click("follow-theme");
    assert_eq!(ui.scene().color_scheme(), Some(ColorScheme::System));
    assert_eq!(ui.node("follow-theme").text("text"), Some("Use light"));
    ui.click("follow-theme");
    assert_eq!(ui.scene().color_scheme(), Some(ColorScheme::Light));
    assert_eq!(ui.node("follow-theme").text("text"), Some("Follow system"));
}

#[test]
fn system_theme_switch_has_the_same_binding_in_both_backends()
-> Result<(), Box<dyn std::error::Error>> {
    exercise_system_theme(MobileNavigation::new());
    exercise_system_theme(interpreted_navigation()?);
    Ok(())
}

fn locate(node: &Node, property: &str, value: &str) -> Option<Vec<usize>> {
    if node.text(property) == Some(value) || property == "kind" && node.kind == value {
        return Some(Vec::new());
    }
    node.children.iter().enumerate().find_map(|(index, child)| {
        let mut path = locate(child, property, value)?;
        path.insert(0, index);
        Some(path)
    })
}

fn path<C: Component>(ui: &UiDriver<C, CatalogText>, property: &str, value: &str) -> Vec<usize> {
    locate(ui.scene(), property, value).unwrap_or_else(|| panic!("Missing {property} {value:?}"))
}

fn click<C: Component>(ui: &mut UiDriver<C, CatalogText>, text: &str) {
    let path = path(ui, "text", text);
    ui.click_at_path(&path);
}

fn control<C: Component>(ui: &mut UiDriver<C, CatalogText>, name: &str, action: ControlAction) {
    let path = path(ui, "accessible-name", name);
    let scene = ui.scene().clone();
    ui.update(|app| assert!(app.control_at(&scene, &path, action)));
}

fn exercise<C: Component>(component: C) {
    let text = CatalogText(vec!["Catalog Sans".into(), "Catalog Serif".into()]);
    let mut ui = UiDriver::new(component, text, Size::new(390.0, 750.0));
    ui.click("settings");
    let original = Color(0x4080c0ff);
    ui.click("pick-color");
    let old = ui.scene().clone();
    let hex = path(&ui, "accessible-name", "Hex color");
    let popup = ui.path("paint");
    let plane = path(&ui, "kind", "ColorPlane");
    ui.update(|app| {
        assert!(app.adjust_range_at(&old, &plane, RangeAction::SetPoint { x: 0.8, y: 0.6 }))
    });
    assert_eq!(ui.node("paint-preview").color("background"), Some(original));
    ui.escape();
    assert_eq!(ui.node("paint-preview").color("background"), Some(original));
    ui.click("pick-color");
    ui.update(|app| {
        assert!(!app.edit_at(&old, &hex, "#ffffff".into()));
        assert!(!app.control_at(
            &old,
            &popup,
            ControlAction::Dialog(xrack_runtime::DialogRole::Accept)
        ));
    });
    ui.fill_focused("#12");
    let save = path(&ui, "text", "Save color");
    assert!(!ui.scene().at_path(&save).unwrap().boolean("enabled", true));
    ui.fill_focused("#204060ff");
    click(&mut ui, "Save color");
    assert_eq!(
        ui.node("paint-preview").color("background"),
        Some(Color(0x204060ff))
    );
    assert_eq!(ui.node("picker-result").text("text"), Some("Color saved"));

    let original_font = ui.node("type-preview").font("font").unwrap().clone();
    ui.click("pick-font");
    let family = path(&ui, "accessible-name", "Family");
    ui.click_at_path(&family);
    ui.choice_key(ChoiceKey::End);
    ui.choice_key(ChoiceKey::Accept);
    assert_eq!(ui.node("type-preview").font("font"), Some(&original_font));
    ui.escape();
    assert_eq!(ui.node("type-preview").font("font"), Some(&original_font));
    ui.click("pick-font");
    let family = path(&ui, "accessible-name", "Family");
    ui.click_at_path(&family);
    ui.choice_key(ChoiceKey::End);
    ui.choice_key(ChoiceKey::Accept);
    control(
        &mut ui,
        "Size (px)",
        ControlAction::Spin(SpinAction::CommitDecimal(SpinDecimal::new(24.0).unwrap())),
    );
    control(
        &mut ui,
        "Weight",
        ControlAction::Spin(SpinAction::Commit(650)),
    );
    let style = path(&ui, "accessible-name", "Style");
    ui.click_at_path(&style);
    ui.choice_key(ChoiceKey::End);
    ui.choice_key(ChoiceKey::Accept);
    click(&mut ui, "Save font");
    assert_eq!(
        ui.node("type-preview").font("font"),
        Some(&Font {
            family: "Catalog Serif".into(),
            size: Length(24.0),
            weight: 650,
            italic: true
        })
    );
    assert_eq!(
        ui.node("picker-result").text("text"),
        Some("Font saved Catalog Serif")
    );
}

#[test]
fn color_and_font_drafts_commit_the_same_values_in_both_backends()
-> Result<(), Box<dyn std::error::Error>> {
    exercise(MobileNavigation::new());
    exercise(interpreted_navigation()?);
    Ok(())
}

#[test]
fn picker_fields_and_actions_fit_portrait_and_landscape() {
    for size in [Size::new(390.0, 750.0), Size::new(760.0, 340.0)] {
        let text = CatalogText(vec!["Catalog Sans".into()]);
        let mut ui = UiDriver::new(MobileNavigation::new(), text, size);
        ui.click("settings");
        for (opener, picker, save, fields) in [
            (
                "pick-color",
                "paint",
                "Save color",
                vec![
                    "Hex color",
                    "Red",
                    "Green",
                    "Blue",
                    "Opacity",
                    "Color saturation and brightness",
                    "Hue",
                ],
            ),
            (
                "pick-font",
                "typography",
                "Save font",
                vec!["Family", "Size (px)", "Weight", "Style"],
            ),
        ] {
            ui.click(opener);
            let dialog = ui.bounds(picker);
            let mut viewport_path = ui.path(picker);
            viewport_path.push(0);
            let viewport = ui.layout_at(&viewport_path).unwrap().bounds;
            for field in fields {
                let p = path(&ui, "accessible-name", field);
                let mut rect = ui.layout_at(&p).unwrap().bounds;
                let delta = if rect.y < viewport.y {
                    rect.y - viewport.y
                } else {
                    (rect.y + rect.height - viewport.y - viewport.height).max(0.0)
                };
                if delta.abs() > 0.01 {
                    assert!(ui.scroll_at(
                        Point {
                            x: viewport.x + 4.0,
                            y: viewport.y + viewport.height * 0.5
                        },
                        Point { x: 0.0, y: delta }
                    ));
                    rect = ui.layout_at(&p).unwrap().bounds;
                }
                let exposed = rect.intersect(ui.layout_at(&p).unwrap().clip);
                assert!(
                    exposed.width >= 44.0 && exposed.height >= 22.0,
                    "{size:?} cannot reveal {field}: {rect:?} clip {:?}",
                    ui.layout_at(&p).unwrap().clip
                );
                assert!(
                    rect.width >= 44.0 && rect.height >= 22.0,
                    "{size:?} {field}: {rect:?}"
                );
                assert!(
                    rect.x >= dialog.x
                        && rect.y >= dialog.y
                        && rect.x + rect.width <= dialog.x + dialog.width + 0.01
                        && rect.y + rect.height <= dialog.y + dialog.height + 0.01,
                    "{field} outside {dialog:?}: {rect:?}"
                );
            }
            let p = path(&ui, "text", save);
            let button = ui.layout_at(&p).unwrap().bounds;
            assert!(
                button.width >= 44.0 && button.height >= 44.0,
                "{size:?} {save}: {button:?}"
            );
            assert!(
                button.y >= dialog.y && button.y + button.height <= dialog.y + dialog.height + 0.01
            );
            ui.escape();
        }
    }
}

fn exercise_images<C: Component>(component: C) {
    let mut ui = UiDriver::headless(component, Size::new(390.0, 750.0));
    ui.click("open-menu");
    ui.click("open-image-workshop");
    let atlas = ui.node("atlas-tile");
    assert_eq!(atlas.image_asset("source").unwrap().size(), [8, 8]);
    assert_eq!(atlas.image_fit("image-fit"), Some(ImageFit::Contain));
    assert_eq!(atlas.int("source-clip-y"), Some(0));
    let viewport = ui.bounds("atlas-tile");
    ui.click("fit-atlas");
    assert_eq!(
        ui.node("atlas-tile").image_fit("image-fit"),
        Some(ImageFit::Cover)
    );
    assert_eq!(ui.bounds("atlas-tile"), viewport);
    ui.click("crop-atlas");
    assert_eq!(ui.node("atlas-tile").int("source-clip-y"), Some(4));
    assert_eq!(ui.node("atlas-tile").int("source-clip-height"), Some(4));
    assert_eq!(ui.node("atlas-size").text("text"), Some("Atlas 8 by 8"));

    let start = Instant::now();
    let at = |ms| start + Duration::from_millis(ms);
    let mut playback = ImagePlaybackQueue::default();
    let sync = |ui: &mut UiDriver<C>, playback: &mut ImagePlaybackQueue, ms| {
        playback.sync(
            at(ms),
            WindowEffects::collect(ui.scene(), ui.layout()).playback,
        );
        ui.update(|_| {});
    };
    assert_eq!(
        ui.node("pulse").image_asset("source").unwrap().size(),
        [32, 20]
    );
    ui.click("pause-pulse");
    ui.click("play-pulse");
    sync(&mut ui, &mut playback, 0);
    assert!(playback.deadline().is_none());
    assert!(!playback.advance(at(2000)));
    assert_eq!(ui.node("pulse-count").text("text"), Some("Frames 3"));
    assert_eq!(ui.node("pulse-frame").text("text"), Some("Frame 0"));

    ui.click("pause-pulse");
    sync(&mut ui, &mut playback, 2000);
    assert_eq!(playback.deadline(), Some(at(2320)));
    assert!(playback.advance(at(2320)));
    ui.update(|_| {});
    assert_eq!(ui.node("pulse-frame").text("text"), Some("Frame 1"));
    assert!(playback.advance(at(3040)));
    assert!(playback.next_finished().unwrap().finish());
    assert!(playback.next_finished().is_none());
    ui.update(|_| {});
    assert_eq!(ui.node("pulse-frame").text("text"), Some("Frame 2"));
    assert_eq!(ui.node("pulse-finished").text("text"), Some("Finished 1"));
    ui.click("play-pulse");
    sync(&mut ui, &mut playback, 3040);
    assert_eq!(ui.node("pulse-frame").text("text"), Some("Frame 0"));
    assert_eq!(ui.node("pulse-finished").text("text"), Some("Finished 1"));
}

#[test]
fn image_workshop_assets_and_playback_match_in_both_backends()
-> Result<(), Box<dyn std::error::Error>> {
    exercise_images(MobileNavigation::new());
    exercise_images(interpreted_navigation()?);
    Ok(())
}
