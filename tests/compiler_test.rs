#![cfg(test)]

use xrack_compiler::{Compilation, CompilerError, ir, mlir, parse, semantic};

fn compile(source: &str, filename: &str) -> Result<Compilation, String> {
    xrack_compiler::compile(source, filename).map_err(|error| error.to_string())
}

fn analyze(source: &str) -> Result<ir::Module, String> {
    let ast = parse(source).map_err(|e| format!("{e:?}"))?;
    semantic::analyze(&ast).map_err(|e| format!("{e:?}"))
}

#[test]
fn repeat_button_properties_are_typed_and_restricted_to_push_buttons() {
    for kind in ["Button", "ToolButton", "RoundButton"] {
        let source = format!(
            r#"
            component App inherits Window {{
                {kind} {{ auto-repeat: true; auto-repeat-delay: 200; auto-repeat-interval: 75; }}
            }}
        "#
        );
        let compiled = compile(&source, "button-repeat.xr").unwrap();
        assert!(compiled.mlir.contains("name = \"auto-repeat-interval\""));
        for (from, to) in [
            ("auto-repeat: true", "auto-repeat: 1"),
            ("auto-repeat-delay: 200", "auto-repeat-delay: 2px"),
            ("auto-repeat-interval: 75", "auto-repeat-interval: false"),
        ] {
            assert!(compile(&source.replace(from, to), "button-repeat.xr").is_err());
        }
        let invalid = compiled
            .mlir
            .replace(&format!("kind = \"{kind}\""), "kind = \"CheckBox\"");
        assert_ne!(invalid, compiled.mlir);
        assert!(mlir::Module::parse(&invalid).is_err());
    }
}

#[test]
fn push_button_press_and_hold_is_a_typed_zero_argument_event() {
    for kind in ["Button", "RoundButton", "ToolButton"] {
        let source = format!(
            r#"
            component App inherits Window {{
                in-out property <int> holds: 0;
                target := {kind} {{
                    press-and-hold-interval: 550;
                    press-and-hold => {{ root.holds += 1; }}
                }}
            }}
        "#
        );
        let compiled = compile(&source, "button-hold.xr").unwrap();
        assert!(compiled.mlir.contains("name = \"press-and-hold-interval\""));
        assert!(compiled.mlir.contains("name = \"press-and-hold\""));
        assert!(compiled.rust.to_string().contains("press_and_hold_handler"));
        for (from, to) in [
            (
                "press-and-hold-interval: 550",
                "press-and-hold-interval: 550px",
            ),
            ("press-and-hold =>", "press-and-hold(key) =>"),
        ] {
            assert!(compile(&source.replace(from, to), "button-hold.xr").is_err());
        }
        let invalid = compiled
            .mlir
            .replace(&format!("kind = \"{kind}\""), "kind = \"CheckBox\"");
        assert_ne!(invalid, compiled.mlir);
        assert!(mlir::Module::parse(&invalid).is_err());
    }
}

#[test]
fn push_buttons_accept_zero_argument_double_click_handlers() {
    for kind in ["Button", "RoundButton", "ToolButton"] {
        let source = format!(
            r#"
            component App inherits Window {{
                command := Action {{}}
                target := {kind} {{ action: command; double-clicked => {{}} }}
            }}
        "#
        );
        let compiled = compile(&source, "button-double-click.xr").unwrap();
        assert!(compiled.mlir.contains("name = \"double-clicked\""));
        assert!(
            compile(
                &source.replace("double-clicked =>", "double-clicked(value) =>"),
                "button-double-click.xr"
            )
            .is_err()
        );
        let invalid = compiled
            .mlir
            .replace(&format!("kind = \"{kind}\""), "kind = \"CheckBox\"");
        assert_ne!(compiled.mlir, invalid);
        assert!(mlir::Module::parse(&invalid).is_err());
    }
}

#[test]
fn push_button_pressed_is_a_typed_read_only_state_property() {
    let source = r#"
        component App inherits Window {
            out property <bool> down: button.pressed;
            button := Button {}
        }
    "#;
    let compiled = compile(source, "pressed.xr").unwrap();
    assert!(compiled.mlir.contains("\"xr.button_pressed\""));
    assert!(compiled.rust.to_string().contains("button_press_handle"));
    for invalid in [
        source.replace("Button {}", "Button { pressed: true; }"),
        source.replace("Button {}", "Label {}"),
    ] {
        assert!(compile(&invalid, "pressed.xr").is_err());
    }
    let invalid = compiled
        .mlir
        .replace("kind = \"Button\"", "kind = \"Label\"");
    assert!(mlir::Module::parse(&invalid).is_err());
}

#[test]
fn checked_reads_verify_owners_and_reject_binding_cycles() {
    let source = r#"
        export component App inherits Window {
            out property <bool> selected: target.checked;
            target := Button { checked: true; }
        }
    "#;
    let compiled = compile(source, "checked.xr").unwrap();
    assert!(compiled.mlir.contains("\"xr.button_checked\""));
    assert!(
        mlir::Module::parse(
            &compiled
                .mlir
                .replace("kind = \"Button\"", "kind = \"Label\"")
        )
        .is_err()
    );
    let assignment = source.replace("checked: true;", "clicked => { target.checked = false; }");
    assert!(
        compile(&assignment, "checked.xr")
            .err()
            .unwrap()
            .to_string()
            .contains("read-only")
    );
    for declarations in [
        "target := Button { checked: !target.checked; }",
        "shared := Action { checked: target.checked; } target := Button { action: shared; }",
        "group := ActionGroup { current-value: target.checked ? \"x\" : \"\"; item := Action { value: \"x\"; } } target := Button { action: item; }",
        "group := ButtonGroup { current-index: target.checked ? 0 : -1; } target := Button { group: group; }",
        "group := ButtonGroup { current-value: \"x\"; } target := Button { group: group; text: target.checked ? \"x\" : \"\"; }",
    ] {
        let invalid = source.replace("target := Button { checked: true; }", declarations);
        let error = compile(&invalid, "checked.xr").err().unwrap().to_string();
        assert!(error.contains("cyclic"), "{error}");
    }
    let stable_value = source.replace(
        "target := Button { checked: true; }",
        "group := ButtonGroup { current-value: \"x\"; } target := Button { group: group; value: \"x\"; text: target.checked ? \"Selected\" : \"Idle\"; }",
    );
    compile(&stable_value, "checked.xr").unwrap();
}

#[test]
fn range_position_read_types_directions_and_cycle_checks_match_the_control() {
    let source = r#"
        export component App inherits Window {
            out property <float> rate: knob.position + slider.visual-position + pair.lower-position + pair.upper-visual-position;
            knob := Dial {}
            slider := Slider { value: 10; }
            pair := RangeSlider {}
        }
    "#;
    let compiled = compile(source, "range-position.xr").unwrap();
    assert_eq!(
        compiled.mlir.matches("\"xr.range_position_read\"").count(),
        4
    );
    let invalid = compiled
        .mlir
        .replace("value = \"upper-visual-position\"", "value = \"position\"");
    let error = mlir::Module::parse(&invalid).err().unwrap();
    assert!(error.contains("supported range position"), "{error}");
    let cycle = source.replace(
        "value: 10;",
        "value: 10; layout-direction: self.visual-position < 0.5 ? LayoutDirection.left-to-right : LayoutDirection.right-to-left;",
    );
    let error = compile(&cycle, "range-position.xr")
        .err()
        .unwrap()
        .to_string();
    assert!(error.contains("cyclic"), "{error}");
    let state_cycle = source.replace(
        "slider := Slider { value: 10; }",
        "Rectangle { states [ mirror when slider.visual-position < 0.5: { layout-direction: LayoutDirection.right-to-left; } ] slider := Slider { value: 10; } }",
    );
    let error = compile(&state_cycle, "range-position.xr")
        .err()
        .unwrap()
        .to_string();
    assert!(error.contains("cyclic"), "{error}");
    let depends_on_position = source.replace("value: 10;", "value: 10; step: self.position + 1;");
    compile(&depends_on_position, "range-position.xr").unwrap();
    let delayed = depends_on_position.replace("value: 10;", "value: 10; live: false;");
    let error = compile(&delayed, "range-position.xr")
        .err()
        .unwrap()
        .to_string();
    assert!(error.contains("cyclic"), "{error}");
}

#[test]
fn numeric_control_values_are_typed_reads_and_preserve_writable_aliases() {
    let source = r#"
        export component App inherits Window {
            in-out property <float> slider-alias <=> slider.value;
            out property <float> meter: slider.value + dial.value + range.lower-value + range.upper-value + decimal.value;
            out property <int> count: integer.value;
            slider := Slider { value: 3; }
            dial := Dial {}
            range := RangeSlider {}
            decimal := DoubleSpinBox {}
            integer := SpinBox {}
        }
    "#;
    let compiled = compile(source, "numeric-state.xr").unwrap();
    assert!(compiled.mlir.contains("\"xr.control_read\""));
    assert_eq!(
        compiled.mlir.matches("\"xr.numeric_value_read\"").count(),
        6
    );
    let invalid = compiled
        .mlir
        .replace("value = \"upper-value\"", "value = \"value\"");
    let error = mlir::Module::parse(&invalid).err().unwrap();
    assert!(error.contains("numeric control value"), "{error}");
    let cycle = source.replace("value: 3;", "value: self.value + 1;");
    let error = compile(&cycle, "numeric-state.xr")
        .err()
        .unwrap()
        .to_string();
    assert!(error.contains("cyclic"), "{error}");
}

#[test]
fn navigation_active_reads_verify_the_parent_and_reject_eligibility_cycles() {
    let source = r#"
        export component App inherits Window {
            out property <bool> state: option.checked && page.active && slide.active;
            bar := TabBar {
                option := TabButton { text: self.checked ? "Selected" : "Overview"; }
            }
            tabs := TabWidget { page := Tab { title: "Page"; } }
            view := SwipeView { slide := SwipePage {} }
        }
    "#;
    let compiled = compile(source, "navigation-active.xr").unwrap();
    for group in ["bar", "tabs", "view"] {
        assert!(compiled.mlir.lines().any(|line| {
            line.contains("\"xr.navigation_active\"")
                && line.contains(&format!("value = \"{group}\""))
        }));
    }
    let invalid = compiled.mlir.replace("value = \"bar\"", "value = \"view\"");
    assert_ne!(invalid, compiled.mlir);
    let error = mlir::Module::parse(&invalid).err().unwrap();
    assert!(error.contains("owning selection control"), "{error}");
    let cycle = source.replace(
        "text: self.checked ?",
        "enabled: !self.checked; text: self.checked ?",
    );
    let error = compile(&cycle, "navigation-active.xr")
        .err()
        .unwrap()
        .to_string();
    assert!(error.contains("cyclic"), "{error}");
}

#[test]
fn radio_checked_reads_verify_the_selection_owner_and_reject_cycles() {
    let source = r#"
        export component App inherits Window {
            out property <bool> picked: item.checked && distributed.checked;
            options := RadioGroup {
                current-value: "a";
                item := RadioButton { value: "a"; text: self.checked ? "yes" : "no"; }
            }
            external := ButtonGroup { current-value: "a"; }
            distributed := RadioButton { value: "a"; group: external; }
        }
    "#;
    let compiled = compile(source, "radio-checked.xr").unwrap();
    assert!(compiled.mlir.contains("\"xr.radio_checked\""));
    assert!(compiled.mlir.contains("\"xr.button_checked\""));
    let invalid = compiled
        .mlir
        .replace("value = \"options\"", "value = \"external\"");
    assert_ne!(invalid, compiled.mlir);
    let error = mlir::Module::parse(&invalid).err().unwrap();
    assert!(error.contains("owning selection control"), "{error}");
    let cycle = source.replace("value: \"a\"; text:", "text:");
    let error = compile(&cycle, "radio-checked.xr")
        .err()
        .unwrap()
        .to_string();
    assert!(error.contains("cyclic"), "{error}");
}

#[test]
fn toggle_state_reads_validate_types_and_cannot_feed_their_own_storage() {
    let source = r#"
        export component App inherits Window {
            out property <CheckState> state: choice.check-state;
            out property <bool> checked: choice.checked && network.checked;
            choice := CheckBox { check-state: CheckState.partially-checked; }
            network := Switch { checked: true; }
        }
    "#;
    let compiled = compile(source, "toggle-state.xr").unwrap();
    assert_eq!(compiled.mlir.matches("\"xr.toggle_state_read\"").count(), 3);
    let invalid = compiled
        .mlir
        .replace("value = \"check-state\"", "value = \"checked\"");
    assert!(mlir::Module::parse(&invalid).is_err());
    let cycle = source.replace(
        "check-state: CheckState.partially-checked",
        "check-state: self.checked ? CheckState.checked : CheckState.unchecked",
    );
    let error = compile(&cycle, "toggle-state.xr")
        .err()
        .unwrap()
        .to_string();
    assert!(error.contains("cyclic"), "{error}");
    let invalid = source.replace("choice.check-state", "network.check-state");
    assert!(compile(&invalid, "toggle-state.xr").is_err());
}

#[test]
fn repeated_button_checked_is_verified_after_the_row_is_bound() {
    let source = r#"
        export component App inherits Window {
            for i in 3: Button {
                checkable: true;
                checked: i == 1;
                text: self.checked ? "on" : "off";
            }
        }
    "#;
    let compiled = compile(source, "row-checked.xr").unwrap();
    assert!(compiled.mlir.contains("\"xr.button_checked\""));
    let read = compiled
        .mlir
        .lines()
        .find(|line| line.contains("\"xr.button_checked\""))
        .unwrap();
    let repeat = compiled
        .mlir
        .lines()
        .find(|line| line.contains("\"xr.repeat\""))
        .unwrap();
    let invalid = compiled
        .mlir
        .replace(&format!("{read}\n"), "")
        .replace(repeat, &format!("{read}\n{repeat}"));
    let error = mlir::Module::parse(&invalid).err().unwrap();
    assert!(error.contains("row scope"), "{error}");
}

#[test]
fn repeated_button_toggle_uses_its_row_and_verifies_the_effect_target() {
    let source = r#"
        export component App inherits Window {
            for i in 2: Button {
                checkable: false;
                clicked => { self.toggle(); }
            }
            for j in 2: RoundButton {
                checkable: false;
                clicked => { self.toggle(); }
            }
        }
    "#;
    let compiled = compile(source, "row-toggle.xr").unwrap();
    let toggles: Vec<_> = compiled
        .mlir
        .lines()
        .filter(|line| line.contains("\"xr.button_toggle\""))
        .collect();
    let [first, second] = toggles.as_slice() else {
        panic!("expected two row-local toggles");
    };
    assert_eq!(
        compiled.mlir.matches("toggle_storage = \"true\"").count(),
        2
    );
    let wrong_row = compiled.mlir.replacen(*first, second, 1);
    let error = mlir::Module::parse(&wrong_row).err().unwrap();
    assert!(error.contains("row scope"), "{error}");
    let invalid = source.replace("self.toggle()", "self.click()");
    let error = compile(&invalid, "row-toggle.xr")
        .err()
        .unwrap()
        .to_string();
    assert!(error.contains("statically addressed"), "{error}");
}

#[test]
fn programmatic_toggle_marks_only_its_own_storage_and_rejects_invalid_owners() {
    let source = r#"
        export component App inherits Window {
            callback run();
            target := ToolButton {}
            ignored := ToolButton {}
            run => { target.toggle(); }
        }
    "#;
    let compiled = compile(source, "button-toggle.xr").unwrap();
    assert!(compiled.mlir.contains("\"xr.button_toggle\""));
    assert_eq!(
        compiled.mlir.matches("toggle_storage = \"true\"").count(),
        1
    );
    let invalid = compiled
        .mlir
        .replace("toggle_storage = \"true\"", "toggle_storage = \"false\"");
    assert!(mlir::Module::parse(&invalid).is_err());
    let missing = compiled.mlir.replace(", toggle_storage = \"true\"", "");
    assert_ne!(missing, compiled.mlir);
    assert!(mlir::Module::parse(&missing).is_err());
    for (from, to) in [
        ("target.toggle()", "target.toggle(1)"),
        ("callback run();", "pure callback run();"),
        ("target := ToolButton", "target := Rectangle"),
    ] {
        assert!(compile(&source.replace(from, to), "button-toggle.xr").is_err());
    }
}

#[test]
fn programmatic_button_click_is_a_verified_effect() {
    let source = r#"
        export component App inherits Window {
            callback run();
            target := ToolButton {}
            run => { target.click(); }
        }
    "#;
    let compiled = compile(source, "button-click.xr").unwrap();
    assert!(compiled.mlir.contains("\"xr.button_click\""));
    let animated = compile(
        &source.replace("target.click()", "target.animate-click()"),
        "button-click.xr",
    )
    .unwrap();
    assert!(animated.mlir.contains("animated = \"true\""));
    for (from, to) in [
        ("target.click()", "target.click(1)"),
        ("target.click()", "target.animate-click(1)"),
        ("callback run();", "pure callback run();"),
        ("target := ToolButton", "target := Rectangle"),
    ] {
        assert!(compile(&source.replace(from, to), "button-click.xr").is_err());
    }
}

#[test]
fn button_appearance_bindings_have_paint_effects_and_accept_visual_states() {
    let source = r#"
        export component App inherits Window {
            in property <bool> prominent: true;
            pick := Button {
                primary: root.prominent;
                flat: false;
                states [ quiet when !root.prominent: { flat: true; } ]
            }
            Dialog { DialogButton { primary: root.prominent; flat: true; } }
        }
    "#;
    let compiled = compile(source, "button-appearance.xr").unwrap();
    for property in ["primary", "flat"] {
        assert!(compiled.mlir.lines().any(|line| {
            line.contains(&format!("name = \"{property}\""))
                && line.contains("xr.update_effect = \"paint\"")
        }));
    }
    assert!(compiled.mlir.contains("\"xr.state_set\""));
    assert!(
        compile(
            &source.replace("primary: root.prominent", "primary: 12"),
            "button-appearance.xr"
        )
        .is_err()
    );
    assert!(
        compile(
            "component App inherits Window { Rectangle { flat: true; } }",
            "button-appearance.xr"
        )
        .is_err()
    );
}

#[test]
fn button_display_and_replaceable_content_are_verified() {
    let source = r#"
        export component App inherits Window {
            in property <ButtonDisplay> mode: ButtonDisplay.text-under-icon;
            Button { display: root.mode; colorize-icon: true; spacing: 3px; }
            ToolButton { ButtonContent { Text { text: "Tool"; } } }
            RoundButton { ButtonContent { Text { text: "Round"; } } }
            DelayButton { ButtonContent { Text { text: "Delay"; } } }
        }
    "#;
    let compilation = compile(source, "button-display.xr").unwrap();
    assert!(compilation.mlir.contains("!xr.buttondisplay"));
    assert!(compilation.mlir.contains("text-under-icon"));
    assert!(
        compile(
            &source.replace("display: root.mode", "display: true"),
            "button-display.xr"
        )
        .is_err()
    );
}

#[test]
fn push_button_groups_validate_membership_and_state_ownership() {
    let source = r#"
        export component App inherits Window {
            in-out property <bool> checked: false;
            modes := ButtonGroup { current-value: "list"; }
            flags := ButtonGroup { exclusive: false; }
            command := Action {}
            ToolButton { group: modes; value: "list"; }
            RoundButton { group: modes; value: "grid"; checkable: true; }
            Button { group: flags; checked <=> root.checked; }
        }
    "#;
    let compiled = compile(source, "button-groups.xr").unwrap();
    assert!(compiled.mlir.contains("\"xr.radio_group_ref\""));
    assert!(compiled.mlir.contains("\"xr.check_group_ref\""));
    for (from, to) in [
        (
            "group: modes; value: \"list\";",
            "group: modes; checked: true;",
        ),
        (
            "group: flags; checked <=> root.checked;",
            "group: modes; checked <=> root.checked;",
        ),
        ("checkable: true;", "checkable: false;"),
        (
            "group: modes; value: \"list\";",
            "group: modes; action: command;",
        ),
    ] {
        assert!(compile(&source.replace(from, to), "button-groups.xr").is_err());
    }
}

#[test]
fn checkable_buttons_accept_boolean_bindings_and_zero_argument_toggle() {
    for kind in ["Button", "ToolButton", "RoundButton"] {
        let source = format!(
            r#"
            export component App inherits Window {{
                in-out property <bool> pinned: false;
                pin := {kind} {{ checkable: true; checked <=> root.pinned; toggled => {{}} }}
            }}
        "#
        );
        let compiled = compile(&source, "checkable-button.xr").unwrap();
        assert!(compiled.mlir.contains("name = \"checked\""));
        for (from, to) in [
            ("checkable: true", "checkable: 1"),
            ("toggled =>", "toggled(value) =>"),
        ] {
            assert!(compile(&source.replace(from, to), "checkable-button.xr").is_err());
        }
        let linked = source
            .replace("pin :=", "command := Action {} pin :=")
            .replace("checkable: true", "action: command");
        assert!(compile(&linked, "checkable-button.xr").is_err());
    }
}

#[test]
fn push_button_lifecycle_handlers_use_zero_arguments_and_coexist_with_action() {
    let source = r#"
        component App inherits Window {
            command := Action {}
            button := ToolButton {
                action: command;
                pressed => {}
                released => {}
                canceled => {}
            }
        }
    "#;
    let compiled = compile(source, "button-lifecycle.xr").unwrap();
    for event in ["pressed", "released", "canceled"] {
        assert!(compiled.mlir.contains(&format!("name = \"{event}\"")));
        assert!(
            compile(
                &source.replace(&format!("{event} =>"), &format!("{event}(arg) =>")),
                "button-lifecycle.xr"
            )
            .is_err()
        );
    }
    assert!(compiled.rust.to_string().contains("button_press_handle"));
}

#[test]
fn scroll_controls_require_a_local_viewport_reference() {
    let source = r#"
        component App inherits Window {
            canvas := ScrollView { content-height: 400px; }
            panel := Rectangle {}
            ScrollBar { target: canvas; orientation: vertical; policy: always-on; }
            ScrollIndicator { target: canvas; }
        }
    "#;
    let compiled = compile(source, "scroll-controls.xr").unwrap();
    assert_eq!(compiled.mlir.matches("\"xr.scroll_target\"").count(), 2);
    assert!(compiled.rust.to_string().contains("scroll_target"));
    for (replacement, expected) in [
        ("target: panel;", "target requires the id of a viewport"),
        ("target <=> canvas;", "target requires the id of a viewport"),
        (
            "target: canvas; position: 0.5;",
            "attached scroll controls derive their range from the viewport",
        ),
    ] {
        let invalid = source.replacen("target: canvas;", replacement, 1);
        let error = compile(&invalid, "scroll-controls.xr")
            .err()
            .expect("invalid scroll control compiled");
        assert!(error.contains(expected), "{error}");
    }
}

#[test]
fn system_tray_icon_is_a_windowless_root_with_typed_native_menu_inputs() {
    let source = r#"export component Tray inherits SystemTrayIcon {
        in property <image> artwork;
        icon: root.artwork;
        tooltip: "XRack tray";
        icon-template: true;
        visible: true;
        clicked() => { }
        open := Action { text: "Open"; }
        Menu {
            MenuItem { action: open; }
            Menu { title: "Details"; MenuItem { text: "Status"; enabled: false; } }
        }
    }"#;
    let output = compile(source, "system-tray.xr").unwrap();
    assert!(output.mlir.contains("SystemTrayIcon"));
    assert!(output.mlir.contains("\"xr.action_ref\""));
    compile(
        "export component ClickTray inherits SystemTrayIcon { clicked() => { } }",
        "click-tray.xr",
    )
    .unwrap();
    for (from, to, expected) in [
        ("icon-template: true", "icon-template: 1", "type mismatch"),
        ("Menu {\n", "Menu {} Menu {\n", "at most one Menu"),
        ("Menu {\n", "if true : Menu {\n", "optional static Menu"),
        ("Menu {\n", "Rectangle {} Menu {\n", "optional static Menu"),
    ] {
        let error = compile(&source.replacen(from, to, 1), "system-tray.xr")
            .err()
            .unwrap();
        assert!(error.contains(expected), "{error}");
    }
    let nested = format!("{source}\ncomponent Host inherits Window {{ Tray {{}} }}");
    assert!(
        compile(&nested, "embedded-tray.xr")
            .err()
            .unwrap()
            .contains("top-level component")
    );
}

#[test]
fn flickable_bounds_modes_lower_as_typed_options_and_effects() {
    let source = r#"component App inherits Window {
        out property <length> overshoot: canvas.horizontal-overshoot;
        out property <bool> returning: canvas.rebounding;
        callback reset();
        reset() => { canvas.return-to-bounds(); }
        canvas := Flickable {
            bounds-behavior: FlickBoundsBehavior.drag-and-overshoot-bounds;
            bounds-movement: FlickBoundsMovement.stop-at-bounds;
            rebound-duration: 210;
        }
    }"#;
    let compiled = compile(source, "elastic-bounds.xr").unwrap();
    assert!(compiled.mlir.contains("!xr.flickboundsbehavior"));
    assert!(compiled.mlir.contains("!xr.flickboundsmovement"));
    assert!(compiled.mlir.contains("\"xr.return_to_bounds\""));
    for (from, to, expected) in [
        (
            "rebound-duration: 210",
            "rebound-duration: 210px",
            "type mismatch",
        ),
        (
            "rebound-duration: 210",
            "horizontal-overshoot: 5px",
            "read-only",
        ),
        (
            "canvas.return-to-bounds()",
            "canvas.return-to-bounds(2)",
            "takes no arguments",
        ),
    ] {
        let error = compile(&source.replace(from, to), "elastic-bounds.xr")
            .err()
            .unwrap();
        assert!(error.contains(expected), "{error}");
    }
}

#[test]
fn flickable_lowers_motion_metrics_and_scoped_inertia_cancellation() {
    let source = r#"component App inherits Window {
        out property <bool> active: canvas.moving;
        out property <length> position: canvas.content-x;
        callback stop();
        stop() => { canvas.cancel-flick(); }
        canvas := Flickable {
            clip: false;
            interactive: true;
            flick-direction: FlickDirection.horizontal;
            content-width: 200%;
            flicked() => { }
            Rectangle { width: 100%; height: 40px; }
        }
        ScrollBar { target: canvas; orientation: horizontal; }
    }"#;
    let compiled = compile(source, "flickable.xr").unwrap();
    assert!(compiled.mlir.contains("\"xr.flick_motion_read\""));
    assert!(compiled.mlir.contains("\"xr.cancel_flick\""));
    assert!(compiled.rust.to_string().contains("for_flickable"));
    for (from, to, expected) in [
        (
            "flick-direction: FlickDirection.horizontal",
            "flick-direction: true",
            "type mismatch",
        ),
        ("interactive: true", "moving: true", "read-only"),
        (
            "canvas.cancel-flick()",
            "canvas.cancel-flick(1)",
            "takes no arguments",
        ),
        (
            "canvas.cancel-flick()",
            "canvas.position-view-at-index(1, beginning)",
            "requires a list",
        ),
    ] {
        let error = compile(&source.replace(from, to), "flickable.xr")
            .err()
            .unwrap();
        assert!(error.contains(expected), "{error}");
    }
    let raw = lower(source, "flickable.xr");
    let invalid = raw.replace(
        "owner = \"canvas\", value = \"moving\"",
        "owner = \"canvas\", value = \"enabled\"",
    );
    assert_ne!(raw, invalid);
    assert!(
        mlir::process(&invalid)
            .unwrap_err()
            .contains("motion property on Flickable")
    );
}

#[test]
fn swipe_gestures_lower_typed_metrics_and_cancel_effects() {
    let source = r#"component App inherits Window {
        out property <length> distance: swipe.current-x - swipe.pressed-x;
        out property <bool> active: swipe.swiping;
        callback stop();
        stop() => { swipe.cancel(); }
        swipe := SwipeGestureHandler {
            handle-swipe-left: true;
            moved() => { }
            swiped() => { }
            cancelled() => { }
        }
    }"#;
    let compiled = compile(source, "swipe-gesture.xr").unwrap();
    assert!(compiled.mlir.contains("\"xr.swipe_read\""));
    assert!(compiled.mlir.contains("\"xr.swipe_cancel\""));
    assert!(compiled.rust.to_string().contains("SwipeHandle"));
    for (from, to, expected) in [
        (
            "handle-swipe-left: true",
            "handle-swipe-left: 3",
            "type mismatch",
        ),
        ("handle-swipe-left: true", "current-x: 20px", "read-only"),
        ("swipe.cancel()", "swipe.cancel(1)", "takes no arguments"),
        (
            "moved() => { }",
            "moved() => { swipe.swiping = false; }",
            "read-only",
        ),
    ] {
        let error = compile(&source.replace(from, to), "swipe-gesture.xr")
            .err()
            .unwrap();
        assert!(error.contains(expected), "{error}");
    }
    let raw = lower(source, "swipe-gesture.xr");
    let invalid = raw.replace(
        "\"xr.swipe_cancel\"() <{owner = \"swipe\"}>",
        "\"xr.swipe_cancel\"() <{owner = \"root\"}>",
    );
    assert_ne!(raw, invalid);
    assert!(
        mlir::process(&invalid)
            .unwrap_err()
            .contains("statically addressed swipe handler")
    );
}

#[test]
fn scale_rotate_handlers_expose_typed_read_only_metrics_and_lifecycle() {
    let source = r#"component App inherits Window {
        out property <float> factor: gesture.scale;
        out property <float> angle: gesture.rotation;
        out property <length> centroid: gesture.center-x;
        out property <bool> recognizing: gesture.active;
        gesture := ScaleRotateGestureHandler {
            width: 200px;
            updated() => { }
            Rectangle { width: 100%; height: 100%; }
        }
    }"#;
    let compiled = compile(source, "scale-rotate.xr").unwrap();
    assert!(compiled.mlir.contains("\"xr.scale_rotate_read\""));
    assert!(compiled.rust.to_string().contains("ScaleRotateHandle"));
    for (from, to, expected) in [
        ("width: 200px", "scale: 2.0", "read-only"),
        (
            "updated() => { }",
            "updated() => { gesture.rotation = 20.0; }",
            "read-only",
        ),
        ("updated()", "updated(value)", "takes no parameters"),
    ] {
        let error = compile(&source.replace(from, to), "scale-rotate.xr")
            .err()
            .unwrap();
        assert!(error.contains(expected), "{error}");
    }
    let raw = lower(source, "scale-rotate.xr");
    let invalid = raw.replacen("value = \"scale\"", "value = \"center-y\"", 1);
    assert_ne!(raw, invalid);
    assert!(
        mlir::process(&invalid)
            .unwrap_err()
            .contains("typed metric")
    );
}

#[test]
fn scroll_snap_modes_are_typed_on_both_scroll_bar_modes() {
    let source = r#"
        component App inherits Window {
            in property <ScrollSnapMode> mode: ScrollSnapMode.on-release;
            view := ScrollView { content-height: 400px; }
            ScrollBar { target: view; snap-mode: root.mode; step-size: 0.3; }
            ScrollBar { size: 0.25; snap-mode: always; step-size: 0.25; }
        }
    "#;
    let output = compile(source, "scroll-snap.xr").unwrap();
    assert!(output.mlir.contains("!xr.scrollsnapmode"));
    assert!(
        output
            .rust
            .to_string()
            .contains("ScrollSnapMode :: OnRelease")
    );
    for (from, to, expected) in [
        ("snap-mode: always", "snap-mode: 1", "type mismatch"),
        (
            "ScrollBar { size: 0.25",
            "ScrollIndicator { size: 0.25",
            "snap-mode",
        ),
    ] {
        let error = compile(&source.replace(from, to), "scroll-snap.xr")
            .err()
            .unwrap();
        assert!(error.contains(expected), "{error}");
    }
    let raw = lower(source, "scroll-snap.xr");
    let invalid = raw.replace("value = \"on-release\"", "value = \"unrecognized\"");
    assert_ne!(raw, invalid);
    assert!(
        mlir::process(&invalid)
            .unwrap_err()
            .contains("literal spelling")
    );
}

#[test]
fn attached_scroll_metrics_are_read_only_and_follow_layout_phases() {
    let source = r#"
        component App inherits Window {
            out property <float> extent: bar.size;
            canvas := ScrollView { content-height: 400px; }
            bar := ScrollBar { target: canvas; }
            ScrollIndicator { position: bar.position; size: bar.size; }
            open := Button {
                details := PopupWindow { width: bar.size * 100px; height: 40px; }
            }
        }
    "#;
    let compiled = compile(source, "attached-ranges.xr").unwrap();
    assert!(compiled.mlir.contains("\"xr.scroll_control_read\""));
    assert!(compiled.rust.to_string().contains("scroll_size"));
    for (from, to, expected) in [
        (
            "content-height: 400px",
            "content-height: root.extent * 400px",
            "layout feedback",
        ),
        (
            "bar := ScrollBar { target: canvas; }",
            "bar := ScrollBar { target: canvas; orientation: bar.position > 0.5 ? Orientation.horizontal : Orientation.vertical; }",
            "cyclic property binding",
        ),
        (
            "details := PopupWindow",
            "details := Rectangle",
            "layout feedback",
        ),
        (
            "open := Button {",
            "open := Button { clicked() => { bar.position = 0.5; }",
            "read-only",
        ),
    ] {
        let error = compile(&source.replace(from, to), "attached-ranges.xr")
            .err()
            .unwrap();
        assert!(error.contains(expected), "{error}");
    }
    let raw = lower(source, "attached-ranges.xr");
    let feedback = raw.replace("kind = \"PopupWindow\"", "kind = \"Rectangle\"");
    assert_ne!(raw, feedback);
    assert!(
        mlir::process(&feedback)
            .unwrap_err()
            .contains("layout feedback")
    );
}

#[test]
fn standalone_scroll_controls_support_observable_normalized_ranges() {
    let source = r#"
        component App inherits Window {
            in-out property <float> offset: 0;
            out property <float> position: bar.position;
            bar := ScrollBar { position <=> root.offset; size: 0.25; }
            indicator := ScrollIndicator { position: bar.position; size: bar.size; }
        }
    "#;
    let compiled = compile(source, "standalone-scroll.xr").unwrap();
    assert!(compiled.mlir.contains("\"xr.scroll_control_read\""));
    assert!(compiled.mlir.contains("\"xr.control_link\""));
    assert!(compiled.rust.to_string().contains("ScrollBarRange"));
    compile(
        "component CustomBar inherits ScrollBar { out property <float> effective: self.position; size: 0.25; }",
        "custom-scroll-bar.xr",
    )
    .unwrap();
    for (from, to, expected) in [
        (
            "size: 0.25",
            "size: root.position",
            "cyclic property binding",
        ),
        (
            "position: bar.position; size: bar.size;",
            "position <=> root.offset;",
            "two-way binding requires an editable control property",
        ),
    ] {
        let error = compile(&source.replace(from, to), "standalone-scroll.xr")
            .err()
            .unwrap();
        assert!(error.contains(expected), "{error}");
    }
}

#[test]
fn non_exclusive_button_groups_verify_members_reads_and_effects() {
    let source = r#"
        component App inherits Window {
            in property <bool> choice-mode: false;
            out property <CheckState> status: checks.check-state;
            checks := ButtonGroup { exclusive: false; }
            master := CheckBox { check-state <=> checks.check-state; }
            CheckBox { group: checks; text: "Option"; }
            Switch { group: checks; text: "Switch"; }
            callback clear();
            clear() => { checks.set-check-state(CheckState.unchecked); }
        }
    "#;
    let compiled = compile(source, "check-groups.xr").unwrap();
    assert_eq!(compiled.mlir.matches("\"xr.check_group_ref\"").count(), 2);
    assert!(compiled.mlir.contains("\"xr.check_group_read\""));
    assert!(compiled.mlir.contains("\"xr.check_group_command\""));
    assert!(compiled.rust.to_string().contains("summarize_checks"));
    for (from, to, expected) in [
        (
            "exclusive: false",
            "exclusive: root.choice-mode",
            "requires a literal bool",
        ),
        (
            "Switch { group: checks",
            "RadioButton { group: checks",
            "requires an exclusive ButtonGroup",
        ),
        (
            "exclusive: false;",
            "exclusive: false; current-index: 0;",
            "require an exclusive ButtonGroup",
        ),
        (
            "group: checks; text: \"Option\"",
            "group: checks; check-state: CheckState.checked; text: \"Option\"",
            "require writable check-state",
        ),
        (
            "master := CheckBox {",
            "master := CheckBox { group: checks;",
            "cyclic property binding",
        ),
        (
            "group: checks; text: \"Option\"",
            "group: checks; checked: checks.check-state == CheckState.checked",
            "cyclic property binding",
        ),
        (
            "callback clear();",
            "pure callback clear();",
            "not allowed in a pure callback",
        ),
        (
            "set-check-state(CheckState.unchecked)",
            "set-check-state(true)",
            "type mismatch",
        ),
        (
            "checks.set-check-state(CheckState.unchecked)",
            "checks.check-state = CheckState.unchecked",
            "read-only",
        ),
    ] {
        let error = compile(&source.replacen(from, to, 1), "check-groups.xr")
            .err()
            .unwrap();
        assert!(error.contains(expected), "{error}");
    }
    let raw = lower(source, "check-groups.xr");
    let wrong_property = raw.replace("property = \"checked\"", "property = \"check-state\"");
    assert!(
        mlir::process(&wrong_property)
            .unwrap_err()
            .contains("incorrect check group member property")
    );
    let wrong_owner = raw.replace("group = \"checks\"", "group = \"master\"");
    assert!(
        mlir::process(&wrong_owner)
            .unwrap_err()
            .contains("non-exclusive ButtonGroup")
    );
}

#[test]
fn native_check_groups_reject_cycles_between_aggregate_checkboxes() {
    let source = r#"
        component App inherits Window {
            outer := ButtonGroup { exclusive: false; }
            inner := ButtonGroup { exclusive: false; }
            CheckBox { group: outer; check-state <=> inner.check-state; }
            CheckBox { group: inner; }
        }
    "#;
    assert!(compile(source, "nested-check-groups.xr").is_ok());
    let raw = lower(source, "nested-check-groups.xr");
    let read = raw
        .lines()
        .find(|line| line.contains("\"xr.check_group_read\""))
        .unwrap();
    let wrong = raw.replace(
        read,
        &read.replace("owner = \"inner\"", "owner = \"outer\""),
    );
    assert!(
        mlir::process(&wrong)
            .unwrap_err()
            .contains("cyclic check group links")
    );
}

#[test]
fn button_group_membership_is_a_verified_reference_with_typed_selection() {
    let source = r#"
        export component App inherits Window {
            in-out property <string> mode: "quick";
            in property <bool> show-detail: true;
            modes := ButtonGroup { current-value <=> root.mode; selected(value) => { } }
            HorizontalLayout {
                RadioButton { group: modes; value: "quick"; text: "Quick"; }
                VerticalLayout {
                    if root.show-detail : RadioButton { group: modes; value: "detail"; text: "Detailed"; }
                }
            }
        }
    "#;
    let compiled = compile(source, "button-group.xr").unwrap();
    assert_eq!(compiled.mlir.matches("\"xr.radio_group_ref\"").count(), 2);
    assert!(compiled.rust.to_string().contains("bind_button_group"));
    for (from, to, expected) in [
        ("group: modes", "group: missing", "id of a ButtonGroup"),
        ("group: modes", "group: \"modes\"", "id of a ButtonGroup"),
        ("value: \"quick\"", "value: 1", "type mismatch"),
        (
            "current-value <=> root.mode;",
            "current-value <=> root.mode; current-index: 0;",
            "either current-index or current-value",
        ),
    ] {
        let error = compile(&source.replacen(from, to, 1), "button-group.xr")
            .err()
            .unwrap();
        assert!(error.contains(expected), "{error}");
    }
    let raw = lower(source, "button-group.xr").replace("group = \"modes\"", "group = \"missing\"");
    assert!(
        mlir::process(&raw)
            .unwrap_err()
            .contains("a ButtonGroup in the same component")
    );
    let nested = "component App inherits Window { VerticalLayout { modes := ButtonGroup {} } }";
    assert!(
        compile(nested, "button-group.xr")
            .err()
            .unwrap()
            .contains("at the component root")
    );
    let double_group = "component App inherits Window { modes := ButtonGroup {} RadioGroup { RadioButton { group: modes; } } }";
    assert!(
        compile(double_group, "button-group.xr")
            .err()
            .unwrap()
            .contains("already belongs to its parent RadioGroup")
    );
}

#[test]
fn native_window_status_and_commands_have_verified_types_and_scope() {
    let source = r#"
        export component App inherits Window {
            in property <bool> fullscreen-request: true;
            out property <bool> actual-fullscreen: root.full-screen;
            callback update();
            update() => { root.maximize(); root.set-full-screen(root.fullscreen-request); }
            restore-button := Button { clicked => { root.restore(); } }
        }
    "#;
    let compiled = compile(source, "window-commands.xr").unwrap();
    assert_eq!(compiled.mlir.matches("\"xr.window_command\"").count(), 3);
    assert!(compiled.mlir.contains("\"xr.window_read\""));
    assert!(
        compiled
            .rust
            .to_string()
            .contains("WindowCommandMethod :: SetFullScreen")
    );
    for (from, to, expected) in [
        (
            "set-full-screen(root.fullscreen-request)",
            "set-full-screen(1)",
            "type mismatch",
        ),
        (
            "root.maximize()",
            "root.maximize(true)",
            "takes no arguments",
        ),
        (
            "callback update();",
            "pure callback update();",
            "not allowed in a pure callback",
        ),
        ("root.maximize()", "root.maximized = true", "read-only"),
        (
            "callback update();",
            "maximized: true; callback update();",
            "read-only",
        ),
    ] {
        let error = compile(&source.replace(from, to), "window-commands.xr")
            .err()
            .unwrap();
        assert!(error.contains(expected), "{error}");
    }
    let raw = lower(source, "window-commands.xr");
    let wrong_args = raw.replace("command = \"set-full-screen\"", "command = \"minimize\"");
    assert!(
        mlir::process(&wrong_args)
            .unwrap_err()
            .contains("invalid window command or arguments")
    );
    let wrong_scope = raw.replace("kind = \"Window\"", "kind = \"Rectangle\"");
    assert!(
        mlir::process(&wrong_scope)
            .unwrap_err()
            .contains("root Window")
    );
}

#[test]
fn frameless_window_properties_are_typed_in_source_and_mlir() {
    let source = r#"component App inherits Window {
        no-frame: true;
        resizable: false;
        resize-border-width: 8px;
        WindowMoveArea { enabled: true; Text { text: "Header"; } }
    }"#;
    let compiled = compile(source, "frameless.xr").unwrap();
    assert!(compiled.mlir.contains("kind = \"WindowMoveArea\""));
    let raw = lower(source, "frameless.xr");
    assert!(
        mlir::process(&raw.replace("name = \"no-frame\"", "name = \"resize-border-width\""))
            .unwrap_err()
            .contains("incorrect window chrome property or type")
    );
    for (from, to) in [
        ("no-frame: true", "no-frame: 1"),
        ("resize-border-width: 8px", "resize-border-width: 8%"),
        ("enabled: true", "enabled: 1"),
    ] {
        let error = compile(&source.replace(from, to), "frameless.xr")
            .err()
            .unwrap();
        assert!(error.contains("type mismatch"), "{error}");
    }
}

#[test]
fn context_menu_opt_out_requires_a_typed_editor_property() {
    let source = "component App inherits Window { LineEdit { context-menu-enabled: false; } }";
    let compiled = compile(source, "editor-menu.xr").unwrap();
    assert!(compiled.mlir.contains("name = \"context-menu-enabled\""));
    assert!(
        compile(&source.replace("false", "1"), "editor-menu.xr")
            .err()
            .unwrap()
            .contains("type mismatch")
    );
    let raw = lower(source, "editor-menu.xr").replace("kind = \"LineEdit\"", "kind = \"Button\"");
    assert!(
        mlir::process(&raw)
            .unwrap_err()
            .contains("context-menu-enabled requires a boolean on a text editor")
    );
}

#[test]
fn compiler_errors_retain_source_diagnostics() {
    let Err(CompilerError::Diagnostics {
        filename,
        diagnostics,
    }) = xrack_compiler::compile("component App inherits Window { width: true; }", "bad.xr")
    else {
        panic!("expected a compiler diagnostic");
    };
    assert_eq!(filename, "bad.xr");
    assert_eq!(diagnostics[0].span.line, 1);
    assert!(diagnostics[0].message.contains("type mismatch"));
}

#[test]
fn viewport_metrics_and_scroll_commands_are_typed_and_verified() {
    let source = r#"export component Demo inherits Window {
        out property <length> offset: view.content-y;
        view := ScrollView {}
        list := ListView { for item in 100 : Text { text: "Row " + item; } }
        jump := Button { clicked() => {
            view.scroll-to(24px, 48px);
            list.position-view-at-index(40, ScrollAlignment.center);
        } }
    }"#;
    let compiled = compile(source, "scroll.xr").unwrap();
    assert!(compiled.mlir.contains("\"xr.viewport_read\""));
    assert!(compiled.mlir.contains("\"xr.scroll_command\""));
    assert!(compiled.mlir.contains("!xr.scrollalignment"));
    assert!(
        compiled
            .rust
            .to_string()
            .contains("ScrollCommandMethod :: Index")
    );
    for (from, to, diagnostic) in [
        (
            "view.scroll-to(24px, 48px)",
            "view.scroll-to(24, 48px)",
            "type mismatch",
        ),
        (
            "list.position-view-at-index",
            "view.position-view-at-index",
            "requires a list",
        ),
        (
            "view.scroll-to(24px, 48px)",
            "view.content-y = 48px",
            "read-only",
        ),
        (
            "view := ScrollView {}",
            "view := ScrollView { visible-height: 4px; }",
            "read-only",
        ),
    ] {
        let invalid = source.replace(from, to);
        assert!(
            compile(&invalid, "scroll.xr")
                .err()
                .unwrap()
                .contains(diagnostic)
        );
    }
    let pure = r#"component Demo inherits Window {
        view := ScrollView {}
        pure callback jump() -> int;
        jump() => { view.scroll-to(0px, 0px); 0 }
    }"#;
    assert!(
        compile(pure, "scroll.xr")
            .err()
            .unwrap()
            .contains("scroll commands are not allowed in a pure callback")
    );
    let repeated = r#"component Demo inherits Window {
        for item in 3 : ScrollView { content-width: self.visible-width; }
    }"#;
    assert!(
        compile(repeated, "scroll.xr")
            .err()
            .unwrap()
            .contains("requires a component delegate")
    );

    let raw = lower(source, "scroll.xr");
    let invalid = raw.replace(
        "method = \"scroll-to\"",
        "method = \"position-view-at-index\"",
    );
    assert!(
        mlir::process(&invalid)
            .unwrap_err()
            .contains("position-view-at-index(int, ScrollAlignment)")
    );
    let invalid = raw.replace(
        "owner = \"view\", value = \"content-y\"",
        "owner = \"jump\", value = \"content-y\"",
    );
    assert!(
        mlir::process(&invalid)
            .unwrap_err()
            .contains("requires a scrollable element")
    );
}

#[test]
fn source_scroll_commands_capture_row_keys_and_require_virtual_view_owners() {
    let source = r#"
        component App inherits Window {
            list := ListView { for item in 100 : Text {} }
            table := TableView {
                TableColumn {}
                for item in 100 : TableRow { Text {} }
            }
            jump := Button {
                clicked => {
                    list.position-view-at-source-index(4, beginning);
                    table.position-view-at-source-cell(7, 0, contain, center);
                }
            }
        }
    "#;
    let output = compile(source, "source-scroll.xr").unwrap();
    let rust = output.rust.to_string();
    assert!(rust.contains("ScrollCommandMethod :: SourceIndex"));
    assert!(rust.contains("ScrollCommandMethod :: SourceCell"));
    assert_eq!(rust.matches("ListModel :: key_at").count(), 2);
    let invalid = source.replace("list := ListView", "list := ScrollView");
    let error = compile(&invalid, "source-scroll.xr").err().unwrap();
    assert!(
        error.contains("position-view-at-source-index() requires a list, grid, tree, or table")
    );
    let raw = lower(source, "source-scroll.xr");
    let invalid = raw.replace(
        "position-view-at-source-index",
        "position-view-at-source-cell",
    );
    assert!(
        mlir::process(&invalid)
            .unwrap_err()
            .contains("position-view-at-source-cell(int, int, ScrollAlignment, ScrollAlignment)")
    );
}

#[test]
fn source_index_queries_are_typed_and_reject_projection_cycles() {
    let source = r#"
        component App inherits Window {
            in property <int> source: 3;
            out property <int> shown: list.view-index-for-source(root.source);
            out property <int> original: table.source-index-for-view(root.shown);
            list := ListView { for item in 5 : Text {} }
            table := TableView {
                TableColumn {}
                for item in 5 : TableRow { Text {} }
            }
            callback select();
            select => {
                list.select-source-index(root.source);
                table.select-source-cell(root.source, 0);
            }
        }
    "#;
    compile(source, "source-index.xr").unwrap();
    let raw = lower(source, "source-index.xr");
    for name in ["xr.view_index", "xr.source_selection"] {
        assert!(raw.contains(name));
    }
    let invalid = source.replace("table.select-source-cell", "list.select-source-cell");
    assert!(compile(&invalid, "source-index.xr").is_err());
    let invalid = source.replace("callback select();", "pure callback select();");
    assert!(
        compile(&invalid, "source-index.xr")
            .err()
            .unwrap()
            .contains("pure callback")
    );
    let invalid = source.replace(
        "list.view-index-for-source(root.source)",
        "list.view-index-for-source(true)",
    );
    assert!(compile(&invalid, "source-index.xr").is_err());
    let invalid = raw.replace(
        "value = \"source-index-for-view\"",
        "value = \"invalid-index-method\"",
    );
    assert_ne!(invalid, raw);
    assert!(
        mlir::process(&invalid)
            .unwrap_err()
            .contains("requires view-index-for-source or source-index-for-view")
    );
    let invalid = source.replace("for item in 5 : Text", "for item in root.shown : Text");
    let error = compile(&invalid, "source-index.xr").err().unwrap();
    assert!(error.contains("cyclic"), "{error}");
}

#[test]
fn list_visibility_projects_record_models_for_lists_and_grids() {
    let source = r#"
        struct Flags { shown: bool }
        struct Entry { label: string, flags: Flags }
        component App inherits Window {
            in property <[Entry]> entries;
            ListView {
                row-visible-field: "flags.shown";
                for entry in root.entries : Text { text: entry.label; }
            }
            GridView {
                row-visible-field: "flags.shown";
                for entry in root.entries : Text { text: entry.label; }
            }
            StandardListView {
                row-visible-field: "flags.shown";
                text-role: "label";
                model: root.entries;
            }
        }
    "#;
    let output = compile(source, "list-visible.xr").unwrap();
    assert_eq!(output.rust.to_string().matches(". visible_rows").count(), 3);
    let invalid = source.replace("flags.shown", "label");
    let error = compile(&invalid, "list-visible.xr").err().unwrap();
    assert!(error.contains("row-visible-field requires a bool field"));
    let raw = lower(source, "list-visible.xr").replace("flags.shown", "label");
    assert!(
        mlir::process(&raw)
            .unwrap_err()
            .contains("row-visible-field requires a bool field")
    );
}

#[test]
fn tree_visibility_projects_a_constant_boolean_path_alongside_text_filtering() {
    let source = r#"
        struct Flags { shown: bool }
        struct Entry { key: int, parent: int, label: string, flags: Flags }
        component App inherits TreeView {
            in property <[Entry]> entries;
            row-visible-field: "flags.shown";
            filter-field: "label";
            filter-text: "entry";
            for entry in root.entries : TreeRow {
                key: entry.key;
                parent-key: entry.parent;
                Text { text: entry.label; }
            }
        }
    "#;
    let output = compile(source, "tree-visible.xr").unwrap();
    assert!(output.rust.to_string().contains("project_model"));
    let invalid = source.replace("flags.shown", "parent");
    let error = compile(&invalid, "tree-visible.xr").err().unwrap();
    assert!(error.contains("row-visible-field requires a bool field"));
    let raw = lower(source, "tree-visible.xr").replace("flags.shown", "parent");
    assert!(
        mlir::process(&raw)
            .unwrap_err()
            .contains("row-visible-field requires a bool field")
    );
}

#[test]
fn row_visibility_fields_are_constant_boolean_paths_in_both_table_kinds() {
    let source = r#"
        export struct Flags { shown: bool }
        export struct Entry { title: string, flags: Flags }
        export component App inherits Window {
            in property <[Entry]> entries;
            StandardTableView {
                row-visible-field: "flags.shown";
                model: root.entries;
                TableColumn { field: "title"; }
            }
            TableView {
                row-visible-field: "flags.shown";
                TableColumn {}
                for entry in root.entries : TableRow { Text { text: entry.title; } }
            }
        }
    "#;
    let output = compile(source, "row-visible-field.xr").unwrap();
    assert_eq!(
        output
            .rust
            .to_string()
            .matches("projected_rows_with_visibility")
            .count(),
        2
    );
    let wrong_field = source.replace("flags.shown", "title");
    assert!(
        compile(&wrong_field, "row-visible-field.xr")
            .err()
            .unwrap()
            .contains("row-visible-field requires a bool field")
    );
    let dynamic_field = source.replace("\"flags.shown\"", "root.field-name");
    assert!(
        compile(&dynamic_field, "row-visible-field.xr")
            .err()
            .unwrap()
            .contains("row-visible-field requires a constant string field name")
    );
    let raw = lower(source, "row-visible-field.xr").replace("flags.shown", "title");
    assert!(
        mlir::process(&raw)
            .unwrap_err()
            .contains("row-visible-field requires a bool field")
    );
}

#[test]
fn table_column_visibility_is_a_typed_binding() {
    let source = r#"
        component ColumnDemo inherits TableView {
            in property <bool> show-detail: true;
            TableColumn { width: 120px; visible: root.show-detail; }
            for item in 4 : TableRow { Text { text: "" + item; } }
        }
    "#;
    assert!(compile(source, "column-visibility.xr").is_ok());
    assert!(
        compile(
            &source.replace("root.show-detail", "12"),
            "column-visibility.xr"
        )
        .is_err()
    );
    let source = source.replace(" visible: root.show-detail;", "");
    let raw = lower(&source, "column-visibility.xr");
    let invalid = raw.replace("name = \"width\"", "name = \"visible\"");
    assert!(
        mlir::process(&invalid)
            .unwrap_err()
            .contains("TableColumn.visible requires a bool")
    );
}

#[test]
fn table_cell_positioning_has_typed_axes_and_a_table_owner() {
    let source = r#"struct Entry { label: string }
    export component Demo inherits Window {
        table := StandardTableView {
            model: [Entry { label: "item" }];
            TableColumn { field: "label"; }
        }
        view := ScrollView {}
        callback jump();
        jump() => {
            table.position-view-at-cell(9000, 0, end, center);
            table.position-view-at-column(0, contain);
            table.position-view-at-row(9000, beginning);
        }
    }"#;
    let compiled = compile(source, "table-position.xr").unwrap();
    assert_eq!(compiled.mlir.matches("\"xr.scroll_command\"").count(), 3);
    let rust = compiled.rust.to_string();
    assert!(rust.contains("ScrollCommandMethod :: Cell"));
    assert!(rust.contains("ScrollCommandMethod :: Column"));
    for (from, to, diagnostic) in [
        ("9000, 0, end, center", "9000, 0, end", "takes 4 arguments"),
        (
            "9000, 0, end, center",
            "9000, 0px, end, center",
            "type mismatch",
        ),
        (
            "9000, 0, end, center",
            "9000, 0, 1, center",
            "type mismatch",
        ),
        (
            "table.position-view-at-cell",
            "view.position-view-at-cell",
            "requires a table",
        ),
        (
            "table.position-view-at-row",
            "view.position-view-at-row",
            "requires a table",
        ),
        (
            "table.position-view-at-column",
            "view.position-view-at-column",
            "requires a table",
        ),
        (
            "callback jump()",
            "pure callback jump()",
            "not allowed in a pure callback",
        ),
    ] {
        let error = compile(&source.replace(from, to), "table-position.xr")
            .err()
            .unwrap();
        assert!(error.contains(diagnostic), "{error}");
    }
    let raw = lower(source, "table-position.xr");
    let error = mlir::process(&raw.replace("owner = \"table\"", "owner = \"view\"")).unwrap_err();
    assert!(error.contains("statically addressed table"), "{error}");
    let error = mlir::process(&raw.replace(
        "method = \"position-view-at-column\"",
        "method = \"position-view-at-cell\"",
    ))
    .unwrap_err();
    assert!(
        error.contains("position-view-at-cell(int, int, ScrollAlignment, ScrollAlignment)"),
        "{error}"
    );
}

#[test]
fn custom_clipboard_parsers_require_a_pure_typed_projection_to_a_row_link() {
    let source = r#"
        struct Parsed { accepted: bool, value: string }
        component Demo inherits Window {
            in-out property <[string]> names: ["one"];
            pure callback decode(string) -> Parsed;
            decode(raw) => { Parsed { accepted: raw != "", value: raw } }
            TableView {
                pasteable: true;
                TableColumn { title: "Name"; }
                for row[index] in root.names : TableRow {
                    HorizontalLayout {
                        Editor {
                            value <=> row;
                            clipboard-target: "value";
                            clipboard-paste(raw) => { root.decode(raw) }
                        }
                    }
                }
            }
        }
        component Editor inherits Rectangle { in-out property <string> value; }
    "#;
    let compiled = compile(source, "custom-clipboard.xr").unwrap();
    assert!(compiled.mlir.contains("\"xr.clipboard_paste\""));
    assert!(compiled.mlir.contains("target = \"value\""));
    for (from, to, expected) in [
        (
            "value <=> row",
            "value: row",
            "must name an in-out property linked",
        ),
        (
            "clipboard-paste(raw)",
            "clipboard-paste(index)",
            "must differ from the row",
        ),
        ("root.decode(raw)", "raw", "must return a record"),
        ("pure callback decode", "callback decode", "impure callback"),
        (
            "clipboard-target: \"value\";",
            "",
            "requires clipboard-target",
        ),
    ] {
        let error = analyze(&source.replace(from, to)).unwrap_err();
        assert!(error.contains(expected), "{error}");
    }
    let raw = lower(source, "custom-clipboard.xr");
    let invalid = raw.replace(
        "target = \"value\", text_parameter",
        "target = \"missing\", text_parameter",
    );
    assert!(
        mlir::process(&invalid)
            .unwrap_err()
            .contains("requires a component input linked to a table model row")
    );
}

#[test]
fn row_component_delegates_share_typed_repeat_context_and_validate_clipboard_text() {
    let source = r#"component Demo inherits Window {
        in-out property <[string]> names: ["one"];
        out property <int> committed: -1;
        TableView {
            copyable: true;
            TableColumn { title: "Name"; }
            for row[index] in root.names : TableRow {
                Editor {
                    value <=> row;
                    clipboard-text: row;
                    submitted => { root.committed = index; }
                }
            }
        }
    }
    component Editor inherits Rectangle {
        in-out property <string> value;
        callback submitted;
    }"#;
    let compiled = compile(source, "row-delegate.xr").unwrap();
    assert!(compiled.mlir.contains("\"xr.instance_link\""));
    assert!(compiled.mlir.contains("name = \"clipboard-text\""));
    assert!(compiled.rust.to_string().contains("ComponentRepeater"));
    assert!(
        analyze(&source.replace("clipboard-text: row", "clipboard-text: index"))
            .unwrap_err()
            .contains("type mismatch")
    );
    assert!(
        analyze(&source.replace(
            "Editor {\n                    value",
            "named := Editor {\n                    value"
        ))
        .unwrap_err()
        .contains("anonymous instance")
    );
    let raw = lower(source, "row-delegate.xr");
    let invalid = raw.replace(
        "name = \"names\", visibility = \"in-out\"",
        "name = \"names\", visibility = \"in\"",
    );
    assert!(
        mlir::process(&invalid)
            .unwrap_err()
            .contains("cannot write read-only property")
    );
}

#[test]
fn table_copy_projections_are_typed_and_scoped_to_direct_cells() {
    let source = r#"component Demo inherits Window {
        outside := Text { text: "Other"; }
        TableView {
            copyable: true;
            TableColumn { title: "Label"; }
            for row in 3 : TableRow {
                Rectangle { clipboard-text: "Row " + row; Text { text: "Shown"; } }
            }
        }
    }"#;
    let compiled = compile(source, "table-copy.xr").unwrap();
    assert!(compiled.mlir.contains("name = \"clipboard-text\""));
    assert!(compiled.rust.to_string().contains("copy_table_text"));
    let invalid = source.replace("clipboard-text: \"Row \" + row", "clipboard-text: row");
    assert!(
        compile(&invalid, "table-copy.xr")
            .err()
            .unwrap()
            .contains("type mismatch")
    );
    let invalid = source.replace("text: \"Other\"", "clipboard-text: \"Other\"");
    assert!(
        compile(&invalid, "table-copy.xr")
            .err()
            .unwrap()
            .contains("direct table cell")
    );
    let raw = lower(source, "table-copy.xr");
    let invalid = raw.replacen("name = \"text\"", "name = \"clipboard-text\"", 1);
    assert!(
        mlir::process(&invalid)
            .unwrap_err()
            .contains("direct table cell")
    );
}

#[test]
fn table_paste_compiles_writable_row_links_and_checks_the_flag_type() {
    let source = r#"component Demo inherits Window {
        in-out property <[string]> names: ["One", "Two"];
        TableView {
            row-height: 32px;
            pasteable: true;
            TableColumn { title: "Name"; }
            for name in root.names : TableRow {
                TableCell { text <=> name; }
            }
        }
    }"#;
    let compiled = compile(source, "table-paste.xr").unwrap();
    assert!(compiled.mlir.contains("name = \"pasteable\""));
    assert!(compiled.mlir.contains("xr.row_link"));
    let rust = compiled.rust.to_string();
    assert!(rust.contains("TableEditHistory"));
    assert!(rust.contains("replay_table_paste_at"));
    let edit_only = source.replace("pasteable: true", "undoable: true");
    let rust = compile(&edit_only, "table-history.xr")
        .unwrap()
        .rust
        .to_string();
    assert!(rust.contains("TableEditHistory"));
    assert!(rust.contains("commit_cell"));
    assert!(!rust.contains("paste_in_order"));
    assert!(
        compile(
            &edit_only.replace("undoable: true", "undoable: 1"),
            "table-history.xr"
        )
        .err()
        .unwrap()
        .contains("type mismatch")
    );
    let invalid = source.replace("pasteable: true", "pasteable: 1");
    assert!(
        compile(&invalid, "table-paste.xr")
            .err()
            .unwrap()
            .contains("type mismatch")
    );
    let raw = lower(source, "table-paste.xr");
    let invalid = raw.replacen("name = \"row-height\"", "name = \"pasteable\"", 1);
    assert!(
        mlir::process(&invalid)
            .unwrap_err()
            .contains("pasteable requires a boolean on a table view")
    );
    let invalid = raw.replacen("name = \"row-height\"", "name = \"undoable\"", 1);
    assert!(
        mlir::process(&invalid)
            .unwrap_err()
            .contains("undoable requires a boolean on a table view")
    );
}

#[test]
fn round_button_and_tool_separator_validate_geometry_and_actions() {
    let source = r#"
        component Ring inherits RoundButton {
            radius: 6px;
            clicked() => {}
        }
        component Demo inherits Window {
            command := Action { text: "Grid"; }
            RoundButton { radius: 4px; action: command; icon-only: true; }
            ToolSeparator { orientation: horizontal; stroke-width: 2px; }
        }
    "#;
    let compiled = compile(source, "round-tools.xr").unwrap();
    assert!(compiled.mlir.contains("kind = \"RoundButton\""));
    assert!(compiled.mlir.contains("kind = \"ToolSeparator\""));
    assert!(compiled.mlir.contains("xr.action_ref"));
    for (from, to, expected) in [
        ("radius: 4px", "radius: false", "type mismatch"),
        ("stroke-width: 2px;", "Button {}", "cannot contain child"),
    ] {
        let error = analyze(&source.replace(from, to)).unwrap_err();
        assert!(error.contains(expected), "{error}");
    }
    let raw = lower(source, "round-tools.xr");
    assert!(
        mlir::process(&raw.replace("kind = \"RoundButton\"", "kind = \"Button\""))
            .unwrap_err()
            .contains("incorrect RoundButton or ToolSeparator property type")
    );
}

#[test]
fn application_window_supports_native_commands_and_validates_its_bands() {
    let source = r#"
        component Notes inherits ApplicationWindow {
            PageFooter { height: 20px; }
        }
        component Demo inherits ApplicationWindow {
            padding-left: 16px;
            out property <length> content: root.available-width;
            close-requested() => { true }
            MenuBar { Menu { title: "File"; } }
            PageHeader { zoom := Button { clicked() => { root.maximize(); } } }
            secondary := SecondaryWindow { Notes {} }
        }
    "#;
    let result = compile(source, "application-window.xr").unwrap();
    assert!(result.mlir.contains("kind = \"ApplicationWindow\""));
    assert!(result.mlir.contains("xr.window_command"));
    assert!(
        result
            .rust
            .to_string()
            .contains("application_window = true")
    );
    for (from, to, expected) in [
        ("padding-left: 16px", "padding-left: false", "type mismatch"),
        (
            "PageHeader { zoom",
            "PageHeader {} PageHeader { zoom",
            "only one PageHeader",
        ),
        (
            "MenuBar { Menu",
            "MenuBar {} MenuBar { Menu",
            "only one MenuBar",
        ),
        (
            "PageFooter { height: 20px; }",
            "ApplicationWindow {}",
            "requires a component root",
        ),
        (
            "padding-left: 16px",
            "padding-left: root.available-width",
            "layout feedback",
        ),
    ] {
        let error = analyze(&source.replace(from, to)).unwrap_err();
        assert!(error.contains(expected), "{error}");
    }
}

#[test]
fn pane_and_frame_validate_padding_available_size_and_layout_feedback() {
    let source = r#"
        component Demo inherits Window {
            out property <length> inner: frame.available-width;
            pane := Pane {
                content-width: 120px; padding-left: 8px;
                frame := ControlFrame { padding: 4px; clip: true; border-radius: 5px; }
            }
        }
    "#;
    let compiled = compile(source, "pane.xr").unwrap();
    assert!(compiled.mlir.contains("value = \"available-width\""));
    for (from, to, expected) in [
        ("padding-left: 8px", "padding-left: true", "type mismatch"),
        ("padding-left: 8px", "available-width: 8px", "read-only"),
        (
            "padding: 4px",
            "padding: self.available-width",
            "layout feedback",
        ),
    ] {
        let error = analyze(&source.replace(from, to)).unwrap_err();
        assert!(error.contains(expected), "{error}");
    }
    let raw = lower(source, "pane.xr");
    assert!(
        mlir::process(&raw.replace("kind = \"ControlFrame\"", "kind = \"Rectangle\""))
            .unwrap_err()
            .contains("available size requires a Pane or ControlFrame")
    );
}

#[test]
fn layout_geometry_reads_are_typed_and_only_flow_into_later_layout_phases() {
    let source = r#"component Demo inherits Window {
        out property <length> measured: panel.layout-width;
        out property <bool> ready: panel.layout-ready;
        pure callback metric() -> length;
        metric => { root.measured }
        panel := Rectangle { width: 120px; height: 40px; }
        label := Text { width: 280px; height: 20px; text: "width " + (root.metric() / 1px); }
        button := Button {
            details := PopupWindow { width: panel.layout-width; height: 36px; }
        }
    }"#;
    let compiled = compile(source, "geometry.xr").unwrap();
    assert!(compiled.mlir.contains("\"xr.geometry_read\""));
    assert!(
        compiled
            .rust
            .to_string()
            .contains("GeometryMetric :: LayoutWidth")
    );
    for (from, to) in [
        ("width: 120px", "width: root.metric()"),
        ("width: 280px; height: 20px;", ""),
        ("width: panel.layout-width;", "width: details.layout-width;"),
    ] {
        let error = analyze(&source.replace(from, to)).unwrap_err();
        assert!(error.contains("layout feedback"), "{error}");
    }
    let cross_component = r#"component App inherits Window {
        panel := Rectangle { width: 120px; }
        peer := Peer { size: panel.layout-width; }
    }
    component Peer inherits Rectangle {
        in property <length> size;
        width: root.size;
    }"#;
    assert!(
        analyze(cross_component)
            .unwrap_err()
            .contains("layout feedback")
    );
    let read_only = source.replace("width: 120px;", "layout-width: 120px;");
    assert!(analyze(&read_only).unwrap_err().contains("read-only"));
    let raw = lower(source, "geometry.xr");
    let malformed = raw.replace(
        "value = \"layout-ready\"",
        "value = \"window-bounds-width\"",
    );
    assert!(
        mlir::process(&malformed)
            .unwrap_err()
            .contains("requires a typed layout geometry")
    );
    let feedback = raw.replace(
        "owner = \"panel\", value = \"layout-width\"",
        "owner = \"details\", value = \"layout-width\"",
    );
    assert!(
        mlir::process(&feedback)
            .unwrap_err()
            .contains("layout feedback")
    );
}

#[test]
fn editor_commands_keep_order_and_verify_operands_and_purity() {
    let source = r#"component Demo inherits Window {
        callback edit-range(int, int);
        edit-range(anchor, caret) => {
            entry.focus();
            entry.set-selection-offsets(anchor, caret);
            entry.paste();
        }
        entry := TextArea { text: "任务"; }
        other := Button { text: "编辑"; }
    }"#;
    let compiled = compile(source, "editor-commands.xr").unwrap();
    let focus = compiled.mlir.find("\"xr.focus_command\"").unwrap();
    let selection = compiled
        .mlir
        .find("method = \"set-selection-offsets\"")
        .unwrap();
    let paste = compiled.mlir.find("method = \"paste\"").unwrap();
    assert!(focus < selection && selection < paste);
    assert!(
        compiled
            .rust
            .to_string()
            .contains("FocusCommandMethod :: SetSelection")
    );

    let invalid = source.replace(
        "set-selection-offsets(anchor, caret)",
        "set-selection-offsets(anchor, true)",
    );
    assert!(
        compile(&invalid, "editor-commands.xr")
            .err()
            .unwrap()
            .contains("type mismatch")
    );
    let invalid = source
        .replace("callback edit-range", "pure callback edit-range")
        .replace("entry.focus();", "");
    assert!(
        compile(&invalid, "editor-commands.xr")
            .err()
            .unwrap()
            .contains("text commands are not allowed in a pure callback")
    );

    let raw = lower(source, "editor-commands.xr");
    let invalid = raw.replace("method = \"set-selection-offsets\"", "method = \"paste\"");
    assert!(
        mlir::process(&invalid)
            .unwrap_err()
            .contains("requires set-selection-offsets(int, int)")
    );
    let invalid = raw.replace(
        "method = \"paste\", owner = \"entry\"",
        "method = \"paste\", owner = \"other\"",
    );
    assert!(
        mlir::process(&invalid)
            .unwrap_err()
            .contains("requires a statically addressed editor")
    );
}

#[test]
fn projected_row_heights_require_constant_length_fields_in_source_and_mlir() {
    let source = r#"struct Metric { label: string, height: length, id: int, parent: int }
    component Demo inherits Window {
        property <[Metric]> rows;
        property <string> field-name: "height";
        StandardTableView {
            model: root.rows;
            row-height-field: "height";
            TableColumn { field: "label"; }
        }
        StandardListView { model: root.rows; text-role: "label"; row-height-field: "height"; }
        ListView { row-height-field: "height"; for item in root.rows : Text { text: item.label; } }
        TableView {
            row-height-field: "height";
            TableColumn { title: "Name"; }
            for item in root.rows : TableRow { Text { text: item.label; } }
        }
        TreeView {
            row-height-field: "height";
            for item in root.rows : TreeRow { key: item.id; parent-key: item.parent; Text { text: item.label; } }
        }
    }"#;
    let compiled = compile(source, "height-field.xr").unwrap();
    assert_eq!(
        compiled.mlir.matches("name = \"row-height-field\"").count(),
        5
    );
    assert!(compiled.rust.to_string().contains("RowHeightProjection"));
    for (from, to, diagnostic) in [
        (
            "row-height-field: \"height\"",
            "row-height-field: \"label\"",
            "length field",
        ),
        (
            "row-height-field: \"height\"",
            "row-height-field: root.field-name",
            "constant string",
        ),
        ("model: root.rows", "model: [\"text\"]", "record model"),
    ] {
        let invalid = source.replace(from, to);
        assert!(
            compile(&invalid, "height-field.xr")
                .err()
                .unwrap()
                .contains(diagnostic)
        );
    }
    let raw = lower(source, "height-field.xr");
    let invalid = raw
        .lines()
        .map(|line| {
            if line.contains("\"xr.literal\"") && line.contains("height") {
                line.replace("height", "label")
            } else {
                line.to_owned()
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        mlir::process(&invalid)
            .unwrap_err()
            .contains("requires a length field")
    );
}

#[test]
fn table_row_heights_lower_to_a_typed_layout_effect() {
    let source = r#"component Demo inherits Window {
        other := Rectangle {}
        table := TableView {
            resizable-rows: true;
            TableColumn { title: "Name"; }
            for entry in 3 : TableRow { Text { text: "Row " + entry; } }
        }
        resize := Button { clicked() => { table.set-row-height(1, 64px); } }
    }"#;
    let compiled = compile(source, "row-height.xr").unwrap();
    assert!(compiled.mlir.contains("\"xr.set_row_height\""));
    assert!(compiled.rust.to_string().contains("RowHeightOverrides"));
    let invalid = source.replace("1, 64px", "1, true");
    assert!(
        compile(&invalid, "row-height.xr")
            .err()
            .unwrap()
            .contains("type mismatch")
    );
    let raw = lower(source, "row-height.xr");
    let invalid = raw.replace("owner = \"table\"", "owner = \"other\"");
    assert!(
        mlir::process(&invalid)
            .unwrap_err()
            .contains("requires a statically addressed table")
    );
}

#[test]
fn frozen_columns_are_typed_on_tables_only() {
    let source = r#"struct Entry { label: string }
    component Demo inherits Window {
        property <[Entry]> rows;
        StandardTableView {
            model: root.rows;
            enabled: true;
            frozen-columns: 1;
            TableColumn { field: "label"; }
        }
    }"#;
    let compiled = compile(source, "frozen-columns.xr").unwrap();
    assert!(compiled.mlir.contains("name = \"frozen-columns\""));
    let invalid = source.replace(": 1", ": true");
    assert!(
        compile(&invalid, "frozen-columns.xr")
            .err()
            .unwrap()
            .contains("type mismatch")
    );
    let raw = lower(source, "frozen-columns.xr");
    let invalid = raw.replace("name = \"enabled\"", "name = \"frozen-columns\"");
    assert!(
        mlir::process(&invalid)
            .unwrap_err()
            .contains("frozen-columns requires an integer on a table")
    );
    assert!(
        compile(
            "component Demo inherits Window { ScrollView { frozen-columns: 1; } }",
            "frozen-columns.xr"
        )
        .err()
        .unwrap()
        .contains("unknown property")
    );
}

#[test]
fn moving_table_columns_is_a_typed_effect_and_event() {
    let source = r#"component Demo inherits Window {
        outside := Rectangle {}
        table := TableView {
            movable-columns: true;
            drag-select: true;
            column-moved(column, visual) => {}
            TableColumn { title: "A"; }
            TableColumn { title: "B"; }
            for value in 3 : TableRow { Text {} Text {} }
        }
        move := Button { clicked() => { table.move-column(0, 1); } }
    }"#;
    let compiled = compile(source, "column-order.xr").unwrap();
    assert!(compiled.mlir.contains("\"xr.move_column\""));
    assert!(compiled.mlir.contains("name = \"column-moved\""));
    assert!(compiled.mlir.contains("name = \"drag-select\""));
    for (from, to, error) in [
        (
            "movable-columns: true",
            "movable-columns: 2",
            "type mismatch",
        ),
        ("drag-select: true", "drag-select: 2", "type mismatch"),
        (
            "column-moved(column, visual)",
            "column-moved(column)",
            "two int parameters",
        ),
        ("move-column(0, 1)", "move-column(0px, 1)", "type mismatch"),
    ] {
        let wrong = source.replace(from, to);
        assert!(
            compile(&wrong, "column-order.xr")
                .err()
                .unwrap()
                .contains(error)
        );
    }
    let raw = lower(source, "column-order.xr");
    let invalid = raw.replace("owner = \"table\"", "owner = \"outside\"");
    assert!(
        mlir::process(&invalid)
            .unwrap_err()
            .contains("requires a statically addressed table")
    );
}

#[test]
fn scroll_anchoring_is_a_boolean_for_virtual_views() {
    let source = "component Demo inherits Window { ListView { preserve-scroll-anchor: true; for row in 3 : Rectangle {} } }";
    let compiled = compile(source, "anchor.xr").unwrap();
    assert!(compiled.mlir.contains("name = \"preserve-scroll-anchor\""));
    assert!(compiled.rust.to_string().contains("ListRowLocator"));
    let wrong = source.replace("true", "24px");
    assert!(
        compile(&wrong, "anchor.xr")
            .err()
            .unwrap()
            .contains("type mismatch")
    );
    let raw = lower(
        "component Demo inherits Window { StandardListView { preserve-scroll-anchor: true; } }",
        "anchor.xr",
    );
    let invalid = raw.replace("kind = \"StandardListView\"", "kind = \"ScrollView\"");
    assert!(
        mlir::process(&invalid)
            .unwrap_err()
            .contains("scroll anchoring requires a boolean on a virtual view")
    );
    let wrong = "component Demo inherits Window { ScrollView { preserve-scroll-anchor: true; } }";
    assert!(
        compile(wrong, "anchor.xr")
            .err()
            .unwrap()
            .contains("unknown property")
    );
}

#[test]
fn mouse_drag_pan_is_a_boolean_viewport_binding() {
    let source = "component Demo inherits Window { ScrollView { mouse-drag-pan-enabled: true; } }";
    let compiled = compile(source, "mouse-pan.xr").unwrap();
    assert!(compiled.mlir.contains("name = \"mouse-drag-pan-enabled\""));
    let wrong_type = source.replace("true", "12px");
    assert!(
        compile(&wrong_type, "mouse-pan.xr")
            .err()
            .unwrap()
            .contains("type mismatch")
    );
    let invalid =
        lower(source, "mouse-pan.xr").replace("kind = \"ScrollView\"", "kind = \"Rectangle\"");
    assert!(
        mlir::process(&invalid)
            .unwrap_err()
            .contains("mouse drag panning requires a boolean on a viewport")
    );
}

#[test]
fn viewport_offset_bindings_require_writable_lengths_and_reject_feedback_cycles() {
    let source = r#"export component Demo inherits Window {
        in-out property <length> shared: -24px;
        first := ScrollView { content-y <=> root.shared; }
        second := ScrollView { content-y: root.shared; }
    }"#;
    let compiled = compile(source, "offset-link.xr").unwrap();
    assert!(compiled.mlir.contains("\"xr.control_link\""));
    assert!(compiled.rust.to_string().contains("bind_offset"));
    for (from, to, diagnostic) in [
        ("in-out property", "in property", "read-only"),
        (
            "<length> shared: -24px",
            "<int> shared: -24",
            "writable length",
        ),
        (
            "content-y <=> root.shared;",
            "content-x <=> root.shared; content-y <=> root.shared;",
            "distinct writable",
        ),
        ("-24px", "first.content-y", "cyclic property binding"),
        (
            "content-y: root.shared;",
            "content-y: self.content-y;",
            "cyclic property binding",
        ),
    ] {
        let invalid = source.replace(from, to);
        assert!(
            compile(&invalid, "offset-link.xr")
                .err()
                .unwrap()
                .contains(diagnostic)
        );
    }
    let raw = lower(source, "offset-link.xr");
    let invalid = raw.replacen("name = \"content-y\"", "name = \"visible-height\"", 1);
    assert!(
        mlir::process(&invalid)
            .unwrap_err()
            .contains("requires a writable property reference")
    );
}

#[test]
fn scrolled_is_a_zero_argument_viewport_event_in_source_and_mlir() {
    let source = r#"export component Demo inherits ScrollView {
        out property <length> last: 0px;
        scrolled() => { root.last = self.content-y; }
    }"#;
    let compiled = compile(source, "scrolled.xr").unwrap();
    assert!(compiled.mlir.contains("name = \"scrolled\""));
    assert!(
        compiled
            .rust
            .to_string()
            .contains("ViewportHandle :: with_scroll_notifications")
    );
    let wrong_arity = source.replace("scrolled()", "scrolled(offset)");
    assert!(
        compile(&wrong_arity, "scrolled.xr")
            .err()
            .unwrap()
            .contains("takes no parameters")
    );
    let source = "component Demo inherits Window { view := ScrollView { scrolled() => {} } }";
    let invalid = source.replace("ScrollView", "Rectangle");
    assert!(
        compile(&invalid, "scrolled.xr")
            .err()
            .unwrap()
            .contains("unknown event")
    );
    let invalid =
        lower(source, "scrolled.xr").replace("kind = \"ScrollView\"", "kind = \"Rectangle\"");
    assert!(
        mlir::process(&invalid)
            .unwrap_err()
            .contains("requires scrolled() on a scrollable element")
    );
}

#[test]
fn focus_reads_are_read_only_and_events_have_typed_reasons() {
    let source = r#"export component App inherits Window {
        out property <bool> observing: pane.focus-within;
        pane := FocusScope {
            background: self.focus-within ? #fff : #000;
            focus-within-changed(active, reason) => {}
            field := LineEdit { background: self.has-focus ? #123 : #456; }
        }
    }"#;
    let compiled = compile(source, "focus-state.xr").unwrap();
    assert!(compiled.mlir.contains("\"xr.focus_read\""));
    assert!(compiled.mlir.contains("FocusReason"));
    assert!(compiled.rust.to_string().contains("FocusHandler :: Within"));
    let bad = source.replace(
        "background: self.focus-within ? #fff : #000;",
        "focus-within: false;",
    );
    assert!(
        compile(&bad, "focus-state.xr")
            .err()
            .unwrap()
            .contains("read-only")
    );
    let raw = lower(source, "focus-state.xr");
    let bad = raw.replace("value = \"has-focus\"", "value = \"focus-within\"");
    assert!(
        mlir::process(&bad)
            .unwrap_err()
            .contains("requires a focus state property")
    );
}

#[test]
fn focus_scope_keys_have_typed_fields_and_a_verified_result() {
    let source = r#"export component App inherits Window {
        scope := FocusScope {
            capture-key-pressed(event) => {
                event.modifiers.primary && !event.repeat && event.key == "k" ? accept : reject
            }
        }
    }"#;
    let compiled = compile(source, "keys.xr").unwrap();
    assert!(compiled.mlir.contains("!xr.keyevent"));
    assert!(compiled.mlir.contains("!xr.keymodifiers"));
    assert!(compiled.mlir.contains("!xr.eventresult"));
    assert!(
        compiled
            .rust
            .to_string()
            .contains("KeyPhase :: CapturePressed")
    );
    let invalid = source.replace("event.modifiers.primary", "event.modifiers.other");
    assert!(
        compile(&invalid, "keys.xr")
            .err()
            .unwrap()
            .contains("has no field")
    );
    let invalid = source.replace("? accept : reject", "? true : false");
    assert!(
        compile(&invalid, "keys.xr")
            .err()
            .unwrap()
            .contains("type mismatch")
    );
    let raw = lower(source, "keys.xr");
    let invalid = raw.replace("value = \"primary\"", "value = \"unknown\"");
    assert!(
        mlir::process(&invalid)
            .unwrap_err()
            .contains("incorrect key event field")
    );
}

#[test]
fn wheel_enabled_requires_a_boolean_on_a_numeric_control() {
    let source = r#"
        component Demo inherits Window {
            Slider { wheel-enabled: true; }
            RangeSlider { wheel-enabled: true; }
            Dial { wheel-enabled: true; }
            SpinBox { wheel-enabled: true; }
            DoubleSpinBox { wheel-enabled: true; }
        }
    "#;
    assert!(compile(source, "control-wheel.xr").is_ok());
    for invalid in [
        source.replace("true", "1"),
        source.replace("DoubleSpinBox", "Button"),
    ] {
        assert!(compile(&invalid, "control-wheel.xr").is_err());
    }
    let raw = lower(source, "control-wheel.xr");
    assert!(
        mlir::process(&raw.replace("kind = \"DoubleSpinBox\"", "kind = \"Button\""))
            .unwrap_err()
            .contains("wheel-enabled requires a boolean on a numeric control")
    );
}

#[test]
fn wheel_handler_checks_payload_fields_result_and_orientation() {
    let source = r#"
        component Demo inherits Window {
            out property <length> moved;
            out property <bool> primary;
            wheel-area := WheelHandler {
                orientation: vertical;
                wheel(event) => {
                    root.moved = event.delta-y;
                    root.primary = event.modifiers.primary;
                    accept
                }
            }
        }
    "#;
    let result = compile(source, "wheel.xr").unwrap();
    assert!(result.mlir.contains("xr.wheel_field"));
    assert!(result.mlir.contains("!xr.wheelevent"));
    for (from, to, expected) in [
        ("wheel(event)", "wheel()", "expects one"),
        ("event.delta-y", "event.missing", "has no field"),
        (
            "                    accept",
            "                    true",
            "type mismatch",
        ),
        (
            "orientation: vertical",
            "orientation: false",
            "type mismatch",
        ),
    ] {
        let error = compile(&source.replace(from, to), "wheel.xr")
            .err()
            .unwrap();
        assert!(error.contains(expected), "{error}");
    }
    let raw = lower(source, "wheel.xr");
    let wrong_field = raw.replace("value = \"delta-y\"", "value = \"lines-y\"");
    assert!(
        mlir::process(&wrong_field)
            .unwrap_err()
            .contains("incorrect wheel event field")
    );
    let wrong_target = raw.replace("kind = \"WheelHandler\"", "kind = \"Rectangle\"");
    assert!(
        mlir::process(&wrong_target)
            .unwrap_err()
            .contains("WheelHandler requires wheel")
    );
}

#[test]
fn touch_points_repeat_with_typed_fields_and_a_read_only_source() {
    let source = r#"
        component Demo inherits Window {
            out property <[TouchPoint]> points: area.touch-points;
            area := TouchArea {
                multi-touch: true;
                for point in root.points : Rectangle {
                    x: point.x;
                    y: point.start-y;
                    visible: point.inside;
                }
            }
        }
    "#;
    let compiled = compile(source, "touch-points.xr").unwrap();
    assert!(compiled.mlir.contains("!xr.touchpoints"));
    assert!(compiled.mlir.contains("xr.touch_point_field"));
    for (from, to, expected) in [
        ("point.start-y", "point.missing", "has no field"),
        ("multi-touch: true;", "touch-points: [];", "read-only"),
    ] {
        let error = compile(&source.replace(from, to), "touch-points.xr")
            .err()
            .unwrap();
        assert!(error.contains(expected), "{error}");
    }
    let raw = lower(source, "touch-points.xr");
    assert!(
        mlir::process(&raw.replace("value = \"start-y\"", "value = \"id\""))
            .unwrap_err()
            .contains("incorrect touch point field or type")
    );
}

#[test]
fn touch_area_multitouch_fields_are_verified() {
    let source = r#"
        component Demo inherits Window {
            out property <int> id;
            out property <int> count: area.active-touch-count;
            area := TouchArea {
                multi-touch: true;
                pointer-event(event) => { root.id = event.touch-finger-id; }
            }
        }
    "#;
    let compiled = compile(source, "multi-touch.xr").unwrap();
    assert!(compiled.mlir.contains("value = \"touch-finger-id\""));
    assert!(compiled.mlir.contains("value = \"active-touch-count\""));
    let error = compile(
        &source.replace("multi-touch: true", "multi-touch: 1"),
        "multi-touch.xr",
    )
    .err()
    .unwrap();
    assert!(error.contains("type mismatch"), "{error}");
    let raw = lower(source, "multi-touch.xr");
    assert!(
        mlir::process(&raw.replace("value = \"touch-finger-id\"", "value = \"inside\""))
            .unwrap_err()
            .contains("incorrect pointer event field")
    );
}

#[test]
fn touch_area_accepted_buttons_is_a_typed_set() {
    let source = r#"
        component Demo inherits Window {
            in property <bool> right-only: true;
            out property <PointerButtons> buttons: root.right-only ? [secondary] : [];
            area := TouchArea { accepted-buttons: root.buttons; }
        }
    "#;
    let compiled = compile(source, "touch-buttons.xr").unwrap();
    assert!(compiled.mlir.contains("xr.pointer_buttons"));
    assert!(compiled.mlir.contains("!xr.pointerbuttons"));
    for (from, to, expected) in [
        ("[secondary]", "[true]", "expected pointerbutton"),
        ("TouchArea", "Button", "accepted-buttons"),
    ] {
        let error = compile(&source.replace(from, to), "touch-buttons.xr")
            .err()
            .unwrap();
        assert!(error.contains(expected), "{error}");
    }
    let raw = lower(source, "touch-buttons.xr");
    assert!(
        mlir::process(&raw.replace("kind = \"TouchArea\"", "kind = \"Button\""))
            .unwrap_err()
            .contains("accepted-buttons requires PointerButtons on TouchArea")
    );
}

#[test]
fn touch_area_press_and_hold_verifies_interval_and_callback() {
    let source = r#"
        component Demo inherits Window {
            out property <length> start-x;
            area := TouchArea {
                press-and-hold-interval: 650;
                press-and-hold() => { root.start-x = self.pressed-x; }
            }
        }
    "#;
    let compiled = compile(source, "touch-hold.xr").unwrap();
    assert!(compiled.mlir.contains("name = \"press-and-hold\""));
    assert!(compiled.mlir.contains("name = \"press-and-hold-interval\""));
    for (from, to, expected) in [
        ("650", "false", "type mismatch"),
        (
            "press-and-hold()",
            "press-and-hold(event)",
            "takes no parameters",
        ),
    ] {
        let error = compile(&source.replace(from, to), "touch-hold.xr")
            .err()
            .unwrap();
        assert!(error.contains(expected), "{error}");
    }
    let raw = lower(source, "touch-hold.xr");
    assert!(
        mlir::process(&raw.replace("kind = \"TouchArea\"", "kind = \"Rectangle\""))
            .unwrap_err()
            .contains("press-and-hold-interval requires an integer on TouchArea")
    );
}

#[test]
fn touch_area_scroll_event_uses_the_wheel_payload_and_result() {
    let source = r#"
        component Demo inherits Window {
            out property <length> delta;
            area := TouchArea {
                scroll-event(event) => {
                    root.delta = event.delta-y;
                    event.modifiers.control ? accept : reject
                }
            }
        }
    "#;
    let compiled = compile(source, "touch-scroll.xr").unwrap();
    assert!(compiled.mlir.contains("xr.wheel_field"));
    assert!(compiled.mlir.contains("!xr.wheelevent"));
    for (from, to, expected) in [
        (
            "scroll-event(event)",
            "scroll-event()",
            "expects one wheelevent",
        ),
        (
            "event.modifiers.control ? accept : reject",
            "true",
            "type mismatch",
        ),
        (
            "event.modifiers.control ? accept : reject",
            "",
            "must return EventResult",
        ),
        ("area := TouchArea", "area := WheelHandler", "unknown event"),
    ] {
        let error = compile(&source.replace(from, to), "touch-scroll.xr")
            .err()
            .unwrap();
        assert!(error.contains(expected), "{error}");
    }
    let raw = lower(source, "touch-scroll.xr");
    let wrong_target = raw.replace("kind = \"TouchArea\"", "kind = \"Rectangle\"");
    assert!(
        mlir::process(&wrong_target)
            .unwrap_err()
            .contains("TouchArea requires scroll-event(WheelEvent)")
    );
}

#[test]
fn touch_area_reads_are_typed_read_only_and_independent_of_handlers() {
    let source = r#"
        component Demo inherits Window {
            out property <bool> held: area.pressed && area.contains-press;
            out property <bool> hovered: area.has-hover;
            out property <length> position: area.mouse-x + area.pressed-y;
            area := TouchArea {}
        }
    "#;
    let compiled = compile(source, "touch-state.xr").unwrap();
    assert!(compiled.mlir.contains("xr.touch_area_read"));
    assert!(
        compile(
            &source.replace("TouchArea {}", "TouchArea { pressed: true; }"),
            "touch-state.xr",
        )
        .err()
        .unwrap()
        .contains("read-only")
    );
    let raw = lower(source, "touch-state.xr");
    assert!(
        mlir::process(&raw.replace("value = \"pressed\"", "value = \"mouse-x\""))
            .unwrap_err()
            .contains("incorrect TouchArea state property or type")
    );
    assert!(
        mlir::process(&raw.replace("kind = \"TouchArea\"", "kind = \"Rectangle\""))
            .unwrap_err()
            .contains("requires a statically addressed TouchArea")
    );
}

#[test]
fn pointer_event_checks_payload_and_handler_type() {
    let source = r#"
        component Demo inherits Window {
            out property <length> position;
            out property <bool> secondary;
            area := TouchArea {
                pointer-event(event) => {
                    root.position = event.x;
                    root.secondary = event.kind == PointerEventKind.down
                        && event.button == PointerButton.secondary && event.modifiers.shift;
                }
            }
        }
    "#;
    let compiled = compile(source, "pointer.xr").unwrap();
    assert!(compiled.mlir.contains("xr.pointer_field"));
    assert!(compiled.mlir.contains("!xr.pointerevent"));
    for (from, to, expected) in [
        ("pointer-event(event)", "pointer-event()", "expects one"),
        ("event.x", "event.missing", "has no field"),
        ("area := TouchArea", "area := Rectangle", "pointer-event"),
    ] {
        let error = compile(&source.replace(from, to), "pointer.xr")
            .err()
            .unwrap();
        assert!(error.contains(expected), "{error}");
    }
    let raw = lower(source, "pointer.xr");
    let wrong_field = raw.replace("value = \"x\"", "value = \"inside\"");
    assert!(
        mlir::process(&wrong_field)
            .unwrap_err()
            .contains("incorrect pointer event field")
    );
    let wrong_target = raw.replace("kind = \"TouchArea\"", "kind = \"Rectangle\"");
    assert!(
        mlir::process(&wrong_target)
            .unwrap_err()
            .contains("TouchArea requires pointer-event")
    );
}

#[test]
fn hover_handler_verifies_read_only_metrics_and_passive_events() {
    let source = r#"
        component Demo inherits Window {
            out property <bool> inside: area.hovered;
            out property <length> position: area.hover-x;
            out property <int> entries: 0;
            area := HoverHandler {
                blocking: true;
                entered() => { root.entries += 1; }
            }
        }
    "#;
    let output = compile(source, "hover.xr").unwrap();
    assert!(output.mlir.contains("xr.hover_read"));
    assert!(output.rust.to_string().contains("HoverHandle"));
    for (from, to, expected) in [
        ("blocking: true", "blocking: 1", "type mismatch"),
        ("blocking: true", "hovered: true", "read-only"),
        ("entered()", "entered(event)", "takes no parameters"),
    ] {
        let error = compile(&source.replace(from, to), "hover.xr")
            .err()
            .unwrap();
        assert!(error.contains(expected), "{error}");
    }
    let raw = lower(source, "hover.xr");
    let wrong_type = raw.replace("value = \"hovered\"", "value = \"hover-x\"");
    assert!(
        mlir::process(&wrong_type)
            .unwrap_err()
            .contains("incorrect hover metric")
    );
    let wrong_owner = raw.replace("owner = \"area\"", "owner = \"root\"");
    assert!(
        mlir::process(&wrong_owner)
            .unwrap_err()
            .contains("statically addressed HoverHandler")
    );
}

#[test]
fn cursor_shape_requires_a_typed_value_on_a_pointer_handler() {
    let source = r#"
        component Demo inherits Window {
            in property <CursorShape> shape: CursorShape.crosshair;
            area := HoverHandler { cursor-shape: root.shape; }
            touch := TouchArea { cursor-shape: grab; }
        }
    "#;
    let output = compile(source, "cursor.xr").unwrap();
    assert!(output.mlir.contains("!xr.cursorshape"));
    assert!(output.rust.to_string().contains("CursorShape :: Crosshair"));
    let invalid = source.replace("cursor-shape: grab", "cursor-shape: true");
    assert!(
        compile(&invalid, "cursor.xr")
            .err()
            .unwrap()
            .contains("type mismatch")
    );
    let invalid = source.replace("TouchArea", "Rectangle");
    assert!(
        compile(&invalid, "cursor.xr")
            .err()
            .unwrap()
            .contains("cursor-shape")
    );
    let raw = lower(source, "cursor.xr");
    let bad_literal = raw.replace("value = \"grab\"", "value = \"unknown-cursor\"");
    assert!(mlir::process(&bad_literal).is_err());
    let bad_owner = raw.replace("kind = \"TouchArea\"", "kind = \"Rectangle\"");
    assert!(
        mlir::process(&bad_owner)
            .unwrap_err()
            .contains("cursor-shape requires a CursorShape")
    );
}

#[test]
fn focus_scope_commands_and_initial_targets_are_checked_in_both_stages() {
    let source = r#"export component App inherits Window {
        outside := Button {}
        pane := FocusScope {
            initial-focus: query;
            focus-on-click: false;
            query := LineEdit {}
        }
        jump := Button { clicked() => { pane.focus(); } }
    }"#;
    let compiled = compile(source, "focus.xr").unwrap();
    assert!(compiled.mlir.contains("\"xr.focus_target\""));
    assert!(compiled.mlir.contains("\"xr.focus_command\""));
    assert!(
        compiled
            .rust
            .to_string()
            .contains("FocusCommandMethod :: Focus")
    );
    let raw = lower(source, "focus.xr");
    let invalid = raw.replace("target = \"query\"", "target = \"outside\"");
    assert!(
        mlir::process(&invalid)
            .unwrap_err()
            .contains("requires a descendant")
    );
    let invalid = source.replace("focus-on-click: false", "focus-on-click: 5");
    assert!(
        compile(&invalid, "focus.xr")
            .err()
            .unwrap()
            .contains("type mismatch")
    );
    let invalid = source.replace("pane.focus()", "pane.focus(1)");
    assert!(
        compile(&invalid, "focus.xr")
            .err()
            .unwrap()
            .contains("takes no arguments")
    );
    let invalid = source.replace(
        "outside := Button {}",
        "pure callback helper; helper => { pane.focus(); }",
    );
    assert!(
        compile(&invalid, "focus.xr")
            .err()
            .unwrap()
            .contains("not allowed in a pure callback")
    );
}

#[test]
fn states_use_scoped_selectors_and_reject_invalid_property_changes() {
    let source = r#"export component App inherits Window {
        in property <bool> open: false;
        panel := Rectangle { width: 48px; visible: true; }
        states [ expanded when root.open: {
            panel.width: 96px;
            panel.visible: false;
            in-out { animate * { duration: 160ms; } }
        } ]
    }"#;
    let compiled = compile(source, "states.xr").unwrap();
    assert_eq!(compiled.mlir.matches("\"xr.state_select\"").count(), 1);
    assert_eq!(compiled.mlir.matches("\"xr.state_set\"").count(), 2);
    assert_eq!(compiled.mlir.matches("\"xr.state_timing\"").count(), 2);
    let invalid_case =
        lower(source, "states.xr").replace("case_index = 0 : i64", "case_index = 4 : i64");
    assert!(
        mlir::process(&invalid_case)
            .unwrap_err()
            .contains("state case")
    );
    for (invalid, diagnostic) in [
        (
            source.replace("panel.width: 96px", "panel.width: false"),
            "type mismatch",
        ),
        (
            source.replace("panel.width: 96px", "missing.width: 96px"),
            "unknown state target",
        ),
        (
            source.replace("animate *", "animate panel.visible"),
            "animatable property",
        ),
        (
            source.replace("width: 48px;", ""),
            "requires a base binding",
        ),
        (
            source.replace(
                "width: 48px;",
                "width: 48px; animate width { duration: 10ms; }",
            ),
            "already has a property animation",
        ),
    ] {
        let error = compile(&invalid, "states.xr").err().unwrap();
        assert!(error.contains(diagnostic), "{error}");
    }
    let two_way = r#"component App inherits Window {
        in-out property <string> value;
        editor := LineEdit { text <=> root.value; }
        states [ alert when true: { editor.text: "alert"; } ]
    }"#;
    assert!(
        compile(two_way, "states.xr")
            .err()
            .unwrap()
            .contains("cannot override two-way property")
    );
}

#[test]
fn instance_states_verify_public_inputs_layout_and_composition_dependencies() {
    let source = r#"
        component Badge inherits Rectangle {
            in-out property <string> caption: "Ready";
            out property <string> reported: root.caption;
            in property <int> count: 0;
            Label { text: root.caption; }
        }
        export component App inherits Window {
            in-out property <string> caption;
            in property <bool> warning;
            badge := Badge { caption: root.caption; width: 100px; }
            states [ attention when root.warning: {
                badge.caption: "Warning";
                badge.width: 160px;
                in-out { animate * { duration: 120ms; } }
            } ]
        }
    "#;
    let compiled = compile(source, "instance-states.xr").unwrap();
    assert_eq!(compiled.mlir.matches("\"xr.instance_state\"").count(), 1);
    assert_eq!(compiled.mlir.matches("\"xr.state_timing\"").count(), 2);
    assert!(compiled.rust.to_string().contains("with_state"));
    for (invalid, expected) in [
        (
            source.replace("badge.caption:", "badge.reported:"),
            "public writable component input",
        ),
        (
            source.replace("badge.caption:", "badge.count:"),
            "type mismatch",
        ),
        (
            source.replace(
                "caption: root.caption; width",
                "caption <=> root.caption; width",
            ),
            "cannot override two-way",
        ),
        (
            source.replace("when root.warning", "when badge.reported == \"Ready\""),
            "cyclic property binding",
        ),
        (
            source.replace(
                "badge.caption: \"Warning\"",
                "badge.caption: root.layout-width > 80px ? \"wide\" : \"narrow\"",
            ),
            "layout feedback",
        ),
    ] {
        let error = compile(&invalid, "instance-states.xr").err().unwrap();
        assert!(error.contains(expected), "{error}");
    }
    let raw = lower(source, "instance-states.xr");
    let change = raw
        .lines()
        .find(|line| line.contains("\"xr.instance_state\""))
        .unwrap();
    for (name, expected) in [
        ("reported", "read-only property"),
        ("count", "property type mismatch"),
    ] {
        let replacement = change.replace("name = \"caption\"", &format!("name = \"{name}\""));
        let error = mlir::process(&raw.replace(change, &replacement)).unwrap_err();
        assert!(error.contains(expected), "{error}");
    }
}

#[test]
fn a_two_way_input_cannot_be_routed_to_an_instance_state_through_an_alias() {
    let source = r#"
        component Badge inherits Rectangle { in-out property <string> caption; }
        component Frame inherits Rectangle {
            in-out property <string> caption <=> badge.caption;
            badge := Badge { states [ active when true: { caption: "Warning"; } ] }
        }
        component App inherits Window {
            in-out property <string> caption;
            Frame { caption <=> root.caption; }
        }
    "#;
    let error = compile(source, "instance-state-link.xr").err().unwrap();
    assert!(
        error.contains("state cannot override a two-way component input"),
        "{error}"
    );
}

#[test]
fn grouped_animation_has_typed_timing_and_an_enabled_operand() {
    let source = r#"export component App inherits Window {
        in property <bool> motion: true;
        Rectangle {
            x: 20px; width: 120px;
            animate x, width { duration: 400ms; delay: 120ms; easing: cubic-bezier(0.2, -0.3, 0.8, 1.2); enabled: root.motion; }
        }
    }"#;
    let compiled = compile(source, "animation.xr").unwrap();
    assert_eq!(compiled.mlir.matches("\"xr.animate\"").count(), 2);
    assert!(compiled.mlir.contains("curve = array<f64:"));
    assert!(compiled.mlir.contains("delay = 120 : i64"));
    assert!(compiled.rust.to_string().contains("Animation :: new"));
    let raw = lower(source, "animation.xr");
    let invalid = raw.replace("delay = 120 : i64", "delay = 60001 : i64");
    assert!(
        mlir::process(&invalid)
            .unwrap_err()
            .contains("requires a delay")
    );
    let invalid = raw.replace("curve = array<f64: ", "curve = array<f64: 1.1, ");
    assert!(
        mlir::process(&invalid)
            .unwrap_err()
            .contains("curve coordinates")
    );
    for (invalid, diagnostic) in [
        (
            source.replace("enabled: root.motion", "enabled: 1"),
            "type mismatch",
        ),
        (
            source.replace("delay: 120ms", "duration: 120ms"),
            "duplicate animation option",
        ),
        (
            source.replace("animate x, width", "animate x, x"),
            "duplicate animation for",
        ),
        (
            source.replace("0.8, 1.2", "1.1, 1.2"),
            "x coordinates in [0, 1]",
        ),
    ] {
        assert!(
            compile(&invalid, "animation.xr")
                .err()
                .unwrap()
                .contains(diagnostic)
        );
    }
}

#[test]
fn drag_areas_verify_action_types_and_generate_typed_event_routes() {
    let source = r#"export component App inherits Window {
        out property <DragAction> result: none;
        source := DragArea {
            data: "ticket"; drag-key: "tasks"; allow-move: true;
            drag-finished(action) => { root.result = action; }
        }
        target := DropArea {
            drop-action: move;
            dropped(data, action) => { root.result = action; }
        }
    }"#;
    let compiled = compile(source, "drag.xr").unwrap();
    assert!(compiled.mlir.contains("!xr.dragaction"));
    assert!(
        compiled
            .rust
            .to_string()
            .contains(r#"("dropped" , :: xrack_runtime :: DragArguments :: Data"#)
    );
    let invalid = source.replace("drop-action: move", "drop-action: true");
    assert!(
        compile(&invalid, "drag.xr")
            .err()
            .unwrap()
            .contains("type mismatch")
    );
    let raw = lower(source, "drag.xr");
    let invalid = raw.replace("name = \"drop-action\"", "name = \"keys\"");
    assert!(
        mlir::process(&invalid)
            .unwrap_err()
            .contains("incorrect drag-and-drop property type")
    );
    let invalid = raw.replace("name = \"dropped\"", "name = \"exited\"");
    assert!(
        mlir::process(&invalid)
            .unwrap_err()
            .contains("invalid drag-and-drop event or signature")
    );
}

#[test]
fn native_file_drop_uses_a_typed_string_list_event() {
    let source = r#"export component App inherits Window {
        out property <[string]> selection: [];
        inbox := DropArea {
            accept-files: true;
            file-extensions: ["png", "jpg"];
            files-dropped(paths) => { root.selection = paths; }
        }
    }"#;
    let compiled = compile(source, "files.xr").unwrap();
    assert!(compiled.mlir.contains("files-dropped"));
    assert!(
        compiled
            .rust
            .to_string()
            .contains(r#"("files-dropped" , :: xrack_runtime :: DragArguments :: Files"#)
    );
    let invalid = source.replace("accept-files: true", "accept-files: 1");
    assert!(
        compile(&invalid, "files.xr")
            .err()
            .unwrap()
            .contains("type mismatch")
    );
    let raw = lower(source, "files.xr");
    let invalid = raw.replace("name = \"file-extensions\"", "name = \"drop-action\"");
    assert!(
        mlir::process(&invalid)
            .unwrap_err()
            .contains("incorrect drag-and-drop property type")
    );
    let invalid = raw.replace("name = \"files-dropped\"", "name = \"dropped\"");
    assert!(
        mlir::process(&invalid)
            .unwrap_err()
            .contains("invalid drag-and-drop event or signature")
    );
}

#[test]
fn window_and_control_fonts_keep_typed_bindings_through_mlir() {
    let source = r#"
        export component App inherits Window {
            in property <string> family: "monospace";
            default-font-family: root.family;
            default-font-size: 19px;
            default-font-weight: 400;
            default-font-italic: true;
            Label { text: "sample"; font-weight: 700; font-italic: false; }
            LineEdit { font-family: "Arial"; }
        }
    "#;
    let output = compile(source, "fonts.xr").unwrap();
    for property in [
        "default-font-family",
        "default-font-size",
        "default-font-weight",
        "default-font-italic",
        "font-family",
        "font-weight",
        "font-italic",
    ] {
        assert!(output.mlir.contains(&format!("name = \"{property}\"")));
    }
    let wrong = source.replace("font-weight: 700", "font-weight: true");
    assert!(
        compile(&wrong, "fonts.xr")
            .err()
            .unwrap()
            .contains("type mismatch")
    );

    let raw = lower(source, "fonts.xr");
    let wrong = raw.replace("name = \"font-italic\"", "name = \"font-family\"");
    assert!(
        mlir::process(&wrong)
            .unwrap_err()
            .contains("incorrect text font property or type")
    );
    let wrong = raw.replace("name = \"font-weight\"", "name = \"default-font-weight\"");
    assert!(
        mlir::process(&wrong)
            .unwrap_err()
            .contains("incorrect window font property or type")
    );
}

#[test]
fn translation_catalogs_validate_imported_calls_and_track_build_inputs() {
    use std::{fs, path::PathBuf};
    use xrack_compiler::{compile_with_inputs, export_mlir, parse_document, prepare_mlir_preview};

    let directory = std::env::var_os("TEST_TMPDIR")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join(format!("xrack-translations-{}", std::process::id()));
    let catalog = directory.join("i18n/catalog.toml");
    let english = directory.join("i18n/en.ftl");
    let chinese = directory.join("i18n/zh.ftl");
    let child = directory.join("child.xr");
    fs::create_dir_all(catalog.parent().unwrap()).unwrap();
    fs::write(
        &catalog,
        "default = \"en-US\"\n[locales]\nen-US = \"en.ftl\"\nzh-CN = \"zh.ftl\"\n",
    )
    .unwrap();
    fs::write(&english, "title = Example\ncount = { $count } items\n").unwrap();
    fs::write(&chinese, "count = 共 { $count } 条\n").unwrap();
    fs::write(
        &child,
        r#"export component Child inherits Text { text: @tr("count", count: 3); }"#,
    )
    .unwrap();
    let entry = directory.join("app.xr");
    let filename = entry.to_str().unwrap();
    let source = r#"translations "i18n/catalog.toml";
        import { Child } from "child.xr";
        export component App inherits Window { title: @tr("title"); Child {} }"#;
    let assets = [catalog.clone(), english.clone(), chinese.clone()];
    let (rust, inputs) = compile_with_inputs(source, filename, None, Some(&assets)).unwrap();
    assert!(rust.to_string().contains("共 { $count } 条"));
    for path in [&catalog, &english, &chinese, &child] {
        assert!(inputs.contains(&path.canonicalize().unwrap()), "{path:?}");
    }
    let document = parse_document(source, filename, None).unwrap();
    let mlir_text = export_mlir(&document, filename, Some(&assets)).unwrap();
    let exported = prepare_mlir_preview(&mlir_text, filename).unwrap();
    assert_eq!(exported.components, ["App"]);
    let restricted = export_mlir(&document, filename, Some(&assets[..2]))
        .unwrap_err()
        .to_string();
    let error = compile_with_inputs(source, filename, None, Some(&assets[..2]))
        .unwrap_err()
        .to_string();
    assert_eq!(restricted, error);
    assert!(
        error.contains("zh.ftl") && error.contains("not declared"),
        "{error}"
    );

    fs::write(&chinese, "count = { other }\nother = { $missing }\n").unwrap();
    let error = compile_with_inputs(source, filename, None, Some(&assets))
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("child.xr") && error.contains("requires argument 'missing'"),
        "{error}"
    );

    fs::write(&chinese, "count = 共 { $count } 条\n").unwrap();
    let unreachable = source.replace("Child {}", "if false : Text { text: @tr(\"missing\"); }");
    let error = compile_with_inputs(&unreachable, filename, None, Some(&assets))
        .unwrap_err()
        .to_string();
    let document = parse_document(&unreachable, filename, None).unwrap();
    let exported = export_mlir(&document, filename, Some(&assets)).unwrap_err();
    assert_eq!(exported.to_string(), error);
    let raw = xrack_compiler::mlir::lower(&document, filename).unwrap();
    let from_text = prepare_mlir_preview(&raw, filename).err().unwrap();
    assert_eq!(from_text.to_string(), error);
    assert!(
        error.contains("default language") && error.contains("'missing'"),
        "{error}"
    );
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn font_imports_are_verified_in_the_declaring_module_and_embedded_once() {
    use std::{fs, path::PathBuf};
    use xrack_compiler::{check_resources, compile_with_inputs, generate_rust, parse_document};

    let directory = std::env::var_os("TEST_TMPDIR")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join(format!("xrack-font-import-{}", std::process::id()));
    let dependency = directory.join("parts/font.xr");
    let font = directory.join("parts/demo.ttc");
    fs::create_dir_all(font.parent().unwrap()).unwrap();
    fs::write(
        &dependency,
        "import \"demo.ttc\";\nexport component FontLabel inherits Label { text: \"XRACK\"; }\n",
    )
    .unwrap();
    let font_data = include_bytes!("../examples/layout/fonts/demo.ttc");
    fs::write(&font, font_data).unwrap();
    let source = r#"import { FontLabel } from "parts/font.xr";
        import "./parts/demo.ttc";
        export component App inherits Window { sample := FontLabel {} }
    "#;
    let filename = directory.join("app.xr");
    let filename = filename.to_str().unwrap();
    let doc = parse_document(source, filename, Some(std::slice::from_ref(&dependency))).unwrap();
    let module = xrack_compiler::lower(&doc, filename).unwrap();
    let imports: Vec<_> = module
        .operations()
        .filter(|op| op.name() == "xr.font")
        .collect();
    assert_eq!(imports.len(), 2);
    assert_eq!(imports[0].attr("path").unwrap(), "parts/demo.ttc");
    assert_eq!(imports[0].source_file(), dependency.to_str());
    let rust = generate_rust(&module, filename, Some(std::slice::from_ref(&font)))
        .unwrap()
        .to_string();
    assert_eq!(rust.matches("b\"ttcf").count(), 1);
    let (_, inputs) = compile_with_inputs(source, filename, None, None).unwrap();
    assert_eq!(
        inputs,
        [
            PathBuf::from(filename),
            dependency.canonicalize().unwrap(),
            font.canonicalize().unwrap(),
        ]
        .into_iter()
        .collect()
    );
    let missing = check_resources(&module, filename, Some(&[]))
        .unwrap_err()
        .to_string();
    assert!(missing.contains("parts/font.xr:1:1") && missing.contains("not declared"));

    fs::write(&font, b"ttcf\0\x01\0\0\xFF\xFF\xFF\xFF").unwrap();
    let invalid = check_resources(&module, filename, None)
        .unwrap_err()
        .to_string();
    assert!(invalid.contains("1 and 256 faces"), "{invalid}");
    fs::write(&font, b"invalid font").unwrap();
    let invalid = check_resources(&module, filename, None)
        .unwrap_err()
        .to_string();
    assert!(
        invalid.contains("parts/font.xr:1:1") && invalid.contains("invalid font face 0"),
        "{invalid}"
    );
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn standard_table_checks_column_fields_in_source_and_mlir() {
    let source = r#"
        export struct Part { device-name: string, header-label: string, count: int, time: Time, color: color }
        export component App inherits Window {
            in property <[Part]> parts;
            in property <string> field-name: "device-name";
            in-out property <int> current: -1;
            view := StandardTableView {
                model: root.parts;
                current-row <=> root.current;
                row-header-width: 72px;
                row-header-field: "header-label";
                TableColumn { title: "Device"; field: "device-name"; }
                TableColumn { title: "Count"; field: "count"; }
                TableColumn { title: "Time"; field: "time"; }
            }
        }
    "#;
    let output = compile(source, "standard-table.xr").unwrap();
    assert!(output.mlir.contains("kind = \"StandardTableView\""));
    let rust = output.rust.to_string();
    assert!(rust.contains("render_standard_table_rows"));
    assert!(rust.contains("TableField :: cell_text (& item . device_name)"));
    assert!(rust.contains("TableField :: cell_text (& item . header_label)"));
    for (replacement, expected) in [
        ("\"missing\"", "has no field `missing`"),
        ("root.field-name", "requires a constant string field name"),
        (
            "\"color\"",
            "requires a string, int, float, bool, Date or Time field",
        ),
    ] {
        for (role, name) in [
            ("field", "device-name"),
            ("row-header-field", "header-label"),
        ] {
            let invalid = source.replace(
                &format!("{role}: \"{name}\""),
                &format!("{role}: {replacement}"),
            );
            assert!(
                compile(&invalid, "standard-table.xr")
                    .err()
                    .unwrap()
                    .contains(expected)
            );
        }
    }
    let raw = lower(source, "standard-table.xr");
    assert!(
        mlir::process(&raw.replacen("header-label", "renamed", 1))
            .unwrap_err()
            .contains("record has no field header-label")
    );
    assert!(
        mlir::process(&raw.replacen("device-name", "renamed", 1))
            .unwrap_err()
            .contains("record has no field device-name")
    );
    assert!(
        mlir::process(&raw.replacen("name = \"field\"", "name = \"ignored\"", 1))
            .unwrap_err()
            .contains("requires exactly one field")
    );
}

#[test]
fn table_row_header_text_can_read_the_repeated_item() {
    let source = r#"
        component App inherits Window {
            property <[string]> rows: ["First"];
            TableView {
                row-header-width: 64px;
                TableColumn { title: "Value"; }
                for item in root.rows : TableRow {
                    row-header-text: item;
                    TableCell { text: item; }
                }
            }
        }
    "#;
    let compiled = compile(source, "row-header.xr").unwrap();
    assert!(compiled.mlir.contains("name = \"row-header-text\""));
    let invalid = source.replace("row-header-text: item", "row-header-text: 4");
    assert!(compile(&invalid, "row-header.xr").is_err());
}

#[test]
fn move_row_lowers_to_a_verified_table_command() {
    let source = r#"
        component App inherits Window {
            property <int> destination: -1;
            callback move-one();
            move-one() => { table.move-row(0, 1); }
            table := TableView {
                row-header-width: 48px;
                movable-rows: true;
                row-moved(model-row, from, to) => { root.destination = to; }
                TableColumn { title: "Name"; }
                for item in ["A", "B"] : TableRow { TableCell { text: item; } }
            }
        }
    "#;
    let output = compile(source, "move-row.xr").unwrap();
    assert!(output.mlir.contains("xr.move_row"));
    assert!(output.rust.to_string().contains("TableRowOrder"));
    let invalid = source.replace("table.move-row(0, 1)", "table.move-row(0, 1px)");
    assert!(compile(&invalid, "move-row.xr").is_err());
    let invalid = source.replace("row-moved(model-row, from, to)", "row-moved(from, to)");
    assert!(compile(&invalid, "move-row.xr").is_err());
}

#[test]
fn nested_record_fields_are_typed_and_layouts_must_be_acyclic() {
    let source = r#"
        struct Entry { details: Details }
        struct Details { label: string, units: int }
        component App inherits Window {
            in property <[Entry]> rows: [Entry { details: Details { label: "one", units: 1 } }];
            StandardListView { model: root.rows; text-role: "details.label"; }
        }
    "#;
    let compiled = compile(source, "nested.xr").unwrap();
    assert!(compiled.rust.to_string().contains("details . label"));
    for (invalid, expected) in [
        (
            source.replace("details: Details", "details: Missing"),
            "unknown record type `Missing`",
        ),
        (
            source.replace("label: string, units: int", "owner: Entry"),
            "cyclic record layout",
        ),
        (
            source.replace("details.label", "details.units"),
            "requires a string field",
        ),
        (
            source.replace("details.label", "details.label."),
            "requires a string field",
        ),
    ] {
        let error = compile(&invalid, "nested.xr").err().unwrap();
        assert!(error.contains(expected), "{error}");
    }

    let raw = lower(source, "nested.xr");
    for (old, new, expected) in [
        (
            r#"\22ty\22:\22Details\22"#,
            r#"\22ty\22:\22Missing\22"#,
            "unknown record type Missing",
        ),
        (
            r#"\22ty\22:\22Details\22"#,
            r#"\22ty\22:\22Entry\22"#,
            "cyclic record layout",
        ),
        ("details.label", "details.label.", "requires a string field"),
    ] {
        assert!(raw.contains(old), "{raw}");
        let error = mlir::process(&raw.replacen(old, new, 1)).unwrap_err();
        assert!(error.contains(expected), "{error}");
    }
}

#[test]
fn popup_window_checks_placement_open_links_and_commands() {
    let source = r#"
        export component App inherits Window {
            in-out property <bool> shown: false;
            trigger := Button {
                clicked() => { popover.show(); }
                popover := PopupWindow {
                    open <=> root.shown;
                    placement: below;
                    anchor-gap: 8px;
                    close-button := Button { clicked() => { popover.close(); } }
                }
            }
        }
    "#;
    let result = compile(source, "popup.xr").unwrap();
    assert!(result.mlir.contains("!xr.popupplacement"));
    assert!(result.mlir.contains("xr.popup_command"));
    let raw = lower(source, "popup.xr");
    let invalid = raw.replace("name = \"anchor-gap\"", "name = \"placement\"");
    assert!(
        mlir::process(&invalid)
            .unwrap_err()
            .contains("incorrect PopupWindow property type")
    );
    let invalid = raw.replace("owner = \"popover\"", "owner = \"root\"");
    assert!(
        mlir::process(&invalid)
            .unwrap_err()
            .contains("requires a PopupWindow owner")
    );
    assert!(
        compile("export component App inherits PopupWindow {}", "popup.xr")
            .err()
            .unwrap()
            .contains("requires a visual owner")
    );
}

#[test]
fn text_and_label_layout_options_are_typed_in_mlir() {
    let source = r#"
        export component App inherits Window {
            Label {
                text: "A👩‍💻B";
                horizontal-alignment: end;
                vertical-alignment: bottom;
                overflow: elide;
                wrap: char-wrap;
                max-lines: 2;
            }
            Text { wrap: no-wrap; overflow: clip; }
        }
    "#;
    let output = compile(source, "label.xr").unwrap();
    for kind in [
        "Label",
        "!xr.textoverflow",
        "!xr.texthorizontalalignment",
        "!xr.textverticalalignment",
    ] {
        assert!(output.mlir.contains(kind), "missing {kind}");
    }
    let raw = lower(source, "label.xr");
    let wrong = raw.replace(
        "name = \"horizontal-alignment\"",
        "name = \"vertical-alignment\"",
    );
    assert!(
        mlir::process(&wrong)
            .unwrap_err()
            .contains("incorrect text layout property type")
    );
    let wrong = source.replace("max-lines: 2", "max-lines: true");
    assert!(
        compile(&wrong, "label.xr")
            .err()
            .unwrap()
            .contains("type mismatch")
    );
}

#[test]
fn flexbox_layout_keeps_typed_direction_wrap_and_line_alignment() {
    let source = r#"
        export component App inherits Window {
            in property <FlexboxLayoutDirection> direction: row-reverse;
            FlexboxLayout {
                flex-direction: root.direction;
                flex-wrap: wrap-reverse;
                cross-axis-line-alignment: space-evenly;
                spacing-horizontal: 8px;
                Button { preferred-width: 100px; }
            }
        }
    "#;
    let result = compile(source, "flex.xr").unwrap();
    assert!(result.mlir.contains("!xr.flexboxlayoutdirection"));
    assert!(result.mlir.contains("!xr.flexboxlayoutwrap"));
    let raw = lower(source, "flex.xr");
    let invalid = raw.replace(
        "name = \"spacing-horizontal\"",
        "name = \"cross-axis-line-alignment\"",
    );
    assert!(
        mlir::process(&invalid)
            .unwrap_err()
            .contains("incorrect FlexboxLayout property type")
    );
    let invalid = source.replace("wrap-reverse", "horizontal");
    assert!(compile(&invalid, "flex.xr").is_err());
}

#[test]
fn timer_validates_nonvisual_properties_and_effectful_commands() {
    let source = r#"
        export component App inherits Window {
            in-out property <bool> active: false;
            out property <int> hits: 0;
            timer := Timer {
                running <=> root.active;
                interval: 80;
                triggered() => { root.hits += 1; self.stop(); }
            }
            start := Button { clicked() => { timer.restart(); } }
        }
    "#;
    let output = compile(source, "timer.xr").unwrap();
    assert!(output.mlir.contains("xr.timer_command"));
    assert!(output.rust.to_string().contains("TimerSlot"));
    let raw = lower(source, "timer.xr");
    let invalid = raw.replace("command = \"restart\"", "command = \"unknown\"");
    assert!(
        mlir::process(&invalid)
            .unwrap_err()
            .contains("requires start, stop, or restart")
    );
    let invalid = raw.replace("kind = \"Timer\"", "kind = \"Rectangle\"");
    assert!(mlir::process(&invalid).is_err());
    let invalid = source.replace("interval: 80;", "width: 80px;");
    assert!(
        compile(&invalid, "timer.xr")
            .err()
            .unwrap()
            .contains("unknown property `Timer.width`")
    );
    let invalid = source.replace("timer.restart()", "timer.restart(1)");
    assert!(
        compile(&invalid, "timer.xr")
            .err()
            .unwrap()
            .contains("takes no arguments")
    );
}

#[test]
fn swipe_delegate_checks_slots_and_the_typed_side() {
    let source = r#"
        export component App inherits Window {
            in-out property <SwipeSide> side: closed;
            row := SwipeDelegate {
                open-side <=> root.side;
                SwipeContent { Text { text: "Item"; } }
                SwipeRight { Button { text: "Archive"; } }
                changed(side) => { root.side = side; }
            }
        }
    "#;
    let output = compile(source, "swipe-delegate.xr").unwrap();
    assert!(output.mlir.contains("!xr.swipeside"));
    let raw = lower(source, "swipe-delegate.xr");
    let missing = raw.replace("kind = \"SwipeRight\"", "kind = \"SwipeContent\"");
    assert!(
        mlir::process(&missing)
            .unwrap_err()
            .contains("duplicate SwipeContent slot")
    );
    let invalid = source.replace("SwipeRight { Button { text: \"Archive\"; } }", "");
    assert!(
        compile(&invalid, "swipe-delegate.xr")
            .err()
            .unwrap()
            .contains("at least one action slot")
    );
}

#[test]
fn standard_buttons_use_a_verified_enum_and_button_box() {
    let source = r#"
        export component App inherits Window {
            in property <StandardButtonKind> primary-kind: save-all;
            out property <int> applies: 0;
            dialog := Dialog {
                open: true;
                applied() => { root.applies += 1; }
                discarded() => { root.applies = 0; }
                DialogButtonBox {
                    StandardButton { kind: root.primary-kind; primary: true; }
                    StandardButton { kind: discard; }
                    DialogButton { role: reject; }
                }
            }
        }
    "#;
    let result = compile(source, "buttons.xr").unwrap();
    assert!(result.mlir.contains("!xr.standardbuttonkind"));
    assert!(result.mlir.contains("kind = \"DialogButtonBox\""));
    assert!(
        result
            .rust
            .to_string()
            .contains("StandardButtonKind :: SaveAll")
    );
    let raw = lower(source, "buttons.xr");
    let error =
        mlir::process(&raw.replace("value = \"discard\"", "value = \"unknown\"")).unwrap_err();
    assert!(
        error.contains("literal spelling does not match result type")
            && error.contains("standardbuttonkind"),
        "{error}"
    );
    assert!(
        mlir::process(&raw.replace("kind = \"DialogButton\"", "kind = \"Rectangle\""))
            .unwrap_err()
            .contains("requires direct button children")
    );
    assert!(
        compile(
            &source.replace("StandardButton { kind: discard; }", "Text {}"),
            "buttons.xr"
        )
        .err()
        .unwrap()
        .contains("requires direct button children")
    );
}

#[test]
fn date_picker_popup_verifies_date_binding_and_modal_events() {
    let source = r#"
        export component App inherits Window {
            in-out property <Date> selected: Date { year: 2024, month: 2, day: 29 };
            in-out property <bool> picker-open: true;
            out property <int> day: 0;
            picker := DatePickerPopup {
                open <=> root.picker-open;
                date <=> root.selected;
                minimum-date: Date { year: 2024, month: 2, day: 5 };
                accepted(chosen) => { root.day = chosen.day; }
                canceled() => { root.day = 0; }
            }
        }
    "#;
    let output = compile(source, "date-picker.xr").unwrap();
    assert!(output.mlir.contains("kind = \"DatePickerPopup\""));
    assert!(output.rust.to_string().contains("DatePopupState"));
    let raw = lower(source, "date-picker.xr");
    assert!(
        mlir::process(&raw.replace("name = \"accepted\"", "name = \"changed\""))
            .unwrap_err()
            .contains("invalid picker event or signature")
    );
    assert!(
        mlir::process(&raw.replace("name = \"minimum-date\"", "name = \"week-start\""))
            .unwrap_err()
            .contains("incorrect Calendar property type")
    );
    assert!(
        compile(
            &source.replace("accepted(chosen)", "accepted(chosen, extra)"),
            "date-picker.xr"
        )
        .err()
        .unwrap()
        .contains("expects one date parameter")
    );
}

#[test]
fn time_picker_verifies_types_events_and_time_fields_in_mlir() {
    let source = r#"
        export component App inherits Window {
            in-out property <Time> chosen: Time { hour: 12, minute: 32 };
            in-out property <bool> shown: false;
            out property <int> seconds: root.chosen.second;
            inline := TimePicker { time <=> root.chosen; changed(value) => { root.seconds = value.second; } }
            popup := TimePickerPopup {
                open <=> root.shown;
                time <=> root.chosen;
                accepted(value) => { root.seconds = value.second; }
                canceled => { root.seconds = 0; }
            }
        }
    "#;
    let raw = lower(source, "time.xr");
    for text in ["!xr.time", "xr.make_time", "xr.time_field"] {
        assert!(raw.contains(text), "missing {text}");
    }
    assert!(
        compile(source, "time.xr")
            .unwrap()
            .rust
            .to_string()
            .contains("TimePopupState")
    );
    assert!(
        mlir::process(&raw.replace("value = \"second\"", "value = \"day\""))
            .unwrap_err()
            .contains("requires hour, minute, or second")
    );
    assert!(
        compile(&source.replace("accepted(value)", "accepted()"), "time.xr")
            .err()
            .unwrap()
            .contains("expects one time parameter")
    );
    assert!(
        compile(
            &source.replace(
                "Time { hour: 12, minute: 32 }",
                "Date { year: 2024, month: 1, day: 1 }"
            ),
            "time.xr"
        )
        .err()
        .unwrap()
        .contains("type mismatch")
    );
}

#[test]
fn combo_enabled_role_requires_a_constant_nested_boolean_field() {
    let source = r#"
        struct Access { allowed: bool }
        struct OptionRow { title: string, access: Access }
        component App inherits Window {
            in property <[OptionRow]> options;
            in property <string> role: "access.allowed";
            ComboBox { model: root.options; text-role: "title"; enabled-role: "access.allowed"; }
        }
    "#;
    let compiled = compile(source, "enabled-role.xr").unwrap();
    assert!(
        compiled
            .rust
            .to_string()
            .contains("ComboItems :: with_enabled")
    );
    for (from, to, expected) in [
        (
            "enabled-role: \"access.allowed\"",
            "enabled-role: \"title\"",
            "requires a bool field",
        ),
        (
            "enabled-role: \"access.allowed\"",
            "enabled-role: root.role",
            "requires a constant string",
        ),
        (
            "model: root.options",
            "model: [\"One\"]",
            "roles require a record model",
        ),
    ] {
        let error = compile(&source.replace(from, to), "enabled-role.xr")
            .err()
            .unwrap();
        assert!(error.contains(expected), "{error}");
    }
    let raw = lower(source, "enabled-role.xr").replace("access.allowed", "title");
    assert!(
        mlir::process(&raw)
            .unwrap_err()
            .contains("enabled-role requires a bool field")
    );
}

#[test]
fn combo_roles_and_business_value_types_are_verified() {
    let source = r#"
        struct OptionRow { title: string, key: int }
        export component App inherits Window {
            in-out property <[OptionRow]> rows: [OptionRow { title: "One", key: 1 }];
            in-out property <int> selection: 1;
            choice := ComboBox {
                model: root.rows;
                text-role: "title";
                value-role: "key";
                current-value <=> root.selection;
                value-selected(value) => { root.selection = value; }
            }
        }
    "#;
    let compiled = compile(source, "combo_roles.xr").unwrap();
    assert!(compiled.mlir.contains("!xr.recordlist"));
    assert!(compiled.rust.to_string().contains("ComboItems :: new"));
    let raw = lower(source, "combo_roles.xr");
    let bad_role = raw.replace(r#"value = "\22key\22""#, r#"value = "\22title\22""#);
    assert!(
        mlir::process(&bad_role)
            .unwrap_err()
            .contains("ComboBox current-value requires string")
    );
    assert!(
        compile(
            &source.replace("text-role: \"title\"", "text-role: \"key\""),
            "combo_roles.xr"
        )
        .is_err()
    );
    assert!(
        compile(
            &source.replace("value-role: \"key\"", "value-role: \"unknown\""),
            "combo_roles.xr"
        )
        .is_err()
    );
}

#[test]
fn tab_widget_repeats_pages_and_exposes_a_read_only_count() {
    let source = r#"
        component Editor inherits Rectangle { in property <string> title; Text { text: root.title; } }
        export component App inherits Window {
            in-out property <int> selected: 1;
            out property <int> total: tabs.count;
            tabs := TabWidget {
                current-index <=> root.selected;
                for title in ["one", "two"] : Tab {
                    title: title;
                    Editor { title: title; }
                }
            }
        }
    "#;
    let compiled = compile(source, "model_tabs.xr").unwrap();
    assert!(compiled.mlir.contains("xr.navigation_count"));
    assert!(
        compiled
            .rust
            .to_string()
            .contains("append_navigation_pages")
    );
    let invalid = source.replace("current-index <=> root.selected;", "count: 3;");
    assert!(
        compile(&invalid, "model_tabs.xr")
            .err()
            .unwrap()
            .to_string()
            .contains("read-only")
    );
    let mixed = source.replace(
        "current-index <=> root.selected;",
        "current-index: 0; Tab {}",
    );
    assert!(
        compile(&mixed, "model_tabs.xr")
            .err()
            .unwrap()
            .to_string()
            .contains("only option")
    );
    let cyclic = source
        .replace(r#"["one", "two"]"#, "root.total")
        .replace("title: title;", r#"title: "Tab " + title;"#);
    assert!(
        compile(&cyclic, "model_tabs.xr")
            .err()
            .unwrap()
            .to_string()
            .contains("cycl")
    );
}

#[test]
fn table_selection_gestures_and_live_state_are_typed_and_read_only() {
    let source = r#"
        export component App inherits Window {
            in-out property <SelectionGesture> gesture: press-and-hold;
            property <[string]> rows: ["one", "two"];
            out property <bool> active: grid.selection-active;
            out property <bool> dragging: grid.selection-dragging;
            grid := TableView {
                selection-gesture: root.gesture;
                selection-hold-interval: 360;
                drag-select: true;
                multi-select: true;
                TableColumn { title: "Label"; }
                for row in root.rows : TableRow { TableCell { text: row; } }
            }
        }
    "#;
    let compiled = compile(source, "selection_gesture.xr").unwrap();
    assert!(compiled.mlir.contains("!xr.selectiongesture"));
    assert!(compiled.rust.to_string().contains("dragging"));
    for (before, after, message) in [
        (
            "selection-gesture: root.gesture;",
            "selection-gesture: true;",
            "expected selectiongesture",
        ),
        (
            "selection-hold-interval: 360;",
            "selection-hold-interval: 360px;",
            "expected int",
        ),
        ("drag-select: true;", "selection-active: true;", "read-only"),
    ] {
        let error = compile(&source.replace(before, after), "selection_gesture.xr")
            .err()
            .unwrap()
            .to_string();
        assert!(error.contains(message), "{error}");
    }
}

#[test]
fn selection_handle_parts_are_static_unique_and_positioned_by_the_table() {
    let source = r#"
        export component App inherits Window {
            property <[string]> labels: ["one", "two"];
            TableView {
                drag-select: true;
                multi-select: true;
                TableColumn { title: "Label"; }
                for row in root.labels : TableRow { TableCell { text: row; } }
                SelectionStartHandle { width: 12px; background: #ee9922; }
                SelectionEndHandle { height: 14px; }
            }
        }
    "#;
    let compiled = compile(source, "selection_handles.xr").unwrap();
    assert!(compiled.mlir.contains("SelectionStartHandle"));
    assert!(
        compiled
            .rust
            .to_string()
            .contains("set_table_selection_part")
    );
    let duplicate = source.replace("SelectionEndHandle", "SelectionStartHandle");
    assert!(
        compile(&duplicate, "selection_handles.xr")
            .err()
            .unwrap()
            .to_string()
            .contains("at most one")
    );
    let misplaced = source.replace("width: 12px;", "x: 12px;");
    assert!(
        compile(&misplaced, "selection_handles.xr")
            .err()
            .unwrap()
            .to_string()
            .contains("property")
    );
    let raw = lower(source, "selection_handles.xr");
    let error =
        mlir::process(&raw.replace("SelectionEndHandle", "SelectionStartHandle")).unwrap_err();
    assert!(
        error.contains("at most one SelectionStartHandle"),
        "{error}"
    );
}

#[test]
fn model_edits_verify_the_property_row_type_and_callback_effects() {
    let source = r#"
        export component App inherits Window {
            in-out property <[int]> rows: [1, 2];
            property <[int]> alias <=> root.rows;
            callback change();
            change() => { Model.append(root.alias, 3); }
        }
    "#;
    let compiled = compile(source, "model_edit.xr").unwrap();
    assert!(compiled.mlir.contains("xr.model_edit"));
    assert!(compiled.rust.to_string().contains("ModelEdit :: Append"));
    for (before, after, message) in [
        (
            "Model.append(root.alias, 3)",
            "Model.append([1, 2], 3)",
            "writable model property",
        ),
        (
            "Model.append(root.alias, 3)",
            r#"Model.append(root.alias, "wrong")"#,
            "expected int",
        ),
        (
            "Model.append(root.alias, 3)",
            "Model.append(root.alias)",
            "expects 2 arguments",
        ),
        ("callback change", "pure callback change", "pure callback"),
    ] {
        let error = compile(&source.replace(before, after), "model_edit.xr")
            .err()
            .unwrap()
            .to_string();
        assert!(error.contains(message), "{error}");
    }

    let direct = source
        .replace("root.alias, 3", "root.rows, 3")
        .replace("property <[int]> alias <=> root.rows;", "");
    let raw = lower(&direct, "model_edit.xr");
    let bad_method = raw.replace(r#"method = "append""#, r#"method = "insert""#);
    assert!(
        mlir::process(&bad_method)
            .unwrap_err()
            .contains("argument types")
    );
    let readonly = raw.replace(r#"visibility = "in-out""#, r#"visibility = "in""#);
    assert!(mlir::process(&readonly).unwrap_err().contains("read-only"));
}

#[test]
fn navigation_move_commands_verify_owners_writability_and_effects() {
    let source = r#"
        export component App inherits Window {
            in-out property <[string]> labels: ["a", "b"];
            callback move(int, int);
            move(from, to) => { tabs.move-item(from, to); }
            tabs := TabWidget {
                reorderable: true;
                for label in root.labels : Tab { title: label; }
            }
        }
    "#;
    let compiled = compile(source, "move_navigation_item.xr").unwrap();
    assert!(compiled.mlir.contains("xr.move_navigation_item"));
    assert!(compiled.rust.to_string().contains("move_row"));

    for (old, new, message) in [
        ("in-out property", "in property", "read-only"),
        ("root.labels :", r#"["a", "b"] :"#, "writable list property"),
        ("callback move", "pure callback move", "pure callback"),
        (
            "move-item(from, to)",
            "move-item(from)",
            "two int arguments",
        ),
        (
            "move-item(from, to)",
            r#"move-item(from, "last")"#,
            "expected int",
        ),
    ] {
        let error = compile(&source.replace(old, new), "move_navigation_item.xr")
            .err()
            .unwrap()
            .to_string();
        assert!(error.contains(message), "{error}");
    }

    let raw = lower(source, "move_navigation_item.xr");
    let readonly = raw.replace(r#"visibility = "in-out""#, r#"visibility = "in""#);
    assert!(mlir::process(&readonly).unwrap_err().contains("read-only"));
    let (before, after) = raw.split_once(r#""xr.move_navigation_item""#).unwrap();
    let invalid_owner = format!(
        r#"{before}"xr.move_navigation_item"{}"#,
        after.replacen(r#"owner = "tabs""#, r#"owner = "root""#, 1)
    );
    assert_ne!(invalid_owner, raw);
    assert!(
        mlir::process(&invalid_owner)
            .unwrap_err()
            .contains("writable list model")
    );
}

#[test]
fn stack_layout_supports_regular_children_and_checks_keyed_templates() {
    let source = r#"
        export component App inherits Window {
            in-out property <int> index: 0;
            out property <int> selected: layers.active-index;
            out property <int> items: layers.count;
            layers := StackLayout {
                current-index <=> root.index;
                padding: 5px;
                Text { text: "First"; }
                second := StackItem { Text { text: "Second"; } }
            }
        }
    "#;
    let compiled = compile(source, "stack_layout.xr").unwrap();
    assert!(compiled.mlir.contains("xr.navigation_index"));
    assert!(compiled.mlir.contains("xr.navigation_count"));
    let keyed = source.replace(
        "current-index <=> root.index;",
        r#"current-value: "second";"#,
    );
    assert!(
        compile(&keyed, "stack_layout.xr")
            .err()
            .unwrap()
            .to_string()
            .contains("requires StackItem children")
    );
    let readonly = source.replace("padding: 5px;", "active-index: 1;");
    assert!(
        compile(&readonly, "stack_layout.xr")
            .err()
            .unwrap()
            .to_string()
            .contains("read-only")
    );
    let cycle = "export component App inherits Window { layers := StackLayout { current-index: layers.active-index; } }";
    assert!(
        compile(cycle, "stack_layout.xr")
            .err()
            .unwrap()
            .to_string()
            .contains("cyc")
    );
    let inline = "export component App inherits Window { ListView { for index in 2 : Rectangle { StackLayout { current-index: index; Rectangle {} } } } }";
    assert!(
        compile(inline, "stack_layout.xr")
            .err()
            .unwrap()
            .to_string()
            .contains("requires a component delegate")
    );
}

#[test]
fn close_tab_properties_and_event_payload_have_verified_types() {
    let source = r#"
        export component App inherits Window {
            in-out property <string> last: "";
            tabs := TabBar {
                tabs-closable: true;
                TabButton { text: "Notes"; value: "notes"; closable: true; }
                tab-close-requested(index, value) => { root.last = value + index; }
            }
        }
    "#;
    let compiled = compile(source, "tab_close.xr").unwrap();
    assert!(
        compiled
            .rust
            .to_string()
            .contains("ControlAction :: CloseTab")
    );
    for (old, new, message) in [
        ("tabs-closable: true;", "tabs-closable: 1;", "expected bool"),
        ("closable: true; }", "closable: 0; }", "expected bool"),
        (
            "tab-close-requested(index, value)",
            "tab-close-requested(index)",
            "an int index and a string value",
        ),
    ] {
        let error = compile(&source.replace(old, new), "tab_close.xr")
            .err()
            .unwrap()
            .to_string();
        assert!(error.contains(message), "{error}");
    }
    let input = lower(source, "tab_close.xr");
    assert!(
        mlir::process(&input.replace("tab-close-requested", "tab-moved"))
            .unwrap_err()
            .contains("tab-moved(int, int)")
    );
}

#[test]
fn reorderable_tabs_require_a_writable_list_and_typed_move_event() {
    let source = r#"
        export component App inherits Window {
            in-out property <[string]> entries: ["a", "b"];
            out property <int> last-move: 0;
            tabs := TabBar {
                reorderable: true;
                for entry in root.entries : TabButton { text: entry; }
                tab-moved(from, to) => { root.last-move = from * 10 + to; }
            }
        }
    "#;
    let compiled = compile(source, "reorderable_tabs.xr").unwrap();
    assert!(
        compiled
            .rust
            .to_string()
            .contains("ControlAction :: MoveTab")
    );
    for (old, new, message) in [
        ("in-out property", "in property", "read-only"),
        (
            "root.entries :",
            r#"["a", "b"] :"#,
            "writable list property",
        ),
        ("reorderable: true;", "reorderable: 4;", "bool"),
        (
            "tab-moved(from, to)",
            "tab-moved(from)",
            "two int parameters",
        ),
    ] {
        let error = compile(&source.replace(old, new), "reorderable_tabs.xr")
            .err()
            .unwrap()
            .to_string();
        assert!(error.contains(message), "{error}");
    }

    let raw = lower(source, "reorderable_tabs.xr");
    let readonly = raw.replace(r#"visibility = "in-out""#, r#"visibility = "in""#);
    assert_ne!(readonly, raw);
    assert!(mlir::process(&readonly).unwrap_err().contains("read-only"));
}

#[test]
fn navigation_value_checks_keys_conflicts_and_derived_index_reads() {
    let source = r#"
        export component App inherits Window {
            in-out property <string> selection: "draft";
            out property <int> index: bar.current-index;
            bar := TabBar {
                current-value <=> root.selection;
                TabButton { text: "Draft"; value: "draft"; }
            }
        }
    "#;
    let compiled = compile(source, "value_tabs.xr").unwrap();
    assert!(compiled.mlir.contains("xr.navigation_index"));
    for (before, after, message) in [
        (r#"value: "draft";"#, "", "requires value"),
        (
            "current-value <=> root.selection;",
            "current-value <=> root.selection; current-index: 0;",
            "either current-index or current-value",
        ),
        (r#"value: "draft";"#, "value: 42;", "string"),
        (
            r#"value: "draft";"#,
            r#"value: "doc" + bar.current-index;"#,
            "cycl",
        ),
    ] {
        let error = compile(&source.replace(before, after), "value_tabs.xr")
            .err()
            .unwrap()
            .to_string();
        assert!(error.contains(message), "{error}");
    }
    let raw = lower(source, "value_tabs.xr");
    let invalid = raw.replace(r#"value = "current-index""#, r#"value = "current-value""#);
    assert!(
        mlir::process(&invalid)
            .unwrap_err()
            .contains("requires current-index")
    );
}

#[test]
fn path_view_compiles_svg_commands_and_typed_position() {
    let source = r#"
        export component App inherits Window {
            PathView {
                path: "M 0 0.5 Q 0.5 0 1 0.5";
                count: 20;
                item-width: 42px;
                item-height: 34px;
                orientation: horizontal;
                PathViewOption {
                    opacity: option-progress;
                    Text { text: "Item " + (option-data + 1); }
                }
            }
        }
    "#;
    let compiled = compile(source, "path_view.xr").unwrap();
    assert!(compiled.mlir.contains("xr.precompiled_path"));
    assert!(compiled.rust.to_string().contains("set_precompiled_path"));
    let bad_path = source.replace("M 0 0.5 Q 0.5 0 1 0.5", "not a path");
    let error = compile(&bad_path, "path_view.xr")
        .err()
        .unwrap()
        .to_string();
    assert!(error.contains("invalid PathView path"), "{error}");
    let raw = lower(source, "path_view.xr");
    let wrong_type = raw.replace("option-progress", "option-index");
    assert!(
        mlir::process(&wrong_type)
            .unwrap_err()
            .contains("PathViewOption parameter or type")
    );
    let wrong_parent = source.replace("PathView {", "Tumbler {");
    assert!(compile(&wrong_parent, "path_view.xr").is_err());
}

#[test]
fn tumbler_flick_properties_are_float_and_read_only_motion() {
    let source = r#"
        export component App inherits Window {
            out property <bool> moving: wheel.moving;
            out property <bool> dragging: wheel.dragging;
            out property <bool> flicking: wheel.flicking;
            wheel := Tumbler { count: 60; flick-deceleration: 1600; }
        }
    "#;
    let compiled = compile(source, "tumbler_motion.xr").unwrap();
    assert!(compiled.mlir.contains("xr.flick_motion_read"));
    let conflict = source.replace("count: 60;", "count: 60; moving: true;");
    let error = compile(&conflict, "tumbler_motion.xr")
        .err()
        .unwrap()
        .to_string();
    assert!(error.contains("read-only"), "{error}");
    let raw = lower(source, "tumbler_motion.xr");
    let error = mlir::process(&raw.replace(r#"value = "flicking""#, r#"value = "rebounding""#))
        .unwrap_err();
    assert!(
        error.contains("motion property on a statically addressed Tumbler"),
        "{error}"
    );
}

#[test]
fn tumbler_option_templates_are_typed_and_confined_to_wheels() {
    let source = r#"
        export component App inherits Window {
            Tumbler {
                count: 80;
                TumblerOption {
                    opacity: Math.abs(option-displacement);
                    Text { text: option-data % 12 + " o'clock"; }
                }
            }
        }
    "#;
    let compiled = compile(source, "tumbler_option.xr").unwrap();
    assert!(compiled.rust.to_string().contains("ComboOptionRenderer"));
    let raw = lower(source, "tumbler_option.xr");
    assert!(
        mlir::process(&raw.replace("option-displacement", "option-index"))
            .unwrap_err()
            .contains("invalid TumblerOption parameter or type")
    );
    for (from, to, message) in [
        (
            "Tumbler {",
            "Rectangle {",
            "requires a static child of Tumbler",
        ),
        (
            "TumblerOption {",
            "named := TumblerOption {",
            "templates are anonymous",
        ),
        (
            "TumblerOption {",
            "TumblerOption {} TumblerOption {",
            "at most one TumblerOption",
        ),
        (
            "TumblerOption {",
            "if true: TumblerOption {",
            "requires a static child of Tumbler",
        ),
        (
            "Text { text: option-data % 12 + \" o'clock\"; }",
            "Button {}",
            "accepts visual elements",
        ),
    ] {
        let error = compile(&source.replace(from, to), "tumbler_option.xr")
            .err()
            .unwrap()
            .to_string();
        assert!(error.contains(message), "{error}");
    }

    let strings = source
        .replace("count: 80;", r#"model: ["A", "B"];"#)
        .replace("option-data % 12", "option-data");
    compile(&strings, "tumbler_option.xr").unwrap();
}

#[test]
fn tumbler_record_roles_and_selection_type_are_checked() {
    let source = r#"
        struct OptionRow { title: string, key: int }
        export component App inherits Window {
            in-out property <[OptionRow]> rows: [OptionRow { title: "One", key: 1 }];
            in-out property <int> selection: 1;
            out property <string> label: choice.current-text;
            choice := Tumbler {
                model: root.rows;
                text-role: "title";
                value-role: "key";
                current-value <=> root.selection;
                value-selected(value) => { root.selection = value; }
            }
        }
    "#;
    let compiled = compile(source, "tumbler_roles.xr").unwrap();
    assert!(compiled.mlir.contains("xr.combo_state_read"));
    assert!(compiled.rust.to_string().contains("populate_tumbler"));
    let indexed = source.replace(
        "current-value <=> root.selection;",
        "current-index: self.count - 1;",
    );
    compile(&indexed, "tumbler_roles.xr").unwrap();
    let raw = lower(source, "tumbler_roles.xr");
    let bad_role = raw.replace(r#"value = "\22key\22""#, r#"value = "\22title\22""#);
    assert!(
        mlir::process(&bad_role)
            .unwrap_err()
            .contains("Tumbler current-value requires string")
    );
    for (original, replacement, message) in [
        (
            r#"text-role: "title";"#,
            r#"text-role: "key";"#,
            "requires a string field",
        ),
        (
            "current-value <=> root.selection;",
            r#"current-value: "wrong";"#,
            "int",
        ),
        (
            "current-value <=> root.selection;",
            "current-value <=> root.selection; current-index: 0;",
            "either current-index or current-value",
        ),
        (
            r#"text-role: "title";"#,
            r#"text-role: "title"; current-text: "wrong";"#,
            "read-only",
        ),
    ] {
        let error = compile(&source.replace(original, replacement), "tumbler_roles.xr")
            .err()
            .unwrap()
            .to_string();
        assert!(error.contains(message), "{error}");
    }
    let invalid = r#"export component App inherits Window {
        Tumbler { count: 3; current-value: "two"; }
    }"#;
    assert!(compile(invalid, "tumbler_roles.xr").is_err());
}

#[test]
fn combo_popup_commands_and_state_are_checked_in_source_and_mlir() {
    let source = r#"
        export component App inherits Window {
            out property <bool> showing: choice.popup-open;
            out property <int> active-option: choice.highlighted-index;
            choice := ComboBox { model: ["Alpha"]; }
            trigger := Button { clicked => { choice.open-popup(); choice.close-popup(); } }
        }
    "#;
    let compiled = compile(source, "combo_popup_commands.xr").unwrap();
    assert!(compiled.mlir.contains("xr.combo_popup_read"));
    assert!(compiled.mlir.contains("action = \"open-popup\""));
    assert!(compiled.mlir.contains("action = \"close-popup\""));
    for (from, to, expected) in [
        (
            "choice.open-popup()",
            "choice.open-popup(1)",
            "takes no arguments",
        ),
        (
            "model: [\"Alpha\"];",
            "model: [\"Alpha\"]; highlighted-index: 0;",
            "read-only",
        ),
    ] {
        let error = compile(&source.replace(from, to), "combo_popup_commands.xr")
            .err()
            .unwrap();
        assert!(error.contains(expected), "{error}");
    }
    let bad_target = compiled
        .mlir
        .replace("target = \"choice\"", "target = \"trigger\"");
    assert_ne!(bad_target, compiled.mlir);
    assert!(
        mlir::process(&bad_target)
            .unwrap_err()
            .contains("popup commands require a ComboBox target")
    );
    let bad_type = compiled
        .mlir
        .replace("value = \"highlighted-index\"", "value = \"popup-open\"");
    assert!(
        mlir::process(&bad_type)
            .unwrap_err()
            .contains("ComboBox popup state property of matching type")
    );
}

#[test]
fn combo_popup_checks_its_dimensions_and_rejects_nested_elements() {
    let source = r#"
        export component App inherits Window {
            ComboBox {
                ComboPopup { width: 208px; row-height: 32px; scroll-indicator: true; }
            }
        }
    "#;
    let compiled = compile(source, "combo_popup.xr").unwrap();
    assert!(compiled.mlir.contains("kind = \"ComboPopup\""));
    assert!(
        compile(
            &source.replace("width: 208px", "width: 125%; max-height: 60%"),
            "combo_popup.xr"
        )
        .is_ok()
    );
    for (from, to, expected) in [
        (
            "ComboBox {",
            "Rectangle {",
            "requires a static child of ComboBox",
        ),
        (
            "ComboPopup {",
            "ComboPopup {} ComboPopup {",
            "at most one ComboPopup",
        ),
        (
            "ComboPopup {",
            "ComboPopup { Rectangle {} ",
            "only one-way property bindings",
        ),
        ("row-height: 32px", "row-height: \"32px\"", "type mismatch"),
    ] {
        let error = compile(&source.replace(from, to), "combo_popup.xr")
            .err()
            .unwrap();
        assert!(error.contains(expected), "{error}");
    }
    let invalid = compiled
        .mlir
        .replace("name = \"row-height\"", "name = \"x\"");
    assert!(
        mlir::process(&invalid)
            .unwrap_err()
            .contains("invalid ComboPopup property or type")
    );
}

#[test]
fn combo_content_is_a_single_static_child() {
    let source = r#"
        export component App inherits Window {
            choice := ComboBox {
                model: ["Alpha", "Beta"];
                ComboContent { Label { text: choice.current-text; } }
            }
        }
    "#;
    let compiled = compile(source, "combo_content.xr").unwrap();
    assert!(compiled.mlir.contains("kind = \"ComboContent\""));
    assert!(compiled.rust.to_string().contains("combo_editable"));
    for (from, to, expected) in [
        (
            "ComboBox {",
            "Rectangle {",
            "requires a static child of ComboBox",
        ),
        (
            "ComboContent {",
            "if true : ComboContent {",
            "requires a static child of ComboBox",
        ),
        (
            "ComboContent {",
            "ComboContent {} ComboContent {",
            "at most one ComboContent",
        ),
    ] {
        let changed = source.replace(from, to);
        let error = compile(&changed, "combo_content.xr").err().unwrap();
        assert!(error.contains(expected), "{error}");
    }
    let invalid_ir = compiled
        .mlir
        .replace("kind = \"ComboBox\"", "kind = \"Rectangle\"");
    assert!(
        mlir::process(&invalid_ir)
            .unwrap_err()
            .contains("requires a static child of ComboBox")
    );
}

#[test]
fn combo_option_data_preserves_the_model_item_type() {
    let source = r#"
        struct Detail { online: bool }
        struct Entry { title: string, detail: Detail }
        export component App inherits Window {
            in property <[Entry]> choices;
            ComboBox {
                model: root.choices;
                text-role: "title";
                ComboOption {
                    background: option-data.detail.online ? #ffffff : #000000;
                    Label { text: option-data.title; }
                }
            }
            ComboBox {
                model: ["Alpha", "Beta"];
                ComboOption { Label { text: option-data; } }
            }
        }
    "#;
    let compiled = compile(source, "combo_data.xr").unwrap();
    assert!(compiled.mlir.contains("value = \"option-data\""));
    assert!(compiled.rust.to_string().contains("record_data"));
    assert!(
        compile(
            &source.replace("option-data.title", "option-data.missing"),
            "combo_data.xr"
        )
        .is_err()
    );
    assert!(
        compile(
            &source.replace(
                "Label { text: option-data; }",
                "Label { text: option-data.title; }"
            ),
            "combo_data.xr"
        )
        .is_err()
    );
    let wrong_types = compiled
        .mlir
        .replace("value = \"option-data\"", "value = \"option-text\"");
    let error = mlir::process(&wrong_types).unwrap_err();
    assert!(
        error.contains("invalid ComboOption parameter or type"),
        "{error}"
    );
}

#[test]
fn combo_option_templates_are_visual_and_use_typed_row_inputs() {
    let source = r#"
        export component App inherits Window {
            ComboBox {
                model: ["Alpha", "Beta"];
                ComboOption {
                    background: option-highlighted ? #ffffff : #000000;
                    Label { text: (option-index + 1) + " " + option-text; }
                }
            }
        }
    "#;
    let compiled = compile(source, "combo_option.xr").unwrap();
    assert!(compiled.mlir.contains("kind = \"ComboOption\""));
    assert!(compiled.rust.to_string().contains("ComboOptionRenderer"));
    for (from, to, expected) in [
        (
            "ComboBox {",
            "Rectangle {",
            "requires a static child of ComboBox",
        ),
        (
            "ComboOption {",
            "row := ComboOption {",
            "templates are anonymous",
        ),
        (
            "ComboOption {",
            "ComboOption {} ComboOption {",
            "at most one ComboOption",
        ),
        (
            "Label {",
            "Button {",
            "visual elements and read-only bindings",
        ),
    ] {
        let changed = source.replace(from, to);
        let error = compile(&changed, "combo_option.xr").err().unwrap();
        assert!(error.contains(expected), "{error}");
    }
    let bad_parameter = compiled
        .mlir
        .replace("value = \"option-text\"", "value = \"option-enabled\"");
    assert_ne!(compiled.mlir, bad_parameter);
    let error = mlir::process(&bad_parameter).unwrap_err();
    assert!(
        error.contains("invalid ComboOption parameter or type"),
        "{error}"
    );
}

#[test]
fn combo_indicator_is_a_single_static_child_of_a_combo_box() {
    let source = r#"
        export component App inherits Window {
            ComboBox {
                ComboIndicator {
                    width: 44px; height: 24px;
                    Rectangle { background: #437599; }
                }
            }
        }
    "#;
    let raw = lower(source, "combo_indicator.xr");
    assert!(raw.contains("kind = \"ComboIndicator\""));
    let message = "ComboIndicator requires a static child of ComboBox";
    for misplaced in [
        source.replace("ComboBox {", "Rectangle {"),
        source.replace("ComboIndicator {", "if true : ComboIndicator {"),
    ] {
        let error = compile(&misplaced, "combo_indicator.xr").err().unwrap();
        assert!(error.contains(message), "{error}");
    }
    let duplicate = source.replace("ComboBox {", "ComboBox { ComboIndicator {}");
    assert!(
        compile(&duplicate, "combo_indicator.xr")
            .err()
            .unwrap()
            .contains("at most one ComboIndicator")
    );
    let invalid_ir = raw.replace("kind = \"ComboBox\"", "kind = \"Rectangle\"");
    assert!(
        mlir::process(&invalid_ir)
            .unwrap_err()
            .contains("requires a static child of ComboBox")
    );
}

#[test]
fn combo_state_read_types_and_dependencies_are_verified() {
    let source = r#"
        struct OptionRow { title: string, code: int }
        export component App inherits Window {
            in property <[OptionRow]> options: [OptionRow { title: "First", code: 42 }];
            out property <int> picked: choice.current-value;
            out property <string> label: choice.current-text;
            out property <int> size: choice.count;
            choice := ComboBox {
                model: root.options;
                text-role: "title";
                value-role: "code";
            }
        }
    "#;
    let compiled = compile(source, "combo_state.xr").unwrap();
    assert!(compiled.mlir.contains("xr.combo_state_read"));
    let wrong_type = compiled
        .mlir
        .replace("value = \"current-value\"", "value = \"current-text\"");
    assert_ne!(wrong_type, compiled.mlir);
    let error = mlir::Module::parse(&wrong_type).err().unwrap();
    assert!(
        error.contains("ComboBox state property of matching type"),
        "{error}"
    );

    for condition in ["self.count > 0", "self.current-value > 0"] {
        let cycle = source.replace(
            "model: root.options",
            &format!("model: {condition} ? root.options : root.options"),
        );
        let error = compile(&cycle, "combo_cycle.xr").err().unwrap();
        assert!(error.contains("cycl"), "{error}");
    }
}

#[test]
fn secondary_windows_require_a_window_component_and_typed_visibility() {
    let source = r#"
        component Content inherits Window { title: "Inspector"; }
        component Panel inherits Rectangle {}
        export component App inherits Window {
            in-out property <bool> inspecting: false;
            inspector := SecondaryWindow { visible <=> root.inspecting; Content {} }
            open := Button { clicked => { inspector.show(); } }
        }
    "#;
    let compiled = compile(source, "secondary.xr").unwrap();
    assert!(compiled.mlir.contains("xr.secondary_window_command"));
    assert!(compiled.rust.to_string().contains("SecondaryWindowSlot"));
    let raw = lower(source, "secondary.xr");
    let wrong_root = raw.replace("component = @Content", "component = @Panel");
    assert!(
        mlir::process(&wrong_root)
            .unwrap_err()
            .contains("SecondaryWindow requires a component rooted in Window")
    );
    let wrong_command = raw.replace("command = \"show\"", "command = \"raise\"");
    assert!(
        mlir::process(&wrong_command)
            .unwrap_err()
            .contains("requires show, hide, or close")
    );
    assert!(compile(&source.replace("Content {}", "Panel {}"), "secondary.xr").is_err());
    assert!(
        compile(
            &source.replace("Content {}", "Content { width: 200px; }"),
            "secondary.xr"
        )
        .is_err()
    );
}

#[test]
fn window_close_commands_and_guard_results_are_verified() {
    let source = r#"
        export component App inherits Window {
            close-requested() => { false }
            quit := Button { clicked => { root.close(); } }
        }
    "#;
    let compiled = compile(source, "close.xr").unwrap();
    assert!(compiled.mlir.contains("xr.window_close"));
    assert!(compiled.mlir.contains("xr.return"));
    assert!(
        compiled
            .rust
            .to_string()
            .contains("take_window_close_request")
    );

    let raw = lower(source, "close.xr");
    let wrong_type = raw
        .replace("value = \"false\"", "value = \"1\"")
        .replace("!xr.bool", "!xr.int");
    assert!(
        mlir::process(&wrong_type)
            .unwrap_err()
            .contains("close-requested requires no parameters and a bool result")
    );
    let wrong_event = raw.replace("name = \"close-requested\"", "name = \"clicked\"");
    assert!(
        mlir::process(&wrong_event)
            .unwrap_err()
            .contains("only close-requested may return a value")
    );
    let missing = source.replace("{ false }", "{ }");
    assert!(
        compile(&missing, "close.xr")
            .err()
            .unwrap()
            .contains("close-requested must return a bool")
    );
}

#[test]
fn file_dialog_open_and_result_signatures_are_verified() {
    let source = r#"
        export component App inherits Window {
            out property <[string]> paths: [];
            callback choose;
            choose => { picker.open(); }
            picker := FileDialog {
                mode: open-files;
                name-filters: ["Text files (*.txt)"];
                accepted(files) => { root.paths = files; }
            }
        }
    "#;
    let compiled = compile(source, "files.xr").unwrap();
    assert!(compiled.mlir.contains("xr.native_dialog_open"));
    assert!(compiled.mlir.contains("!xr.filedialogmode"));
    assert!(compiled.rust.to_string().contains("NativeDialogSlot"));
    let raw = lower(source, "files.xr");
    assert!(mlir::process(&raw.replace("xr.native_dialog_open", "xr.file_dialog_open")).is_ok());
    let wrong_owner = raw.replace("owner = \"picker\"", "owner = \"root\"");
    assert!(
        mlir::process(&wrong_owner)
            .unwrap_err()
            .contains("requires a native dialog owner")
    );
    let bad_event = raw.replace("name = \"accepted\"", "name = \"failed\"");
    assert!(
        mlir::process(&bad_event)
            .unwrap_err()
            .contains("FileDialog requires accepted([string])")
    );
    let pure = source.replace("callback choose;", "pure callback choose;");
    assert!(
        compile(&pure, "files.xr")
            .err()
            .unwrap()
            .contains("FileDialog.open() is not allowed in a pure callback")
    );
}

#[test]
fn native_dialog_result_properties_are_typed_and_read_only() {
    let source = r#"
        component App inherits Window {
            out property <[string]> folders: picker.selected-files;
            out property <string> first: picker.selected-file;
            out property <bool> pending: picker.pending || prompt.pending;
            callback choose;
            choose => { picker.open(); }
            picker := FileDialog { mode: select-folders; }
            prompt := MessageDialog { text: "Done"; }
        }
    "#;
    let compiled = compile(source, "file-state.xr").unwrap();
    assert!(compiled.mlir.contains("xr.native_dialog_read"));
    assert!(compiled.rust.to_string().contains("SelectFolders"));
    let raw = lower(source, "file-state.xr");
    let wrong_type = raw.replace("value = \"selected-files\"", "value = \"pending\"");
    assert!(
        mlir::process(&wrong_type)
            .unwrap_err()
            .contains("requires a typed native dialog result property")
    );
    let write = source.replace("picker.open();", "picker.selected-file = \"bad\";");
    assert!(
        compile(&write, "file-state.xr")
            .err()
            .unwrap()
            .contains("read-only")
    );
    let wrong_dialog = source.replace("picker.selected-file;", "prompt.selected-file;");
    assert!(compile(&wrong_dialog, "file-state.xr").is_err());
}

#[test]
fn message_dialog_has_typed_buttons_levels_and_results() {
    let source = r#"
        export component App inherits Window {
            out property <StandardButtonKind> button;
            callback ask;
            ask => { prompt.open(); }
            prompt := MessageDialog {
                level: warning;
                buttons: MessageButtons.yes-no-cancel;
                text: "Replace settings?";
                finished(answer) => { root.button = answer; }
            }
        }
    "#;
    let compiled = compile(source, "message.xr").unwrap();
    assert!(compiled.mlir.contains("!xr.messagebuttons"));
    assert!(compiled.mlir.contains("!xr.messagelevel"));
    let rust = compiled.rust.to_string();
    assert!(rust.contains("MessageDialogOptions :: read_bindings"));
    assert!(rust.contains("NativeDialogEvent :: MessageFinished"));
    let raw = lower(source, "message.xr");
    let bad_type = raw.replace("name = \"buttons\"", "name = \"level\"");
    assert!(
        mlir::process(&bad_type)
            .unwrap_err()
            .contains("incorrect MessageDialog property or type")
    );
    let bad_event = raw.replace("name = \"finished\"", "name = \"canceled\"");
    assert!(
        mlir::process(&bad_event)
            .unwrap_err()
            .contains("invalid MessageDialog result handler or signature")
    );
}

#[test]
fn shortcut_bindings_and_action_links_are_checked_in_mlir() {
    let source = r#"
        export component App inherits Window {
            out property <int> hits: 0;
            save := Action { shortcut: "Primary+S"; activated => { root.hits += 1; } }
            Shortcut { action: save; sequences: ["Primary+K, Primary+S", "F9"]; }
            Shortcut { sequence: "F6"; activated-ambiguously => { root.hits += 2; } }
        }
    "#;
    let output = compile(source, "shortcuts.xr").unwrap();
    assert!(output.mlir.contains("kind = \"Shortcut\""));
    assert!(output.rust.to_string().contains("ActivateShortcut"));
    let raw = lower(source, "shortcuts.xr");
    let bad_type = raw.replace("name = \"sequences\"", "name = \"sequence\"");
    assert!(
        mlir::process(&bad_type)
            .unwrap_err()
            .contains("incorrect Shortcut property or type")
    );
    let bad_event = raw.replace("name = \"activated-ambiguously\"", "name = \"clicked\"");
    assert!(
        mlir::process(&bad_event)
            .unwrap_err()
            .contains("Shortcut requires an activation handler")
    );
    let ambiguous_source =
        source.replace("sequence: \"F6\"", "sequence: \"F6\"; sequences: [\"F7\"]");
    assert!(compile(&ambiguous_source, "shortcuts.xr").is_err());
}

#[test]
fn window_palette_is_typed_in_xr_and_mlir() {
    let source = r#"
        export component App inherits Window {
            in-out property <ColorScheme> scheme: ColorScheme.light;
            color-scheme: root.scheme;
            accent-color: #c36728;
            Button { text: "Preview"; }
        }
    "#;
    let output = compile(source, "theme.xr").unwrap();
    assert!(output.mlir.contains("!xr.colorscheme"));
    assert!(output.rust.to_string().contains("ColorScheme :: Light"));
    let raw = lower(source, "theme.xr");
    let bad_binding = raw.replace("name = \"accent-color\"", "name = \"color-scheme\"");
    assert!(
        mlir::process(&bad_binding)
            .unwrap_err()
            .contains("incorrect window palette property or type")
    );
    let bad_variant = raw.replace("value = \"light\"", "value = \"sepia\"");
    assert!(mlir::process(&bad_variant).is_err());
    let bad_owner = source.replace(
        "Button { text: \"Preview\"; }",
        "Button { color-scheme: dark; }",
    );
    assert!(compile(&bad_owner, "theme.xr").is_err());
}

#[test]
fn color_popup_verifies_control_links_and_callback_types() {
    let source = r#"
        export component App inherits Window {
            in-out property <color> selected: #124380a0;
            in-out property <bool> shown: false;
            out property <color> accepted-color;
            picker := ColorPickerPopup {
                open <=> root.shown;
                color <=> root.selected;
                show-alpha: true;
                show-color-plane: true;
                accepted(value) => { root.accepted-color = value; }
            }
        }
    "#;
    let raw = lower(source, "color.xr");
    assert!(raw.contains("!xr.color"));
    let compiled = compile(source, "color.xr").unwrap();
    let rust = compiled.rust.to_string();
    assert!(rust.contains("ColorPopupState"));
    assert!(rust.contains("edit_hex"));
    assert!(rust.contains("adjust_plane"));
    let invalid_plane = source.replace("show-color-plane: true", "show-color-plane: 1");
    assert!(
        compile(&invalid_plane, "color.xr")
            .err()
            .unwrap()
            .contains("type mismatch")
    );
    let bad_binding = raw.replace("name = \"show-alpha\"", "name = \"color\"");
    assert!(
        mlir::process(&bad_binding)
            .unwrap_err()
            .contains("incorrect color picker property type")
    );
    let bad_event = raw.replace("name = \"accepted\"", "name = \"canceled\"");
    assert!(
        mlir::process(&bad_event)
            .unwrap_err()
            .contains("invalid picker event or signature")
    );
    let bad_source = source.replace("color <=> root.selected", "color <=> root.shown");
    assert!(compile(&bad_source, "color.xr").is_err());
}

#[test]
fn font_picker_verifies_construction_fields_links_and_callbacks() {
    let source = r#"
        export component App inherits Window {
            in-out property <Font> selected: Font {
                family: "serif", size: 18.5px, weight: 550, italic: false
            };
            out property <string> family: root.selected.family;
            out property <Font> accepted-font;
            default-font: root.selected;
            Text { font: root.selected; font-weight: 700; }
            picker := FontPickerPopup {
                open: true;
                font <=> root.selected;
                preview-text: root.family;
                accepted(value) => { root.accepted-font = value; }
            }
        }
    "#;
    let raw = lower(source, "font.xr");
    let compiled = compile(source, "font.xr").unwrap();
    for op in [
        "xr.make_font",
        "xr.font_field",
        "xr.control_link",
        "!xr.font",
    ] {
        assert!(compiled.mlir.contains(op), "missing {op}");
    }
    assert!(compiled.rust.to_string().contains("FontPopupState"));
    for (from, to, message) in [
        (
            "value = \"family\"",
            "value = \"size\"",
            "incorrect font field or type",
        ),
        (
            "name = \"preview-text\"",
            "name = \"font\"",
            "incorrect font picker property type",
        ),
        (
            "name = \"accepted\"",
            "name = \"canceled\"",
            "invalid picker event or signature",
        ),
    ] {
        let invalid = raw.replace(from, to);
        assert_ne!(raw, invalid);
        let error = mlir::process(&invalid).unwrap_err();
        assert!(error.contains(message), "{error}");
    }
    for (from, to, message) in [
        ("size: 18.5px", "size: true", "type mismatch"),
        (", italic: false", "", "missing record field"),
        (".family", ".unknown", "Font has no field"),
        (
            "font <=> root.selected",
            "font <=> root.family",
            "two-way binding requires a writable font property",
        ),
    ] {
        let invalid = source.replacen(from, to, 1);
        let error = compile(&invalid, "font.xr")
            .err()
            .expect("invalid Font compiled");
        assert!(error.contains(message), "{error}");
    }
}

#[test]
fn grid_view_checks_geometry_and_repeated_template_in_source_and_mlir() {
    let source = r#"
        component Tile inherits Rectangle { in property <int> index; }
        export component App inherits Window {
            in-out property <int> chosen: -1;
            grid := GridView {
                cell-width: 100px; cell-height: 84px; spacing: 10px;
                current-index <=> root.chosen;
                for item in 1000 : Tile { index: item; }
                changed(index) => { root.chosen = index; }
            }
        }
    "#;
    let output = compile(source, "grid.xr").unwrap();
    assert!(output.mlir.contains("kind = \"GridView\""));
    assert!(output.mlir.contains("xr.row_scene_cache"));
    assert!(output.rust.to_string().contains("render_resolved_indices"));
    let malformed = source.replace("100px", "true");
    assert!(
        compile(&malformed, "grid.xr")
            .err()
            .unwrap()
            .contains("type mismatch")
    );
    let raw = lower(source, "grid.xr");
    assert!(
        mlir::process(&raw.replace("name = \"cell-width\"", "name = \"follow-current\""))
            .unwrap_err()
            .contains("incorrect GridView property type")
    );
    let malformed = source.replace("for item in 1000 : Tile { index: item; }", "Rectangle {}");
    assert!(
        compile(&malformed, "grid.xr")
            .err()
            .unwrap()
            .contains("requires exactly one for element")
    );
}

#[test]
fn page_slots_have_a_unique_direct_parent_in_source_and_mlir() {
    let source = r#"
        export component App inherits Window {
            Page {
                title: "Settings";
                PageFooter { height: 40px; }
                Rectangle {}
                PageHeader { height: 36px; }
            }
        }
    "#;
    assert!(compile(source, "page.xr").is_ok());
    let duplicate = source.replace("PageFooter", "PageHeader");
    assert!(
        compile(&duplicate, "page.xr")
            .err()
            .unwrap()
            .contains("only one PageHeader")
    );
    let raw = lower(source, "page.xr");
    assert!(
        mlir::process(&raw.replace("kind = \"PageFooter\"", "kind = \"PageHeader\""))
            .unwrap_err()
            .contains("only one PageHeader")
    );
    assert!(
        mlir::process(&raw.replace("kind = \"Page\"", "kind = \"Rectangle\""))
            .unwrap_err()
            .contains("requires a direct Page parent")
    );
    let repeated = source.replace("PageFooter {", "for n in [1, 2] : PageFooter {");
    assert!(
        compile(&repeated, "page.xr")
            .err()
            .unwrap()
            .contains("cannot repeat")
    );
}

#[test]
fn standard_list_uses_a_verified_record_text_field() {
    let source = r#"
        export struct Entry { label: string, rank: int }
        export component App inherits Window {
            in property <[Entry]> records: [Entry { label: "one", rank: 1 }];
            in-out property <int> selected: -1;
            list := StandardListView {
                model: root.records;
                text-role: "label";
                current-index <=> root.selected;
                changed(index) => { root.selected = index; }
            }
        }
    "#;
    let result = compile(source, "standard-list.xr").unwrap();
    assert!(result.mlir.contains("kind = \"StandardListView\""));
    assert!(result.rust.to_string().contains("standard_list_rows"));
    assert!(
        compile(
            &source.replace("text-role: \"label\"", "text-role: \"rank\""),
            "standard-list.xr"
        )
        .err()
        .unwrap()
        .contains("string field named `rank`")
    );
    let raw = lower(source, "standard-list.xr");
    assert!(
        mlir::process(&raw.replace("name = \"text-role\"", "name = \"unknown-role\""))
            .unwrap_err()
            .contains("string field named text")
    );
    assert!(compile(
        "export component App inherits Window { StandardListView { model: [\"one\", \"two\"]; } }",
        "strings.xr"
    ).is_ok());
}

#[test]
fn progress_range_positions_are_typed_read_only_and_acyclic() {
    let source = r#"
        export component App inherits Window {
            out property <float> fraction: meter.position;
            out property <float> visual: meter.visual-position;
            meter := ProgressIndicator { minimum: 100; maximum: 900; progress: 380; }
        }
    "#;
    let compiled = compile(source, "progress_range.xr").unwrap();
    assert!(compiled.mlir.contains("xr.range_position_read"));
    assert!(compiled.rust.to_string().contains("ProgressRange"));
    for (from, to, expected) in [
        ("minimum: 100", "minimum: true", "type mismatch"),
        ("minimum: 100", "position: 0.5", "read-only"),
        ("progress: 380", "progress: root.fraction", "cyclic"),
    ] {
        let error = compile(&source.replace(from, to), "progress_range.xr")
            .err()
            .unwrap();
        assert!(error.contains(expected), "{error}");
    }
    let bad_position = compiled
        .mlir
        .replace("value = \"visual-position\"", "value = \"upper-position\"");
    assert!(
        mlir::process(&bad_position)
            .unwrap_err()
            .contains("requires a supported range position")
    );
}

#[test]
fn progress_artwork_parts_are_unique_static_children_of_the_indicator() {
    let source = r#"
        export component App inherits Window {
            ProgressIndicator {
                progress: 40%;
                ProgressTrack { background: #273b55; }
                ProgressFill { Rectangle { background: #53af9b; } }
            }
        }
    "#;
    let compiled = compile(source, "progress_parts.xr").unwrap();
    assert!(compiled.mlir.contains("kind = \"ProgressTrack\""));
    assert!(compiled.mlir.contains("kind = \"ProgressFill\""));
    for (from, to, expected) in [
        (
            "ProgressIndicator {",
            "Rectangle {",
            "requires a static child of ProgressIndicator",
        ),
        (
            "ProgressFill {",
            "ProgressFill {} ProgressFill {",
            "at most one ProgressFill",
        ),
        (
            "ProgressFill {",
            "if true : ProgressFill {",
            "requires a static child of ProgressIndicator",
        ),
    ] {
        let error = compile(&source.replace(from, to), "progress_parts.xr")
            .err()
            .unwrap();
        assert!(error.contains(expected), "{error}");
    }
    let invalid = compiled
        .mlir
        .replace("kind = \"ProgressIndicator\"", "kind = \"Rectangle\"");
    assert!(
        mlir::process(&invalid)
            .unwrap_err()
            .contains("requires a static child of ProgressIndicator")
    );
    let feedback = source
        .replace("progress: 40%", "progress: fill-art.layout-width / 100px")
        .replace("ProgressFill {", "fill-art := ProgressFill {");
    assert!(
        compile(&feedback, "progress_parts.xr")
            .err()
            .unwrap()
            .contains("layout feedback")
    );
}

#[test]
fn spinner_and_busy_indicator_verify_progress_and_motion_types() {
    let source = r#"
        export component App inherits Window {
            in property <bool> loading: true;
            BusyIndicator { running: root.loading; stroke-width: 3px; animated: false; }
            Spinner { progress: 65%; track-color: #384761; }
        }
    "#;
    let result = compile(source, "indicators.xr").unwrap();
    assert!(result.mlir.contains("kind = \"BusyIndicator\""));
    assert!(result.mlir.contains("kind = \"Spinner\""));
    let raw = lower(source, "indicators.xr");
    assert!(
        mlir::process(&raw.replace("name = \"stroke-width\"", "name = \"track-color\""))
            .unwrap_err()
            .contains("incorrect activity indicator property type")
    );
    assert!(
        compile(
            &source.replace("running: root.loading", "progress: 0.5"),
            "indicators.xr"
        )
        .err()
        .unwrap()
        .contains("progress")
    );
}

#[test]
fn expander_uses_a_boolean_link_and_expansion_event() {
    let source = r#"
        export component App inherits Window {
            in-out property <bool> active: false;
            details := Expander {
                title: "Details";
                expanded <=> root.active;
                expanded-changed(value) => { root.active = value; }
                Button { text: "Nested action"; }
            }
        }
    "#;
    let output = compile(source, "expander.xr").unwrap();
    assert!(output.mlir.contains("xr.control_link"));
    assert!(output.mlir.contains(r#"name = "expanded-changed""#));
    assert!(
        compile(
            &source.replace(
                "in-out property <bool> active: false",
                "in-out property <int> active: 1"
            ),
            "expander.xr"
        )
        .is_err()
    );
    let raw = lower(source, "expander.xr").replace(r#"name = "title""#, r#"name = "expanded""#);
    assert!(
        mlir::process(&raw)
            .unwrap_err()
            .contains("incorrect Expander property type")
    );
}

#[test]
fn group_box_checks_its_header_and_checked_binding_in_mlir() {
    let source = r#"
        export component App inherits Window {
            in-out property <bool> enabled-group: true;
            settings := GroupBox {
                title: "Settings";
                checkable: true;
                checked <=> root.enabled-group;
                content-padding: 12px;
                toggled(active) => { root.enabled-group = active; }
                LineEdit { placeholder-text: "Address"; }
            }
        }
    "#;
    let result = compile(source, "group.xr").unwrap();
    assert!(result.mlir.contains("xr.control_link"));
    assert!(result.mlir.contains("content-padding"));
    assert!(
        compile(&source.replace("toggled(active)", "toggled()"), "group.xr")
            .err()
            .unwrap()
            .contains("expects one bool parameter")
    );
    let raw = lower(source, "group.xr");
    assert!(
        mlir::process(&raw.replace("name = \"content-padding\"", "name = \"title\""))
            .unwrap_err()
            .contains("incorrect GroupBox property type")
    );
}

#[test]
fn check_state_bindings_and_callbacks_use_the_typed_mlir_enum() {
    let source = r#"
        export component App inherits Window {
            in-out property <CheckState> selection: CheckState.partially-checked;
            out property <CheckState> last;
            item := CheckBox {
                check-state <=> root.selection;
                tristate: true;
                check-state-changed(state) => { root.last = state; }
            }
        }
    "#;
    let result = compile(source, "checked.xr").unwrap();
    assert!(result.mlir.contains("!xr.checkstate"));
    assert!(result.mlir.contains("xr.control_link"));
    assert!(
        result
            .rust
            .to_string()
            .contains("CheckState :: PartiallyChecked")
    );
    let conflict = source.replace("tristate: true;", "checked: false;");
    assert!(
        compile(&conflict, "checked.xr")
            .err()
            .unwrap()
            .contains("CheckBox accepts checked or check-state")
    );
    let raw = lower(source, "checked.xr");
    let wrong = raw.replace("value = \"partially-checked\"", "value = \"mixed\"");
    assert!(
        mlir::process(&wrong)
            .unwrap_err()
            .contains("literal spelling")
    );
}

#[test]
fn delay_button_verifies_delay_checked_link_and_activation_handler() {
    let source = r#"
        export component App inherits Window {
            in-out property <bool> confirmed: false;
            confirm := DelayButton {
                delay: 250;
                checked <=> root.confirmed;
                progress-color: #366483;
                activated() => {}
            }
        }
    "#;
    let compilation = compile(source, "hold.xr").unwrap();
    assert!(compilation.mlir.contains("xr.control_link"));
    assert!(
        compilation
            .rust
            .to_string()
            .contains("ControlAction :: DelayButton")
    );
    assert!(
        compile(&source.replace("activated()", "clicked()"), "hold.xr")
            .err()
            .unwrap()
            .contains("unknown event")
    );
    let raw = lower(source, "hold.xr");
    assert!(
        mlir::process(&raw.replace("name = \"delay\"", "name = \"progress-color\""))
            .unwrap_err()
            .contains("incorrect DelayButton property type")
    );
}

#[test]
fn tumbler_verifies_the_model_source_and_index_link() {
    let source = r#"
        export component App inherits Window {
            in-out property <int> selected: 0;
            wheel := Tumbler {
                model: ["one", "two"];
                current-index <=> root.selected;
                visible-item-count: 5;
                changed(index) => { root.selected = index; }
            }
        }
    "#;
    let compilation = compile(source, "wheel.xr").unwrap();
    assert!(compilation.mlir.contains("xr.control_link"));
    assert!(compilation.rust.to_string().contains("TumblerItems :: new"));
    let invalid = source.replace("visible-item-count: 5", "count: 5");
    assert!(
        compile(&invalid, "wheel.xr")
            .err()
            .unwrap()
            .contains("either model or count")
    );
    let raw = lower(source, "wheel.xr");
    assert!(
        mlir::process(&raw.replace("name = \"visible-item-count\"", "name = \"wrap\""))
            .unwrap_err()
            .contains("incorrect Tumbler property type")
    );
}

#[test]
fn password_mode_and_reveal_are_typed_in_mlir() {
    let source = r#"
        export component App inherits Window {
            in-out property <string> secret;
            in property <InputType> mode: InputType.password;
            in property <bool> reveal: false;
            LineEdit {
                input-type: root.mode;
                password-visible: root.reveal;
                text <=> root.secret;
            }
        }
    "#;
    let compilation = compile(source, "password.xr").unwrap();
    assert!(compilation.mlir.contains("!xr.inputtype"));
    assert!(
        compilation
            .rust
            .to_string()
            .contains("InputType :: Password")
    );
    let raw = lower(source, "password.xr");
    assert!(
        mlir::process(&raw.replace("name = \"input-type\"", "name = \"password-visible\""))
            .unwrap_err()
            .contains("incorrect password input property or type")
    );
    assert!(
        compile(&source.replace("LineEdit", "SearchField"), "password.xr")
            .err()
            .unwrap()
            .contains("input-type")
    );
}

#[test]
fn text_editor_constraints_are_verified_in_semantics_and_mlir() {
    let source = r#"
        export component App inherits Window {
            LineEdit { input-type: number; maximum-length: 4; }
            TextInput { input-type: decimal; decimal-separator: ","; }
            LineEdit { input-type: search; }
            TextArea { maximum-length: 20; }
        }
    "#;
    let compilation = compile(source, "constraints.xr").unwrap();
    let rust = compilation.rust.to_string();
    for mode in ["Number", "Decimal", "Search"] {
        assert!(rust.contains(&format!("InputType :: {mode}")));
    }
    let wrong_type = source.replace("maximum-length: 4", "maximum-length: true");
    assert!(compile(&wrong_type, "constraints.xr").is_err());

    let raw = lower(source, "constraints.xr");
    let wrong_type = raw.replace("name = \"decimal-separator\"", "name = \"maximum-length\"");
    assert!(
        mlir::process(&wrong_type)
            .unwrap_err()
            .contains("maximum-length requires an integer")
    );
    let wrong_kind = raw.replace("kind = \"TextArea\"", "kind = \"Text\"");
    assert!(
        mlir::process(&wrong_kind)
            .unwrap_err()
            .contains("maximum-length requires an integer")
    );
    let wrong_enum = raw.replace("value = \"decimal\"", "value = \"hexadecimal\"");
    assert!(mlir::process(&wrong_enum).is_err());
}

#[test]
fn search_field_checks_live_query_and_candidate_model_types() {
    let source = r#"
        export component App inherits Window {
            in-out property <string> query;
            in property <[string]> matches;
            in property <string> result-query;
            out property <string> requested;
            search := SearchField {
                text <=> root.query;
                live: true;
                suggestions: root.matches;
                suggestions-for: root.result-query;
                search-triggered(query) => { root.requested = query; }
            }
        }
    "#;
    let compiled = compile(source, "search.xr").unwrap();
    assert!(compiled.mlir.contains("xr.control_link"));
    assert!(compiled.mlir.contains("search-triggered"));
    let raw = lower(source, "search.xr");
    assert!(
        mlir::process(&raw.replace("name = \"live\"", "name = \"suggestions\""))
            .unwrap_err()
            .contains("incorrect SearchField property type")
    );
    assert!(
        compile(&source.replace("live: true", "live: 1"), "search.xr")
            .err()
            .unwrap()
            .contains("type mismatch")
    );
}

#[test]
fn decimal_spin_box_uses_float_links_and_typed_precision() {
    let source = r#"
        export component App inherits Window {
            in-out property <float> amount: 0.5;
            out property <float> last: 0.0;
            amount-field := DoubleSpinBox {
                value <=> root.amount;
                decimals: 2;
                step-size: 0.05;
                edited(value) => { root.last = value; }
            }
        }
    "#;
    let compiled = compile(source, "decimal-spin.xr").unwrap();
    assert!(compiled.mlir.contains("xr.control_link"));
    assert!(
        compiled
            .rust
            .to_string()
            .contains("decimal_spin_value_after")
    );
    let raw = lower(source, "decimal-spin.xr");
    assert!(
        mlir::process(&raw.replace("name = \"decimals\"", "name = \"maximum\""))
            .unwrap_err()
            .contains("incorrect DoubleSpinBox property type")
    );
    let wrong = source.replace("<float> amount: 0.5", "<int> amount: 1");
    assert!(
        compile(&wrong, "decimal-spin.xr")
            .err()
            .unwrap()
            .contains("two-way binding requires a writable float property")
    );
    assert!(
        compile(
            "export component Amount inherits DoubleSpinBox { edited(value) => {} }",
            "decimal-root.xr",
        )
        .is_ok()
    );
}

#[test]
fn day_cell_templates_verify_typed_context_and_visual_content() {
    let source = r#"
        component App inherits Window {
            MonthGrid {
                DayCell {
                    accessible-description: cell-today ? "Today" : "";
                    Text { text: "" + cell-date.day + " / " + cell-week; }
                }
            }
        }
    "#;
    let compiled = compile(source, "day-cell.xr").unwrap();
    assert!(compiled.mlir.contains("kind = \"DayCell\""));
    assert!(compiled.mlir.contains("value = \"cell-week\""));
    let raw = lower(source, "day-cell.xr");
    let invalid = raw.replace("value = \"cell-week\"", "value = \"cell-selected\"");
    assert!(
        mlir::process(&invalid)
            .unwrap_err()
            .contains("invalid DayCell parameter or type")
    );
    for (invalid, diagnostic) in [
        (
            source.replace("MonthGrid {", "Rectangle {"),
            "DayCell requires an anonymous template",
        ),
        (
            source.replace("DayCell {", "DayCell { Button {}"),
            "DayCell accepts visual elements",
        ),
        (
            source.replace("DayCell {", "DayCell {} DayCell {"),
            "MonthGrid accepts only one DayCell",
        ),
        (
            source.replace("MonthGrid {", "Text { text: cell-date; } MonthGrid {"),
            "unknown property `cell-date`",
        ),
    ] {
        assert!(
            compile(&invalid, "day-cell.xr")
                .err()
                .unwrap()
                .contains(diagnostic)
        );
    }
}

#[test]
fn calendar_parts_verify_typed_months_weekday_labels_and_activation() {
    let source = r#"
        component App inherits Window {
            in-out property <Date> selected: Date { year: 2021, month: 1, day: 1 };
            in-out property <Date> month: root.selected;
            DayOfWeekRow { week-start: 7; weekday-labels: ["M", "T", "W", "T", "F", "S", "S"]; }
            WeekNumberColumn { display-month: root.month; week-start: 7; }
            grid := MonthGrid {
                date <=> root.selected;
                display-month <=> root.month;
                today: root.selected;
                activated(date) => { root.selected = date; }
            }
        }
    "#;
    let compiled = compile(source, "month-grid.xr").unwrap();
    for kind in ["MonthGrid", "DayOfWeekRow", "WeekNumberColumn"] {
        assert!(compiled.mlir.contains(&format!("kind = \"{kind}\"")));
    }
    for invalid in [
        source.replace("today: root.selected", "today: 7"),
        source.replace(
            "display-month <=> root.month",
            "display-month <=> root.selected",
        ),
        source.replace("activated(date)", "activated()"),
    ] {
        assert!(compile(&invalid, "month-grid.xr").is_err());
    }
    let raw = lower(source, "month-grid.xr");
    let invalid = raw.replace("name = \"today\"", "name = \"weekday-labels\"");
    assert!(
        mlir::process(&invalid)
            .unwrap_err()
            .contains("incorrect Calendar property type")
    );
}

#[test]
fn calendar_lowers_date_values_and_rejects_invalid_schema_and_aliases() {
    let source = r#"
        export component App inherits Window {
            in-out property <Date> selection: Date { year: 2024, month: 2, day: 29 };
            in-out property <Date> month: Date { year: 2024, month: 2, day: 1 };
            out property <int> picked: 0;
            picker := Calendar {
                date <=> root.selection;
                display-month <=> root.month;
                changed(value) => { root.picked = value.day; }
            }
        }
    "#;
    let result = compile(source, "calendar.xr").unwrap();
    assert!(result.mlir.contains("!xr.date"));
    assert!(result.mlir.contains("xr.make_date"));
    assert!(result.mlir.contains("xr.date_field"));
    assert!(result.mlir.contains("xr.control_link"));
    assert!(result.rust.to_string().contains("xrack_runtime :: Date"));
    let raw = lower(source, "calendar.xr");
    assert!(
        mlir::process(&raw.replace("value = \"day\"", "value = \"hours\""))
            .unwrap_err()
            .contains("requires year, month, or day")
    );
    assert!(
        compile(&source.replace("value.day", "value.hours"), "calendar.xr")
            .err()
            .unwrap()
            .contains("Date has no field")
    );
    assert!(
        compile(
            &source.replace(
                "display-month <=> root.month",
                "display-month <=> root.selection"
            ),
            "calendar.xr"
        )
        .err()
        .unwrap()
        .contains("require distinct writable properties")
    );
}

#[test]
fn flow_layout_spacing_and_orientation_are_typed() {
    let source = r#"
        component App inherits Window {
            FlowLayout {
                orientation: vertical;
                horizontal-spacing: 12px;
                alignment: space-evenly;
                cross-axis-alignment: center;
                Text { text: "Item"; }
            }
        }
    "#;
    let raw = lower(source, "flow.xr");
    assert!(mlir::process(&raw).is_ok());
    let wrong_type = raw.replace("name = \"orientation\"", "name = \"vertical-spacing\"");
    assert!(
        mlir::process(&wrong_type)
            .unwrap_err()
            .contains("incorrect FlowLayout property type")
    );
    let wrong_source = source.replace("12px", "true");
    assert!(
        compile(&wrong_source, "flow.xr")
            .err()
            .unwrap()
            .contains("type mismatch")
    );
}

#[test]
fn toolbar_accepts_typed_icons_and_action_references() {
    let source = r#"
        component App inherits Window {
            property <image> marker;
            grid := Action { icon: root.marker; text: "Grid"; }
            ToolBar {
                orientation: vertical;
                wrap: true;
                grid-button := ToolButton { action: grid; icon-only: true; icon-size: 20px; }
            }
        }
    "#;
    assert!(compile(source, "toolbar.xr").is_ok());
    let raw = lower(source, "toolbar.xr");
    let invalid_icon = raw.replace("name = \"icon-size\"", "name = \"icon\"");
    assert!(
        mlir::process(&invalid_icon)
            .unwrap_err()
            .contains("icon requires an image")
    );
    let invalid_wrap = source.replace("wrap: true", "wrap: 10px");
    assert!(
        compile(&invalid_wrap, "toolbar.xr")
            .err()
            .unwrap()
            .contains("type mismatch")
    );
}

#[test]
fn editable_combo_box_validates_text_types_and_keeps_links_independent() {
    let source = r#"
        component App inherits Window {
            in-out property <string> draft;
            in-out property <string> chosen;
            choices := ComboBox {
                editable: true;
                auto-complete: true;
                edit-text <=> root.draft;
                current-value <=> root.chosen;
                accepted(text) => {}
                edited(text) => {}
            }
        }
    "#;
    assert!(compile(source, "combo.xr").is_ok());
    let alias = source.replace(
        "current-value <=> root.chosen",
        "current-value <=> root.draft",
    );
    assert!(
        compile(&alias, "combo.xr")
            .err()
            .unwrap()
            .contains("distinct writable properties")
    );
    let raw = lower(source, "combo.xr");
    assert!(raw.contains("name = \"auto-complete\""));
    let native_alias = raw.replace("value = \"chosen\"", "value = \"draft\"");
    assert!(
        mlir::process(&native_alias)
            .unwrap_err()
            .contains("distinct writable properties")
    );
    let wrong_type = raw.replace("name = \"editable\"", "name = \"placeholder-text\"");
    assert!(
        mlir::process(&wrong_type)
            .unwrap_err()
            .contains("incorrect editable ComboBox property type")
    );
    let invalid_completion = source.replace("auto-complete: true", "auto-complete: 1");
    assert!(
        compile(&invalid_completion, "combo.xr")
            .err()
            .unwrap()
            .contains("type mismatch")
    );
}

fn lower(source: &str, filename: &str) -> String {
    mlir::lower(&parse(source).unwrap(), filename).unwrap()
}

#[test]
fn parses_comments_unicode_and_operator_precedence() {
    let source = r#"/* banner */ export component App inherits Window {
        in-out property <int> count: 1 + 2 * 3;
        property <string> name: "你好\n\"XRack\"";
        property <color> color: #abc;
        title: name;
        Text { text: count > 2 && true ? "yes" : "no"; color: root.color; }
    }"#;
    let model = analyze(source).unwrap();
    let e = &model.components[0].properties[0].init;
    assert_eq!(e.value, "+");
    assert_eq!(e.args[1].value, "*");
    assert_eq!(model.components[0].properties[1].ty.name(), "string");
    assert_eq!(
        model.components[0].root.children[0].bindings[1]
            .value
            .ty
            .name(),
        "color"
    );
}

#[test]
fn reports_both_lexer_and_recovered_parser_errors() {
    for source in [
        "component A inherits Window { @ }",
        "component A inherits Window { width: ; }",
        "component A inherits Window {} trailing",
        "component A inherits Window { title: \"broken; }",
        "",
    ] {
        let errors = parse(source).unwrap_err();
        assert!(!errors.is_empty(), "{source}");
        assert!(errors[0].span.line >= 1);
        assert!(errors[0].span.column >= 1);
    }
}

#[test]
fn rejects_invalid_ui_programs_before_codegen() {
    let cases = [
        ("property <int> a: true;", "type mismatch"),
        ("property <int> _;", "reserved identifier"),
        ("property <int> a: missing;", "unknown property"),
        ("property <int> a: b; property <int> b: a;", "cyclic"),
        ("property <int> a; property <int> a;", "duplicate"),
        (
            "property <int> foo-bar; property <int> foo_bar;",
            "duplicate",
        ),
        ("width: 5;", "type mismatch"),
        (
            "MenuBar { Menu { MenuItem { shortcut: 1; } } }",
            "type mismatch",
        ),
        ("wrong: 5;", "unknown property"),
        ("width: 5px; width: 6px;", "duplicate binding"),
        ("property <bool> a: 1 && true;", "invalid operands"),
        ("property <color> c: #12;", "colors require"),
        (
            "property <int> a: 999999999999999999999999999;",
            "i64 range",
        ),
        ("property <float> a: 1e999;", "non-finite"),
        ("property <string> a: \"\\uD800\";", "invalid string"),
        ("property <int> a: other.count;", "unknown element id"),
        ("foo := Text {} foo := Text {}", "duplicate element"),
        ("Unknown {}", "unknown element"),
        ("ListView { Text {} }", "requires exactly one for element"),
        (
            "TableColumn { title: \"stray\"; }",
            "must be a direct child of TableView",
        ),
        (
            "TableView { TableColumn {} }",
            "requires at least one TableColumn and one repeated TableRow",
        ),
        (
            "TableView { TableColumn {} TableColumn {} for row in 3 : TableRow { Text {} } }",
            "one cell element per column",
        ),
        (
            "TableView { for col in 3 : TableColumn {} for row in 3 : TableRow { Text {} } }",
            "requires static TableColumn",
        ),
        (
            "TableView { resizable-columns: 1; TableColumn {} for row in 3 : TableRow { Text {} } }",
            "type mismatch",
        ),
        (
            "TableView { multi-select: \"yes\"; TableColumn {} for row in 3 : TableRow { Text {} } }",
            "type mismatch",
        ),
        (
            "TableView { selection-target: 1; TableColumn {} for row in 3 : TableRow { Text {} } }",
            "type mismatch",
        ),
        (
            "TableCell { text: \"outside\"; }",
            "must be a direct cell of TableRow",
        ),
        (
            "TableView { TableColumn {} for item in [\"one\"] : TableRow { TableCell { text <=> item; } } }",
            "requires the repeated item of a writable [string] property",
        ),
        (
            "in property <[string]> rows; TableView { TableColumn {} for item in rows : TableRow { TableCell { text <=> item; } } }",
            "requires the repeated item of a writable [string] property",
        ),
        (
            "property <[string]> rows; TableView { TableColumn {} for item[index] in rows : TableRow { TableCell { text <=> index; } } }",
            "requires the repeated item of a writable [string] property",
        ),
        (
            "ListView { for row in 3 : Text {} Text {} }",
            "requires exactly one for element",
        ),
        (
            "for row in 3 : ListView { for item in 4 : Text {} }",
            "repeat templates require anonymous display elements",
        ),
        (
            "Text { callback hit(); callback hit(); }",
            "duplicate or conflicting member",
        ),
        ("Button { clicked() => {} }", "require an id"),
        ("callback hit(step: int); hit() => {}", "expects 1"),
        ("callback hit(a: int, a: int);", "duplicate parameter"),
        (
            "callback hit(); hit() => { root.missing = 1; }",
            "unknown property",
        ),
    ];
    for (body, expected) in cases {
        let source = format!("component App inherits Window {{ {body} }}");
        let error = analyze(&source).unwrap_err();
        assert!(error.contains(expected), "expected {expected}: {error}");
    }
}

#[test]
fn native_mlir_checks_menu_shortcut_binding_type() {
    let raw = lower(
        "export component App inherits Window { MenuBar { Menu { MenuItem { checked: true; } } } }",
        "shortcut.xr",
    );
    let invalid = raw.replace("name = \"checked\"", "name = \"shortcut\"");
    assert!(
        mlir::process(&invalid)
            .unwrap_err()
            .contains("menu shortcut requires a string")
    );
}

#[test]
fn item_delegate_traits_and_writable_fields_are_verified() {
    let source = r#"
        struct Setting { label: string, checked: bool }
        component App inherits Window {
            in-out property <[Setting]> settings;
            ListView {
                for setting in root.settings : CheckDelegate {
                    text: setting.label;
                    checked <=> setting.checked;
                    highlighted: true;
                }
            }
        }
    "#;
    let compiled = compile(source, "item-delegate.xr").unwrap();
    assert!(compiled.mlir.contains("delegate = \"CheckDelegate\""));
    assert!(compiled.mlir.contains("xr.row_link"));
    let no_link = source.replace("checked <=> setting.checked", "checked: setting.checked");
    assert!(
        compile(&no_link, "item-delegate.xr")
            .err()
            .unwrap()
            .contains("two-way binding to a writable row field")
    );
    let bad_highlight = source.replace("highlighted: true", "highlighted: \"true\"");
    assert!(compile(&bad_highlight, "item-delegate.xr").is_err());
    let raw = lower(source, "item-delegate.xr");
    let invalid = raw.replace(
        "delegate = \"CheckDelegate\"",
        "delegate = \"SwitchDelegate\"",
    );
    assert!(
        mlir::process(&invalid)
            .unwrap_err()
            .contains("invalid item delegate kind")
    );
}

#[test]
fn action_groups_verify_membership_values_and_exclusive_state() {
    let source = r#"
        component App inherits Window {
            out property <string> last;
            modes := ActionGroup {
                normal := Action { checked: true; }
                activated(value) => { root.last = value + modes.current-value; }
            }
            compact := Action { group: modes; value: "dense"; }
            Button { action: normal; }
            Button { action: compact; }
        }
    "#;
    let output = compile(source, "action-group.xr").unwrap();
    assert!(output.mlir.contains("xr.action_group_ref"));
    assert!(output.mlir.contains("xr.action_group_read"));
    assert!(output.mlir.contains("name = \"current-value\""));
    for (invalid, diagnostic) in [
        (
            source.replace("value: \"dense\"", "value: \"normal\""),
            "duplicate ActionGroup value",
        ),
        (
            source.replace("group: modes", "group: compact"),
            "requires the id of an ActionGroup",
        ),
        (
            source.replace("checked: true;", "group: modes;"),
            "already belongs",
        ),
        (
            source.replace("value: \"dense\";", "checked: true;"),
            "single initial selection",
        ),
        (
            source.replace(
                "checked: true;",
                "checked: modes.current-value == \"normal\";",
            ),
            "requires a literal",
        ),
        (
            source.replace(
                "modes := ActionGroup {",
                "modes := ActionGroup { current-value: \"dense\";",
            ),
            "single initial selection",
        ),
        (
            source.replace(
                "modes := ActionGroup {",
                "modes := ActionGroup { exclusive: false;",
            ),
            "current-value requires an exclusive group",
        ),
    ] {
        assert!(
            compile(&invalid, "action-group.xr")
                .err()
                .unwrap()
                .contains(diagnostic)
        );
    }
    let raw = lower(source, "action-group.xr");
    let duplicate = raw.replace("value = \"dense\"", "value = \"normal\"");
    assert_ne!(raw, duplicate);
    assert!(
        mlir::process(&duplicate)
            .unwrap_err()
            .contains("duplicate ActionGroup value")
    );
}

#[test]
fn action_references_are_validated_before_and_after_lowering() {
    let source = r#"
        export component App inherits Window {
            toggle := Action { text: "切换"; checkable: true; }
            button := Button { action: toggle; enabled: true; }
        }
    "#;
    let raw = lower(source, "action.xr");
    assert!(raw.contains("xr.action_ref"));
    assert!(compile(source, "action.xr").is_ok());
    let wrong_owner = raw.replace("owner = \"toggle\"", "owner = \"button\"");
    assert!(
        mlir::process(&wrong_owner)
            .unwrap_err()
            .contains("requires an Action owner")
    );
    for (bad, diagnostic) in [
        (
            source.replace("action: toggle", "action: button"),
            "requires the id of an Action",
        ),
        (
            source.replace("enabled: true;", "clicked() => {}"),
            "activation handler belongs",
        ),
        (
            source.replace("toggle := Action", "Action"),
            "named static declaration",
        ),
    ] {
        assert!(
            compile(&bad, "action.xr")
                .err()
                .unwrap()
                .contains(diagnostic)
        );
    }
}

#[test]
fn table_column_and_cell_structure_is_verified_in_mlir() {
    let source = r#"component App inherits Window {
        TableView {
            row-height: 32px;
            selection-target: cells;
            TableColumn { title: "A"; resizable: false; }
            for row in 3 : TableRow { Text { text: "Row " + row; } }
        }
    }"#;
    let lowered = lower(source, "table.xr");
    assert!(mlir::process(&lowered).is_ok());
    assert!(lowered.contains("!xr.selectiontarget"));
    let invalid_target = lowered.replace("value = \"cells\"", "value = \"diagonal\"");
    assert!(mlir::process(&invalid_target).is_err());
    for flag in ["resizable-columns", "multi-select"] {
        let invalid_flag = lowered.replace("name = \"row-height\"", &format!("name = \"{flag}\""));
        assert!(
            mlir::process(&invalid_flag)
                .unwrap_err()
                .contains("table interaction flags require a bool value")
        );
    }
    let malformed = lowered.replace("kind = \"TableColumn\"", "kind = \"Rectangle\"");
    assert_ne!(malformed, lowered);
    assert!(
        mlir::process(&malformed)
            .unwrap_err()
            .contains("TableView requires static TableColumn")
    );
}

#[test]
fn table_edit_links_a_writable_model_to_the_repeat_item() {
    let source = r#"component App inherits Window {
        in-out property <[string]> rows: ["one"];
        TableView {
            TableColumn { title: "A"; }
            for entry[index] in root.rows : TableRow { TableCell { text <=> entry; } }
        }
    }"#;
    let mlir = lower(source, "table-edit.xr");
    assert!(mlir.contains("\"xr.row_link\""));
    assert!(mlir::process(&mlir).is_ok());
    let wrong_item = mlir.replace("item = \"entry\"", "item = \"other\"");
    assert!(
        mlir::process(&wrong_item)
            .unwrap_err()
            .contains("requires the repeated string item")
    );
    let read_only = mlir.replace("visibility = \"in-out\"", "visibility = \"in\"");
    assert!(mlir::process(&read_only).is_err());
}

#[test]
fn record_fields_are_typed_through_mlir_and_rust_generation() {
    let source = r#"
        export struct Entry { name: string, count: int }
        export component App inherits Window {
            property <Entry> active: Entry { count: 2, name: "alpha" };
            in-out property <[Entry]> entries: [Entry { name: "beta", count: 3 }];
            pure callback label(Entry) -> string;
            label(value) => { value.name + value.count }
            title: root.label(root.active);
            TableView {
                TableColumn { title: "名称"; }
                for entry in root.entries : TableRow { TableCell { text <=> entry.name; } }
            }
        }
    "#;
    let ir = lower(source, "record.xr");
    assert!(ir.contains("\"xr.record_decl\""));
    assert!(ir.contains("\"xr.get_field\""));
    assert!(ir.contains("!xr.recordlist<\"Entry\">"));
    assert!(mlir::process(&ir).is_ok());
    let invalid = ir.replace("value = \"name\"", "value = \"missing\"");
    assert!(
        mlir::process(&invalid)
            .unwrap_err()
            .contains("unknown record field")
    );
    let invalid = source.replace("text <=> entry.name", "text <=> entry.count");
    assert!(
        compile(&invalid, "record.xr")
            .err()
            .unwrap()
            .contains("type mismatch")
    );
    let invalid = source.replace(
        "Entry { count: 2, name: \"alpha\" }",
        "Entry { name: \"alpha\" }",
    );
    assert!(
        compile(&invalid, "record.xr")
            .err()
            .unwrap()
            .contains("missing record field")
    );
    assert!(compile(source, "record.xr").is_ok());
}

#[test]
fn table_editors_verify_typed_record_links() {
    let source = r#"
        struct RowData { units: int, available: bool, load: float }
        export component App inherits Window {
            in-out property <[RowData]> items;
            TableView {
                TableColumn { title: "数量"; }
                TableColumn { title: "可用"; }
                TableColumn { title: "负载"; }
                for row in root.items : TableRow {
                    SpinBox { value <=> row.units; }
                    CheckBox { checked <=> row.available; }
                    Slider { value <=> row.load; }
                }
            }
        }
    "#;
    let ir = lower(source, "editors.xr");
    assert!(mlir::process(&ir).is_ok());
    assert_eq!(ir.matches("\"xr.row_link\"").count(), 3);
    assert!(compile(&source.replace("CheckBox", "Switch"), "editors.xr").is_ok());
    assert!(compile(&source.replace("Slider", "Dial"), "editors.xr").is_ok());
    let malformed = ir.replace("name = \"value\"", "name = \"checked\"");
    assert!(
        mlir::process(&malformed)
            .unwrap_err()
            .contains("matching field of a table model")
    );
    let wrong_type = source.replace("value <=> row.units", "value <=> row.available");
    assert!(
        compile(&wrong_type, "editors.xr")
            .err()
            .unwrap()
            .contains("type mismatch")
    );
    let int_slider = source.replace("value <=> row.load", "value <=> row.units");
    assert!(
        compile(&int_slider, "editors.xr")
            .err()
            .unwrap()
            .contains("table editor expects float, found int")
    );
    let read_only = source.replace("in-out property", "in property");
    assert!(
        compile(&read_only, "editors.xr")
            .err()
            .unwrap()
            .contains("writable record list")
    );
    let unlinked = source.replace("value <=> row.units", "value: row.units");
    assert!(
        compile(&unlinked, "editors.xr")
            .err()
            .unwrap()
            .contains("require an anonymous editor")
    );
}

#[test]
fn split_view_requires_static_panes_and_typed_handle_properties() {
    let source = r#"export component App inherits SplitView {
        orientation: vertical;
        handle-size: 10px;
        handle-hit-size: 24px;
        out property <bool> dragging: self.resizing;
        Rectangle { min-height: 35px; }
        Rectangle { preferred-height: 60px; }
        resized() => { }
    }"#;
    let lowered = lower(source, "split.xr");
    assert!(mlir::process(&lowered).is_ok());
    let output = compile(source, "split.xr").unwrap();
    assert!(output.mlir.contains("xr.split_resizing"));
    let invalid = lowered.replace("name = \"handle-hit-size\"", "name = \"handle-color\"");
    assert!(
        mlir::process(&invalid)
            .unwrap_err()
            .contains("incorrect SplitView property type")
    );
    let invalid = lowered.replace("name = \"handle-hit-size\"", "name = \"resizing\"");
    assert!(
        mlir::process(&invalid)
            .unwrap_err()
            .contains("SplitView resizing is read-only")
    );
    let invalid = source.replace("handle-hit-size: 24px", "resizing: true");
    assert!(
        analyze(&invalid)
            .unwrap_err()
            .contains("property `SplitView.resizing` is read-only")
    );
    let repeated = source.replace(
        "Rectangle { min-height",
        "for pane in 2 : Rectangle { min-height",
    );
    assert!(
        compile(&repeated, "split.xr")
            .err()
            .unwrap()
            .contains("stable child positions")
    );
}

#[test]
fn split_handle_template_parameters_are_typed_and_scoped() {
    let source = r#"
        export component App inherits SplitView {
            SplitHandle {
                background: handle-hovered ? #ff0000 : #0000ff;
                if handle-pressed: Rectangle { visible: handle-index == 0; }
            }
            Rectangle {}
            Rectangle {}
        }
    "#;
    let result = compile(source, "split-handle.xr").unwrap();
    assert!(result.mlir.contains("kind = \"SplitHandle\""));
    assert!(result.mlir.contains("value = \"handle-hovered\""));
    assert!(result.rust.to_string().contains(". decorate"));
    let raw = lower(source, "split-handle.xr");
    let wrong_type = raw.replace("value = \"handle-hovered\"", "value = \"handle-index\"");
    assert!(
        mlir::process(&wrong_type)
            .unwrap_err()
            .contains("invalid SplitHandle parameter or type")
    );
    for (from, to, expected) in [
        (
            "inherits SplitView",
            "inherits Rectangle",
            "directly in SplitView",
        ),
        (
            "SplitHandle {",
            "grip := SplitHandle {",
            "anonymous template",
        ),
        (
            "SplitHandle {",
            "SplitHandle {} SplitHandle {",
            "only one SplitHandle",
        ),
        (
            "if handle-pressed: Rectangle",
            "if handle-pressed: Button",
            "read-only bindings",
        ),
        (
            "Rectangle {}\n        }",
            "Rectangle { visible: handle-pressed; }\n        }",
            "unknown property",
        ),
    ] {
        let invalid = source.replace(from, to);
        assert_ne!(invalid, source);
        let error = analyze(&invalid).unwrap_err();
        assert!(error.contains(expected), "{error}");
    }
}

#[test]
fn split_fill_is_a_typed_direct_pane_reference() {
    let source = r#"
        component Editor inherits Rectangle {}
        component Demo inherits Window {
            outside := Rectangle {}
            view := SplitView {
                fill-pane: editor;
                sidebar := Rectangle { inner := Rectangle {} }
                editor := Editor {}
            }
        }
    "#;
    let result = compile(source, "split-fill.xr").unwrap();
    assert!(result.mlir.contains("xr.split_fill"));
    let rust = result.rust.to_string();
    assert!(rust.contains(":: xrack_runtime :: SplitState"));
    assert!(
        rust.match_indices(". populate (& mut ").any(|(start, _)| {
            rust[start..]
                .split_once(';')
                .is_some_and(|(call, _)| call.trim_end().ends_with(", Some (1usize))"))
        }),
        "{rust}"
    );
    for target in ["outside", "inner", "missing"] {
        let error = analyze(&source.replace("fill-pane: editor", &format!("fill-pane: {target}")))
            .unwrap_err();
        assert!(
            error.contains("fill-pane requires a named direct pane"),
            "{error}"
        );
    }
    let raw = lower(source, "split-fill.xr");
    assert!(
        mlir::process(&raw.replace("target = \"editor\"", "target = \"outside\""))
            .unwrap_err()
            .contains("fill-pane requires a named direct pane")
    );
}

#[test]
fn split_state_uses_typed_reads_and_validates_restore_effects() {
    let source = r#"
        export component App inherits Window {
            out property <string> layout: split.save-state();
            in property <string> saved;
            callback restore();
            restore => { split.restore-state(root.saved); }
            split := SplitView { left := Rectangle {} right := Rectangle {} }
        }
    "#;
    let output = compile(source, "split-state.xr").unwrap();
    assert!(output.mlir.contains("xr.split_state_read"));
    assert!(output.mlir.contains("xr.split_restore"));
    assert!(output.rust.to_string().contains("save_split_state"));
    for (from, to, expected) in [
        (
            "split.save-state()",
            "split.save-state(1)",
            "takes no arguments",
        ),
        (
            "split.restore-state(root.saved)",
            "split.restore-state(1)",
            "type mismatch",
        ),
        (
            "callback restore();",
            "pure callback restore();",
            "not allowed in a pure callback",
        ),
    ] {
        let error = compile(&source.replace(from, to), "split-state.xr")
            .err()
            .unwrap();
        assert!(error.contains(expected), "{error}");
    }
    let raw = lower(source, "split-state.xr");
    let wrong_owner = raw.replace("owner = \"split\"", "owner = \"root\"");
    assert!(
        mlir::process(&wrong_owner)
            .unwrap_err()
            .contains("statically addressed SplitView")
    );
}

#[test]
fn tooltip_requires_one_attachment_and_exclusive_content() {
    let source = r#"component App inherits Window {
        Button { text: "Save"; Tooltip { text: "Saves the document"; delay: 20; timeout: 100; } }
        Rectangle { Tooltip { VerticalLayout { Text { text: "Details"; } } } }
    }"#;
    let ir = lower(source, "tooltip.xr");
    assert!(mlir::process(&ir).is_ok());
    assert!(compile(source, "tooltip.xr").is_ok());
    let wrong_type = ir.replace("name = \"delay\"", "name = \"text\"");
    assert!(
        mlir::process(&wrong_type)
            .unwrap_err()
            .contains("incorrect Tooltip property type")
    );
    for (source, expected) in [
        (
            "component A inherits Tooltip {}",
            "Tooltip requires a visual owner",
        ),
        (
            "component A inherits Window { Rectangle { Tooltip {} Tooltip {} } }",
            "at most one Tooltip",
        ),
        (
            "component A inherits Window { Rectangle { Tooltip { text: \"A\"; Text {} } } }",
            "Tooltip requires text or a single content child",
        ),
        (
            "component A inherits Window { Rectangle { for index in 3 : Tooltip {} } }",
            "Tooltip cannot be a repeated child",
        ),
        (
            "component Popup inherits Dialog {} component A inherits Window { Rectangle { Tooltip { Popup {} } } }",
            "Tooltip content cannot be a popup or virtual view",
        ),
    ] {
        assert!(
            compile(source, "tooltip.xr")
                .err()
                .unwrap()
                .contains(expected)
        );
    }
}

#[test]
fn programmatic_list_selection_compiles_with_typed_queries_and_read_only_state() {
    let source = r#"
        component App inherits Window {
            out property <int> count: list.selection-count;
            out property <bool> found: list.is-row-selected(1);
            callback choose();
            choose => { list.select-row(1); list.toggle-row(0); list.select-all(); list.clear-selection(); }
            list := StandardListView {
                model: ["one", "two"];
                multi-select: true;
            }
        }
    "#;
    let compiled = compile(source, "list-selection-api.xr").unwrap();
    assert!(compiled.mlir.contains("\"xr.list_row_selected\""));
    assert!(compiled.mlir.contains("\"xr.table_selection_command\""));
    assert!(compiled.mlir.contains("method = \"toggle-row\""));
    let readonly = source.replace("multi-select: true", "selection-count: 2");
    assert!(compile(&readonly, "list-selection-api.xr").is_err());
    let bad_query = source.replace("is-row-selected(1)", "is-row-selected(true)");
    assert!(compile(&bad_query, "list-selection-api.xr").is_err());
    let pure = source.replace("callback choose();", "pure callback choose();");
    assert!(compile(&pure, "list-selection-api.xr").is_err());
}

#[test]
fn tree_selection_compiles_with_typed_queries_and_effects() {
    let source = r#"
        struct Branch { id: int, parent: int }
        component App inherits Window {
            in property <[Branch]> branches;
            out property <int> count: tree.selection-count;
            out property <bool> found: tree.is-key-selected(5) && tree.is-row-selected(0);
            callback choose();
            choose => {
                tree.select-key(5);
                tree.select-row(0);
                tree.toggle-row(1);
                tree.select-all();
                tree.clear-selection();
            }
            tree := TreeView {
                multi-select: true;
                for row in root.branches : TreeRow { key: row.id; parent-key: row.parent; }
            }
        }
    "#;
    let compiled = compile(source, "tree-selection.xr").unwrap();
    for op in [
        "\"xr.tree_query\"",
        "\"xr.table_selection_read\"",
        "\"xr.table_selection_command\"",
        "\"xr.source_selection\"",
    ] {
        assert!(compiled.mlir.contains(op), "missing {op}");
    }
    for (from, to) in [
        ("multi-select: true", "selection-count: 2"),
        ("is-key-selected(5)", "is-key-selected(true)"),
        ("tree.select-key(5)", "tree.select-column(5)"),
        ("callback choose();", "pure callback choose();"),
        ("multi-select: true", "multi-select: tree.has-selection"),
    ] {
        assert!(
            compile(&source.replace(from, to), "tree-selection.xr").is_err(),
            "{to}"
        );
    }
    let invalid = compiled
        .mlir
        .replace("method = \"toggle-row\"", "method = \"select-column\"");
    assert_ne!(invalid, compiled.mlir);
    assert!(mlir::Module::parse(&invalid).is_err());
}

#[test]
fn tree_batch_selection_uses_a_typed_integer_model() {
    let source = r#"
        struct Branch { id: int, parent: int }
        export component App inherits Window {
            in property <[Branch]> branches;
            in property <[int]> keys: [9, 3];
            out property <int> first: Model.at(tree.selected-keys, 0, -1);
            callback choose();
            choose => { tree.select-keys(root.keys); }
            tree := TreeView {
                multi-select: true;
                for row in root.branches : TreeRow { key: row.id; parent-key: row.parent; }
            }
            for key in root.keys : Text { text: "" + key; }
        }
    "#;
    let compiled = compile(source, "tree-key-batch.xr").unwrap();
    for op in [
        "!xr.intlist",
        "\"xr.int_array\"",
        "\"xr.tree_selection_keys\"",
        "\"xr.model_at\"",
    ] {
        assert!(compiled.mlir.contains(op), "missing {op}");
    }
    for (from, to) in [
        ("multi-select: true", "selected-keys: [9, 3]"),
        (
            "Model.at(tree.selected-keys, 0, -1)",
            "Model.at(tree.selected-keys, 0, false)",
        ),
        ("select-keys(root.keys)", "select-keys(\"9\")"),
        ("select-keys(root.keys)", "select-keys([9, \"3\"])"),
        ("callback choose();", "pure callback choose();"),
    ] {
        assert!(
            compile(&source.replace(from, to), "tree-key-batch.xr").is_err(),
            "{to}"
        );
    }
    let invalid = compiled
        .mlir
        .replace("method = \"select-keys\"", "method = \"select-key\"");
    assert_ne!(invalid, compiled.mlir);
    assert!(mlir::Module::parse(&invalid).is_err());
}

#[test]
fn keyed_lists_share_typed_selection_snapshots_and_batch_commands() {
    for (kind, rows) in [
        (
            "StandardListView",
            "model: root.records; text-role: \"label\";",
        ),
        (
            "ListView",
            "for row in root.records : Text { text: row.label; }",
        ),
        (
            "GridView",
            "for row in root.records : Text { text: row.label; }",
        ),
    ] {
        let source = format!(
            r#"
            struct ListKeyRow {{ id: int, label: string }}
            export component App inherits Window {{
                in property <[ListKeyRow]> records;
                out property <[int]> saved: view.selected-keys;
                callback restore();
                restore => {{ view.select-keys(root.saved); }}
                view := {kind} {{
                    row-key-field: "id";
                    {rows}
                }}
            }}
        "#
        );
        let compiled = compile(&source, "list-key-batch.xr").unwrap();
        assert!(compiled.mlir.contains("\"xr.list_selection_keys\""));
        assert!(compiled.mlir.contains("method = \"select-keys\""));
        let unkeyed = source.replace("row-key-field: \"id\";", "");
        assert!(compile(&unkeyed, "list-key-batch.xr").is_err());
        let invalid = compiled
            .mlir
            .replace("\"xr.list_selection_keys\"", "\"xr.tree_selection_keys\"");
        assert!(mlir::Module::parse(&invalid).is_err());
    }
}

#[test]
fn keyed_tables_validate_role_and_projection_lookups() {
    for (kind, rows) in [
        (
            "TableView",
            "TableColumn {} for entry in root.items : TableRow { TableCell { text: entry.label; } }",
        ),
        (
            "StandardTableView",
            "model: root.items; TableColumn { field: \"label\"; }",
        ),
    ] {
        let source = format!(
            r#"
            struct KeyedTableItem {{ id: int, label: string }}
            export component App inherits Window {{
                in property <[KeyedTableItem]> items;
                out property <[int]> keys: grid.selected-keys;
                out property <int> index: grid.view-index-for-key(123);
                callback go();
                go => {{
                    grid.select-key(123);
                    grid.select-keys([123, 42]);
                    grid.position-view-at-key(42, ScrollAlignment.end);
                }}
                grid := {kind} {{ row-key-field: "id"; {rows} }}
            }}
        "#
        );
        let compiled = compile(&source, "keyed-table.xr").unwrap();
        assert!(compiled.mlir.contains("\"xr.list_selection_keys\""));
        assert!(compiled.mlir.contains("value = \"view-index-for-key\""));
        assert!(compiled.mlir.contains("method = \"position-view-at-key\""));
        assert!(
            compile(
                &source.replace("row-key-field: \"id\";", ""),
                "keyed-table.xr"
            )
            .is_err()
        );
        assert!(
            compile(
                &source.replace("row-key-field: \"id\"", "row-key-field: \"label\""),
                "keyed-table.xr"
            )
            .is_err()
        );
        let raw = compiled
            .mlir
            .replace("\"xr.list_selection_keys\"", "\"xr.tree_selection_keys\"");
        assert_ne!(raw, compiled.mlir);
        assert!(mlir::Module::parse(&raw).is_err());
    }
}

#[test]
fn live_integer_list_bindings_emit_explicit_runtime_values() {
    let document = parse(
        r#"
        export component App inherits Window {
            in-out property <[int]> numbers: [1, 2];
            pure callback echo([int]) -> [int];
            echo(keys) => { keys }
        }
    "#,
    )
    .unwrap();
    let rust = xrack_compiler::generate_live_document(&document, "numbers.xr", None)
        .unwrap()
        .to_string();
    assert!(rust.contains("Value :: IntList (value)"));
    assert!(rust.contains("Value :: IntList (arg0)"));
}

#[test]
fn virtual_lists_accept_typed_multi_selection_bindings_and_events() {
    for kind in ["ListView", "GridView", "StandardListView"] {
        let rows = if kind == "StandardListView" {
            "model: [\"one\", \"two\"];"
        } else {
            "for item in [\"one\", \"two\"] : Text { text: item; }"
        };
        let source = format!(
            r#"
            component App inherits Window {{
                out property <int> selected: 0;
                view := {kind} {{
                    {rows}
                    multi-select: true;
                    selection-changed(count) => {{ root.selected = count; }}
                }}
            }}
        "#
        );
        let compiled = compile(&source, "list-multi.xr").unwrap();
        assert!(compiled.mlir.contains("name = \"selection-changed\""));
        assert!(compiled.mlir.contains("name = \"multi-select\""));
        let invalid = source.replace("multi-select: true", "multi-select: 1");
        assert!(compile(&invalid, "list-multi.xr").is_err());
    }
}

#[test]
fn list_typeahead_roles_validate_against_record_fields() {
    let source = r#"
        struct Entry { id: int, alias: string }
        component App inherits Window {
            property <[Entry]> rows;
            property <string> field: "alias";
            GridView {
                typeahead-field: "alias";
                typeahead-enabled: true;
                for row in root.rows : Text { text: row.alias; }
            }
        }
    "#;
    let compiled = compile(source, "typeahead.xr").unwrap();
    assert!(compiled.mlir.contains("name = \"typeahead-field\""));
    for (invalid, expected) in [
        ("\"id\"", "string field"),
        ("root.field", "constant string"),
    ] {
        let input = source.replace(
            "typeahead-field: \"alias\"",
            &format!("typeahead-field: {invalid}"),
        );
        let error = compile(&input, "typeahead.xr").err().unwrap();
        assert!(error.contains(expected), "{error}");
    }
    let raw = lower(
        &source.replace("typeahead-field: \"alias\"", "row-key-field: \"id\""),
        "typeahead.xr",
    );
    let broken = raw.replace("name = \"row-key-field\"", "name = \"typeahead-field\"");
    assert_ne!(raw, broken);
    assert!(
        mlir::process(&broken)
            .unwrap_err()
            .contains("typeahead-field")
    );
}

#[test]
fn list_sections_require_a_constant_string_record_field() {
    let source = r#"
        struct Entry { id: int, title: string, group: string }
        component App inherits Window {
            property <[Entry]> records;
            property <string> dynamic: "group";
            StandardListView {
                model: root.records;
                text-role: "title";
                section-field: "group";
                section-height: 26px;
                sticky-section-headers: true;
            }
            ListView {
                section-field: "group";
                for row in root.records : Text { text: row.title; }
            }
        }
    "#;
    let valid = compile(source, "section-list.xr").unwrap();
    assert!(valid.mlir.contains("name = \"section-field\""));
    for (bad, expected) in [
        ("section-field: \"id\"", "string field"),
        ("section-field: \"missing\"", "string field"),
        ("section-field: root.dynamic", "constant string"),
    ] {
        let broken = source.replace("section-field: \"group\"", bad);
        let error = compile(&broken, "section-list.xr").err().unwrap();
        assert!(error.contains(expected), "{error}");
    }
    let key_only = source.replace("section-field: \"group\"", "row-key-field: \"id\"");
    let raw = lower(&key_only, "section-list.xr");
    let invalid = raw.replace("name = \"row-key-field\"", "name = \"section-field\"");
    assert_ne!(raw, invalid);
    assert!(
        mlir::process(&invalid)
            .unwrap_err()
            .contains("section-field")
    );
}

#[test]
fn section_header_template_has_typed_parameters_and_a_sectioned_list_owner() {
    let template = r#"SectionHeader {
        background: section-pinned ? #815329 : #304565;
        accessible-name: section-label;
        Text { text: section-label + " " + section-first-index + " " + section-count; }
    }"#;
    let source = format!(
        r#"
        struct Entry {{ group: string, title: string }}
        component App inherits Window {{
            property <[Entry]> entries;
            StandardListView {{
                model: root.entries;
                text-role: "title";
                section-field: "group";
                {template}
            }}
        }}
        "#
    );
    let compiled = compile(&source, "section-header.xr").unwrap();
    assert!(compiled.mlir.contains("kind = \"SectionHeader\""));
    let invalid_type = compiled
        .mlir
        .replace("value = \"section-label\"", "value = \"section-count\"");
    assert_ne!(compiled.mlir, invalid_type);
    assert!(
        mlir::Module::parse(&invalid_type)
            .err()
            .unwrap()
            .contains("invalid SectionHeader parameter")
    );
    let missing_field = source.replace("section-field: \"group\";", "");
    assert!(
        compile(&missing_field, "section-header.xr")
            .err()
            .unwrap()
            .contains("section-field")
    );
    let duplicate = source.replace(template, &format!("{template} {template}"));
    assert!(
        compile(&duplicate, "section-header.xr")
            .err()
            .unwrap()
            .contains("at most one SectionHeader")
    );
    let outside = source.replace(
        "property <[Entry]> entries;",
        "property <[Entry]> entries; property <string> invalid: section-label;",
    );
    assert!(compile(&outside, "section-header.xr").is_err());
}

#[test]
fn tree_view_roles_require_int_fields_from_a_record_model() {
    let source = r#"
        struct Entry { id: int, parent: int, label: string }
        component App inherits Window {
            property <[Entry]> records;
            TreeView {
                for row in root.records : TreeRow {
                    key: row.id;
                    parent-key: row.parent;
                    Text { text: row.label; }
                }
            }
        }
    "#;
    let ir = lower(source, "tree.xr");
    assert!(mlir::process(&ir).is_ok());
    assert!(compile(source, "tree.xr").is_ok());
    let wrong_type = source.replace("key: row.id", "key: row.label");
    assert!(
        compile(&wrong_type, "tree.xr")
            .err()
            .unwrap()
            .contains("type mismatch")
    );
    let invalid_role = source.replace("parent-key: row.parent", "parent-key: 0");
    assert!(
        compile(&invalid_role, "tree.xr")
            .err()
            .unwrap()
            .contains("requires an integer field of the repeated record")
    );
    let malformed = ir.replace("name = \"parent-key\"", "name = \"other\"");
    assert!(
        mlir::process(&malformed)
            .unwrap_err()
            .contains("TreeRow requires integer key and parent-key")
    );
}

#[test]
fn tree_disclosure_is_a_static_visual_part_of_a_tree_row() {
    let part = r#"TreeDisclosure {
        background: tree.is-expanded(row.id) ? #007d66 : #b15d1e;
        Text { text: row.label; }
    }"#;
    let source = format!(
        r#"
        struct Entry {{ id: int, parent: int, label: string }}
        component App inherits Window {{
            property <[Entry]> rows;
            tree := TreeView {{
                for row in root.rows : TreeRow {{
                    key: row.id;
                    parent-key: row.parent;
                    {part}
                }}
            }}
        }}
        "#
    );
    let compiled = compile(&source, "tree-disclosure.xr").unwrap();
    assert!(compiled.mlir.contains("kind = \"TreeDisclosure\""));
    assert!(compiled.mlir.contains("\"xr.tree_query\""));
    for (replacement, message) in [
        (format!("{part} TreeDisclosure {{}}"), "at most one"),
        (format!("if true : {part}"), "requires a static child"),
        ("TreeDisclosure { width: 80px; }".into(), "unknown property"),
        (
            "TreeDisclosure { TreeCell {} }".into(),
            "visual elements and read-only bindings",
        ),
    ] {
        let broken = source.replace(part, &replacement);
        let error = compile(&broken, "tree-disclosure.xr").err().unwrap();
        assert!(error.contains(message), "{error}");
    }
    let misplaced = "component App inherits Window { TreeDisclosure {} }";
    assert!(
        compile(misplaced, "tree-disclosure.xr")
            .err()
            .unwrap()
            .contains("static child of TreeRow")
    );

    let invalid_mlir = compiled
        .mlir
        .replace("kind = \"Text\"", "kind = \"Button\"");
    assert_ne!(compiled.mlir, invalid_mlir);
    let error = mlir::Module::parse(&invalid_mlir).err().unwrap();
    assert!(
        error.contains("TreeDisclosure accepts visual elements"),
        "{error}"
    );
}

#[test]
fn tree_commands_and_queries_keep_typed_operands_and_owner_validation() {
    let source = r#"
        struct Entry { id: int, parent: int }
        component App inherits Window {
            property <[Entry]> records;
            out property <bool> open: directory.is-expanded(1);
            out property <int> depth: directory.depth(3);
            out property <int> position: directory.row-for-key(3);
            callback reveal();
            reveal => {
                directory.expand-to(3);
                directory.expand-recursively(1, 2);
                directory.collapse-recursively();
                directory.toggle-expanded(1);
            }
            directory := TreeView {
                for row in root.records : TreeRow { key: row.id; parent-key: row.parent; Text { text: "row"; } }
            }
        }
    "#;
    let raw = lower(source, "tree-commands.xr");
    let compiled = compile(source, "tree-commands.xr").unwrap();
    assert!(compiled.mlir.contains("xr.tree_command"));
    assert!(compiled.mlir.contains("xr.tree_query"));
    for (from, to, expected) in [
        (
            "directory.expand-to(3)",
            "directory.expand-to(true)",
            "type mismatch",
        ),
        (
            "directory.expand-to(3)",
            "directory.expand-to()",
            "incorrect number of arguments",
        ),
        (
            "callback reveal();",
            "pure callback reveal();",
            "not allowed in a pure callback",
        ),
    ] {
        let invalid = source.replace(from, to);
        let error = compile(&invalid, "tree-commands.xr").err().unwrap();
        assert!(error.contains(expected), "{error}");
    }
    for (from, to, expected) in [
        (
            "owner = \"directory\"",
            "owner = \"root\"",
            "statically addressed TreeView",
        ),
        (
            "value = \"is-expanded\"",
            "value = \"depth\"",
            "typed tree query",
        ),
        (
            "method = \"expand-to\"",
            "method = \"unknown\"",
            "invalid tree command",
        ),
    ] {
        let invalid = raw.replace(from, to);
        assert_ne!(raw, invalid);
        let error = mlir::process(&invalid).unwrap_err();
        assert!(error.contains(expected), "{error}");
    }
    let cyclic = source.replace(
        "property <[Entry]> records;",
        "property <[Entry]> records: [Entry { id: directory.depth(1), parent: -1 }];",
    );
    let error = compile(&cyclic, "tree-commands.xr").err().unwrap();
    assert!(error.contains("cyclic"), "{error}");
}

#[test]
fn tree_root_keys_are_typed_and_cannot_form_query_initialization_cycles() {
    let source = r#"
        struct Entry { id: int, parent: int }
        component App inherits Window {
            property <[Entry]> records;
            in property <int> branch: 1;
            directory := TreeView {
                root-key: root.branch;
                for row in root.records : TreeRow { key: row.id; parent-key: row.parent; Text { text: "row"; } }
            }
        }
    "#;
    assert!(compile(source, "tree-root.xr").is_ok());
    for (value, expected) in [("true", "type mismatch"), ("directory.depth(1)", "cyclic")] {
        let invalid = source.replace("root-key: root.branch", &format!("root-key: {value}"));
        let error = compile(&invalid, "tree-root.xr").err().unwrap();
        assert!(error.contains(expected), "{error}");
    }
    let labels = source.replace("root-key: root.branch", "indentation: 18px");
    let invalid =
        lower(&labels, "tree-root.xr").replace("name = \"indentation\"", "name = \"root-key\"");
    assert!(
        mlir::process(&invalid)
            .unwrap_err()
            .contains("root-key requires an integer")
    );
}

#[test]
fn tree_filters_require_a_typed_record_field() {
    let source = r#"
        struct Details { label: string }
        struct Entry { id: int, parent: int, details: Details }
        component App inherits Window {
            property <[Entry]> records;
            in property <string> query;
            directory := TreeView {
                filter-field: "details.label";
                filter-text: root.query;
                for row in root.records : TreeRow { key: row.id; parent-key: row.parent; Text { text: row.details.label; } }
            }
        }
    "#;
    let output = compile(source, "tree-filter.xr").unwrap();
    assert!(output.rust.to_string().contains("project_model"));
    for (from, to, expected) in [
        (
            r#"filter-field: "details.label";"#,
            "",
            "filter-text requires filter-field",
        ),
        (r#""details.label""#, r#""id""#, "requires a string field"),
        ("filter-text: root.query", "filter-text: 1", "type mismatch"),
    ] {
        let error = compile(&source.replace(from, to), "tree-filter.xr")
            .err()
            .unwrap();
        assert!(error.contains(expected), "{error}");
    }
    let raw = lower(source, "tree-filter.xr");
    let invalid = raw.replace("details.label", "id");
    assert!(
        mlir::process(&invalid)
            .unwrap_err()
            .contains("requires a string field")
    );
}

#[test]
fn tree_cells_lower_to_verified_writable_record_links() {
    let source = r#"
        struct Entry { id: int, parent: int, label: string, quota: int, active: bool, load: float }
        component App inherits Window {
            in-out property <[Entry]> records;
            tree := TreeView {
                multi-select: true;
                selection-changed(count) => {}
                for row in root.records : TreeRow {
                    key: row.id;
                    parent-key: row.parent;
                    HorizontalLayout {
                        TreeCell { text <=> row.label; }
                        SpinBox { value <=> row.quota; }
                        CheckBox { checked <=> row.active; }
                        if row.active : HorizontalLayout {
                            Slider { value <=> row.load; }
                        }
                    }
                }
            }
        }
    "#;
    let ir = lower(source, "tree-edit.xr");
    let optimized = mlir::process(&ir).unwrap().to_mlir();
    assert_eq!(optimized.matches("xr.row_link").count(), 4);
    assert!(optimized.contains("selection-changed"));
    assert!(compile(source, "tree-edit.xr").is_ok());
    assert!(compile(&source.replace("CheckBox", "Switch"), "tree-edit.xr").is_ok());
    let invalid_kind = ir.replace("kind = \"TreeCell\"", "kind = \"TableCell\"");
    assert_ne!(invalid_kind, ir);
    assert!(mlir::process(&invalid_kind).is_err());
    let invalid_link = ir.replace("name = \"value\"", "name = \"checked\"");
    assert!(
        mlir::process(&invalid_link)
            .unwrap_err()
            .contains("matching field of a writable tree model")
    );
    let read_only = source.replace("in-out property", "in property");
    assert!(
        compile(&read_only, "tree-edit.xr")
            .err()
            .unwrap()
            .contains("writable record list")
    );
    let invalid_field = source.replace("text <=> row.label", "text <=> row.id");
    assert!(
        compile(&invalid_field, "tree-edit.xr")
            .err()
            .unwrap()
            .contains("TreeCell editor expects string, found int")
    );
    let unlinked = source.replace("value <=> row.quota", "value: row.quota");
    assert!(
        compile(&unlinked, "tree-edit.xr")
            .err()
            .unwrap()
            .contains("require an anonymous editor")
    );
    let repeated = source.replace(
        "if row.active : HorizontalLayout",
        "for nested in 2 : HorizontalLayout",
    );
    assert!(
        compile(&repeated, "tree-edit.xr")
            .err()
            .unwrap()
            .contains("repeat templates require anonymous display elements")
    );
}

#[test]
fn range_slider_verifies_both_links_and_pair_events() {
    let source = r#"
        export component App inherits Window {
            in-out property <float> lo: 15;
            in-out property <float> hi: 85;
            range := RangeSlider {
                lower-value <=> root.lo;
                upper-value <=> root.hi;
                orientation: vertical;
                changed(first, second) => {}
                released(first, second) => {}
            }
        }
    "#;
    let ir = lower(source, "interval.xr");
    let module = mlir::process(&ir).unwrap();
    assert_eq!(module.to_mlir().matches("xr.control_link").count(), 2);
    assert!(compile(source, "interval.xr").is_ok());
    let alias = source.replace("upper-value <=> root.hi", "upper-value <=> root.lo");
    assert!(
        compile(&alias, "interval.xr")
            .err()
            .unwrap()
            .contains("distinct writable properties")
    );
    let native_alias = ir.replace("value = \"hi\"", "value = \"lo\"");
    assert_ne!(native_alias, ir);
    assert!(
        mlir::process(&native_alias)
            .unwrap_err()
            .contains("distinct writable properties")
    );
    let wrong_arity = source.replace("released(first, second)", "released(first)");
    assert!(
        compile(&wrong_arity, "interval.xr")
            .err()
            .unwrap()
            .contains("two float parameters")
    );
    let native_event = ir.replace("name = \"released\"", "name = \"unexpected\"");
    assert!(
        mlir::process(&native_event)
            .unwrap_err()
            .contains("two float parameters")
    );
}

#[test]
fn dial_keeps_its_typed_input_mode_through_mlir() {
    let source = r#"
        export component App inherits Window {
            in-out property <float> value: 20;
            in-out property <DialInputMode> mode: DialInputMode.vertical;
            knob := Dial {
                value <=> root.value;
                input-mode: root.mode;
                start-angle: -120;
                end-angle: 100;
                wrap: true;
                changed(next) => {}
                released(last) => {}
            }
        }
    "#;
    let raw = lower(source, "dial.xr");
    let optimized = mlir::process(&raw).unwrap().to_mlir();
    assert!(optimized.contains("!xr.dialinputmode"));
    assert!(optimized.contains("xr.control_link"));
    assert!(compile(source, "dial.xr").is_ok());
    let invalid_mode = raw.replace("value = \"vertical\"", "value = \"diagonal\"");
    assert_ne!(invalid_mode, raw);
    assert!(mlir::process(&invalid_mode).is_err());
    let invalid_binding = source.replace("input-mode: root.mode", "input-mode: 42");
    assert!(
        compile(&invalid_binding, "dial.xr")
            .err()
            .unwrap()
            .contains("type mismatch")
    );
}

#[test]
fn scroll_bar_policy_is_typed_and_requires_a_supported_axis() {
    let source = r#"
        export component App inherits Window {
            in-out property <ScrollBarPolicy> policy: ScrollBarPolicy.always-on;
            ScrollView {
                vertical-scroll-bar-policy: root.policy;
                horizontal-scroll-bar-policy: as-needed;
                content-width: 150%;
                content-height: 500px;
                Rectangle { height: 500px; }
            }
        }
    "#;
    let raw = lower(source, "scroll-bar.xr");
    assert!(
        mlir::process(&raw)
            .unwrap()
            .to_mlir()
            .contains("!xr.scrollbarpolicy")
    );
    assert!(compile(source, "scroll-bar.xr").is_ok());
    let invalid_axis = raw.replace("kind = \"ScrollView\"", "kind = \"Rectangle\"");
    assert!(
        mlir::process(&invalid_axis)
            .unwrap_err()
            .contains("on a scrollable axis")
    );
    let invalid_policy = raw.replace("value = \"always-on\"", "value = \"temporary\"");
    assert_ne!(invalid_policy, raw);
    assert!(mlir::process(&invalid_policy).is_err());
    let invalid_extent = source.replace("content-height: 500px", "content-height: true");
    assert!(
        compile(&invalid_extent, "scroll-bar.xr")
            .err()
            .unwrap()
            .contains("type mismatch")
    );
}

#[test]
fn drawer_checks_the_edge_and_its_open_link_in_mlir() {
    let source = r#"
        component App inherits Window {
            in-out property <bool> showing: false;
            in property <DrawerEdge> side: DrawerEdge.right;
            drawer := Drawer {
                open <=> root.showing;
                edge: root.side;
                drag-margin: 16px;
                opened() => {}
                closed() => {}
            }
        }
    "#;
    let raw = lower(source, "drawer.xr");
    let optimized = mlir::process(&raw).unwrap().to_mlir();
    assert!(optimized.contains("!xr.draweredge"));
    assert!(optimized.contains("xr.control_link"));
    assert!(compile(source, "drawer.xr").is_ok());
    let invalid_edge = raw.replace("value = \"right\"", "value = \"center\"");
    assert_ne!(invalid_edge, raw);
    assert!(mlir::process(&invalid_edge).is_err());
    let invalid_event = raw.replace("name = \"closed\"", "name = \"rejected\"");
    assert!(
        mlir::process(&invalid_event)
            .unwrap_err()
            .contains("requires opened() or closed()")
    );
    let invalid_margin = source.replace("drag-margin: 16px", "drag-margin: true");
    assert!(
        compile(&invalid_margin, "drawer.xr")
            .err()
            .unwrap()
            .contains("type mismatch")
    );
}

#[test]
fn swipe_view_verifies_static_pages_and_its_index_link() {
    let source = r#"
        component App inherits Window {
            in-out property <int> page: 0;
            views := SwipeView {
                current-index <=> root.page;
                orientation: vertical;
                interactive: true;
                changed(index) => {}
                SwipePage { Text { text: "first"; } }
                SwipePage { visible: false; Text { text: "second"; } }
            }
        }
    "#;
    let raw = lower(source, "swipe.xr");
    assert!(
        mlir::process(&raw)
            .unwrap()
            .to_mlir()
            .contains("xr.control_link")
    );
    assert!(compile(source, "swipe.xr").is_ok());
    let invalid_child = raw.replace("kind = \"SwipePage\"", "kind = \"Rectangle\"");
    assert!(
        mlir::process(&invalid_child)
            .unwrap_err()
            .contains("requires a SwipePage child")
    );
    let repeat = source.replace("SwipePage { Text", "for page in 3 : SwipePage { Text");
    assert!(
        compile(&repeat, "swipe.xr")
            .err()
            .unwrap()
            .contains("a repeated SwipePage must be the only option")
    );
    let invalid_event = raw.replace("name = \"changed\"", "name = \"released\"");
    assert!(
        mlir::process(&invalid_event)
            .unwrap_err()
            .contains("one int parameter")
    );
}

#[test]
fn swipe_view_accepts_one_repeated_page_with_component_content() {
    let source = r#"
        component Editor inherits Rectangle {
            in property <string> title;
            Label { text: root.title; }
        }
        export component App inherits Window {
            view := SwipeView {
                current-value: "two";
                for title in ["one", "two"] : SwipePage {
                    value: title;
                    Editor { title: title; }
                }
            }
        }
    "#;
    let compiled = compile(source, "model_pages.xr").unwrap();
    assert!(compiled.mlir.contains("xr.repeat"));
    assert!(
        compiled
            .rust
            .to_string()
            .contains("append_navigation_pages")
    );
    let invalid = source.replace(
        "Editor { title: title; }",
        "editor := Editor { title: title; }",
    );
    assert!(compile(&invalid, "model_pages.xr").is_err());
}

#[test]
fn page_indicator_verifies_count_style_and_index_binding() {
    let source = r#"
        component App inherits Window {
            in-out property <int> page: 0;
            indicator := PageIndicator {
                count: 8;
                current-index <=> root.page;
                orientation: vertical;
                dot-size: 10px;
                spacing: 4px;
                color: #abc;
                current-color: #345;
                interactive: true;
                changed(index) => {}
            }
        }
    "#;
    let raw = lower(source, "indicator.xr");
    assert!(
        mlir::process(&raw)
            .unwrap()
            .to_mlir()
            .contains("xr.control_link")
    );
    assert!(compile(source, "indicator.xr").is_ok());
    let invalid_type = raw.replace("name = \"count\"", "name = \"dot-size\"");
    assert!(
        mlir::process(&invalid_type)
            .unwrap_err()
            .contains("incorrect PageIndicator property type")
    );
    let invalid_child = source.replace("count: 8;", "count: 8; Rectangle {}");
    assert!(
        compile(&invalid_child, "indicator.xr")
            .err()
            .unwrap()
            .contains("only accepts Tooltip children")
    );
}

#[test]
fn swipe_transition_duration_is_verified_in_source_and_mlir() {
    let source = r#"
        export component App inherits Window {
            pages := SwipeView {
                interactive: true;
                transition-duration: 180;
                SwipePage { Rectangle {} }
                SwipePage { Rectangle {} }
            }
        }
    "#;
    assert!(compile(source, "swipe-transition.xr").is_ok());
    let invalid = source.replace("transition-duration: 180", "transition-duration: true");
    let error = compile(&invalid, "swipe-transition.xr").err().unwrap();
    assert!(error.contains("type mismatch"), "{error}");
    let raw = lower(
        &source.replace("transition-duration: 180;", ""),
        "swipe-transition.xr",
    );
    let invalid = raw.replace("name = \"interactive\"", "name = \"transition-duration\"");
    let error = mlir::process(&invalid).unwrap_err();
    assert!(
        error.contains("transition-duration requires an integer"),
        "{error}"
    );
}

#[test]
fn stack_navigation_uses_verified_effects_and_named_routes() {
    let source = r#"
        export component App inherits Window {
            go := Button {
                clicked() => { nav.push("detail"); }
            }
            nav := StackView {
                initial-page: "home";
                transition-duration: 180;
                changed(page, depth) => {}
                home := StackPage {}
                detail := StackPage {}
            }
        }
    "#;
    let raw = lower(source, "stack.xr");
    assert!(
        mlir::process(&raw)
            .unwrap()
            .to_mlir()
            .contains("xr.navigate")
    );
    assert!(compile(source, "stack.xr").is_ok());
    let bad_duration = source.replace("transition-duration: 180", "transition-duration: true");
    assert!(
        compile(&bad_duration, "stack.xr")
            .err()
            .unwrap()
            .contains("type mismatch")
    );
    let bad_native_duration =
        raw.replace("name = \"initial-page\"", "name = \"transition-duration\"");
    assert!(
        mlir::process(&bad_native_duration)
            .unwrap_err()
            .contains("transition-duration requires an integer")
    );
    let bad_owner = raw.replace("owner = \"nav\"", "owner = \"root\"");
    assert!(
        mlir::process(&bad_owner)
            .unwrap_err()
            .contains("requires a StackView owner")
    );
    let bad_args = raw.replace("method = \"push\"", "method = \"pop\"");
    assert!(
        mlir::process(&bad_args)
            .unwrap_err()
            .contains("navigation method or arguments")
    );
    let bad_route = source.replace("push(\"detail\")", "push(\"missing\")");
    assert!(
        compile(&bad_route, "stack.xr")
            .err()
            .unwrap()
            .contains("unknown StackPage route")
    );
    let pure = source.replace(
        "go := Button {",
        "pure callback probe() -> int; probe() => { nav.pop(); 0 } go := Button {",
    );
    assert!(
        compile(&pure, "stack.xr")
            .err()
            .unwrap()
            .contains("not allowed in a pure callback")
    );
}

#[test]
fn stack_state_reads_are_typed_and_detect_initialization_cycles() {
    let source = r#"
        component App inherits Window {
            out property <int> level: nav.depth;
            pure callback route() -> string;
            route() => { nav.current-page }
            nav := StackView {
                initial-page: "home";
                home := StackPage {}
                next := StackPage {}
            }
            back := Button { enabled: nav.can-go-back; }
        }
    "#;
    let raw = lower(source, "stack-read.xr");
    assert_eq!(
        mlir::process(&raw)
            .unwrap()
            .to_mlir()
            .matches("xr.stack_read")
            .count(),
        3
    );
    assert!(compile(source, "stack-read.xr").is_ok());
    let invalid_type = raw.replace("value = \"depth\"", "value = \"current-page\"");
    assert!(
        mlir::process(&invalid_type)
            .unwrap_err()
            .contains("typed StackView state property")
    );
    let write = source.replace("initial-page: \"home\";", "depth: 1;");
    assert!(
        compile(&write, "stack-read.xr")
            .err()
            .unwrap()
            .contains("read-only")
    );
    for initializer in [
        "nav.current-page",
        "root.route()",
        "nav.depth > 0 ? \"home\" : \"next\"",
    ] {
        let cycle = source.replace(
            "initial-page: \"home\"",
            &format!("initial-page: {initializer}"),
        );
        assert!(
            compile(&cycle, "stack-read.xr")
                .err()
                .unwrap()
                .contains("cyclic")
        );
    }
    let cross_component = r#"
        component Child inherits StackView {
            in property <string> seed: "home";
            out property <int> level: root.depth;
            initial-page: root.seed;
            home := StackPage {}
        }
        component App inherits Window {
            child := Child { seed: child.level > 0 ? "home" : "home"; }
        }
    "#;
    assert!(
        compile(cross_component, "stack-cycle.xr")
            .err()
            .unwrap()
            .contains("cyclic")
    );
}

#[test]
fn stack_page_lifecycle_and_guards_are_verified_in_both_compilers() {
    let source = r#"
        component App inherits Window {
            out property <int> detail-index: detail.stack-index;
            out property <bool> detail-active: detail.active;
            in-out property <bool> dirty: false;
            nav := StackView {
                initial-page: "home";
                home := StackPage {}
                detail := StackPage {
                    leave-requested(destination, removing) => { !removing || !root.dirty }
                    activated => { root.dirty = false; }
                    deactivated => { }
                    removed => { }
                }
            }
        }
    "#;
    let compiled = compile(source, "stack-lifecycle.xr").unwrap();
    assert!(compiled.mlir.contains("xr.stack_read"));
    assert!(compiled.rust.to_string().contains("navigate_guarded"));
    for (before, after, expected) in [
        (
            "leave-requested(destination, removing)",
            "leave-requested(destination)",
            "expects a string destination",
        ),
        ("!removing || !root.dirty", "", "must return a bool"),
        (
            "activated => {",
            "activated(flag) => {",
            "takes no parameters",
        ),
        (
            "detail := StackPage {",
            "detail := StackPage { stack-index: 0;",
            "read-only",
        ),
        (
            "initial-page: \"home\"",
            "initial-page: home.active ? \"home\" : \"detail\"",
            "cyclic",
        ),
    ] {
        let invalid = source.replace(before, after);
        let error = compile(&invalid, "stack-lifecycle.xr")
            .err()
            .expect("invalid stack page compiled");
        assert!(error.contains(expected), "{error}");
    }

    let raw = lower(source, "stack-lifecycle.xr");
    for (before, after, expected) in [
        (
            "value = \"active\"",
            "value = \"stack-index\"",
            "typed StackPage state property",
        ),
        (
            "name = \"removed\"",
            "name = \"clicked\"",
            "invalid StackPage lifecycle",
        ),
        (
            "name = \"leave-requested\"",
            "name = \"clicked\"",
            "cannot return a value",
        ),
    ] {
        let invalid = raw.replace(before, after);
        assert_ne!(invalid, raw);
        let error = mlir::process(&invalid).unwrap_err();
        assert!(error.contains(expected), "{error}");
    }
}

#[test]
fn stack_path_operations_verify_list_arguments_and_literal_routes() {
    let source = r#"
        component App inherits Window {
            go := Button { clicked() => { nav.reset-to(["home", "detail"]); } }
            nav := StackView {
                home := StackPage {}
                detail := StackPage {}
            }
        }
    "#;
    let raw = lower(source, "stack-path.xr");
    assert!(
        mlir::process(&raw)
            .unwrap()
            .to_mlir()
            .contains("method = \"reset-to\"")
    );
    assert!(compile(source, "stack-path.xr").is_ok());
    let invalid_method = raw.replace("method = \"reset-to\"", "method = \"push\"");
    assert!(
        mlir::process(&invalid_method)
            .unwrap_err()
            .contains("navigation method or arguments")
    );
    let duplicate = source.replace("[\"home\", \"detail\"]", "[\"home\", \"home\"]");
    assert!(
        compile(&duplicate, "stack-path.xr")
            .err()
            .unwrap()
            .contains("duplicate StackPage route")
    );
}

#[test]
fn exported_record_types_and_literals_resolve_through_import_aliases() {
    let directory = std::env::var_os("TEST_TMPDIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join(format!("xrack-record-import-{}", std::process::id()));
    std::fs::create_dir(&directory).unwrap();
    std::fs::write(
        directory.join("types.xr"),
        "export struct Data { details: Details } export struct Details { label: string }",
    )
    .unwrap();
    let source = r#"import { Data as Row, Details as Info } from "types.xr";
        export component App inherits Window {
            in property <[Row]> rows: [Row { details: Info { label: "one" } }];
            VerticalLayout { for item in root.rows : Text { text: item.details.label; } }
        }
    "#;
    let filename = directory.join("app.xr");
    let result = compile(source, &filename.to_string_lossy());
    std::fs::write(
        directory.join("types.xr"),
        "export struct Data { details: Details }",
    )
    .unwrap();
    let missing = compile(
        r#"import { Data as Row } from "types.xr";
        struct Details { label: string }
        component App inherits Window { in property <[Row]> rows; }
    "#,
        &filename.to_string_lossy(),
    )
    .err()
    .unwrap();
    std::fs::remove_dir_all(directory).unwrap();
    let compiled = result.unwrap();
    assert!(compiled.mlir.matches("\"xr.get_field\"").count() >= 2);
    assert!(
        missing.contains("types.xr:1:") && missing.contains("unknown record type `Details`"),
        "{missing}"
    );
}

#[test]
fn selection_groups_enforce_child_structure() {
    for (body, expected) in [
        (
            "Tab { title: \"outside\"; }",
            "matching selection container",
        ),
        ("TabWidget { Button {} }", "requires static Tab children"),
        (
            "TabButton { text: \"outside\"; }",
            "matching selection container",
        ),
        ("TabBar { Tab {} }", "requires static TabButton children"),
        (
            "TabBar { TabButton { Rectangle {} } }",
            "TabButton cannot contain child elements",
        ),
        (
            "TabBar { TabButton {} for option in [\"a\"] : TabButton { text: option; } }",
            "repeated TabButton must be the only option",
        ),
        (
            "RadioGroup { if true : Button {} }",
            "requires RadioButton children",
        ),
    ] {
        let source = format!("component Demo inherits Window {{ {body} }}");
        let error = compile(&source, "groups.xr").err().unwrap();
        assert!(error.contains(expected), "{error}");
    }
}

#[test]
fn radio_group_has_focus_lowers_to_descendant_focus_and_allows_focus_commands() {
    let source = r#"
        export component App inherits Window {
            out property <bool> selecting: options.has-focus;
            FocusScope {
                initial-focus: options;
                options := RadioGroup { title: "Options"; RadioButton { text: "One"; } }
                enter := Button { clicked => { options.focus(); } }
                leave := Button { clicked => { options.clear-focus(); } }
            }
        }
    "#;
    let compiled = compile(source, "radio-focus.xr").unwrap();
    assert!(compiled.mlir.contains("\"xr.focus_read\""));
    assert!(compiled.mlir.contains("value = \"focus-within\""));
    assert!(compiled.mlir.contains("\"xr.focus_target\""));
    assert!(compiled.mlir.contains("action = \"clear\""));

    let invalid_binding = source.replace("title: \"Options\";", "has-focus: true;");
    assert!(compile(&invalid_binding, "radio-focus.xr").is_err());
}

#[test]
fn radio_groups_accept_mixed_options_without_relaxing_tab_rules() {
    let source = r#"
        export component App inherits Window {
            in property <bool> optional: false;
            RadioGroup {
                RadioButton { text: "Fixed"; }
                if root.optional: RadioButton { text: "Extra"; }
                for label in ["A","B"]: RadioButton { text: label; }
                for number in 2: RadioButton { text: "Entry " + number; }
            }
        }
    "#;
    let result = compile(source, "mixed-options.xr").unwrap();
    assert_eq!(result.mlir.matches("\"xr.repeat\"").count(), 2);
    assert!(result.mlir.contains("\"xr.show_if\""));
}

#[test]
fn repeated_components_require_a_direct_list_view_slot() {
    for (source, expected) in [
        (
            "component RowEditor inherits Rectangle {} component App inherits Window { for row in 3 : RowEditor {} }",
            "require an anonymous ListView template",
        ),
        (
            "component RowEditor inherits Rectangle {} component App inherits ListView { for row in 3 : named := RowEditor {} }",
            "require an anonymous ListView template",
        ),
        (
            "component RowEditor inherits Window {} component App inherits TableView { TableColumn {} for row in 3 : TableRow { RowEditor {} } }",
            "delegates cannot contain a window",
        ),
    ] {
        let error = compile(source, "delegates.xr").err().unwrap();
        assert!(error.contains(expected), "{error}");
    }
}

#[test]
fn virtual_views_can_be_instantiated_inside_component_delegates() {
    let source = r#"
        struct Entry { id: int, parent: int }
        component AlbumRow inherits Rectangle {
            in property <[Entry]> records;
            ListView { height: 60px; for item in 1000 : Text { text: "Row " + item; } }
            GridView { height: 60px; for tile in 1000 : Rectangle {} }
            StandardListView { height: 60px; model: ["A", "B"]; }
            TableView {
                TableColumn {}
                for entry in root.records : TableRow { Text { text: "Entry " + entry.id; } }
            }
            TreeView {
                for entry in root.records : TreeRow {
                    key: entry.id;
                    parent-key: entry.parent;
                    Text { text: "Entry " + entry.id; }
                }
            }
        }
        export component App inherits ListView { for item in 1000 : AlbumRow {} }
    "#;
    let result = compile(source, "nested-lists.xr").unwrap();
    assert_eq!(
        result
            .rust
            .to_string()
            .matches("ListRowRenderer :: with_columns")
            .count(),
        6
    );
}

#[test]
fn dialog_open_and_button_role_are_typed() {
    let source = r#"export component Confirm inherits Dialog {
        in-out property <bool> showing: false;
        open <=> root.showing;
        accepted() => { }
        DialogButton { role: DialogRole.accept; }
    }"#;
    let result = compile(source, "dialog.xr").unwrap();
    assert!(result.mlir.contains("!xr.dialogrole"));
    assert!(result.mlir.contains("xr.control_link"));
    assert!(
        compile(&source.replace("DialogRole.accept", "true"), "dialog.xr")
            .err()
            .unwrap()
            .contains("type mismatch")
    );
    assert!(
        compile(
            &source.replace("accepted()", "accepted(value)"),
            "dialog.xr"
        )
        .err()
        .unwrap()
        .contains("takes no parameters")
    );
}

#[test]
fn multiline_edit_has_a_typed_wrap_and_edit_callback() {
    let source = r#"export component Editor inherits TextArea {
        in-out property <string> value;
        wrap: TextWrap.char-wrap;
        text <=> root.value;
        edited(text) => { root.value = text; }
    }"#;
    let result = compile(source, "editor.xr").unwrap();
    assert!(result.mlir.contains("!xr.textwrap"));
    assert!(result.mlir.contains("xr.control_link"));
    assert!(
        compile(&source.replace("edited(text)", "edited()"), "editor.xr")
            .err()
            .unwrap()
            .contains("expects one string parameter")
    );
    assert!(
        compile(&source.replace("TextWrap.char-wrap", "true"), "editor.xr")
            .err()
            .unwrap()
            .contains("type mismatch")
    );
}

#[test]
fn numeric_and_choice_controls_validate_bindings_and_events() {
    let source = r#"export component Demo inherits Window {
        in-out property <int> quantity: 2;
        in-out property <int> selection-index: 0;
        spin := SpinBox { value <=> root.quantity; edited(value) => { root.quantity = value; } }
        picker := ComboBox { model: ["alpha"]; current-index <=> root.selection-index; selected(value) => {} }
    }"#;
    let compilation = compile(source, "selection.xr").unwrap();
    assert!(compilation.mlir.contains("!xr.stringlist"));
    assert!(compilation.mlir.contains("\"xr.control_link\""));
    let bad = source.replace("edited(value)", "edited()");
    assert!(
        compile(&bad, "selection.xr")
            .err()
            .unwrap()
            .contains("expects one int parameter")
    );
    let bad = source.replace(
        "current-index <=> root.selection-index;",
        "current-index <=> root.selection-index; current-value: \"alpha\";",
    );
    assert!(
        compile(&bad, "selection.xr")
            .err()
            .unwrap()
            .contains("either current-index or current-value")
    );
}

#[test]
fn slider_callbacks_and_progress_ratios_have_typed_mlir() {
    let source = r#"export component Demo inherits Window {
        in-out property <float> level: 12;
        adjustment := Slider {
            orientation: Orientation.vertical;
            value <=> root.level;
            changed(value) => { root.level = value; }
        }
        ProgressIndicator { progress: 65%; }
    }"#;
    let compilation = compile(source, "controls.xr").unwrap();
    assert!(compilation.mlir.contains("!xr.orientation"));
    assert!(lower(source, "controls.xr").contains("\"xr.to_ratio\""));
    assert!(compilation.mlir.contains("\"xr.control_link\""));
    let bad = source.replace("changed(value)", "changed()");
    assert!(
        compile(&bad, "controls.xr")
            .err()
            .unwrap()
            .contains("expects one float parameter")
    );
    let bad = source.replace("property <float> level", "property <int> level");
    assert!(
        compile(&bad, "controls.xr")
            .err()
            .unwrap()
            .contains("writable float property")
    );
}

#[test]
fn callback_calls_enforce_signatures_and_purity() {
    for (body, expected) in [
        (
            "callback count() -> int; property <int> value: count();",
            "impure callback",
        ),
        (
            "pure callback count(int) -> int; property <int> value: count();",
            "expects 1 arguments",
        ),
        (
            "pure callback count(int) -> int; property <int> value: count(true);",
            "expected int",
        ),
        (
            "pure callback ping; property <int> value: ping();",
            "no return value",
        ),
        (
            "property <int> n; pure callback ping; ping => { root.n = 3; }",
            "assignment is not allowed in a pure callback",
        ),
        (
            "pure callback count() -> int; count => { root.n } property <int> n: count();",
            "cyclic property binding or callback call",
        ),
        (
            "callback ping; ping => { root.pong() } callback pong; pong => { root.ping(); }",
            "cyclic property binding or callback call",
        ),
    ] {
        let source = format!("component Demo inherits Window {{ {body} }}");
        let error = compile(&source, "calls.xr").err().unwrap();
        assert!(error.contains(expected), "{error}");
    }

    let source = r#"export component Demo inherits Window {
        pure callback number(int) -> int;
        pure callback text(string) -> string;
        callback read-number(int) -> int;
        out property <int> result: number(3);
    }"#;
    let compilation = compile(source, "calls.xr").unwrap();
    assert!(compilation.mlir.contains("\"xr.call\""));
    assert!(
        compilation
            .mlir
            .contains("owner = \"root\", value = \"number\"")
    );
    for (target, expected) in [
        ("text", "incorrect argument type"),
        ("read-number", "impure callback in a pure context"),
        ("missing", "unknown callback"),
    ] {
        let corrupted = compilation.mlir.replace(
            "owner = \"root\", value = \"number\"",
            &format!("owner = \"root\", value = \"{target}\""),
        );
        let error = mlir::process(&corrupted).unwrap_err();
        assert!(error.contains(expected), "{error}");
    }

    let cross_instance_cycle = r#"component Child inherits Rectangle {
        in property <int> input: 0;
        pure callback value() -> int;
        value => { input }
    }
    export component Demo inherits Window {
        property <int> result: child.value();
        child := Child { input: root.result; }
    }"#;
    let error = compile(cross_instance_cycle, "calls.xr").err().unwrap();
    assert!(
        error.contains("cyclic property binding or callback call"),
        "{error}"
    );
}

#[test]
fn component_instances_resolve_and_verify_typed_property_references() {
    let source = r#"
        export component App inherits Window {
            out property <int> result: child.result;
            child := Child { input: 3; max-width: 140px; }
        }
        component Child inherits Rectangle {
            in property <int> input;
            out property <int> result: local.value + input;
            local := Text { property <int> value: root.input + 2; text: "" + self.value; }
        }
    "#;
    let compilation = compile(source, "instances.xr").unwrap();
    assert!(compilation.mlir.contains("\"xr.instance\""));
    assert!(compilation.mlir.contains("\"xr.layout_bind\""));
    assert!(compilation.mlir.contains("component = @Child"));
    assert!(compilation.mlir.contains("owner = \"child\""));
    assert!(compilation.mlir.contains("owner = \"local\""));
    let parsed = mlir::process(&compilation.mlir).unwrap();
    assert_eq!(
        xrack_compiler::codegen::generate(&parsed)
            .unwrap()
            .to_string(),
        compilation.rust.to_string(),
    );
    for (from, to, expected) in [
        (
            "component = @Child",
            "component = @Missing",
            "unknown component",
        ),
        (
            "owner = \"child\"",
            "owner = \"missing\"",
            "unknown property owner",
        ),
        (
            "value = \"result\"",
            "value = \"missing\"",
            "unknown property",
        ),
        (
            "name = \"max-width\"",
            "name = \"bogus-width\"",
            "unknown instance layout property",
        ),
    ] {
        let err = mlir::process(&compilation.mlir.replace(from, to)).unwrap_err();
        assert!(err.contains(expected), "{err}");
    }
    let private = source.replace(
        "out property <int> result: local",
        "private property <int> result: local",
    );
    assert!(analyze(&private).unwrap_err().contains("private"));
    let read_only = source.replace("input: 3", "result: 3");
    assert!(analyze(&read_only).unwrap_err().contains("read-only"));
}

#[test]
fn rejects_recursive_instances_and_cross_instance_binding_cycles() {
    let recursive =
        "component A inherits Rectangle { B {} } component B inherits Rectangle { A {} }";
    assert!(
        analyze(recursive)
            .unwrap_err()
            .contains("cyclic component instantiation")
    );

    let cycle = r#"
        component App inherits Window {
            property <int> feedback: child.result;
            child := Child { input: root.feedback; }
        }
        component Child inherits Rectangle {
            in property <int> input;
            out property <int> result: input;
        }
    "#;
    assert!(
        analyze(cycle)
            .unwrap_err()
            .contains("cyclic property binding")
    );
}

#[test]
fn component_links_verify_writable_matching_endpoints_and_alias_cycles() {
    let source = r#"
        component App inherits Window {
            in-out property <string> shared: "host";
            left := Editor { draft <=> root.shared; }
            right := Editor { draft <=> left.draft; }
        }
        component Editor inherits Rectangle {
            in-out property <string> draft: "child";
        }
    "#;
    let compiled = compile(source, "links.xr").unwrap();
    assert_eq!(compiled.mlir.matches("\"xr.instance_link\"").count(), 2);
    assert!(
        compiled
            .rust
            .to_string()
            .contains("InputProperty :: linked")
    );
    let raw = lower(source, "links.xr");
    let cycle = source.replace("draft <=> root.shared", "draft <=> right.draft");
    assert!(
        analyze(&cycle)
            .unwrap_err()
            .contains("cyclic property binding")
    );
    let invalid = raw.replace(
        "owner = \"root\", value = \"shared\"",
        "owner = \"right\", value = \"draft\"",
    );
    assert!(
        mlir::process(&invalid)
            .unwrap_err()
            .contains("cyclic instance property link")
    );
    let invalid = raw.replace(
        "name = \"draft\", visibility = \"in-out\"",
        "name = \"draft\", visibility = \"in\"",
    );
    assert!(
        mlir::process(&invalid)
            .unwrap_err()
            .contains("requires an in-out component property")
    );
    let read_only = source.replace(
        "in-out property <string> shared",
        "in property <string> shared",
    );
    assert!(analyze(&read_only).unwrap_err().contains("read-only"));
    let mismatch = source.replace(
        "in-out property <string> shared: \"host\"",
        "property <int> shared: 1",
    );
    assert!(
        analyze(&mismatch)
            .unwrap_err()
            .contains("requires a writable string")
    );
}

#[test]
fn repeated_component_links_require_live_writable_models_and_typed_fields() {
    let source = r#"
        struct Task { title: string, rank: int }
        component App inherits Window {
            in-out property <[Task]> tasks;
            ListView {
                for row[index] in root.tasks : Editor { draft <=> row.title; }
            }
        }
        component Editor inherits Rectangle {
            in-out property <string> draft;
        }
    "#;
    let compiled = compile(source, "row-links.xr").unwrap();
    assert!(compiled.mlir.contains("\"xr.instance_link\""));
    for (from, to, expected) in [
        ("row.title", "index", "only the repeated item"),
        ("row.title", "row.rank", "requires a writable string"),
        (
            "in-out property <[Task]> tasks",
            "in property <[Task]> tasks",
            "read-only",
        ),
    ] {
        assert!(
            analyze(&source.replace(from, to))
                .unwrap_err()
                .contains(expected)
        );
    }
    let raw = lower(source, "row-links.xr");
    let invalid = raw.replace(
        "name = \"tasks\", visibility = \"in-out\"",
        "name = \"tasks\", visibility = \"in\"",
    );
    assert!(
        mlir::process(&invalid)
            .unwrap_err()
            .contains("cannot write read-only property")
    );
}

#[test]
fn callback_aliases_and_instance_handlers_share_a_verified_signature() {
    let source = r#"export component App inherits Window {
        property <int> bias: 3;
        out property <int> result: root.compute(4);
        pure callback compute <=> worker.score;
        worker := Worker { score(input) => { input + root.bias } }
    }
    component Worker inherits Rectangle {
        pure callback score <=> body.evaluate;
        body := Rectangle {
            pure callback evaluate(int) -> int;
            evaluate(input) => { input * 2 }
        }
    }"#;
    let compiled = compile(source, "callback-aliases.xr").unwrap();
    assert!(compiled.mlir.contains("alias_owner = \"worker\""));
    assert_eq!(compiled.mlir.matches("\"xr.callback\"").count(), 4);
    for (from, to, expected) in [
        ("score(input)", "score()", "expects 1 parameters"),
        (
            "input + root.bias",
            "root.bias = input; input",
            "assignment is not allowed",
        ),
        ("input + root.bias", "\"wrong\"", "type mismatch"),
    ] {
        let error = analyze(&source.replace(from, to)).unwrap_err();
        assert!(error.contains(expected), "{error}");
    }
    let raw = lower(source, "callback-aliases.xr");
    let bad = raw.replace("alias_name = \"score\"", "alias_name = \"missing\"");
    assert!(
        mlir::process(&bad)
            .unwrap_err()
            .contains("unknown callback target")
    );
    assert!(
        analyze(
            r#"component App inherits Window {
        callback left <=> root.right;
        callback right <=> root.left;
    }"#
        )
        .unwrap_err()
        .contains("cyclic callback alias")
    );
    assert!(
        analyze(
            r#"component App inherits Window {
        pure callback exposed <=> button.clicked;
        button := Button {}
    }"#
        )
        .unwrap_err()
        .contains("pure alias requires a pure callback")
    );
}

#[test]
fn property_aliases_verify_controls_types_and_cycles() {
    let source = r#"export component Editor inherits Rectangle {
        in-out property <string> document <=> entry.text;
        entry := LineEdit { text: "initial"; }
    }"#;
    let compiled = compile(source, "aliases.xr").unwrap();
    assert!(compiled.mlir.contains("\"xr.control_read\""));
    assert!(compiled.mlir.contains("alias = \"true\""));
    let raw = lower(source, "aliases.xr");
    let invalid = raw.replace(
        "owner = \"entry\", value = \"text\"",
        "owner = \"entry\", value = \"enabled\"",
    );
    assert!(
        mlir::process(&invalid)
            .unwrap_err()
            .contains("supported writable control")
    );
    assert!(
        analyze(&source.replace("<string> document", "<int> document"))
            .unwrap_err()
            .contains("property alias type mismatch")
    );
    let cycle = source.replace("text: \"initial\"", "text <=> root.document");
    assert!(
        analyze(&cycle)
            .unwrap_err()
            .contains("cyclic property binding")
    );
    let cycle = source.replace("text: \"initial\"", "text: root.document");
    assert!(
        analyze(&cycle)
            .unwrap_err()
            .contains("cyclic property binding")
    );

    let raw = lower(
        r#"component App inherits Window {
        property <string> data: "value";
        property <string> alias <=> root.data;
    }"#,
        "aliases.xr",
    );
    assert!(
        mlir::process(&raw.replace("value = \"data\"", "value = \"alias\""))
            .unwrap_err()
            .contains("cyclic property alias")
    );
}

#[test]
fn alias_inputs_detect_duplicate_and_recursive_initializers_at_the_leaf() {
    let source = r#"
        component Leaf inherits Rectangle {
            in-out property <string> a <=> entry.text;
            in-out property <string> b <=> entry.text;
            entry := LineEdit {}
        }
        component Wrapper inherits Rectangle {
            in-out property <string> label-a <=> child.a;
            in-out property <string> label-b <=> child.b;
            child := Leaf {}
        }
        component App inherits Window {
            editor := Wrapper { label-a: "left"; }
        }
    "#;
    compile(source, "alias-inputs.xr").unwrap();
    let duplicate = source.replace(
        "label-a: \"left\";",
        "label-a: \"left\"; label-b: \"right\";",
    );
    assert!(
        compile(&duplicate, "alias-inputs.xr")
            .err()
            .unwrap()
            .contains("multiple inputs to the same property")
    );
    let cycle = source.replace("label-a: \"left\";", "label-a: editor.label-b;");
    assert!(
        analyze(&cycle)
            .unwrap_err()
            .contains("cyclic property binding")
    );
}

#[test]
fn mlir_roundtrip_preserves_typed_expressions_and_unicode() {
    let source = r#"export component App inherits Window {
        in-out property <int> n: 2;
        out property <float> scale: n + 0.5;
        title: "你好\n\"MLIR\"";
        width: 100px * scale;
        callback add(step: int);
        add(step) => { root.n += step; }
    }"#;
    let text = lower(source, "中文 file.xr");
    let processed = mlir::process(&text).unwrap();
    let reparsed = mlir::process(&processed.to_mlir()).unwrap();
    let rust = compile(source, "test.xr").unwrap().rust.to_string();
    assert_eq!(
        xrack_compiler::codegen::generate(&reparsed)
            .unwrap()
            .to_string(),
        rust,
    );
    assert!(processed.to_mlir().contains("xr.component"));
    assert!(rust.contains("pub struct App"));
    assert!(rust.contains("pub fn invoke_add"));
    assert!(rust.contains("你好"));
}

#[test]
fn mlir_rejects_invalid_structure_and_unknown_operations() {
    assert!(mlir::process("this is not MLIR").is_err());
    assert!(mlir::process("module { \"xr.unknown\"() : () -> () }").is_err());
    assert!(mlir::process("module { %a = \"xr.read\"() {value = \"a\"} : () -> !xr.int\n \"xr.bind\"(%missing) {name = \"x\"} : (!xr.int) -> () }").is_err());
    let valid = lower(
        "component App inherits Window { property <int> count: 7; }",
        "test.xr",
    );
    assert!(mlir::process(&valid.replace("!xr.int", "!xr.unknown_type")).is_err());
    assert!(mlir::process(&valid.replace("name = \"count\"", "extra = \"count\"")).is_err());
    assert!(
        mlir::process(
            &valid
                .replace("value = \"7\"", "value = \"invalid\"")
                .as_str()
        )
        .unwrap_err()
        .contains("literal spelling")
    );
}

#[test]
fn multiple_components_forward_references_and_float_promotion() {
    let m = analyze("component A inherits Rectangle { property <int> derived: seed * 2; property <int> seed: 4; } export component B inherits Window { property <float> scale: 1; }").unwrap();
    assert_eq!(m.components.len(), 2);
    assert_eq!(m.components[1].properties[0].init.op, "cast");
}

#[test]
fn gradient_stops_are_typed_and_verified_in_mlir() {
    let source = r#"export component Gallery inherits Window {
        in property <color> accent: #123;
        property <brush> fill: @linear-gradient(-90deg, #000 20%, #fff, accent 80%);
        background: fill;
    }"#;
    let output = compile(source, "gallery.xr").unwrap();
    assert!(output.mlir.contains("\"xr.linear_gradient\""));
    assert!(output.mlir.contains("!xr.gradientstop"));
    assert!(output.mlir.contains("value = \"0.5\""));
    assert!(output.mlir.contains("value = \"-90\""));
    let malformed = output.mlir.replace("value = \"0.8\"", "value = \"1.8\"");
    assert!(mlir::process(&malformed).unwrap_err().contains("gradient"));

    for (stops, diagnostic) in [
        ("#fff 120%", "between 0% and 100%"),
        ("#fff 70%, #000 40%", "nondecreasing"),
        ("true 0%", "expected color"),
    ] {
        let source = format!(
            "component Gallery inherits Window {{ background: @radial-gradient(circle, {stops}); }}"
        );
        let err = compile(&source, "invalid-gradient.xr").err().unwrap();
        assert!(err.contains(diagnostic), "{err}");
    }
}

#[test]
fn image_and_path_bindings_report_resource_errors() {
    let source = r#"component Demo inherits Window {
        Image { source: @image-url("missing.png"); image-fit: contain; }
    }"#;
    let ir = mlir::process(&lower(source, "image.xr")).unwrap();
    assert!(ir.to_mlir().contains("\"xr.image_url\""));
    assert!(ir.to_mlir().contains("!xr.imagefit"));
    assert!(ir.to_mlir().contains("!xr.image"));
    let document = parse(source).unwrap();
    let export = xrack_compiler::export_mlir(&document, "image.xr", None)
        .unwrap_err()
        .to_string();
    assert_eq!(export, compile(source, "image.xr").err().unwrap());
    assert!(
        compile(source, "image.xr")
            .err()
            .unwrap()
            .contains("missing.png")
    );
    let empty = source.replace("missing.png", "");
    assert!(compile(&empty, "image.xr").is_ok());

    let bad_path = r#"component Demo inherits Window { Path { commands: "M0 0 L10"; } }"#;
    assert!(
        compile(bad_path, "path.xr")
            .err()
            .unwrap()
            .contains("invalid Path commands")
    );
}

#[test]
fn native_frontend_and_mlir_are_safe_to_call_from_multiple_threads() {
    let threads: Vec<_> = (0..8)
        .map(|_| {
            std::thread::spawn(|| {
                compile(
                    "export component A inherits Window { title: \"ok\"; }",
                    "thread.xr",
                )
                .unwrap();
            })
        })
        .collect();
    for t in threads {
        t.join().unwrap();
    }
}

#[test]
fn markdown_literals_are_typed_in_mlir_and_compiled_to_static_style_ranges() {
    let source = r#"export component App inherits Window {
        in property <styled-text> body: @markdown("**bold** `code`");
        StyledText { text: root.body; }
    }"#;
    let compiled = compile(source, "markdown.xr").unwrap();
    assert!(compiled.mlir.contains("\"xr.markdown\""));
    assert!(compiled.mlir.contains("!xr.styledtext"));
    let tokens = compiled.rust.to_string();
    assert!(tokens.contains("StyledText :: from_compiled_parts (\"bold code\""));
    assert!(tokens.contains("bold : true"));
    assert!(tokens.contains("monospace : true"));
    let invalid = source.replace("StyledText {", "Label {");
    assert!(
        compile(&invalid, "markdown.xr")
            .err()
            .unwrap()
            .contains("type mismatch")
    );

    let invalid = source.replace("**bold** `code`", "![image](assets/image.png)");
    let error = compile(&invalid, "markdown.xr").err().unwrap();
    assert!(error.contains("markdown.xr:2:"), "{error}");
    assert!(
        error.contains("unsupported Markdown syntax: images"),
        "{error}"
    );

    let raw = lower(source, "markdown.xr");
    let malformed = raw.replace("kind = \"StyledText\"", "kind = \"Label\"");
    assert!(
        mlir::process(&malformed)
            .unwrap_err()
            .contains("incorrect text layout property type")
    );
}

#[test]
fn markdown_links_compile_with_a_typed_activation_handler() {
    let source = r#"export component App inherits Window {
        out property <string> destination: "";
        body := StyledText {
            text: @markdown("[**manual**](#docs)");
            link-color: #41a6e7;
            link-activated(target) => { root.destination = target; }
        }
    }"#;
    let compiled = compile(source, "links.xr").unwrap();
    assert!(compiled.mlir.contains("name = \"link-activated\""));
    let tokens = compiled.rust.to_string();
    assert!(tokens.contains("StyledLink"));
    assert!(tokens.contains("Borrowed (\"#docs\")"));
    assert!(tokens.contains("ControlAction :: ActivateTextLink"));
    let raw = lower(source, "links.xr");
    let malformed = raw.replace("link-activated", "clicked");
    assert!(
        mlir::process(&malformed)
            .unwrap_err()
            .contains("StyledText requires link-activated(string)")
    );
    let malformed = source.replace("link-activated(target)", "link-activated(target, extra)");
    assert!(
        compile(&malformed, "links.xr")
            .err()
            .unwrap()
            .contains("expects one string parameter")
    );
}

#[test]
fn nine_slice_keeps_typed_edges_and_validates_against_the_embedded_asset() {
    const SVG: &[u8] = include_bytes!("../examples/media/assets/nine-slice.svg");
    let dir = std::env::var_os("TEST_TMPDIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join(format!("xrack-nine-slice-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("media.xr");
    let asset = dir.join("frame.svg");
    std::fs::write(&asset, SVG).unwrap();
    let file = path.to_str().unwrap();
    let source = r#"export component Media inherits Window {
        Image {
            source: @image-url("frame.svg", nine-slice(4 6 8 10));
            horizontal-tile-mode: repeat;
            vertical-tile-mode: round;
        }
    }"#;
    let compiled = compile(source, file).unwrap();
    assert!(compiled.mlir.contains("\"xr.nine_slice\""));
    assert!(compiled.mlir.contains("!xr.imagetilemode"));
    let tokens = compiled.rust.to_string();
    assert!(
        tokens.contains("NineSlice { top : 4u32 , right : 6u32 , bottom : 8u32 , left : 10u32")
    );
    assert!(tokens.contains("ImageTileMode :: Repeat"));
    let malformed = lower(source, file).replace("value = \"10\"", "value = \"-1\"");
    assert!(
        mlir::process(&malformed)
            .unwrap_err()
            .contains("nonnegative integer literals")
    );
    for (edges, diagnostic) in [
        ("3 1 2", "expects one, two or four edges"),
        ("2.5", "nonnegative integers"),
        ("18", "must leave a nonempty center"),
        ("4294967295", "must leave a nonempty center"),
    ] {
        let invalid = source.replace("4 6 8 10", edges);
        assert!(compile(&invalid, file).err().unwrap().contains(diagnostic));
    }
    let invalid = source.replace("4 6 8 10", "16");
    let module = xrack_compiler::lower(&parse(&invalid).unwrap(), file).unwrap();
    assert!(
        xrack_compiler::check_resources(&module, file, Some(std::slice::from_ref(&asset)))
            .unwrap_err()
            .to_string()
            .contains("must leave a nonempty center")
    );
    for (edges, top, right) in [("4", 4, 4), ("4 7", 4, 7)] {
        let source = source.replace("4 6 8 10", edges);
        let tokens = compile(&source, file).unwrap().rust.to_string();
        assert!(tokens.contains(&format!(
            "NineSlice {{ top : {top}u32 , right : {right}u32 , bottom : {top}u32 , left : {right}u32"
        )));
    }
    std::fs::remove_file(asset).unwrap();
    std::fs::remove_dir(dir).unwrap();
}

#[test]
fn animated_image_embeds_gif_frames_and_checks_read_only_playback_state() {
    const GIF: &[u8] = include_bytes!("../examples/media/assets/pulse.gif");
    let dir = std::env::var_os("TEST_TMPDIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join(format!("xrack-gif-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let asset = dir.join("pulse.gif");
    std::fs::write(&asset, GIF).unwrap();
    let path = dir.join("animated.xr");
    let file = path.to_str().unwrap();
    let source = r#"export component Media inherits Window {
        out property <int> frame: pulse.current-frame;
        out property <int> frames: pulse.frame-count;
        pulse := AnimatedImage {
            source: @image-url("pulse.gif");
            paused: true;
            speed: 2.0;
            loops: -1;
            finished() => {}
        }
    }"#;
    let compiled = compile(source, file).unwrap();
    assert!(compiled.mlir.contains("\"xr.image_playback_read\""));
    assert!(compiled.mlir.contains("value = \"frame-count\""));
    let tokens = compiled.rust.to_string();
    for time in [80, 200, 260] {
        assert!(tokens.contains(&format!("end_ms : {time}u32")));
    }
    let invalid = source.replace("paused: true", "current-frame: 2");
    assert!(compile(&invalid, file).err().unwrap().contains("read-only"));
    let invalid = source.replace("speed: 2.0", "speed: true");
    assert!(
        compile(&invalid, file)
            .err()
            .unwrap()
            .contains("type mismatch")
    );
    let malformed =
        lower(source, file).replace("value = \"frame-count\"", "value = \"loop-count\"");
    assert!(
        mlir::process(&malformed)
            .unwrap_err()
            .contains("playback state property")
    );
    let invalid = source.replace("\"pulse.gif\"", "\"pulse.gif\", nine-slice(2)");
    assert!(
        compile(&invalid, file)
            .err()
            .unwrap()
            .contains("static PNG or SVG")
    );

    let mut oversized = GIF.to_vec();
    oversized[6..8].copy_from_slice(&5000_u16.to_le_bytes());
    std::fs::write(&asset, oversized).unwrap();
    let error = compile(source, file).err().unwrap();
    assert!(error.contains("image dimensions"), "{error}");
    std::fs::remove_file(asset).unwrap();
    std::fs::remove_dir(dir).unwrap();
}

#[test]
fn image_clips_alignments_and_intrinsic_dimensions_are_typed_in_mlir() {
    let source = r#"export component Atlas inherits Window {
        in property <image> atlas: @image-url("");
        out property <int> pixels: root.atlas.width * root.atlas.height;
        Image {
            source: root.atlas;
            source-clip-x: 1; source-clip-y: -2;
            source-clip-width: root.atlas.width;
            horizontal-alignment: right;
            vertical-alignment: bottom;
            image-rendering: pixelated;
        }
        AnimatedImage {
            source: root.atlas;
            source-clip-height: 4;
            horizontal-tile-mode: repeat;
        }
    }"#;
    let result = compile(source, "atlas.xr").unwrap();
    assert!(result.mlir.contains("\"xr.image_dimension\""));
    assert!(result.mlir.contains("!xr.imagehorizontalalignment"));
    assert!(result.mlir.contains("!xr.imageverticalalignment"));
    assert!(result.mlir.contains("!xr.imagerendering"));
    assert!(result.rust.to_string().contains(". size ()"));
    let invalid = source.replace("source-clip-x: 1", "source-clip-x: 1px");
    assert!(
        compile(&invalid, "atlas.xr")
            .err()
            .unwrap()
            .contains("type mismatch")
    );
    let invalid = source.replace("root.atlas.height", "root.atlas.depth");
    assert!(
        compile(&invalid, "atlas.xr")
            .err()
            .unwrap()
            .contains("image has no field")
    );
    let raw = lower(source, "atlas.xr");
    let invalid = raw.replace("value = \"width\"", "value = \"depth\"");
    assert!(
        mlir::process(&invalid)
            .unwrap_err()
            .contains("image dimension must be width or height")
    );
    let invalid = raw.replace("name = \"source-clip-x\"", "name = \"image-rendering\"");
    assert!(
        mlir::process(&invalid)
            .unwrap_err()
            .contains("incorrect Image property type")
    );
}

#[test]
fn image_colorize_uses_typed_brushes_in_bindings_and_states() {
    let source = r#"export component Art inherits Window {
        in property <bool> alternate: false;
        icon := Image {
            source: @image-url("");
            colorize: #0af;
        }
        AnimatedImage {
            source: @image-url("");
            colorize: root.alternate ? @linear-gradient(90deg, #0000, #f84) : #0af;
        }
        states [ alternate when root.alternate: {
            icon.colorize: @radial-gradient(circle, #fff, #369);
        } ]
    }"#;
    let result = compile(source, "colorize.xr").unwrap();
    assert!(result.mlir.contains("name = \"colorize\""));
    assert!(result.mlir.contains("!xr.brush"));
    assert!(result.mlir.contains("\"xr.state_set\""));
    assert!(result.rust.to_string().contains("Brush :: Solid"));
    let invalid = source.replace("colorize: #0af", "colorize: 5");
    assert!(
        compile(&invalid, "colorize.xr")
            .err()
            .unwrap()
            .contains("type mismatch")
    );
    let raw = lower(source, "colorize.xr");
    let invalid = raw.replace("kind = \"Image\"", "kind = \"Rectangle\"");
    assert!(
        mlir::process(&invalid)
            .unwrap_err()
            .contains("incorrect Image property type")
    );
}

#[test]
fn composition_properties_have_typed_bindings_on_nodes_and_instances() {
    let source = r#"component Card inherits Rectangle { background: #024; }
    export component App inherits Window {
        in property <bool> raised: true;
        panel := Rectangle {
            opacity: 0.5;
            z: 2;
            animate opacity { duration: 120ms; }
        }
        Card { opacity: 0.75; z: -1; }
        states [ raise when root.raised: { panel.opacity: 1; panel.z: 3; } ]
    }"#;
    let result = compile(source, "compositing.xr").unwrap();
    for line in result
        .mlir
        .lines()
        .filter(|line| line.contains("name = \"opacity\"") || line.contains("name = \"z\""))
    {
        if !line.contains("xr.animate") {
            assert!(line.contains("!xr.float"), "{line}");
        }
    }
    assert!(
        result
            .mlir
            .lines()
            .any(|line| line.contains("\"xr.layout_bind\"") && line.contains("name = \"z\""))
    );
    let invalid = source.replace("opacity: 0.5", "opacity: true");
    assert!(
        compile(&invalid, "compositing.xr")
            .err()
            .unwrap()
            .contains("type mismatch")
    );
    let invalid = source.replace("opacity: 0.5;", "Timer { opacity: 0.5; }");
    assert!(compile(&invalid, "compositing.xr").is_err());

    let raw = lower(source, "compositing.xr");
    let invalid = raw.replace("name = \"background\"", "name = \"opacity\"");
    assert!(
        mlir::process(&invalid)
            .unwrap_err()
            .contains("z and opacity require")
    );
}

#[test]
fn rectangle_shadows_keep_typed_brush_geometry_and_state_overrides() {
    let source = r#"export component Card inherits Window {
        in property <bool> lifted: false;
        panel := Rectangle {
            background: #fff;
            drop-shadow-color: #3154;
            drop-shadow-offset-x: -4px;
            drop-shadow-offset-y: 8px;
            drop-shadow-spread: 2px;
            drop-shadow-blur: 10px;
            inner-shadow-color: #0008;
            inner-shadow-offset-x: 2px;
            inner-shadow-offset-y: 1px;
            inner-shadow-spread: -1px;
            inner-shadow-blur: 6px;
            animate drop-shadow-offset-y { duration: 80ms; }
        }
        states [ lift when root.lifted: { panel.drop-shadow-offset-y: 12px; } ]
    }"#;
    let compiled = compile(source, "shadows.xr").unwrap();
    for line in compiled.mlir.lines().filter(|line| {
        (line.contains("name = \"drop-shadow") || line.contains("name = \"inner-shadow"))
            && (line.contains("xr.bind") || line.contains("xr.state_set"))
    }) {
        let ty = if line.contains("shadow-color") {
            "!xr.color"
        } else {
            "!xr.length"
        };
        assert!(line.contains(ty), "{line}");
    }
    assert!(
        compiled
            .mlir
            .lines()
            .any(|line| line.contains("xr.state_set") && line.contains("drop-shadow-offset-y"))
    );
    let invalid = source.replace("inner-shadow-blur: 6px", "inner-shadow-blur: true");
    assert!(
        compile(&invalid, "shadows.xr")
            .err()
            .unwrap()
            .contains("type mismatch")
    );
    let invalid = source.replace("panel := Rectangle", "panel := Image");
    assert!(compile(&invalid, "shadows.xr").is_err());
    let invalid = lower(source, "shadows.xr").replace(
        "name = \"inner-shadow-color\"",
        "name = \"inner-shadow-spread\"",
    );
    assert!(
        mlir::process(&invalid)
            .unwrap_err()
            .contains("incorrect Rectangle shadow property type")
    );
}

#[test]
fn rectangle_clip_is_a_typed_binding_and_state_override() {
    let source = r#"export component Demo inherits Window {
        in property <bool> enabled-clip: false;
        mask := Rectangle { clip: root.enabled-clip; }
        states [ rounded when root.enabled-clip: { mask.clip: true; } ]
    }"#;
    let compiled = compile(source, "clip.xr").unwrap();
    for op in ["xr.bind", "xr.state_set"] {
        assert!(compiled.mlir.lines().any(|line| line.contains(op)
            && line.contains("name = \"clip\"")
            && line.contains("!xr.bool")));
    }
    let invalid = source.replace("clip: root.enabled-clip", "clip: 10px");
    assert!(
        compile(&invalid, "clip.xr")
            .err()
            .unwrap()
            .contains("type mismatch")
    );
    let raw = lower(source, "clip.xr");
    let invalid = raw.replace("kind = \"Rectangle\"", "kind = \"Image\"");
    assert!(
        mlir::process(&invalid)
            .unwrap_err()
            .contains("clip requires a boolean on Rectangle")
    );
}

#[test]
fn transforms_are_typed_on_visual_nodes_instances_and_states() {
    let source = r#"component Tile inherits Rectangle { background: #234; }
    export component Demo inherits Window {
        in property <bool> turned: false;
        tile := Rectangle {
            transform-rotation: 45;
            transform-scale: 125%;
            transform-scale-x: -1.0;
            transform-origin-x: 30%;
            transform-origin-y: 2px;
            animate transform-rotation { duration: 120ms; }
        }
        Tile { transform-origin-x: 50%; transform-rotation: 90; }
        states [ turned when root.turned: { tile.transform-rotation: 0; } ]
    }"#;
    let compiled = compile(source, "transform.xr").unwrap();
    assert!(
        compiled
            .mlir
            .lines()
            .any(|line| line.contains("xr.layout_bind")
                && line.contains("transform-origin-x")
                && line.contains("!xr.percent"))
    );
    assert!(
        compiled
            .mlir
            .lines()
            .any(|line| line.contains("xr.state_set")
                && line.contains("transform-rotation")
                && line.contains("!xr.float"))
    );
    let invalid = source.replace("transform-rotation: 45", "transform-rotation: 45px");
    assert!(
        compile(&invalid, "transform.xr")
            .err()
            .unwrap()
            .contains("type mismatch")
    );
    let raw = lower(source, "transform.xr");
    let invalid = raw.replace(
        "name = \"transform-origin-y\"",
        "name = \"transform-scale-y\"",
    );
    assert!(
        mlir::process(&invalid)
            .unwrap_err()
            .contains("incorrect visual transform property or type")
    );
}

#[test]
fn slider_interaction_bindings_require_boolean_live_and_a_snap_mode() {
    let source = r#"
        component App inherits Window {
            Slider { live: false; snap-mode: ScrollSnapMode.always; }
            RangeSlider { live: true; snap-mode: ScrollSnapMode.on-release; }
        }
    "#;
    let output = compile(source, "slider-snap.xr").unwrap();
    assert!(output.mlir.contains("!xr.scrollsnapmode"));
    assert!(compile(&source.replace("live: false", "live: 1"), "slider-snap.xr").is_err());
    let raw = lower(source, "slider-snap.xr");
    let malformed = raw.replace("name = \"live\"", "name = \"snap-mode\"");
    assert!(
        mlir::process(&malformed)
            .unwrap_err()
            .contains("invalid slider interaction property type")
    );
}

#[test]
fn table_sort_state_and_commands_are_typed_and_require_a_table_owner() {
    let source = r#"
        component App inherits Window {
            in property <[string]> records;
            out property <int> column: table.current-sort-column;
            out property <bool> descending: table.current-sort-descending;
            callback sort();
            sort => { table.sort-by(0, true); table.clear-sort(); }
            table := TableView {
                TableColumn { title: "Name"; }
                for row in root.records : TableRow { Text { text: row; } }
            }
        }
    "#;
    let output = compile(source, "table-sort-state.xr").unwrap();
    assert!(output.mlir.contains("xr.table_sort_read"));
    assert!(output.mlir.contains("xr.table_sort_command"));
    for (from, to, expected) in [
        (
            "table.sort-by(0, true)",
            "table.sort-by(0, 1)",
            "type mismatch",
        ),
        (
            "callback sort();",
            "pure callback sort();",
            "not allowed in a pure callback",
        ),
        (
            "TableColumn {",
            "current-sort-column: 0; TableColumn {",
            "read-only",
        ),
    ] {
        let error = compile(&source.replace(from, to), "table-sort-state.xr")
            .err()
            .unwrap();
        assert!(error.contains(expected), "{error}");
    }
    let raw = lower(source, "table-sort-state.xr");
    let invalid = raw.replace("owner = \"table\"", "owner = \"root\"");
    assert!(
        mlir::process(&invalid)
            .unwrap_err()
            .contains("requires a statically addressed table")
    );
}

#[test]
fn auto_sort_requires_a_boolean_on_a_standard_table() {
    let source = r#"
        struct Entry { units: float }
        component App inherits Window {
            in property <[Entry]> records;
            StandardTableView {
                model: root.records;
                auto-sort: true;
                TableColumn { field: "units"; title: "Units"; }
            }
        }
    "#;
    let output = compile(source, "view-sort.xr").unwrap();
    let rust = output.rust.to_string();
    assert!(rust.contains("sorted_rows"));
    assert!(rust.contains("TableField :: compare_cells (& left . units , & right . units)"));
    let bad = source.replace("auto-sort: true", "auto-sort: 1");
    assert!(
        compile(&bad, "view-sort.xr")
            .err()
            .unwrap()
            .contains("type mismatch")
    );
    let raw = lower(source, "view-sort.xr");
    let invalid = raw.replace("name = \"model\"", "name = \"auto-sort\"");
    assert!(
        mlir::process(&invalid)
            .unwrap_err()
            .contains("auto-sort requires a bool")
    );
}

#[test]
fn table_selection_commands_and_read_properties_are_verified() {
    let source = r#"
        component App inherits Window {
            in property <[string]> records;
            out property <int> column: table.current-column;
            out property <int> count: table.selection-count;
            out property <bool> selected: table.has-selection;
            callback select();
            select => {
                table.select-cell(0, 0);
                table.select-range(0, 0, 2, 1);
                table.set-current-cell(1, 0);
                table.clear-selection();
            }
            table := TableView {
                selection-target: cells; multi-select: true;
                current-cell-changed(row, column) => {}
                TableColumn { title: "Name"; }
                for row in root.records : TableRow { Text { text: row; } }
            }
        }
    "#;
    let output = compile(source, "table-selection-api.xr").unwrap();
    assert!(output.mlir.contains("xr.table_selection_read"));
    assert!(output.mlir.contains("xr.table_selection_command"));
    for (from, to, expected) in [
        (
            "table.select-cell(0, 0)",
            "table.select-cell(0, true)",
            "type mismatch",
        ),
        (
            "callback select();",
            "pure callback select();",
            "not allowed in a pure callback",
        ),
        (
            "selection-target: cells",
            "current-column: 0; selection-target: cells",
            "read-only",
        ),
        (
            "current-cell-changed(row, column)",
            "current-cell-changed(row)",
            "expects two int",
        ),
    ] {
        let error = compile(&source.replace(from, to), "table-selection-api.xr")
            .err()
            .unwrap();
        assert!(error.contains(expected), "{error}");
    }
    let raw = lower(source, "table-selection-api.xr");
    let invalid = raw.replace("owner = \"table\"", "owner = \"root\"");
    assert!(
        mlir::process(&invalid)
            .unwrap_err()
            .contains("statically addressed table")
    );
}

#[test]
fn table_edit_requests_check_types_purity_and_receiver() {
    let source = r#"
        component App inherits Window {
            in-out property <[string]> records;
            callback edit();
            edit => { table.edit-cell(0, 0); table.cancel-edit(); }
            field := LineEdit {}
            table := TableView {
                edit-on-typing: true;
                edit-on-tab: true;
                TableColumn { title: "Name"; }
                for row in root.records : TableRow { TableCell { text <=> row; } }
            }
        }
    "#;
    let output = compile(source, "table-edit.xr").unwrap();
    assert!(output.mlir.contains("xr.text_command"));
    assert!(
        output
            .rust
            .to_string()
            .contains("xrack_runtime :: next_table_edit_cell")
    );
    assert!(
        output
            .rust
            .to_string()
            .contains("FocusCommandMethod :: EditTableCell")
    );
    for (from, to, expected) in [
        ("edit-cell(0, 0)", "edit-cell(0, true)", "type mismatch"),
        ("edit-cell(0, 0)", "edit-cell(0)", "expects 2 arguments"),
        (
            "callback edit();",
            "pure callback edit();",
            "not allowed in a pure callback",
        ),
        ("edit-on-typing: true", "edit-on-typing: 1", "type mismatch"),
        ("edit-on-tab: true", "edit-on-tab: 1", "type mismatch"),
    ] {
        let error = compile(&source.replace(from, to), "table-edit.xr")
            .err()
            .unwrap();
        assert!(error.contains(expected), "{error}");
    }
    let raw = lower(source, "table-edit.xr");
    let invalid = raw.replace("owner = \"table\"", "owner = \"field\"");
    assert!(
        mlir::process(&invalid)
            .unwrap_err()
            .contains("statically addressed table view")
    );
}

#[test]
fn table_history_controls_verify_types_receivers_and_dependency_cycles() {
    let source = r#"
        component Demo inherits Window {
            in-out property <[string]> records: ["One"];
            out property <bool> available: table.can-undo;
            callback revert();
            revert => { table.undo(); table.redo(); }
            table := TableView {
                undoable: true;
                TableColumn { title: "Name"; }
                for row in root.records : TableRow { TableCell { text <=> row; } }
            }
        }
    "#;
    let compiled = compile(source, "table-history-controls.xr").unwrap();
    assert!(compiled.mlir.contains("xr.table_history_read"));
    assert_eq!(
        compiled
            .mlir
            .matches("\"xr.table_history_command\"")
            .count(),
        2
    );
    for (from, to, expected) in [
        ("table.undo()", "table.undo(1)", "takes no arguments"),
        (
            "callback revert();",
            "pure callback revert();",
            "not allowed in a pure callback",
        ),
        ("undoable: true", "can-undo: true", "read-only"),
        (
            "undoable: true",
            "undoable: self.can-undo",
            "cyclic property binding",
        ),
        (
            "records: [\"One\"]",
            "records: table.can-redo ? [\"One\"] : [\"Two\"]",
            "cyclic property binding",
        ),
    ] {
        let error = compile(&source.replace(from, to), "table-history-controls.xr")
            .err()
            .unwrap();
        assert!(error.contains(expected), "{error}");
    }
    let raw = lower(source, "table-history-controls.xr");
    let wrong_owner = raw.replace("owner = \"table\"", "owner = \"root\"");
    assert!(
        mlir::process(&wrong_owner)
            .unwrap_err()
            .contains("statically addressed table")
    );
    let wrong_method = raw.replace("method = \"undo\"", "method = \"clear\"");
    assert!(
        mlir::process(&wrong_method)
            .unwrap_err()
            .contains("requires undo or redo")
    );
}

#[test]
fn standard_table_editable_columns_require_a_writable_record_model() {
    let source = r#"
        struct Stock { quantity: int, due: Date, price: float }
        struct Entry { stock: Stock }
        component App inherits Window {
            in-out property <[Entry]> records;
            in property <bool> allow-edit: true;
            table := StandardTableView {
                model: root.records;
                edit-on-typing: true;
                pasteable: true;
                TableColumn { field: "stock.quantity"; editable: root.allow-edit; }
                TableColumn { field: "stock.due"; editable: true; }
                TableColumn { field: "stock.price"; editable: true; }
            }
            callback edit();
            edit => { table.edit-cell(0, 0); }
        }
    "#;
    let rust = compile(source, "editable-projection.xr")
        .unwrap()
        .rust
        .to_string();
    for field_type in [
        ":: core :: primitive :: i64",
        ":: xrack_runtime :: Date",
        ":: core :: primitive :: f64",
    ] {
        assert!(rust.contains(&format!(
            "{field_type} as :: xrack_runtime :: TableField > :: EDITOR"
        )));
    }
    assert!(rust.contains("TableEditHistory"));
    assert!(rust.contains("paste_in_order"));
    for (from, to, expected) in [
        (
            "in-out property <[Entry]>",
            "in property <[Entry]>",
            "read-only",
        ),
        (
            "model: root.records",
            "model: [Entry { stock: Stock { quantity: 1, due: Date { year: 2026, month: 11, day: 11 }, price: 1.0 } }]",
            "writable model property",
        ),
        ("editable: root.allow-edit", "editable: 12", "type mismatch"),
        ("pasteable: true", "pasteable: 12", "type mismatch"),
    ] {
        let error = compile(&source.replace(from, to), "editable-projection.xr")
            .err()
            .unwrap();
        assert!(error.contains(expected), "{error}");
    }
    let raw = lower(source, "editable-projection.xr");
    let read_only = raw.replace("visibility = \"in-out\"", "visibility = \"in\"");
    assert!(
        mlir::process(&read_only)
            .unwrap_err()
            .contains("read-only property")
    );
}

#[test]
fn standard_table_filters_check_nested_string_fields_in_source_and_mlir() {
    let source = r#"
        struct Details { label: string, units: int }
        struct FilteredRecord { detail: Details }
        component Demo inherits Window {
            in property <[FilteredRecord]> records;
            in property <string> query;
            StandardTableView {
                model: root.records;
                filter-field: "detail.label";
                filter-text: root.query;
                TableColumn { field: "detail.units"; }
            }
        }
    "#;
    let compiled = compile(source, "table-filter.xr").unwrap();
    assert!(compiled.rust.to_string().contains("projected_rows"));
    for (from, to, expected) in [
        (
            "filter-field: \"detail.label\";",
            "",
            "filter-text requires filter-field",
        ),
        (
            "filter-field: \"detail.label\"",
            "filter-field: \"detail.units\"",
            "requires a string field",
        ),
        (
            "filter-field: \"detail.label\"",
            "filter-field: root.query",
            "constant string field name",
        ),
    ] {
        let error = compile(&source.replace(from, to), "table-filter.xr")
            .err()
            .unwrap();
        assert!(error.contains(expected), "{error}");
    }
    let raw = lower(source, "table-filter.xr");
    let invalid = raw.replace("detail.label", "detail.units");
    assert!(
        mlir::process(&invalid)
            .unwrap_err()
            .contains("filter-field requires a string field")
    );
}

#[test]
fn editor_preview_compiles_open_buffers_and_reports_import_dependencies() {
    let dir = std::env::var_os("TEST_TMPDIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join(format!("xrack-editor-preview-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let entry = dir.join("editor.xr");
    let imported = dir.join("widget.xr");
    let new_import = dir.join("new.xr");
    std::fs::write(&entry, "unparseable saved entry").unwrap();
    std::fs::write(&imported, "unparseable saved import").unwrap();
    let preview = xrack_compiler::compile_preview(
        r#"import { Card as Tile } from "widget.xr";
        import { NewItem } from "new.xr";
        component Internal inherits Rectangle {}
        export component App inherits Window { Tile {} NewItem {} }"#,
        entry.to_str().unwrap(),
        [
            (
                imported.clone(),
                "export component Card inherits Rectangle { width: 37px; }".into(),
            ),
            (
                new_import.clone(),
                "export component NewItem inherits Rectangle { width: 41px; }".into(),
            ),
        ]
        .into(),
    )
    .unwrap();
    assert_eq!(preview.components, ["App", "Internal"]);
    assert_eq!(
        preview.inputs,
        [
            entry,
            imported.canonicalize().unwrap(),
            dir.canonicalize().unwrap().join("new.xr"),
        ]
        .into()
    );
    let rust = preview.rust.to_string();
    assert!(
        rust.contains("__xrack_preview_component") && rust.contains("37") && rust.contains("41")
    );
    assert!(!new_import.exists());
    assert_eq!(
        std::fs::read_to_string(imported).unwrap(),
        "unparseable saved import"
    );
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn sound_effect_verifies_nonvisual_bindings_and_playback_commands() {
    let source = r#"
        export component App inherits Window {
            out property <bool> loaded: chime.ready;
            out property <int> remaining: chime.loops-remaining;
            chime := SoundEffect { source: "click.wav"; volume: 0.5; loops: 2; }
            confirm := Button { clicked => { chime.play(); } }
            cancel := Button { clicked => { chime.stop(); } }
        }
    "#;
    let compiled = compile(source, "sound-effect.xr").unwrap();
    assert!(compiled.mlir.contains("\"xr.media_command\""));
    assert!(compiled.mlir.contains("\"xr.media_read\""));
    assert!(compiled.mlir.contains("method = \"stop\""));
    assert!(compiled.mlir.contains("value = \"loops-remaining\""));
    for (from, to) in [
        ("volume: 0.5", "width: 10px"),
        ("volume: 0.5", "ready: true"),
        ("loops: 2", "loops: true"),
        ("loops: 2", "loops-remaining: 1"),
        ("chime.play()", "chime.seek(0)"),
        ("chime.play()", "chime.play(1)"),
        ("chime.stop()", "chime.stop(1)"),
    ] {
        assert!(
            compile(&source.replace(from, to), "sound-effect.xr").is_err(),
            "{to}"
        );
    }

    let invalid = compiled
        .mlir
        .replace("method = \"play\"", "method = \"pause\"");
    assert_ne!(compiled.mlir, invalid);
    let error = mlir::Module::parse(&invalid).err().unwrap();
    assert!(error.contains("supporting this command"), "{error}");
}

#[test]
fn combo_submission_rules_are_typed_and_follow_the_draft() {
    let source = r#"
        export component App inherits Window {
            in-out property <string> draft: "12,50";
            out property <bool> ready: amount.acceptable-input;
            amount := ComboBox {
                editable: true;
                input-type: InputType.decimal;
                decimal-separator: ",";
                minimum-value: 10.0; maximum-value: 25.0;
                minimum-length: 2; maximum-length: 5; decimals: 2;
                edit-text <=> root.draft;
            }
        }
    "#;
    let compiled = compile(source, "combo_submission.xr").unwrap();
    assert!(compiled.mlir.contains("xr.text_accepts"));
    let conditional_model = source.replace(
        "edit-text <=> root.draft;",
        "edit-text <=> root.draft; model: root.ready ? [\"12,50\"] : [\"20,25\"];",
    );
    assert!(compile(&conditional_model, "combo_submission.xr").is_ok());
    for (from, to, expected) in [
        ("minimum-length: 2", "minimum-length: true", "type mismatch"),
        ("minimum-length: 2", "acceptable-input: true", "read-only"),
        ("minimum-length: 2", "submit-when: root.ready", "cyclic"),
    ] {
        let error = compile(&source.replace(from, to), "combo_submission.xr")
            .err()
            .unwrap();
        assert!(error.contains(expected), "{error}");
    }
    let text_cycle = source.replace(
        "edit-text <=> root.draft",
        "edit-text: root.ready ? \"yes\" : \"no\"",
    );
    let error = compile(&text_cycle, "combo_submission.xr").err().unwrap();
    assert!(error.contains("cyclic"), "{error}");
}

#[test]
fn editing_finished_handler_is_typed_and_scoped_to_text_editors() {
    let source = r#"
        export component App inherits Window {
            out property <string> saved: "";
            notes := TextArea {
                editing-finished(text, acceptable) => {
                    root.saved = acceptable ? text : "";
                }
            }
        }
    "#;
    assert!(compile(source, "editor-events.xr").is_ok());
    assert!(compile(&source.replace("TextArea", "Button"), "editor-events.xr").is_err());
    assert!(
        compile(
            &source.replace("(text, acceptable)", "()"),
            "editor-events.xr"
        )
        .is_err()
    );
    let ir = lower(source, "editor-events.xr");
    assert!(mlir::process(&ir).is_ok());
    let invalid = ir.replace(r#"\22ty\22:\22bool\22"#, r#"\22ty\22:\22int\22"#);
    assert_ne!(ir, invalid);
    assert!(
        mlir::process(&invalid)
            .unwrap_err()
            .contains("editing-finished requires")
    );
}

#[test]
fn input_mask_type_and_pattern_are_checked() {
    let source = r#"
        export component App inherits Window {
            code := LineEdit { input-mask: "AA-999"; maximum-length: 6; }
            out property <bool> ready: code.acceptable-input;
        }
    "#;
    assert!(compile(source, "masked.xr").is_ok());
    let cycle = source.replace(r#""AA-999""#, r#"self.acceptable-input ? "A" : "9""#);
    assert!(compile(&cycle, "masked.xr").is_err());
    let invalid = source.replace(r#""AA-999""#, r#""---""#);
    assert!(
        compile(&invalid, "masked.xr")
            .err()
            .unwrap()
            .contains("requires at least one slot")
    );
    let invalid = source.replace("LineEdit", "TextArea");
    assert!(
        compile(&invalid, "masked.xr")
            .err()
            .unwrap()
            .contains("input-mask")
    );
    let raw = lower(&source.replace(r#"input-mask: "AA-999";"#, ""), "masked.xr");
    let invalid = raw.replace(r#"name = "maximum-length""#, r#"name = "input-mask""#);
    assert!(
        mlir::process(&invalid)
            .unwrap_err()
            .contains("input-mask requires a string")
    );
}

#[test]
fn input_validity_is_a_typed_read_only_editor_property() {
    let source = r#"
        export component App inherits Window {
            editor := LineEdit { text: "AB"; }
            out property <InputValidity> state: editor.input-validity;
            out property <bool> may-submit: root.state == InputValidity.acceptable;
        }
    "#;
    let output = compile(source, "input-validity.xr").unwrap();
    assert!(output.mlir.contains("xr.text_validity"));
    assert!(output.mlir.contains("!xr.inputvalidity"));
    let invalid = source.replace(
        r#"text: "AB";"#,
        "input-validity: InputValidity.acceptable;",
    );
    assert!(
        compile(&invalid, "input-validity.xr")
            .err()
            .unwrap()
            .contains("read-only")
    );
    let raw = lower(source, "input-validity.xr");
    let wrong_owner = raw.replace(r#"kind = "LineEdit""#, r#"kind = "Button""#);
    assert_ne!(raw, wrong_owner);
    assert!(
        mlir::process(&wrong_owner)
            .unwrap_err()
            .contains("input-validity requires a statically addressed text editor")
    );
    let invalid_case = raw.replace(r#"value = "acceptable""#, r#"value = "pending""#);
    assert_ne!(raw, invalid_case);
    assert!(mlir::process(&invalid_case).is_err());
}

#[test]
fn iso_time_helpers_and_popup_bounds_use_typed_ir() {
    let source = r#"
        export component App inherits Window {
            in-out property <string> text: "13:30:42";
            in property <Time> latest: Time { hour: 17 };
            out property <Time> typed: Times.parse-iso(root.text, root.latest);
            out property <bool> valid: Times.is-valid-iso(root.text) && Times.in-range(root.typed, Time { hour: 9 }, root.latest);
            out property <string> display: Times.format-iso(root.typed, false);
            TimePickerPopup { minimum-time: Time { hour: 9 }; maximum-time: root.latest; }
        }
    "#;
    let output = compile(source, "time-field.xr").unwrap();
    for op in [
        "xr.time_parse_iso",
        "xr.time_is_valid_iso",
        "xr.time_in_range",
        "xr.time_format_iso",
        r#"name = "maximum-time""#,
    ] {
        assert!(output.mlir.contains(op), "{op}");
    }
    assert!(
        compile(
            &source.replace("maximum-time: root.latest", "maximum-time: 17"),
            "time-field.xr"
        )
        .is_err()
    );
    assert!(
        compile(
            &source.replace("TimePickerPopup", "TimePicker"),
            "time-field.xr"
        )
        .is_err()
    );
    let raw = lower(
        r#"export component App inherits Window { TimePickerPopup { show-seconds: true; } }"#,
        "time-field.xr",
    );
    let raw = raw.replace(r#"name = "show-seconds""#, r#"name = "minimum-time""#);
    assert!(
        mlir::process(&raw)
            .unwrap_err()
            .contains("time bounds require a Time on TimePickerPopup")
    );
}

#[test]
fn date_parsing_and_availability_lower_with_typed_fallback_and_models() {
    let source = r#"
        export component App inherits Window {
            in-out property <string> text: "2024-02-29";
            in property <Date> minimum: Date { year: 2024, month: 2, day: 5 };
            in property <Date> maximum: Date { year: 2024, month: 4, day: 15 };
            in-out property <[Date]> unavailable: [Date { year: 2024, month: 2, day: 29 }];
            out property <Date> parsed: Dates.parse-iso(root.text, root.minimum);
            out property <bool> allowed: Dates.is-selectable(root.parsed, root.minimum, root.maximum, [6, 7], root.unavailable);
        }
    "#;
    let output = compile(source, "date-text.xr").unwrap();
    for op in [
        "xr.date_parse_iso",
        "xr.date_is_selectable",
        "!xr.intlist",
        "!xr.datelist",
    ] {
        assert!(output.mlir.contains(op), "{op}");
    }
    assert!(
        compile(
            &source.replace("root.text, root.minimum", "false, root.minimum"),
            "date-text.xr"
        )
        .is_err()
    );
    assert!(
        compile(
            &source.replace("[6, 7], root.unavailable", "[6, 7], [6]"),
            "date-text.xr"
        )
        .is_err()
    );
}

#[test]
fn calendar_ranges_expose_typed_highlight_bindings_and_day_cell_inputs() {
    let source = r#"
        export component App inherits Window {
            MonthGrid {
                highlight-start: Date { year: 2024, month: 2, day: 27 };
                highlight-end: Date { year: 2024, month: 3, day: 1 };
                DayCell { visible: cell-in-range || cell-range-start || cell-range-end; }
            }
        }
    "#;
    let compiled = compile(source, "highlights.xr").unwrap();
    for name in [
        "highlight-start",
        "highlight-end",
        "cell-in-range",
        "cell-range-start",
        "cell-range-end",
    ] {
        assert!(compiled.mlir.contains(name), "{name}");
    }
    assert!(
        compile(
            &source.replace(
                "year: 2024, month: 2, day: 27",
                "year: true, month: 2, day: 27"
            ),
            "highlights.xr"
        )
        .is_err()
    );
    let raw = lower(
        r#"export component App inherits Window { MonthGrid { week-start: 5; } }"#,
        "highlights.xr",
    );
    let raw = raw.replace(r#"name = "week-start""#, r#"name = "highlight-end""#);
    assert!(
        mlir::process(&raw)
            .unwrap_err()
            .contains("calendar highlight endpoints require a Date")
    );
}

#[test]
fn date_month_models_and_fixed_month_grids_have_typed_ir() {
    let source = r#"
        export component App inherits Window {
            in-out property <Date> start: Date { year: 2024, month: 2, day: 29 };
            in-out property <Date> end: Date { year: 2025, month: 3, day: 2 };
            out property <[Date]> months: Dates.months(root.start, root.end);
            out property <int> last-index: Dates.month-index(root.start, root.end, root.end);
            MonthGrid { fixed-month: true; display-month: root.start; }
        }
    "#;
    let output = compile(source, "date-months.xr").unwrap();
    for op in [
        "xr.date_months",
        "xr.date_month_index",
        "!xr.datelist",
        r#"name = "fixed-month""#,
    ] {
        assert!(output.mlir.contains(op), "{op}");
    }
    assert!(
        compile(
            &source.replace("root.start, root.end", "9, root.end"),
            "date-months.xr"
        )
        .is_err()
    );
    assert!(compile(&source.replace("MonthGrid", "Calendar"), "date-months.xr").is_err());
    let raw = lower(
        r#"export component App inherits Window { MonthGrid { week-start: 1; } }"#,
        "date-months.xr",
    );
    let raw = raw.replace(r#"name = "week-start""#, r#"name = "fixed-month""#);
    assert!(
        mlir::process(&raw)
            .unwrap_err()
            .contains("fixed-month requires a boolean on MonthGrid")
    );
}

#[test]
fn date_lists_support_models_repeats_and_typed_calendar_bindings() {
    let source = r#"
        export component App inherits Window {
            in-out property <[Date]> holidays: [Date { year: 2024, month: 2, day: 29 }];
            out property <Date> first-holiday: Model.at(root.holidays, 0, Date { year: 2024, month: 1, day: 1 });
            pure callback echo([Date]) -> [Date];
            echo(dates) => { dates }
            Calendar { disabled-dates: root.holidays; }
            for holiday in root.holidays : Text { text: "" + holiday; }
            add := Button { clicked => { Model.append(root.holidays, Date { year: 2024, month: 3, day: 1 }); } }
        }
    "#;
    let compiled = compile(source, "holidays.xr").unwrap();
    for op in [
        "!xr.datelist",
        "xr.date_array",
        "xr.model_at",
        "xr.model_edit",
        "xr.repeat",
    ] {
        assert!(compiled.mlir.contains(op), "{op}");
    }
    assert!(
        compile(
            &source.replace("disabled-dates: root.holidays", "disabled-dates: [1]"),
            "holidays.xr"
        )
        .is_err()
    );
    let raw = lower(
        r#"export component App inherits Window { Calendar { disabled-weekdays: [6]; } }"#,
        "holidays.xr",
    );
    let raw = raw.replace(
        r#"name = "disabled-weekdays""#,
        r#"name = "disabled-dates""#,
    );
    assert!(
        mlir::process(&raw)
            .unwrap_err()
            .contains("disabled-dates requires [Date]")
    );
}

#[test]
fn disabled_weekdays_are_typed_in_source_and_raw_mlir() {
    let source =
        r#"export component App inherits Window { Calendar { disabled-weekdays: [6, 7]; } }"#;
    assert!(
        compile(source, "weekdays.xr")
            .unwrap()
            .mlir
            .contains(r#"name = "disabled-weekdays""#)
    );
    assert!(
        compile(
            &source.replace("Calendar", "WeekNumberColumn"),
            "weekdays.xr"
        )
        .is_err()
    );
    let wrong_type = r#"export component App inherits Window { Calendar { week-start: 6; } }"#;
    let raw = lower(wrong_type, "weekdays.xr")
        .replace(r#"name = "week-start""#, r#"name = "disabled-weekdays""#);
    assert!(
        mlir::process(&raw)
            .unwrap_err()
            .contains("disabled-weekdays requires [int]")
    );
}

#[test]
fn editor_appearance_checks_types_and_owners() {
    let source = r#"
        export component App inherits Window {
            editor := LineEdit { padding-left: 16px; placeholder-text-color: #ffb554; }
        }
    "#;
    let output = compile(source, "appearance.xr").unwrap();
    assert!(output.mlir.contains(r#"name = "padding-left""#));
    assert!(output.mlir.contains(r#"name = "placeholder-text-color""#));
    assert!(compile(&source.replace("LineEdit", "Rectangle"), "appearance.xr").is_err());
    assert!(compile(&source.replace("16px", "false"), "appearance.xr").is_err());
    let invalid = lower(
        &source.replace(
            "placeholder-text-color: #ffb554;",
            "placeholder-text: \"hint\";",
        ),
        "appearance.xr",
    )
    .replace(
        r#"name = "placeholder-text""#,
        r#"name = "placeholder-text-color""#,
    );
    assert!(
        mlir::process(&invalid)
            .unwrap_err()
            .contains("incorrect text editor appearance property or type")
    );
}

#[test]
fn password_character_checks_element_and_type_in_source_and_mlir() {
    let source = r#"
        export component App inherits Window {
            editor := LineEdit { input-type: password; password-character: "🔒"; }
        }
    "#;
    let output = compile(source, "password-character.xr").unwrap();
    assert!(output.mlir.contains(r#"name = "password-character""#));
    assert!(compile(&source.replace(r#""🔒""#, "true"), "password-character.xr").is_err());
    let wrong_element = source
        .replace("LineEdit", "TextArea")
        .replace("input-type: password;", "");
    assert!(compile(&wrong_element, "password-character.xr").is_err());
    let wrong_type = lower(
        &source.replace(r#"password-character: "🔒";"#, "password-visible: true;"),
        "password-character.xr",
    );
    let invalid = wrong_type.replace(
        r#"name = "password-visible""#,
        r#"name = "password-character""#,
    );
    assert!(
        mlir::process(&invalid)
            .unwrap_err()
            .contains("incorrect password input property or type")
    );
}

#[test]
fn password_mask_delay_checks_raw_mlir_and_source_types() {
    let source = r#"
        export component App inherits Window {
            editor := LineEdit { input-type: password; password-mask-delay: 750; }
        }
    "#;
    let output = compile(source, "password-mask-delay.xr").unwrap();
    assert!(output.mlir.contains(r#"name = "password-mask-delay""#));
    assert!(compile(&source.replace("750", "true"), "password-mask-delay.xr").is_err());
    let invalid = lower(
        &source.replace("password-mask-delay: 750;", "password-visible: true;"),
        "password-mask-delay.xr",
    )
    .replace(
        r#"name = "password-visible""#,
        r#"name = "password-mask-delay""#,
    );
    assert!(
        mlir::process(&invalid)
            .unwrap_err()
            .contains("incorrect password input property or type")
    );
}

#[test]
fn input_pattern_checks_binding_types_and_reports_invalid_literals() {
    let source = r#"
        export component App inherits Window {
            editor := LineEdit { input-pattern: "[A-Z]{2}-[0-9]{3}"; maximum-length: 6; }
            out property <bool> valid: editor.acceptable-input;
        }
    "#;
    let output = compile(source, "input-pattern.xr").unwrap();
    assert!(output.mlir.contains(r#"name = "input-pattern""#));
    assert!(output.mlir.contains("xr.text_accepts"));
    let invalid = source.replace("[A-Z]{2}-[0-9]{3}", "[");
    let error = compile(&invalid, "input-pattern.xr").err().unwrap();
    assert!(error.contains("invalid input-pattern"), "{error}");
    let oversized = source.replace("[A-Z]{2}-[0-9]{3}", "a{1000000000}");
    assert!(
        compile(&oversized, "input-pattern.xr")
            .err()
            .unwrap()
            .contains("invalid input-pattern")
    );
    let wrong_type = source.replace(r#""[A-Z]{2}-[0-9]{3}""#, "true");
    assert!(compile(&wrong_type, "input-pattern.xr").is_err());
    let cycle = source.replace(
        r#""[A-Z]{2}-[0-9]{3}""#,
        r#"self.acceptable-input ? "A" : "B""#,
    );
    assert!(
        compile(&cycle, "input-pattern.xr")
            .err()
            .unwrap()
            .contains("cycl")
    );

    let raw = lower(
        &source.replace(r#"input-pattern: "[A-Z]{2}-[0-9]{3}"; "#, ""),
        "input-pattern.xr",
    );
    let invalid = raw.replace(r#"name = "maximum-length""#, r#"name = "input-pattern""#);
    assert_ne!(raw, invalid);
    assert!(
        mlir::process(&invalid)
            .unwrap_err()
            .contains("input-pattern requires a string")
    );
}

#[test]
fn text_editing_commands_check_operand_types_and_read_only_targets() {
    let source = r#"
        export component App inherits Window {
            field := LineEdit {}
            button := Button {
                clicked => {
                    field.insert-text(3, "note");
                    field.replace-text(1, 2, "é");
                    field.remove-text(0, 1);
                    field.append-text("!");
                    field.clear-text();
                }
            }
        }
    "#;
    let output = compile(source, "text-commands.xr").unwrap();
    assert!(output.mlir.contains(r#"method = "replace-text""#));
    assert!(output.mlir.contains("!xr.string"));
    assert!(
        compile(&source.replace(r#"3, "note""#, "3, 4"), "text-commands.xr")
            .err()
            .unwrap()
            .contains("type mismatch")
    );
    assert!(
        compile(
            &source.replace("LineEdit", "StyledText"),
            "text-commands.xr"
        )
        .is_err()
    );
    let raw = lower(source, "text-commands.xr");
    let bad = raw.replace(r#"method = "replace-text""#, r#"method = "remove-text""#);
    assert_ne!(raw, bad);
    assert!(
        mlir::process(&bad)
            .unwrap_err()
            .contains("valid text edit signature")
    );
}

#[test]
fn text_selection_colors_are_bound_and_type_checked() {
    let source = r#"
        export component App inherits Window {
            in-out property <color> ink: #ffe600;
            LineEdit { selection-color: #603fa2; selected-text-color: root.ink; }
            TextArea { selection-color: #603fa2; selected-text-color: root.ink; }
            StyledText { selectable: true; selected-text-color: root.ink; }
        }
    "#;
    let output = compile(source, "selection-colors.xr").unwrap();
    assert!(output.mlir.contains(r#"name = "selected-text-color""#));
    assert!(
        compile(&source.replace("root.ink", "true"), "selection-colors.xr")
            .err()
            .unwrap()
            .contains("type mismatch")
    );
    assert!(compile(&source.replace("TextArea", "Label"), "selection-colors.xr").is_err());
    let raw = lower(source, "selection-colors.xr");
    let invalid = raw.replace(r#"kind = "LineEdit""#, r#"kind = "Label""#);
    assert_ne!(raw, invalid);
    assert!(
        mlir::process(&invalid)
            .unwrap_err()
            .contains("requires a color on a text editor or StyledText")
    );
}

#[test]
fn styled_text_selectable_enables_read_only_text_commands() {
    let source = r#"
        export component App inherits Window {
            out property <string> selected: doc.selected-text;
            doc := StyledText { selectable: true; text: @markdown("**Bold** [link](#a)"); }
            trigger := Button { clicked => { doc.focus(); doc.select-all(); doc.copy(); } }
        }
    "#;
    let output = compile(source, "selectable.xr").unwrap();
    assert!(output.mlir.contains(r#"name = "selectable""#));
    assert!(output.mlir.contains(r#"method = "copy""#));
    assert!(
        compile(
            &source.replace("doc.copy()", "doc.paste()"),
            "selectable.xr"
        )
        .is_err()
    );
    let raw = lower(source, "selectable.xr");
    let invalid = raw.replace(r#"method = "copy""#, r#"method = "paste""#);
    assert_ne!(invalid, raw);
    assert!(mlir::process(&invalid).is_err());
}

#[test]
fn persistent_selection_is_a_boolean_editor_binding() {
    let source = r#"
        export component App inherits Window {
            TextArea { persistent-selection: true; }
            LineEdit { persistent-selection: false; }
            ComboBox { editable: true; persistent-selection: true; }
        }
    "#;
    assert!(compile(source, "persistent.xr").is_ok());
    assert!(
        compile(&source.replace("TextArea", "Label"), "persistent.xr")
            .err()
            .unwrap()
            .contains("persistent-selection")
    );
    assert!(
        compile(
            &source.replace("persistent-selection: true", "persistent-selection: 2"),
            "persistent.xr"
        )
        .err()
        .unwrap()
        .contains("type mismatch")
    );
    let raw = lower(source, "persistent.xr");
    let bad = raw.replacen(r#"kind = "TextArea""#, r#"kind = "Label""#, 1);
    assert_ne!(raw, bad);
    assert!(
        mlir::process(&bad)
            .unwrap_err()
            .contains("persistent-selection requires a boolean on a text editor")
    );
}

#[test]
fn editor_overwrite_and_select_word_are_checked() {
    let source = r#"
        export component Editor inherits Window {
            in-out property <bool> overwrite: true;
            line := LineEdit { overwrite-mode: root.overwrite; }
            TextArea { overwrite-mode: true; }
            ComboBox { editable: true; overwrite-mode: root.overwrite; }
            select-tool := Button { clicked => { line.select-word(); } }
        }
    "#;
    let compiled = compile(source, "overwrite.xr").unwrap();
    assert!(compiled.mlir.contains(r#"name = "overwrite-mode""#));
    assert!(compiled.mlir.contains(r#"method = "select-word""#));
    for (from, to, expected) in [
        ("TextArea {", "Label {", "overwrite-mode"),
        (
            "overwrite-mode: true",
            "overwrite-mode: 10",
            "type mismatch",
        ),
        (
            "line.select-word()",
            "line.select-word(1)",
            "expects 0 arguments",
        ),
    ] {
        let error = compile(&source.replace(from, to), "overwrite.xr")
            .err()
            .unwrap();
        assert!(error.contains(expected), "{error}");
    }
    let raw = lower(source, "overwrite.xr");
    let invalid = raw.replacen(r#"kind = "TextArea""#, r#"kind = "Label""#, 1);
    assert_ne!(raw, invalid);
    assert!(
        mlir::process(&invalid)
            .unwrap_err()
            .contains("overwrite-mode requires")
    );
}

#[test]
fn mouse_selection_mode_is_checked_in_source_and_mlir() {
    let source = r#"
        export component App inherits Window {
            in-out property <MouseSelectionMode> mode: MouseSelectionMode.words;
            TextInput { mouse-selection-mode: root.mode; }
            LineEdit { mouse-selection-mode: characters; }
            TextEdit { mouse-selection-mode: caret; }
            TextArea { mouse-selection-mode: root.mode; }
            SearchField { mouse-selection-mode: root.mode; }
            ComboBox { editable: true; mouse-selection-mode: root.mode; }
        }
    "#;
    let output = compile(source, "mouse-selection.xr").unwrap();
    assert!(output.mlir.contains("!xr.mouseselectionmode"));
    assert!(
        output
            .rust
            .to_string()
            .contains("MouseSelectionMode :: Words")
    );
    for (from, to, expected) in [
        (
            "mouse-selection-mode: caret",
            "mouse-selection-mode: true",
            "type mismatch",
        ),
        ("LineEdit {", "Button {", "mouse-selection-mode"),
    ] {
        let error = compile(&source.replace(from, to), "mouse-selection.xr")
            .err()
            .unwrap();
        assert!(error.contains(expected), "{error}");
    }

    let raw = lower(source, "mouse-selection.xr");
    let bad_literal = raw.replace(r#"value = "words""#, r#"value = "unknown""#);
    assert_ne!(raw, bad_literal);
    assert!(mlir::process(&bad_literal).is_err());
    let bad_control = raw.replace(r#"kind = "LineEdit""#, r#"kind = "Button""#);
    assert_ne!(raw, bad_control);
    assert!(
        mlir::process(&bad_control)
            .unwrap_err()
            .contains("mouse-selection-mode requires")
    );
}

#[test]
fn text_submission_status_is_read_only_and_cannot_feed_its_own_rule() {
    let source = r#"
        export component App inherits Window {
            in-out property <string> code: "";
            entry := LineEdit {
                minimum-length: 4;
                submit-when: root.code != "0000";
                text <=> root.code;
            }
            send := Button { enabled: entry.acceptable-input; }
        }
    "#;
    let compiled = compile(source, "text_submission.xr").unwrap();
    assert!(compiled.mlir.contains("\"xr.text_accepts\""));

    let writable = source.replace("minimum-length: 4", "acceptable-input: true");
    let error = compile(&writable, "text_submission.xr").err().unwrap();
    assert!(error.contains("read-only"), "{error}");

    let assignment = source.replace(
        "enabled: entry.acceptable-input;",
        "clicked => { entry.acceptable-input = true; }",
    );
    let error = compile(&assignment, "text_submission.xr").err().unwrap();
    assert!(error.contains("read-only"), "{error}");

    let cyclic = source.replace("root.code != \"0000\"", "self.acceptable-input");
    let error = compile(&cyclic, "text_submission.xr").err().unwrap();
    assert!(error.contains("cycl"), "{error}");

    let invalid = compiled
        .mlir
        .replace("value = \"acceptable-input\"", "value = \"invalid\"");
    assert_ne!(compiled.mlir, invalid);
    let error = mlir::Module::parse(&invalid).err().unwrap();
    assert!(
        error.contains("statically addressed text editor"),
        "{error}"
    );
}

#[test]
fn text_editor_numeric_limits_are_typed_in_source_and_mlir() {
    let source = r#"
        export component App inherits Window {
            entry := TextInput { input-type: decimal; minimum-value: -1; decimals: 2; }
            send := Button { enabled: entry.acceptable-input; }
        }
    "#;
    let compiled = compile(source, "numeric_validation.xr").unwrap();
    for (old, new) in [
        ("minimum-value: -1", "minimum-value: false"),
        ("decimals: 2", "decimals: 2.5"),
        ("minimum-value: -1", "maximum-value: \"100\""),
    ] {
        assert!(compile(&source.replace(old, new), "numeric_validation.xr").is_err());
    }
    let animation = source.replace(
        "decimals: 2;",
        "decimals: 2; animate decimals { duration: 1ms; }",
    );
    let error = compile(&animation, "numeric_validation.xr").err().unwrap();
    assert!(error.contains("cannot animate"), "{error}");

    let invalid = compiled
        .mlir
        .replace("name = \"decimals\"", "name = \"maximum-value\"");
    assert_ne!(compiled.mlir, invalid);
    let error = mlir::Module::parse(&invalid).err().unwrap();
    assert!(error.contains("requires a float"), "{error}");

    let cyclic = source.replace(
        "minimum-value: -1",
        "minimum-value: self.acceptable-input ? -1 : 0",
    );
    let error = compile(&cyclic, "numeric_validation.xr").err().unwrap();
    assert!(error.contains("cycl"), "{error}");
}

#[test]
fn text_selection_reads_verify_type_owner_and_read_only_access() {
    let source = r#"
        export component App inherits Window {
            out property <int> caret: entry.cursor-offset;
            out property <string> selection: entry.selected-text;
            entry := TextArea {}
            panel := Rectangle {}
            run := Button {}
        }
    "#;
    let compiled = compile(source, "editor_selection.xr").unwrap();
    assert!(compiled.mlir.contains("\"xr.text_selection_read\""));

    for invalid in [
        source.replace("entry.selected-text", "panel.selected-text"),
        source.replace("TextArea {}", "TextArea { cursor-offset: 0; }"),
        source.replace(
            "Button {}",
            "Button { clicked => { entry.selected-text = \"x\"; } }",
        ),
    ] {
        assert!(
            compile(&invalid, "editor_selection.xr").is_err(),
            "{invalid}"
        );
    }
    let wrong_type = compiled.mlir.replace(
        "value = \"selected-text\"",
        "value = \"selection-end-offset\"",
    );
    assert_ne!(compiled.mlir, wrong_type);
    let error = mlir::Module::parse(&wrong_type).err().unwrap();
    assert!(error.contains("matching type"), "{error}");
}

#[test]
fn text_command_availability_is_read_only_and_lowers_to_a_boolean() {
    let source = r#"
        export component App inherits Window {
            document := TextEdit {}
            undo-button := Button { enabled: document.can-undo; }
            copy-button := Button { enabled: document.can-copy; }
        }
    "#;
    let compiled = compile(source, "edit_commands.xr").unwrap();
    assert!(compiled.mlir.contains("\"xr.text_command_available\""));
    let invalid = compiled
        .mlir
        .replace("value = \"can-copy\"", "value = \"can-paste\"");
    assert_ne!(compiled.mlir, invalid);
    let error = mlir::Module::parse(&invalid).err().unwrap();
    assert!(
        error.contains("requires a text command property"),
        "{error}"
    );
    let writable = source.replace("TextEdit {}", "TextEdit { can-undo: true; }");
    let error = compile(&writable, "edit_commands.xr").err().unwrap();
    assert!(error.contains("read-only"), "{error}");
}

#[test]
fn view_counts_are_read_only_and_check_projection_dependencies() {
    let source = r#"
        export component App inherits Window {
            out property <int> rows: list.row-count;
            list := ListView { for item in 3 : Text { text: "" + item; } }
        }
    "#;
    let built = compile(source, "view_counts.xr").unwrap();
    assert!(built.mlir.contains("\"xr.view_count\""));
    let write = source.replace("for item", "row-count: 3; for item");
    assert!(
        compile(&write, "view_counts.xr")
            .err()
            .unwrap()
            .contains("read-only")
    );
    let cycle = source.replace("in 3 :", "in root.rows :");
    assert!(
        compile(&cycle, "view_counts.xr")
            .err()
            .unwrap()
            .contains("cycl")
    );

    let cyclic_mlir = built.mlir.replace(
        r#""xr.literal"() <{value = "3"}>"#,
        r#""xr.view_count"() <{owner = "list", value = "source-row-count"}>"#,
    );
    assert_ne!(built.mlir, cyclic_mlir);
    let error = mlir::Module::parse(&cyclic_mlir).err().unwrap();
    assert!(error.contains("cyclic"), "{error}");

    let invalid = built
        .mlir
        .replace("value = \"row-count\"", "value = \"invalid\"");
    assert_ne!(built.mlir, invalid);
    let error = mlir::Module::parse(&invalid).err().unwrap();
    assert!(error.contains("requires a row count property"), "{error}");
}

#[test]
fn empty_state_is_a_single_static_part_of_a_virtual_view() {
    let source = r#"
        export component App inherits Window {
            ListView {
                empty := EmptyState { Text { text: "No rows"; } }
                for index in 1 : Text { text: "" + index; }
            }
        }
    "#;
    let compiled = compile(source, "empty_state.xr").unwrap();
    assert!(compiled.mlir.contains("kind = \"EmptyState\""));
    for bad in [
        source.replace("ListView", "Rectangle"),
        source.replace("empty := EmptyState", "if true : empty := EmptyState"),
        source.replace("for index", "EmptyState {} for index"),
    ] {
        assert!(compile(&bad, "empty_state.xr").is_err(), "{bad}");
    }
    let bad_mlir = compiled
        .mlir
        .replace("kind = \"ListView\"", "kind = \"Rectangle\"");
    let error = mlir::Module::parse(&bad_mlir).err().unwrap();
    assert!(error.contains("static child of virtual view"), "{error}");
}

#[test]
fn list_selection_by_key_checks_model_field_and_control_mode() {
    let valid = r#"
        struct Record { id: int, text: string }
        export component App inherits Window {
            in-out property <int> selection: -1;
            in property <[Record]> records: [];
            StandardListView {
                current-key <=> root.selection;
                row-key-field: "id";
                model: root.records;
            }
        }
    "#;
    let compiled = compile(valid, "keyed_selection.xr").unwrap();
    assert!(compiled.mlir.contains("name = \"row-key-field\""));
    for invalid in [
        valid.replace("row-key-field: \"id\";", ""),
        valid.replace("row-key-field: \"id\";", "row-key-field: \"text\";"),
        valid.replace(
            "row-key-field: \"id\";",
            "row-key-field: \"id\"; current-index: 0;",
        ),
    ] {
        assert!(
            compile(&invalid, "keyed_selection.xr").is_err(),
            "{invalid}"
        );
    }
}

#[test]
fn key_navigation_has_typed_commands_and_read_only_selection_position() {
    let source = r#"
        struct Record { id: int, text: string }
        export component App inherits Window {
            in property <[Record]> records: [];
            out property <int> position: rows.selected-view-index;
            rows := StandardListView { model: root.records; row-key-field: "id"; }
            go := Button {
                clicked => { rows.select-key(8); rows.position-view-at-key(8, center); }
            }
        }
    "#;
    let compiled = compile(source, "key_navigation.xr").unwrap();
    assert!(compiled.mlir.contains("\"xr.list_selection_read\""));
    assert!(compiled.mlir.contains("method = \"select-key\""));
    assert!(compiled.mlir.contains("method = \"position-view-at-key\""));

    for bad in [
        source.replace("row-key-field: \"id\";", ""),
        source.replace("at-key(8, center)", "at-key(8, false)"),
        source.replace(
            "model: root.records;",
            "model: root.records; selected-view-index: 8;",
        ),
        source
            .replace("row-key-field: \"id\";", "current-index: root.position;")
            .replace(
                "rows.select-key(8); rows.position-view-at-key(8, center);",
                "",
            ),
    ] {
        assert!(compile(&bad, "key_navigation.xr").is_err(), "{bad}");
    }
    let raw = compiled
        .mlir
        .replace("name = \"row-key-field\"", "name = \"ignored-field\"");
    let error = mlir::Module::parse(&raw).err().unwrap();
    assert!(error.contains("with row-key-field"), "{error}");
}

#[test]
fn layout_direction_is_typed_on_windows_and_component_instances() {
    let source = r#"
        component Pair inherits HorizontalLayout { Text { text: "item"; } }
        export component App inherits Window {
            in property <LayoutDirection> direction: right-to-left;
            layout-direction: root.direction;
            Pair { layout-direction: left-to-right; }
        }
    "#;
    let compiled = compile(source, "layout_direction.xr").unwrap();
    assert!(compiled.mlir.contains("!xr.layoutdirection"));
    assert!(compiled.mlir.contains("\"xr.layout_bind\""));
    assert!(
        compile(
            &source.replace("layout-direction: root.direction", "layout-direction: 3"),
            "layout_direction.xr"
        )
        .is_err()
    );
    let raw = compiled
        .mlir
        .replace("value = \"right-to-left\"", "value = \"sideways\"");
    assert_ne!(compiled.mlir, raw);
    assert!(mlir::Module::parse(&raw).is_err());
}

#[test]
fn editor_horizontal_alignment_is_typed_through_mlir() {
    let source = r#"
        export component App inherits Window {
            layout-direction: right-to-left;
            LineEdit { horizontal-alignment: start; }
            TextArea { horizontal-alignment: end; }
        }
    "#;
    let compiled = compile(source, "rtl_text.xr").unwrap();
    assert!(compiled.mlir.contains("!xr.texthorizontalalignment"));
    assert!(
        compile(
            &source.replace("horizontal-alignment: start", "horizontal-alignment: false"),
            "rtl_text.xr"
        )
        .is_err()
    );
}

#[test]
fn scroll_visual_metrics_are_read_only_layout_outputs() {
    let source = r#"
        export component App inherits Window {
            out property <float> start: bar.visual-position;
            out property <float> extent: bar.visual-size;
            bar := ScrollBar { width: 120px; minimum-size: 0.2; }
        }
    "#;
    let compiled = compile(source, "thumb-geometry.xr").unwrap();
    assert_eq!(
        compiled.mlir.matches("\"xr.scroll_control_read\"").count(),
        2
    );
    for (field, expected) in [
        ("visual-size: 0.2", "read-only"),
        ("height: root.extent * 50px", "layout feedback"),
        ("minimum-size: root.extent", "cyclic property binding"),
    ] {
        let invalid = source.replace("minimum-size: 0.2", field);
        let error = compile(&invalid, "thumb-geometry.xr").err().unwrap();
        assert!(error.contains(expected), "{field}: {error}");
    }
}

#[test]
fn scroll_artwork_requires_static_unique_parts_owned_by_scroll_controls() {
    let source = r#"
        export component App inherits Window {
            ScrollBar {
                minimum-size: 0.2;
                ScrollTrack { background: #203040; }
                ScrollThumb { Rectangle { width: 50%; background: #506070; } }
            }
            ScrollIndicator { ScrollThumb { background: #708090; } }
        }
    "#;
    let valid = compile(source, "scroll-artwork.xr").unwrap();
    assert!(valid.mlir.contains("kind = \"ScrollTrack\""));
    assert!(valid.mlir.contains("kind = \"ScrollThumb\""));
    for (from, to, expected) in [
        ("ScrollBar {", "Rectangle {", "static child of ScrollBar"),
        ("ScrollTrack {", "if true : ScrollTrack {", "static child"),
        (
            "ScrollTrack {",
            "ScrollTrack {} ScrollTrack {",
            "at most one ScrollTrack",
        ),
        (
            "ScrollTrack {",
            "ScrollTrack { width: 10px;",
            "unknown property",
        ),
        (
            "ScrollTrack {",
            "Label {",
            "only accepts ScrollTrack and ScrollThumb",
        ),
    ] {
        let invalid = source.replacen(from, to, 1);
        let error = compile(&invalid, "scroll-artwork.xr").err().unwrap();
        assert!(error.contains(expected), "{from} -> {to}: {error}");
    }
}

#[test]
fn scroll_activity_is_typed_read_only_and_cannot_feed_layout() {
    let source = r#"
        export component App inherits Window {
            out property <bool> moving: bar.active;
            out property <bool> down: bar.pressed;
            out property <bool> over: bar.hovered;
            out property <bool> indicator-live: indicator.active;
            bar := ScrollBar { width: 120px; height: 12px; }
            indicator := ScrollIndicator {}
        }
    "#;
    let compiled = compile(source, "scroll-activity.xr").unwrap();
    assert_eq!(
        compiled.mlir.matches("\"xr.scroll_control_read\"").count(),
        4
    );
    for (from, to, expected) in [
        ("height: 12px;", "active: true;", "read-only"),
        ("height: 12px;", "visible: root.moving;", "layout feedback"),
        (
            "height: 12px;",
            "policy: root.moving ? always-on : always-off;",
            "cyclic property binding",
        ),
        ("indicator.active", "indicator.pressed", "pressed"),
    ] {
        let bad = source.replace(from, to);
        let error = compile(&bad, "scroll-activity.xr").err().unwrap();
        assert!(error.contains(expected), "{to}: {error}");
    }
}

#[test]
fn relative_scroll_commands_check_receivers_and_operands() {
    let source = r#"
        export component App inherits Window {
            callback move();
            move() => {
                bar.increase();
                bar.decrease();
                view.scroll-by(-12px, 36px);
            }
            bar := ScrollBar {}
            view := ScrollView {}
        }
    "#;
    let compiled = compile(source, "relative-scroll.xr").unwrap();
    for method in ["increase", "decrease", "scroll-by"] {
        assert!(compiled.mlir.contains(&format!("method = \"{method}\"")));
    }
    for (from, to, expected) in [
        ("bar.increase()", "bar.increase(3)", "takes no arguments"),
        ("-12px, 36px", "true, 36px", "expected length"),
        ("callback move()", "pure callback move()", "pure callback"),
        ("bar := ScrollBar", "bar := ScrollIndicator", "increase"),
    ] {
        let invalid = source.replace(from, to);
        let error = compile(&invalid, "relative-scroll.xr").err().unwrap();
        assert!(error.contains(expected), "{to}: {error}");
    }
    let raw = lower(source, "relative-scroll.xr");
    let original = raw
        .lines()
        .find(|line| line.contains("method = \"increase\""))
        .unwrap();
    let redirected = raw.replace(
        original,
        &original.replace("owner = \"bar\"", "owner = \"view\""),
    );
    let error = xrack_compiler::mlir::process(&redirected).unwrap_err();
    assert!(error.contains("statically addressed ScrollBar"), "{error}");
}

#[test]
fn scroll_view_gutters_have_typed_read_only_geometry_and_reject_feedback() {
    let source = r#"
        export component App inherits Window {
            out property <length> bar-width: view.effective-scroll-bar-width;
            out property <length> bar-height: view.effective-scroll-bar-height;
            view := ScrollView { reserve-scroll-bar-space: true; width: 180px; }
        }
    "#;
    let valid = compile(source, "scroll-gutters.xr").unwrap();
    assert_eq!(valid.mlir.matches("\"xr.viewport_read\"").count(), 2);
    for (from, to, diagnostic) in [
        (
            "reserve-scroll-bar-space: true",
            "reserve-scroll-bar-space: 10px",
            "expected bool",
        ),
        (
            "width: 180px",
            "effective-scroll-bar-width: 8px",
            "read-only",
        ),
        ("width: 180px", "height: root.bar-width", "layout feedback"),
        (
            "reserve-scroll-bar-space: true",
            "reserve-scroll-bar-space: root.bar-height > 0px",
            "layout feedback",
        ),
    ] {
        let bad = source.replace(from, to);
        let error = compile(&bad, "scroll-gutters.xr").err().unwrap();
        assert!(error.contains(diagnostic), "{to}: {error}");
    }
}

#[test]
fn focus_reveal_is_a_boolean_on_content_viewports() {
    let source = r#"
        export component Main inherits Window {
            ScrollView { auto-reveal-focus: false; }
            Flickable { auto-reveal-focus: true; }
        }
    "#;
    let compiled = compile(source, "focus-reveal.xr").unwrap();
    assert_eq!(
        compiled
            .mlir
            .matches("name = \"auto-reveal-focus\"")
            .count(),
        2
    );
    let invalid = source.replace("true;", "42px;");
    let error = compile(&invalid, "focus-reveal.xr").err().unwrap();
    assert!(error.contains("expected bool"), "{error}");
}

#[test]
fn child_positioning_checks_the_target_and_both_alignment_types() {
    let source = r#"
        component Card inherits Rectangle { width: 40px; height: 30px; }
        export component App inherits Window {
            callback jump();
            jump() => { view.position-view-at-child(card, center, contain); }
            view := ScrollView {
                Rectangle { card := Card {} }
            }
            elsewhere := Card {}
        }
    "#;
    let compiled = compile(source, "position-child.xr").unwrap();
    assert!(compiled.mlir.contains("\"xr.position_child\""));
    assert!(compiled.mlir.contains("target = \"card\""));
    for (from, to, message) in [
        (
            "card, center",
            "elsewhere, center",
            "descendant in the viewport",
        ),
        ("card, center", "\"card\", center", "named descendant"),
        ("center, contain", "2, contain", "expected scrollalignment"),
        ("callback jump()", "pure callback jump()", "pure callback"),
        (
            "Rectangle { card",
            "PopupWindow { card",
            "descendant in the viewport",
        ),
    ] {
        let error = compile(&source.replace(from, to), "position-child.xr")
            .err()
            .unwrap();
        assert!(error.contains(message), "{to}: {error}");
    }
    let raw = lower(source, "position-child.xr");
    let op = raw
        .lines()
        .find(|line| line.contains("\"xr.position_child\""))
        .unwrap();
    let invalid = raw.replace(
        op,
        &op.replace("target = \"card\"", "target = \"elsewhere\""),
    );
    let error = xrack_compiler::mlir::process(&invalid).unwrap_err();
    assert!(error.contains("descendant in the viewport"), "{error}");
}
