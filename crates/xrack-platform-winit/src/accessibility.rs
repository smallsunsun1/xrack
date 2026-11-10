//! Project the prepared XRack scene into the native accessibility tree.
use crate::accessibility_text::{
    EditorState, TextDocument, focused_editor, project_text, supports_ranges,
};
use accesskit::{
    Action, ActionData, HasPopup, Live, Node as AccessibleNode, NodeId, Role, ScrollUnit,
    SortDirection, Toggled, TreeId, TreeInfo, TreeUpdate,
};
use std::{collections::BTreeMap, rc::Rc};
use xrack_layout::{
    CalendarGeometry, Interaction, LayoutNode, MenuEntry, MenuPanel, Rect, SearchGeometry, Target,
    TextPosition,
};
use xrack_renderer_femtovg::TextSystem;
use xrack_runtime::{
    CalendarAction, CalendarState, CheckState, ControlAction, DecimalSpinRange, IndicatorState,
    InputType, ListMetrics, Node, Orientation, RangeAction, RowKey, ScrollBarRange, SliderInterval,
    SliderRange, SpinRange, StandardButtonKind, SwipeSide, Value, button_text, choice_identity,
    combo_editable, combo_text, group_index, indicator_state, is_window_overlay, spin_editable,
    text_input_value,
};

const ROOT: NodeId = NodeId(0);

fn overlay_row(scene: &Node, path: &[usize]) -> Option<(RowKey, usize)> {
    let mut node = scene;
    let mut row = None;
    for (depth, &index) in path.iter().enumerate() {
        node = node.children.get(index)?;
        if let Some(key) = node.row_key {
            row = Some((key, depth + 1));
        }
    }
    row
}

#[derive(Clone, Debug)]
pub(crate) enum Route {
    Element {
        target: Target,
        kind: &'static str,
        standard_kind: Option<StandardButtonKind>,
    },
    TextLink(Target),
    Choice {
        target: Target,
        index: usize,
        value: String,
        record: Option<(RowKey, Value)>,
    },
    ControlPart {
        target: Target,
        action: ControlAction,
    },
    MenuEntry {
        trigger: Target,
        entry: MenuEntry,
    },
    MenuPanel {
        trigger: Target,
        path: Vec<usize>,
        session: u64,
    },
}

#[derive(PartialEq, Eq, PartialOrd, Ord)]
enum Location {
    Static(Vec<usize>),
    Row {
        view: Vec<usize>,
        key: RowKey,
        child: Vec<usize>,
    },
}

#[derive(PartialEq, Eq, PartialOrd, Ord)]
struct Key {
    location: Location,
    kind: &'static str,
    id: Option<&'static str>,
    option: Option<(usize, String)>,
    row_option: Option<RowKey>,
    text_run: Option<(usize, String)>,
    menu_session: Option<u64>,
}

impl Key {
    fn element(node: &Node, path: &[usize], row: Option<(RowKey, usize)>) -> Self {
        let location = match row {
            Some((key, depth)) => Location::Row {
                view: path[..depth - 1].to_vec(),
                key,
                child: path[depth..].to_vec(),
            },
            None => Location::Static(path.to_vec()),
        };
        Self {
            location,
            kind: node.kind,
            id: node.id,
            option: node
                .standard_button_kind()
                .map(|kind| (0, kind.label().to_owned())),
            row_option: None,
            text_run: None,
            menu_session: None,
        }
    }
}

#[derive(PartialEq)]
struct PopupState {
    bounds: Rect,
    first: usize,
    len: usize,
    highlighted: Option<usize>,
}

#[derive(PartialEq)]
struct MenuFrame {
    session: u64,
    path: Vec<usize>,
    bounds: Rect,
    first: usize,
    len: usize,
    highlighted: Option<usize>,
}

struct Frame {
    scene: Rc<Node>,
    layout: Rc<LayoutNode>,
    focus: Option<Target>,
    editor: Option<EditorState>,
    delay: Option<(Target, bool)>,
    popup: Option<PopupState>,
    menu_trigger: Option<Target>,
    menus: Vec<MenuFrame>,
    scale: f64,
}

#[derive(Default)]
pub(crate) struct Projection {
    frame: Option<Frame>,
    ids: BTreeMap<Key, NodeId>,
    routes: BTreeMap<NodeId, Route>,
    documents: BTreeMap<NodeId, TextDocument>,
    next_id: u64,
}

impl Projection {
    pub fn clear(&mut self) {
        self.frame = None;
        self.ids.clear();
        self.routes.clear();
        self.documents.clear();
    }

    pub fn route(&self, id: NodeId) -> Option<Route> {
        self.routes.get(&id).cloned()
    }

    pub fn text_selection(
        &self,
        id: NodeId,
        selection: accesskit::TextSelection,
        current_value: &str,
    ) -> Option<(usize, TextPosition)> {
        let document = self.documents.get(&id)?;
        if !document.matches_value(&self.frame.as_ref()?.scene, current_value) {
            return None;
        }
        document.decode(selection)
    }

    pub fn update(
        &mut self,
        scene: &Rc<Node>,
        layout: &Rc<LayoutNode>,
        input: &Interaction,
        scale: f64,
        text_system: Option<&TextSystem>,
    ) -> Option<TreeUpdate> {
        let editor = EditorState::focused(scene, input);
        let delay = input
            .delay_accessibility()
            .map(|(target, ready)| (target.clone(), ready));
        let popup = input.choice_menu(scene, layout).map(|menu| PopupState {
            bounds: menu.bounds,
            first: menu.first,
            len: menu.rows.len(),
            highlighted: menu.highlighted,
        });
        let menu_panels = input.menu_panels(scene, layout);
        let menu_trigger = input.menu_trigger().cloned();
        let menus: Vec<_> = menu_panels
            .iter()
            .map(|panel| MenuFrame {
                session: panel.session(),
                path: panel.path().to_vec(),
                bounds: panel.bounds,
                first: panel.row_index(0),
                len: panel.rows.len(),
                highlighted: panel.highlighted,
            })
            .collect();
        if self.frame.as_ref().is_some_and(|frame| {
            Rc::ptr_eq(&frame.scene, scene)
                && Rc::ptr_eq(&frame.layout, layout)
                && frame.focus == input.focused
                && frame.editor == editor
                && frame.delay == delay
                && frame.popup == popup
                && frame.menu_trigger == menu_trigger
                && frame.menus == menus
                && frame.scale == scale
                && self
                    .documents
                    .values()
                    .all(|document| document.scroll_matches(input))
        }) {
            return None;
        }

        let mut builder = Builder {
            scene,
            layout,
            input,
            scale,
            text_system,
            previous_ids: &self.ids,
            next_id: &mut self.next_id,
            ids: BTreeMap::new(),
            routes: BTreeMap::new(),
            documents: BTreeMap::new(),
            nodes: Vec::new(),
            radio_groups: Vec::new(),
            focus: ROOT,
        };
        let mut children = if let Some(path) = input.dialog_layers(layout).next_back() {
            scene
                .at_path(path)
                .zip(layout.at_path(path))
                .and_then(|(node, item)| {
                    builder.visit(
                        node,
                        item,
                        &mut path.to_vec(),
                        overlay_row(scene, path),
                        None,
                    )
                })
                .into_iter()
                .collect()
        } else if scene.kind == "Window" {
            builder.children(scene, layout, &mut Vec::new(), None, None)
        } else {
            builder
                .visit(scene, layout, &mut Vec::new(), None, None)
                .into_iter()
                .collect()
        };
        for path in input.overlay_layers(layout) {
            if !input.allows_popup(layout, path) {
                continue;
            }
            if let Some((node, item)) = scene.at_path(path).zip(layout.at_path(path))
                && !node.is_modal()
                && let Some(id) = builder.visit(
                    node,
                    item,
                    &mut path.to_vec(),
                    overlay_row(scene, path),
                    None,
                )
            {
                children.push(id);
            }
        }
        if let Some(trigger) = &menu_trigger
            && let Some(popup) = builder.menu_popup(&menu_panels, trigger)
        {
            children.push(popup);
        }
        children.extend(builder.radio_groups.iter().copied());
        let mut root = AccessibleNode::new(Role::Window);
        root.set_label(
            scene
                .text("accessible-name")
                .or_else(|| scene.text("title"))
                .unwrap_or("XRack"),
        );
        if let Some(description) = scene.text("accessible-description") {
            root.set_description(description);
        }
        root.set_bounds(bounds(layout.bounds, scale));
        root.set_children(children);
        builder.nodes.push((ROOT, root));

        let Builder {
            nodes,
            focus,
            ids,
            routes,
            documents,
            ..
        } = builder;
        let update = TreeUpdate {
            nodes,
            tree: Some(TreeInfo {
                toolkit_name: Some("XRack".into()),
                toolkit_version: Some(env!("CARGO_PKG_VERSION").into()),
                ..TreeInfo::new(ROOT)
            }),
            tree_id: TreeId::ROOT,
            focus,
        };
        self.ids = ids;
        self.routes = routes;
        self.documents = documents;
        self.frame = Some(Frame {
            scene: scene.clone(),
            layout: layout.clone(),
            focus: input.focused.clone(),
            editor,
            delay,
            popup,
            menu_trigger,
            menus,
            scale,
        });
        Some(update)
    }
}

struct Builder<'a> {
    scene: &'a Node,
    layout: &'a LayoutNode,
    input: &'a Interaction,
    scale: f64,
    text_system: Option<&'a TextSystem>,
    previous_ids: &'a BTreeMap<Key, NodeId>,
    next_id: &'a mut u64,
    ids: BTreeMap<Key, NodeId>,
    routes: BTreeMap<NodeId, Route>,
    documents: BTreeMap<NodeId, TextDocument>,
    nodes: Vec<(NodeId, AccessibleNode)>,
    radio_groups: Vec<NodeId>,
    focus: NodeId,
}

impl Builder<'_> {
    fn id(&mut self, key: Key) -> NodeId {
        let id = self.previous_ids.get(&key).copied().unwrap_or_else(|| {
            *self.next_id = self
                .next_id
                .checked_add(1)
                .expect("accessibility node IDs exhausted");
            NodeId(*self.next_id)
        });
        self.ids.insert(key, id);
        id
    }

    fn attached_button_group(
        &mut self,
        node: &Node,
        path: &[usize],
        row: Option<(RowKey, usize)>,
    ) -> Option<NodeId> {
        let member = node.button_group.as_ref()?;
        let component = member.component_path(path)?;
        let mut key = Key::element(
            node,
            component,
            row.filter(|(_, depth)| *depth <= component.len()),
        );
        key.kind = "AttachedButtonGroup";
        key.id = Some(member.group.key);
        if let Some(&id) = self.ids.get(&key) {
            return Some(id);
        }
        let id = self.id(key);
        let mut group = AccessibleNode::new(if member.group.exclusive {
            Role::RadioGroup
        } else {
            Role::Group
        });
        if !member.group.name.is_empty() {
            group.set_label(member.group.name.as_str());
        }
        if !member.group.enabled {
            group.set_disabled();
        }
        self.nodes.push((id, group));
        self.radio_groups.push(id);
        Some(id)
    }

    fn children(
        &mut self,
        node: &Node,
        item: &LayoutNode,
        path: &mut Vec<usize>,
        row: Option<(RowKey, usize)>,
        view: Option<&'static str>,
    ) -> Vec<NodeId> {
        xrack_layout::child_reading_order(node)
            .filter_map(|index| {
                let (child, layout) = node.children.get(index).zip(item.children.get(index))?;
                if is_window_overlay(child.kind) {
                    return None;
                }
                path.push(index);
                let mut id = self.visit(child, layout, path, row, view);
                if let Some(content) = id
                    && node.kind == "TableRow"
                    && child.boolean("table-delegate-cell", false)
                {
                    id = self
                        .table_delegate_cell(child, layout, path, row, content)
                        .or(id);
                }
                path.pop();
                id
            })
            .collect()
    }

    fn table_delegate_cell(
        &mut self,
        node: &Node,
        item: &LayoutNode,
        path: &[usize],
        row: Option<(RowKey, usize)>,
        content: NodeId,
    ) -> Option<NodeId> {
        let [table @ .., slot, 0, column] = path else {
            return None;
        };
        let list = self.scene.at_path(table)?.list?;
        let visual = self
            .layout
            .at_path(table)?
            .table
            .as_ref()?
            .visual_index(*column)?;
        let mut key = Key::element(node, path, row);
        key.kind = "TableDelegateCell";
        let id = self.id(key);
        let mut cell = AccessibleNode::new(Role::Cell);
        cell.set_bounds(bounds(item.visible_window_bounds(item.bounds), self.scale));
        cell.set_row_index(list.first_row + slot);
        cell.set_column_index(visual);
        cell.set_selected(node.boolean("table-selected", false));
        cell.set_children(vec![content]);
        self.nodes.push((id, cell));
        Some(id)
    }

    fn visit(
        &mut self,
        node: &Node,
        item: &LayoutNode,
        path: &mut Vec<usize>,
        row: Option<(RowKey, usize)>,
        view: Option<&'static str>,
    ) -> Option<NodeId> {
        let rect = item.visible_window_bounds(
            item.tab_header
                .or_else(|| item.visible.then_some(item.bounds))?,
        );
        let has_bounds = rect.width > 0.0 && rect.height > 0.0;
        if !has_bounds && !item.allows_overflow()
            || matches!(
                node.kind,
                "Tooltip" | "StandardListItem" | "ScrollIndicator"
            )
            || node.kind == "ScrollBar" && item.scroll_control_geometry(node).is_none()
            || matches!(node.kind, "SwipeLeft" | "SwipeRight") && !item.enabled
        {
            return None;
        }
        let row = node
            .row_key
            .filter(|_| !path.is_empty())
            .map(|key| (key, path.len()))
            .or(row);
        let role = role(node, view);
        let child_view = match node.kind {
            "ListView" | "GridView" | "StandardListView" | "TreeView" | "TableView" => {
                Some(node.kind)
            }
            _ => view,
        };
        let mut children = self.children(node, item, path, row, child_view);
        if node.kind == "StyledText" {
            children.extend(self.text_links(node, item, path, row));
        }
        let label = node
            .text("accessible-name")
            .or_else(|| match role {
                Role::Button | Role::CheckBox | Role::Switch | Role::RadioButton => {
                    Some(button_text(node))
                }
                Role::Tab if node.kind == "TabButton" => node.text("text"),
                Role::Group if node.kind == "Calendar" => Some("Calendar"),
                Role::Group if node.kind == "StyledText" => node.text("text"),
                Role::Group if matches!(node.kind, "GroupBox" | "Page") => node.text("title"),
                Role::ProgressIndicator if node.kind == "BusyIndicator" => Some("Loading"),
                Role::ProgressIndicator => Some("Progress"),
                Role::ScrollBar => Some("Scroll bar"),
                Role::ListBoxOption if view == Some("StandardListView") => {
                    node.children.first().and_then(|label| label.text("text"))
                }
                Role::ListItem if view == Some("GridView") => node
                    .children
                    .first()
                    .and_then(|delegate| delegate.text("accessible-name")),
                Role::Dialog | Role::MenuItem | Role::Tab => node.text("title"),
                Role::Cell => node.text("text"),
                _ => None,
            })
            .filter(|label| !label.is_empty());
        let description = node.text("accessible-description");
        if role == Role::Group
            && children.is_empty()
            && label.is_none()
            && description.is_none()
            && !node.group_checkable()
        {
            return None;
        }

        let id = self.id(Key::element(node, path, row));
        let mut accessible = AccessibleNode::new(role);
        if let Some(group) = self.attached_button_group(node, path, row) {
            accessible.set_member_of(group);
        }
        if has_bounds {
            accessible.set_bounds(bounds(rect, self.scale));
        }
        if let Some(label) = label {
            accessible.set_label(label);
        }
        if let Some(description) = description {
            accessible.set_description(description);
        }
        if !item.enabled {
            accessible.set_disabled();
        }
        if node.kind == "Calendar" {
            accessible.set_value(CalendarState::from_node(node).selected.to_string());
            children.extend(self.calendar_parts(node, item, path, row));
        }
        match role {
            Role::Label => accessible.set_value(node.text("text").unwrap_or("")),
            Role::CheckBox => accessible.set_toggled(match node.check_state() {
                CheckState::Unchecked => Toggled::False,
                CheckState::PartiallyChecked => Toggled::Mixed,
                CheckState::Checked => Toggled::True,
            }),
            Role::Switch | Role::RadioButton => {
                let checked = if role == Role::RadioButton && node.button_group.is_none() {
                    path.split_last().is_some_and(|(&index, parent)| {
                        self.scene.at_path(parent).and_then(group_index) == Some(index)
                    })
                } else {
                    node.boolean("checked", false)
                };
                accessible.set_toggled(if checked {
                    Toggled::True
                } else {
                    Toggled::False
                });
            }
            Role::Button if node.kind == "DelayButton" || node.boolean("checkable", false) => {
                let checked = node.boolean("checked", false);
                accessible.set_toggled(if checked {
                    Toggled::True
                } else {
                    Toggled::False
                });
                if node.kind == "DelayButton" {
                    let confirmation = self
                        .input
                        .delay_accessibility()
                        .filter(|(target, _)| target.path == *path)
                        .map(|(_, ready)| ready);
                    let hint = match (checked, confirmation) {
                        (true, _) => "Activate to reset.",
                        (false, Some(true)) => "Ready. Activate again to confirm.",
                        (false, Some(false)) => {
                            "Waiting for the confirmation delay. An early activation cancels."
                        }
                        (false, None) => "Activate to start the confirmation delay.",
                    };
                    accessible.set_description(
                        description
                            .map_or_else(|| hint.to_owned(), |text| format!("{text} {hint}")),
                    );
                    if confirmation.is_some() && !checked {
                        let mut key = Key::element(node, path, row);
                        key.kind = "DelayConfirmationStatus";
                        let id = self.id(key);
                        let mut status = AccessibleNode::new(Role::Status);
                        status.set_label(hint);
                        status.set_live(Live::Polite);
                        self.nodes.push((id, status));
                        children.push(id);
                    }
                }
            }
            Role::Dialog if node.is_modal() => accessible.set_modal(),
            Role::MenuItem => accessible.set_expanded(self.input.menu_header_open(path)),
            Role::Tab if node.kind == "TabButton" => {
                accessible.set_selected(path.split_last().is_some_and(|(&index, parent)| {
                    self.scene.at_path(parent).and_then(group_index) == Some(index)
                }));
            }
            Role::Tab => accessible.set_selected(item.visible),
            Role::TabList if node.kind == "TabBar" => accessible.set_clips_children(),
            Role::ListBox | Role::List if matches!(node.kind, "StandardListView" | "GridView") => {
                if let Some(list) = node.list {
                    accessible.set_size_of_set(list.row_count);
                }
            }
            Role::ListBoxOption | Role::ListItem
                if matches!(view, Some("StandardListView" | "GridView")) =>
            {
                accessible.set_selected(node.boolean("selected", false));
                if let Some((&slot, parent)) = path.split_last()
                    && let Some(list) = self.scene.at_path(parent).and_then(|node| node.list)
                {
                    accessible.set_position_in_set(list.first_row + slot);
                }
            }
            Role::ListItem | Role::TreeItem | Role::Row => {
                accessible.set_selected(node.boolean("selected", false));
                if role == Role::Row
                    && view == Some("TableView")
                    && let Some((&slot, parent)) = path.split_last()
                    && let Some(list) = self.scene.at_path(parent).and_then(|table| table.list)
                {
                    accessible.set_row_index(list.first_row + slot);
                }
                if node.boolean("tree-has-children", false) {
                    accessible.set_expanded(node.boolean("tree-expanded", false));
                }
            }
            Role::Table => {
                if let Some(list) = node.list {
                    accessible.set_row_count(list.row_count);
                }
                if let Some(header) = &node.table_header {
                    accessible.set_column_count(header.columns.len());
                    children.splice(..0, self.table_headers(node, item, path, row));
                }
            }
            Role::Cell if view == Some("TableView") => {
                if let [table @ .., slot, 0, column] = path.as_slice()
                    && let Some(list) = self.scene.at_path(table).and_then(|table| table.list)
                {
                    accessible.set_row_index(list.first_row + slot);
                    if let Some(visual) = self
                        .layout
                        .at_path(table)
                        .and_then(|item| item.table.as_ref())
                        .and_then(|geometry| geometry.visual_index(*column))
                    {
                        accessible.set_column_index(visual);
                    }
                }
                accessible.set_selected(node.boolean("table-selected", false));
            }
            Role::TextInput
            | Role::MultilineTextInput
            | Role::SearchInput
            | Role::PasswordInput => {
                if role != Role::PasswordInput {
                    accessible.set_value(text_input_value(node));
                }
                if let Some(placeholder) = node.text("placeholder-text") {
                    accessible.set_placeholder(placeholder);
                }
                if node.boolean("read-only", false) {
                    accessible.set_read_only();
                }
                if node.kind == "SearchField" {
                    accessible.set_has_popup(HasPopup::Listbox);
                    accessible.set_expanded(self.input.choice_is_open_for(path));
                }
            }
            Role::ComboBox | Role::EditableComboBox => {
                accessible.set_value(if combo_editable(node) {
                    text_input_value(node)
                } else {
                    combo_text(node)
                });
                accessible.set_expanded(self.input.choice_is_open_for(path));
            }
            Role::ProgressIndicator => {
                accessible.set_min_numeric_value(0.0);
                accessible.set_max_numeric_value(1.0);
                match indicator_state(node) {
                    Some(IndicatorState::Determinate(value)) => accessible.set_numeric_value(value),
                    Some(IndicatorState::Indeterminate) => accessible.set_busy(),
                    _ => {}
                }
            }
            Role::ListBox if node.kind == "Tumbler" => {
                if let Some(wheel) = &item.tumbler
                    && let Some(selected) = wheel
                        .rows
                        .iter()
                        .find(|row| Some(row.index) == wheel.selected)
                {
                    accessible.set_value(selected.label.as_str());
                }
            }
            Role::SpinButton => {
                let (min, max, value, step) = if node.kind == "DoubleSpinBox" {
                    let range = DecimalSpinRange::from_node(node);
                    let (min, max) = range.limits();
                    (min, max, range.value(), range.step())
                } else {
                    let min = node.int("minimum").unwrap_or(0);
                    let max = node.int("maximum").unwrap_or(100).max(min);
                    (
                        min as f64,
                        max as f64,
                        SpinRange::from_node(node).value() as f64,
                        node.int("step-size").unwrap_or(1).max(1) as f64,
                    )
                };
                accessible.set_min_numeric_value(min);
                accessible.set_max_numeric_value(max);
                accessible.set_numeric_value(value);
                accessible.set_numeric_value_step(step);
                if node.boolean("read-only", false) {
                    accessible.set_read_only();
                }
            }
            Role::ScrollBar => {
                if let Some(bar) = item.scroll_control_geometry(node) {
                    let (value, maximum, step, page) = if node.scroll_target.is_none() {
                        let range = ScrollBarRange::from_node(node);
                        (range.position, range.maximum(), range.step, range.size)
                    } else {
                        (
                            f64::from(bar.scroll.offset),
                            f64::from(bar.scroll.max_offset),
                            f64::from(bar.step),
                            f64::from(bar.page),
                        )
                    };
                    accessible.set_orientation(match bar.orientation {
                        Orientation::Horizontal => accesskit::Orientation::Horizontal,
                        Orientation::Vertical => accesskit::Orientation::Vertical,
                    });
                    accessible.set_min_numeric_value(0.0);
                    accessible.set_max_numeric_value(maximum);
                    accessible.set_numeric_value(value);
                    accessible.set_numeric_value_step(step);
                    accessible.set_numeric_value_jump(page);
                    if !node.boolean("interactive", true) || maximum == 0.0 {
                        accessible.set_read_only();
                    }
                }
            }
            Role::Slider => {
                let range_node = if node.kind == "RangeThumb" {
                    self.scene
                        .at_path(&path[..path.len().saturating_sub(1)])
                        .unwrap_or(node)
                } else {
                    node
                };
                let min = range_node.number("minimum").unwrap_or(0.0);
                let max = range_node.number("maximum").unwrap_or(100.0).max(min);
                let value = if node.kind == "RangeThumb" {
                    let index = path.last().copied().unwrap_or(0).min(1);
                    accessible.set_label(if index == 0 {
                        "Lower value"
                    } else {
                        "Upper value"
                    });
                    SliderInterval::from_node(range_node).values()[index]
                } else {
                    SliderRange::from_node(node).value()
                };
                accessible.set_min_numeric_value(min);
                accessible.set_max_numeric_value(max);
                accessible.set_numeric_value(value);
                accessible
                    .set_numeric_value_step(range_node.number("step").unwrap_or(1.0).max(0.0));
            }
            _ => {}
        }

        let text_editor = if supports_ranges(node) {
            let editor = focused_editor(self.input, path);
            let (document, runs) = project_text(
                node,
                item.bounds,
                path,
                self.input,
                (self.scale, item.world_transform()),
                self.text_system,
                |start, value| {
                    let mut key = Key::element(node, path, row);
                    key.text_run = Some((start, value.into()));
                    self.id(key)
                },
            );
            children.extend(runs.iter().map(|(id, _)| *id));
            self.nodes.extend(runs);
            if let Some(editor) = editor {
                accessible.set_text_selection(document.selection(editor, text_input_value(node)));
            }
            self.documents.insert(id, document);
            editor.is_none_or(|edit| edit.preedit.is_empty())
        } else {
            false
        };
        if node.group_checkable()
            && let Some(toggle) = self.group_toggle(node, item, path, row)
        {
            children.insert(0, toggle);
        }
        if node.kind != "GroupBox"
            && (!self.input.menu_open() || node.kind == "Menu")
            && let Some(target) = self
                .input
                .accessibility_target(self.scene, self.layout, path)
        {
            if target.focusable {
                accessible.add_action(Action::Focus);
                if self
                    .input
                    .supports_context_menu(self.scene, self.layout, &target)
                {
                    accessible.add_action(Action::ShowContextMenu);
                }
            }
            if supports_click(node.kind) {
                accessible.add_action(Action::Click);
            }
            if let Some(swipe) = item.swipe_delegate.as_deref()
                && (swipe.supports(SwipeSide::Left) || swipe.supports(SwipeSide::Right))
            {
                let expanded = swipe.side != SwipeSide::Closed;
                accessible.set_expanded(expanded);
                accessible.add_action(if expanded {
                    Action::Collapse
                } else {
                    Action::Expand
                });
                if let Some((rect, action)) = swipe
                    .toggle()
                    .map(|(rect, action)| (rect.intersect(item.clip), action))
                    .filter(|(rect, _)| rect.width > 0.0 && rect.height > 0.0)
                {
                    let label = if expanded {
                        "Hide actions"
                    } else {
                        "Show actions"
                    };
                    let mut key = Key::element(node, path, row);
                    key.kind = "SwipeToggle";
                    key.option = Some((0, label.to_owned()));
                    let id = self.id(key);
                    let mut toggle = AccessibleNode::new(Role::Button);
                    toggle.set_label(label);
                    toggle.set_bounds(bounds(item.visible_window_bounds(rect), self.scale));
                    toggle.add_action(Action::Click);
                    self.routes.insert(
                        id,
                        Route::ControlPart {
                            target: target.clone(),
                            action,
                        },
                    );
                    self.nodes.push((id, toggle));
                    children.push(id);
                }
            }
            if supports_text_value(node) || node.kind == "DoubleSpinBox" && spin_editable(node) {
                accessible.add_action(Action::SetValue);
            }
            if text_editor && target.focusable {
                accessible.add_action(Action::SetTextSelection);
            }
            if role == Role::Slider || role == Role::SpinButton && !node.boolean("read-only", false)
            {
                accessible.add_action(Action::Increment);
                accessible.add_action(Action::Decrement);
            }
            if role == Role::ScrollBar {
                accessible.add_action(Action::SetValue);
                accessible.add_action(Action::Increment);
                accessible.add_action(Action::Decrement);
            }
            scroll_actions(&mut accessible, item, self.scale);
            if self.input.focused.as_ref() == Some(&target) {
                self.focus = id;
            }
            if node.kind == "SearchField" {
                children.extend(self.search_parts(node, item, &target, row));
            }
            if let Some(wheel) = &item.tumbler {
                let selection = xrack_runtime::Tumbler::from_node(node);
                if let Some(index) = wheel.selected {
                    if selection.step(-1) != Some(index) {
                        accessible.add_action(Action::Decrement);
                    }
                    if selection.step(1) != Some(index) {
                        accessible.add_action(Action::Increment);
                    }
                }
                children.extend(self.tumbler_parts(node, item, &target, row));
            }
            if matches!(node.kind, "ComboBox" | "SearchField")
                && self.input.choice_is_open_for(path)
                && let Some((popup, highlight)) = self.choice_popup(node, &target, row)
            {
                children.push(popup);
                accessible.set_controls(vec![popup]);
                if let Some(highlight) = highlight {
                    if node.kind == "SearchField" {
                        accessible.set_active_descendant(highlight);
                    } else {
                        self.focus = highlight;
                    }
                }
            }
            self.routes.insert(
                id,
                Route::Element {
                    target,
                    kind: node.kind,
                    standard_kind: node.standard_button_kind(),
                },
            );
        }
        accessible.set_children(children);
        self.nodes.push((id, accessible));
        Some(id)
    }

    fn text_links(
        &mut self,
        node: &Node,
        item: &LayoutNode,
        path: &[usize],
        row: Option<(RowKey, usize)>,
    ) -> Vec<NodeId> {
        let Some(document) = node.styled_text("text") else {
            return Vec::new();
        };
        let mut regions = BTreeMap::<usize, Rect>::new();
        for (index, rect) in item.text_link_regions() {
            regions
                .entry(index)
                .and_modify(|area| {
                    let x = area.x.min(rect.x);
                    let y = area.y.min(rect.y);
                    let right = (area.x + area.width).max(rect.x + rect.width);
                    let bottom = (area.y + area.height).max(rect.y + rect.height);
                    *area = Rect::new(x, y, right - x, bottom - y);
                })
                .or_insert(rect);
        }
        regions
            .into_iter()
            .filter_map(|(index, rect)| {
                let link = document.links().get(index)?;
                let label = &document.text()[link.start..link.end];
                let mut key = Key::element(node, path, row);
                key.kind = "StyledTextLink";
                key.option = Some((index, link.href.to_string()));
                key.text_run = Some((link.start, label.into()));
                let id = self.id(key);
                let mut accessible = AccessibleNode::new(Role::Link);
                accessible.set_bounds(bounds(item.visible_window_bounds(rect), self.scale));
                accessible.set_label(label);
                accessible.set_url(link.href.as_ref());
                if let Some(target) =
                    self.input
                        .text_link_target(self.scene, self.layout, path, index)
                {
                    accessible.add_action(Action::Focus);
                    accessible.add_action(Action::Click);
                    if self.input.focused.as_ref() == Some(&target) {
                        self.focus = id;
                    }
                    self.routes.insert(id, Route::TextLink(target));
                } else {
                    accessible.set_disabled();
                }
                self.nodes.push((id, accessible));
                Some(id)
            })
            .collect()
    }

    fn table_headers(
        &mut self,
        node: &Node,
        item: &LayoutNode,
        path: &[usize],
        row: Option<(RowKey, usize)>,
    ) -> Vec<NodeId> {
        let (Some(header), Some(geometry)) = (&node.table_header, &item.table) else {
            return Vec::new();
        };
        let target = self
            .input
            .accessibility_target(self.scene, self.layout, path);
        let clip = item.clip_rect(geometry.header);
        geometry
            .visual_columns()
            .enumerate()
            .filter_map(|(visual, index)| {
                let column = &header.columns[index];
                let rect = geometry.columns[index].intersect(geometry.column_clip(index, clip));
                if rect.width <= 0.0 || rect.height <= 0.0 {
                    return None;
                }
                let mut key = Key::element(node, path, row);
                key.kind = "TableHeader";
                key.option = Some((index, String::new()));
                let id = self.id(key);
                let mut part = AccessibleNode::new(Role::ColumnHeader);
                part.set_bounds(bounds(item.visible_window_bounds(rect), self.scale));
                part.set_column_index(visual);
                if let Some(title) = column.text("title") {
                    part.set_label(title);
                }
                if let Some(sort) = header.sort.filter(|sort| sort.column == index) {
                    part.set_sort_direction(if sort.descending {
                        SortDirection::Descending
                    } else {
                        SortDirection::Ascending
                    });
                }
                if !item.enabled {
                    part.set_disabled();
                }
                if let Some(target) = &target
                    && header.next_sort(index).is_some()
                {
                    part.add_action(Action::Click);
                    self.routes.insert(
                        id,
                        Route::ControlPart {
                            target: target.clone(),
                            action: ControlAction::SortColumn(index),
                        },
                    );
                }
                self.nodes.push((id, part));
                Some(id)
            })
            .collect()
    }

    fn group_toggle(
        &mut self,
        node: &Node,
        item: &LayoutNode,
        path: &[usize],
        row: Option<(RowKey, usize)>,
    ) -> Option<NodeId> {
        let rect = item.group_header(node).intersect(item.clip);
        if rect.width <= 0.0 || rect.height <= 0.0 {
            return None;
        }
        let mut key = Key::element(node, path, row);
        key.kind = "GroupBoxToggle";
        let id = self.id(key);
        let mut toggle = AccessibleNode::new(Role::CheckBox);
        toggle.set_bounds(bounds(item.visible_window_bounds(rect), self.scale));
        toggle.set_label(
            node.text("accessible-name")
                .or_else(|| node.text("title"))
                .filter(|label| !label.is_empty())
                .unwrap_or("Enable group"),
        );
        toggle.set_toggled(if node.boolean("checked", true) {
            Toggled::True
        } else {
            Toggled::False
        });
        if !item.enabled {
            toggle.set_disabled();
        } else if !self.input.menu_open()
            && let Some(target) = self
                .input
                .accessibility_target(self.scene, self.layout, path)
        {
            toggle.add_action(Action::Click);
            toggle.add_action(Action::Focus);
            if self.input.focused.as_ref() == Some(&target) {
                self.focus = id;
            }
            self.routes.insert(
                id,
                Route::Element {
                    target,
                    kind: node.kind,
                    standard_kind: None,
                },
            );
        }
        self.nodes.push((id, toggle));
        Some(id)
    }

    fn calendar_parts(
        &mut self,
        node: &Node,
        item: &LayoutNode,
        path: &[usize],
        row: Option<(RowKey, usize)>,
    ) -> Vec<NodeId> {
        let geometry = CalendarGeometry::new(node, item.bounds);
        let target = (!self.input.menu_open())
            .then(|| {
                self.input
                    .accessibility_target(self.scene, self.layout, path)
            })
            .flatten();
        let navigation = geometry
            .navigation()
            .into_iter()
            .zip(["Previous month", "Next month"])
            .map(|((rect, action), label)| (rect, action, label.to_owned(), None));
        let days = geometry.days().map(|day| {
            (
                day.bounds,
                day.enabled.then_some(CalendarAction::Select(day.date)),
                day.date.to_string(),
                Some(day.selected),
            )
        });
        navigation
            .chain(days)
            .enumerate()
            .filter_map(|(index, (rect, action, label, selected))| {
                let rect = rect.intersect(item.clip);
                if rect.width <= 0.0 || rect.height <= 0.0 {
                    return None;
                }
                let mut key = Key::element(node, path, row);
                let identity = match action {
                    Some(CalendarAction::Browse(month)) => month.to_string(),
                    _ => label.clone(),
                };
                key.option = Some((index, identity));
                let id = self.id(key);
                let mut part = AccessibleNode::new(if selected.is_some() {
                    Role::RadioButton
                } else {
                    Role::Button
                });
                part.set_label(label);
                part.set_bounds(bounds(item.visible_window_bounds(rect), self.scale));
                if let Some(selected) = selected {
                    part.set_toggled(if selected {
                        Toggled::True
                    } else {
                        Toggled::False
                    });
                }
                if !item.enabled || action.is_none() {
                    part.set_disabled();
                } else if let (Some(target), Some(action)) = (&target, action) {
                    part.add_action(Action::Click);
                    self.routes.insert(
                        id,
                        Route::ControlPart {
                            target: target.clone(),
                            action: ControlAction::Calendar(action),
                        },
                    );
                }
                self.nodes.push((id, part));
                Some(id)
            })
            .collect()
    }

    fn tumbler_parts(
        &mut self,
        node: &Node,
        item: &LayoutNode,
        target: &Target,
        row: Option<(RowKey, usize)>,
    ) -> Vec<NodeId> {
        let Some(wheel) = &item.tumbler else {
            return Vec::new();
        };
        wheel
            .rows
            .iter()
            .filter_map(|entry| {
                let rect = item.clip_rect(entry.bounds.intersect(item.bounds));
                if rect.width <= 0.0 || rect.height <= 0.0 {
                    return None;
                }
                let mut key = Key::element(node, &target.path, row);
                key.kind = "TumblerOption";
                key.row_option = Some(entry.key);
                let id = self.id(key);
                let mut option = AccessibleNode::new(Role::ListBoxOption);
                option.set_label(entry.label.as_str());
                option.set_bounds(bounds(item.visible_window_bounds(rect), self.scale));
                option.set_selected(wheel.selected == Some(entry.index));
                option.add_action(Action::Click);
                self.nodes.push((id, option));
                self.routes.insert(
                    id,
                    Route::ControlPart {
                        target: target.clone(),
                        action: entry.action(),
                    },
                );
                Some(id)
            })
            .collect()
    }

    fn search_parts(
        &mut self,
        node: &Node,
        item: &LayoutNode,
        target: &Target,
        row: Option<(RowKey, usize)>,
    ) -> Vec<NodeId> {
        SearchGeometry::new(item.bounds)
            .actions(node)
            .filter_map(|(rect, action)| {
                let rect = rect.intersect(item.clip);
                if rect.width <= 0.0 || rect.height <= 0.0 {
                    return None;
                }
                let mut key = Key::element(node, &target.path, row);
                key.kind = if action == ControlAction::ClearSearch {
                    "ClearSearchButton"
                } else {
                    "SubmitSearchButton"
                };
                let id = self.id(key);
                let mut part = AccessibleNode::new(Role::Button);
                part.set_bounds(bounds(item.visible_window_bounds(rect), self.scale));
                part.set_label(if action == ControlAction::ClearSearch {
                    "Clear search"
                } else {
                    "Search"
                });
                part.add_action(Action::Click);
                self.nodes.push((id, part));
                self.routes.insert(
                    id,
                    Route::ControlPart {
                        target: target.clone(),
                        action,
                    },
                );
                Some(id)
            })
            .collect()
    }

    fn choice_popup(
        &mut self,
        node: &Node,
        target: &Target,
        row: Option<(RowKey, usize)>,
    ) -> Option<(NodeId, Option<NodeId>)> {
        let menu = self.input.choice_menu(self.scene, self.layout)?;
        let mut options = Vec::new();
        let mut highlight = None;
        for (offset, value) in menu.rows.iter().enumerate() {
            let index = menu.first + offset;
            let mut key = Key::element(node, &target.path, row);
            key.kind = "ChoiceOption";
            let choice = choice_identity(node, index);
            let identity = if let Some((row, key)) =
                choice.as_ref().and_then(|choice| choice.record.as_ref())
            {
                (0, format!("{row:?}\0{key:?}\0{value}"))
            } else if node.kind == "SearchField" {
                (index, format!("{}\0{value}", text_input_value(node)))
            } else {
                (index, value.clone())
            };
            key.option = Some(identity);
            let id = self.id(key);
            let mut option = AccessibleNode::new(Role::ListBoxOption);
            option.set_label(value.as_str());
            option.set_bounds(bounds(menu.row_bounds(offset), self.scale));
            option.set_selected(menu.selected == Some(index));
            if let Some(choice) = choice.filter(|_| menu.row_enabled(offset)) {
                option.add_action(Action::Click);
                self.routes.insert(
                    id,
                    Route::Choice {
                        target: target.clone(),
                        index,
                        value: choice.label,
                        record: choice.record,
                    },
                );
            } else {
                option.set_disabled();
            }
            self.nodes.push((id, option));
            if menu.highlighted == Some(index) {
                highlight = Some(id);
            }
            options.push(id);
        }
        let mut key = Key::element(node, &target.path, row);
        key.kind = "ChoicePopup";
        let id = self.id(key);
        let mut popup = AccessibleNode::new(Role::ListBox);
        popup.set_bounds(bounds(menu.bounds, self.scale));
        popup.set_children(options);
        self.nodes.push((id, popup));
        Some((id, highlight))
    }

    fn menu_popup(&mut self, panels: &[MenuPanel<'_>], trigger: &Target) -> Option<NodeId> {
        let row = (1..=trigger.path.len()).rev().find_map(|depth| {
            self.scene
                .at_path(&trigger.path[..depth])?
                .row_key
                .map(|key| (key, depth))
        });
        let mut nested: Option<(&[usize], NodeId)> = None;
        let mut focus = None;
        for panel in panels.iter().rev() {
            let mut rows = Vec::new();
            let mut highlight = None;
            for (slot, row_data) in panel.rows.iter().enumerate() {
                if row_data.separator {
                    continue;
                }
                let entry = panel.entry(slot);
                let node = row_data.node;
                let mut key = Key::element(node, &entry.path, row);
                key.kind = "PopupMenuRow";
                key.menu_session = Some(panel.session());
                key.option = Some((panel.row_index(slot), entry.label.clone()));
                let id = self.id(key);
                let role = if row_data.checked.is_some() {
                    Role::MenuItemCheckBox
                } else {
                    Role::MenuItem
                };
                let mut accessible = AccessibleNode::new(role);
                accessible.set_bounds(bounds(panel.row_bounds(slot), self.scale));
                accessible.set_label(node.text("accessible-name").unwrap_or(&entry.label));
                if let Some(shortcut) = &row_data.shortcut {
                    accessible.set_keyboard_shortcut(shortcut.as_str());
                }
                if let Some(checked) = row_data.checked {
                    accessible.set_toggled(if checked {
                        Toggled::True
                    } else {
                        Toggled::False
                    });
                }
                if row_data.enabled {
                    accessible.add_action(Action::Click);
                    accessible.add_action(Action::Focus);
                    if row_data.submenu {
                        accessible.add_action(Action::Expand);
                    }
                    self.routes.insert(
                        id,
                        Route::MenuEntry {
                            trigger: trigger.clone(),
                            entry: entry.clone(),
                        },
                    );
                } else {
                    accessible.set_disabled();
                }
                if row_data.submenu {
                    accessible.set_has_popup(HasPopup::Menu);
                    let child = nested
                        .filter(|(path, _)| *path == entry.path)
                        .map(|(_, id)| id);
                    accessible.set_expanded(child.is_some());
                    if let Some(child) = child {
                        accessible.set_children(vec![child]);
                    }
                }
                if panel.row_highlighted(slot) && row_data.enabled {
                    highlight = Some(id);
                }
                rows.push(id);
                self.nodes.push((id, accessible));
            }
            let menu = panel.menu;
            let mut key = Key::element(menu, panel.path(), row);
            key.kind = "PopupMenu";
            key.menu_session = Some(panel.session());
            let id = self.id(key);
            let mut popup = AccessibleNode::new(Role::Menu);
            popup.set_label(menu.text("title").unwrap_or("Context menu"));
            popup.set_bounds(bounds(panel.bounds, self.scale));
            popup.set_children(rows);
            popup.add_action(Action::Collapse);
            self.routes.insert(
                id,
                Route::MenuPanel {
                    trigger: trigger.clone(),
                    path: panel.path().to_vec(),
                    session: panel.session(),
                },
            );
            self.nodes.push((id, popup));
            focus.get_or_insert(highlight.unwrap_or(id));
            nested = Some((panel.path(), id));
        }
        self.focus = focus?;
        nested.map(|(_, id)| id)
    }
}

fn bounds(rect: Rect, scale: f64) -> accesskit::Rect {
    accesskit::Rect::new(
        f64::from(rect.x) * scale,
        f64::from(rect.y) * scale,
        f64::from(rect.x + rect.width) * scale,
        f64::from(rect.y + rect.height) * scale,
    )
}

fn role(node: &Node, view: Option<&str>) -> Role {
    match node.kind {
        "Button" | "ToolButton" | "DelayButton" | "DialogButton" | "TouchArea"
        | "SwipeDelegate" => Role::Button,
        "CheckBox" => Role::CheckBox,
        "Switch" => Role::Switch,
        "RadioButton" => Role::RadioButton,
        "RadioGroup" => Role::RadioGroup,
        "StyledText"
            if node
                .styled_text("text")
                .is_some_and(|text| !text.links().is_empty()) =>
        {
            Role::Group
        }
        "Text" | "Label" | "StyledText" => Role::Label,
        "Image" | "AnimatedImage"
            if ["accessible-name", "accessible-description"]
                .iter()
                .any(|name| node.text(name).is_some_and(|value| !value.is_empty())) =>
        {
            Role::Image
        }
        "TextInput" | "LineEdit" if node.is_password() => Role::PasswordInput,
        "TextInput" | "LineEdit" if node.input_type() == InputType::Search => Role::SearchInput,
        "TextInput" | "LineEdit" => Role::TextInput,
        "SearchField" => Role::SearchInput,
        "TextEdit" | "TextArea" => Role::MultilineTextInput,
        "ComboBox" if combo_editable(node) => Role::EditableComboBox,
        "ComboBox" => Role::ComboBox,
        "Slider" | "Dial" | "RangeThumb" => Role::Slider,
        "SpinBox" | "DoubleSpinBox" => Role::SpinButton,
        "ProgressIndicator" | "Spinner" | "BusyIndicator" => Role::ProgressIndicator,
        "MenuBar" => Role::MenuBar,
        "Menu" => Role::MenuItem,
        "ToolBar" => Role::Toolbar,
        "Dialog" | "Drawer" | "PopupWindow" => Role::Dialog,
        "TabWidget" | "TabBar" => Role::TabList,
        "Tab" | "TabButton" => Role::Tab,
        "Tumbler" | "StandardListView" => Role::ListBox,
        "ListView" | "GridView" => Role::List,
        "ScrollView" | "Flickable" => Role::ScrollView,
        "ScrollBar" => Role::ScrollBar,
        "TableView" => Role::Table,
        "TreeView" => Role::Tree,
        "ListRow" if view == Some("TableView") => Role::Row,
        "ListRow" if view == Some("TreeView") => Role::TreeItem,
        "ListRow" if view == Some("StandardListView") => Role::ListBoxOption,
        "ListRow" => Role::ListItem,
        "TableCell" | "TreeCell" => Role::Cell,
        _ => Role::Group,
    }
}

fn scroll_actions(node: &mut AccessibleNode, item: &LayoutNode, scale: f64) {
    let scroll_on = |axis| {
        item.attached_scroll_geometry()
            .filter(|bar| bar.orientation == axis)
            .map(|bar| bar.scroll)
            .or_else(|| item.scroll_on(axis))
    };
    if let Some(scroll) =
        scroll_on(Orientation::Horizontal).filter(|scroll| scroll.max_offset > 0.0)
    {
        node.set_scroll_x_min(0.0);
        node.set_scroll_x_max(f64::from(scroll.max_offset) * scale);
        node.set_scroll_x(f64::from(scroll.offset) * scale);
        if scroll.offset > 0.0 {
            node.add_action(Action::ScrollLeft);
        }
        if scroll.offset < scroll.max_offset {
            node.add_action(Action::ScrollRight);
        }
        node.add_action(Action::SetScrollOffset);
    }
    if let Some(scroll) = scroll_on(Orientation::Vertical).filter(|scroll| scroll.max_offset > 0.0)
    {
        node.set_scroll_y_min(0.0);
        node.set_scroll_y_max(f64::from(scroll.max_offset) * scale);
        node.set_scroll_y(f64::from(scroll.offset) * scale);
        if scroll.offset > 0.0 {
            node.add_action(Action::ScrollUp);
        }
        if scroll.offset < scroll.max_offset {
            node.add_action(Action::ScrollDown);
        }
        node.add_action(Action::SetScrollOffset);
    }
}

pub(crate) fn requested_scroll(
    action: Action,
    data: Option<&ActionData>,
    node: &Node,
    item: &LayoutNode,
    scale: f64,
) -> [Option<f32>; 2] {
    if let Some(bar) = item.attached_scroll_geometry() {
        let index = usize::from(bar.orientation == Orientation::Vertical);
        let current = f64::from(bar.scroll.offset);
        let line = f64::from(bar.step);
        let step = if matches!(data, Some(ActionData::ScrollUnit(ScrollUnit::Item))) {
            line
        } else {
            f64::from(bar.page)
        };
        let value = match (action, data, bar.orientation) {
            (Action::SetValue, Some(ActionData::NumericValue(value)), _) => *value,
            (Action::SetScrollOffset, Some(ActionData::SetScrollOffset(point)), _) => {
                [point.x, point.y][index] / scale
            }
            (Action::Increment, _, _) => current + line,
            (Action::Decrement, _, _) => current - line,
            (Action::ScrollLeft, _, Orientation::Horizontal)
            | (Action::ScrollUp, _, Orientation::Vertical) => current - step,
            (Action::ScrollRight, _, Orientation::Horizontal)
            | (Action::ScrollDown, _, Orientation::Vertical) => current + step,
            _ => return [None, None],
        };
        let mut offsets = [None, None];
        offsets[index] = value
            .is_finite()
            .then(|| value.clamp(0.0, f64::from(bar.scroll.max_offset)) as f32);
        return offsets;
    }
    if action == Action::SetScrollOffset {
        return match data {
            Some(ActionData::SetScrollOffset(point)) => [point.x, point.y].map(|value| {
                value
                    .is_finite()
                    .then(|| (value / scale).clamp(0.0, f64::from(f32::MAX)) as f32)
            }),
            _ => [None, None],
        };
    }
    let (index, direction, axis) = match action {
        Action::ScrollUp => (1, -1.0, Orientation::Vertical),
        Action::ScrollDown => (1, 1.0, Orientation::Vertical),
        Action::ScrollLeft => (0, -1.0, Orientation::Horizontal),
        Action::ScrollRight => (0, 1.0, Orientation::Horizontal),
        _ => return [None, None],
    };
    let Some(scroll) = item.scroll_on(axis) else {
        return [None, None];
    };
    let viewport = item
        .table
        .as_ref()
        .map_or(item.bounds, |table| match axis {
            Orientation::Horizontal => table.scrollable_body(),
            Orientation::Vertical => table.body,
        })
        .intersect(item.clip);
    let step = match (data, axis) {
        (Some(ActionData::ScrollUnit(ScrollUnit::Item)), Orientation::Vertical)
            if node.list.is_some() =>
        {
            ListMetrics::row_height_at(node, node.list.map_or(0, |list| list.first_row))
        }
        (Some(ActionData::ScrollUnit(ScrollUnit::Item)), _) => 28.0,
        (_, Orientation::Horizontal) => viewport.width,
        (_, Orientation::Vertical) => viewport.height,
    };
    let mut offsets = [None, None];
    offsets[index] = Some((scroll.offset + direction * step).clamp(0.0, scroll.max_offset));
    offsets
}

pub(crate) fn requested_standalone_scroll(
    action: Action,
    data: Option<&ActionData>,
    node: &Node,
) -> Option<RangeAction> {
    let range = ScrollBarRange::from_node(node);
    match (action, data) {
        (Action::SetValue, Some(ActionData::NumericValue(value)))
            if value.is_finite() && range.maximum() > 0.0 =>
        {
            Some(RangeAction::SetFraction(
                value.clamp(0.0, range.maximum()) / range.maximum(),
            ))
        }
        (Action::Increment, _) => Some(RangeAction::Increase),
        (Action::Decrement, _) => Some(RangeAction::Decrease),
        _ => None,
    }
}

pub(crate) fn supports_click(kind: &str) -> bool {
    matches!(
        kind,
        "Button"
            | "ToolButton"
            | "DelayButton"
            | "GroupBox"
            | "DialogButton"
            | "TouchArea"
            | "SwipeDelegate"
            | "CheckBox"
            | "Switch"
            | "RadioButton"
            | "ComboBox"
            | "Menu"
            | "Tab"
            | "TabButton"
            | "ListRow"
    )
}

pub(crate) fn supports_text_value(node: &Node) -> bool {
    (xrack_runtime::is_text_input(node.kind) || combo_editable(node))
        && !node.boolean("read-only", false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use xrack_layout::{ChoiceKey, Size, TextMeasurer, layout};
    use xrack_runtime::{Length, ListMetrics, Value, VecModel};

    struct Text;
    impl TextMeasurer for Text {
        fn measure(&mut self, text: &str, _font_size: f32, width: f32) -> Size {
            Size::new((text.chars().count() as f32 * 8.0).min(width), 16.0)
        }
    }

    fn length(node: &mut Node, name: &'static str, value: f64) {
        node.properties.insert(name, Value::Length(Length(value)));
    }

    fn button(id: &'static str, label: &str, x: f64) -> Node {
        let mut node = Node::new("Button", Some(id));
        length(&mut node, "x", x);
        length(&mut node, "width", 100.0);
        length(&mut node, "height", 32.0);
        node.properties.insert("text", Value::String(label.into()));
        node
    }

    fn named<'a>(update: &'a TreeUpdate, label: &str) -> (NodeId, &'a AccessibleNode) {
        update
            .nodes
            .iter()
            .find_map(|(id, node)| (node.label() == Some(label)).then_some((*id, node)))
            .unwrap()
    }

    #[test]
    fn standalone_scroll_bar_uses_normalized_accessibility_values_at_any_scale() {
        let mut window = Node::new("Window", None);
        let mut bar = Node::new("ScrollBar", Some("position"));
        length(&mut bar, "width", 12.0);
        length(&mut bar, "height", 120.0);
        bar.properties
            .insert("accessible-name", "Position".to_owned().into());
        bar.properties.insert("position", 0.25_f64.into());
        bar.properties.insert("size", 0.25_f64.into());
        bar.properties.insert("step-size", 0.125_f64.into());
        window.children.push(bar);
        let scene = Rc::new(window);
        let frame = Rc::new(layout(&scene, Size::new(200.0, 150.0), &mut Text));
        let input = Interaction::default();
        let update = Projection::default()
            .update(&scene, &frame, &input, 2.0, None)
            .unwrap();
        let (_, bar) = named(&update, "Position");
        assert_eq!(bar.role(), Role::ScrollBar);
        assert_eq!(bar.numeric_value(), Some(0.25));
        assert_eq!(bar.max_numeric_value(), Some(0.75));
        assert_eq!(bar.numeric_value_step(), Some(0.125));
        assert!(bar.supports_action(Action::SetValue));
        assert!(!bar.supports_action(Action::SetScrollOffset));
        let node = &scene.children[0];
        let range = ScrollBarRange::from_node(node);
        let set = requested_standalone_scroll(
            Action::SetValue,
            Some(&ActionData::NumericValue(0.5)),
            node,
        )
        .unwrap();
        assert_eq!(range.apply(set), Some(0.5));
        let increment = requested_standalone_scroll(Action::Increment, None, node).unwrap();
        assert_eq!(range.apply(increment), Some(0.375));
    }

    #[test]
    fn external_scroll_bar_projects_its_range_and_routes_to_the_viewport() {
        let mut window = Node::new("Window", None);
        let handle = xrack_runtime::ViewportHandle::default();
        let mut view = Node::new("ScrollView", Some("view"));
        view.viewport_handle = Some(handle.clone());
        length(&mut view, "width", 100.0);
        length(&mut view, "height", 100.0);
        length(&mut view, "content-height", 300.0);
        window.children.push(view);
        for (kind, label, x) in [
            ("ScrollBar", "Bar", 120.0),
            ("ScrollIndicator", "Indicator", 140.0),
        ] {
            let mut control = Node::new(kind, Some(label));
            control.scroll_target = Some(handle.clone());
            if kind == "ScrollBar" {
                control.properties.insert("step-size", 0.25_f64.into());
            }
            control
                .properties
                .insert("accessible-name", label.to_owned().into());
            length(&mut control, "x", x);
            length(&mut control, "width", 12.0);
            length(&mut control, "height", 100.0);
            window.children.push(control);
        }
        let scene = Rc::new(window);
        let frame = Rc::new(layout(&scene, Size::new(200.0, 150.0), &mut Text));
        let mut input = Interaction::default();
        let update = Projection::default()
            .update(&scene, &frame, &input, 2.0, None)
            .unwrap();
        let (_, bar) = named(&update, "Bar");
        assert_eq!(bar.role(), Role::ScrollBar);
        assert_eq!(bar.orientation(), Some(accesskit::Orientation::Vertical));
        assert_eq!(bar.max_numeric_value(), Some(200.0));
        assert_eq!(bar.numeric_value_step(), Some(75.0));
        assert_eq!(bar.scroll_y_max(), Some(400.0));
        assert!(bar.supports_action(Action::Focus));
        assert!(bar.supports_action(Action::SetValue));
        assert!(
            !update
                .nodes
                .iter()
                .any(|(_, node)| node.label() == Some("Indicator"))
        );
        let offsets = requested_scroll(
            Action::SetValue,
            Some(&ActionData::NumericValue(140.0)),
            &scene.children[1],
            &frame.children[1],
            2.0,
        );
        assert_eq!(offsets, [None, Some(140.0)]);
        assert_eq!(
            requested_scroll(
                Action::Increment,
                None,
                &scene.children[1],
                &frame.children[1],
                2.0
            ),
            [None, Some(75.0)]
        );
        assert!(input.accessibility_scroll_to(
            &frame,
            &[1],
            Orientation::Vertical,
            offsets[1].unwrap()
        ));
        assert_eq!(input.scroll_offsets().get(&[0][..]), Some(&140.0));
    }

    #[test]
    fn native_accessibility_projects_transformed_bounds_and_skips_singular_items() {
        let mut window = Node::new("Window", None);
        let mut tile = Node::new("Rectangle", Some("tile"));
        for (name, value) in [
            ("x", 70.0),
            ("y", 30.0),
            ("width", 100.0),
            ("height", 60.0),
            ("transform-origin-x", 0.0),
            ("transform-origin-y", 0.0),
        ] {
            length(&mut tile, name, value);
        }
        tile.properties
            .insert("transform-rotation", 90.0_f64.into());
        let mut action = button("action", "Rotated action", 20.0);
        length(&mut action, "y", 10.0);
        length(&mut action, "width", 30.0);
        length(&mut action, "height", 20.0);
        tile.children.push(action);
        window.children.push(tile);

        let project = |node: &Node| {
            let scene = Rc::new(node.clone());
            let frame = Rc::new(layout(&scene, Size::new(200.0, 150.0), &mut Text));
            let mut input = Interaction::default();
            input.focus_next(&scene, &frame, false);
            Projection::default()
                .update(&scene, &frame, &input, 2.0, None)
                .unwrap()
        };
        let visible = project(&window);
        let (id, action) = named(&visible, "Rotated action");
        assert_eq!(visible.focus, id);
        assert_eq!(
            action.bounds(),
            Some(accesskit::Rect::new(80.0, 100.0, 120.0, 160.0))
        );
        window.children[0]
            .properties
            .insert("transform-scale", 0.0_f64.into());
        assert!(
            !project(&window)
                .nodes
                .iter()
                .any(|(_, node)| node.label() == Some("Rotated action"))
        );
    }

    #[test]
    fn rounded_clips_hide_occluded_buttons_and_bound_visible_overflow() {
        let mut window = Node::new("Window", None);
        let mut mask = Node::new("Rectangle", Some("mask"));
        length(&mut mask, "width", 100.0);
        length(&mut mask, "height", 100.0);
        length(&mut mask, "border-radius", 40.0);
        mask.properties.insert("clip", true.into());
        let mut corner = button("corner", "Corner", 2.0);
        length(&mut corner, "y", 2.0);
        length(&mut corner, "width", 6.0);
        length(&mut corner, "height", 6.0);
        let mut spill = button("spill", "Spill", 70.0);
        length(&mut spill, "y", 42.0);
        length(&mut spill, "width", 70.0);
        mask.children.extend([corner, spill]);
        window.children.push(mask);
        let project = |node: &Node| {
            let scene = Rc::new(node.clone());
            let frame = Rc::new(layout(&scene, Size::new(160.0, 120.0), &mut Text));
            let mut input = Interaction::default();
            input.focus_next(&scene, &frame, false);
            Projection::default()
                .update(&scene, &frame, &input, 1.0, None)
                .unwrap()
        };
        let clipped = project(&window);
        assert!(
            !clipped
                .nodes
                .iter()
                .any(|(_, node)| node.label() == Some("Corner"))
        );
        let (spill_id, spill) = named(&clipped, "Spill");
        assert_eq!(clipped.focus, spill_id);
        assert_eq!(spill.bounds().unwrap().x1, 100.0);

        window.children[0].properties.insert("clip", false.into());
        let open = project(&window);
        assert_eq!(open.focus, named(&open, "Corner").0);
        assert_eq!(named(&open, "Spill").1.bounds().unwrap().x1, 140.0);

        // An overflow-only group has no own visible bounds.
        length(&mut window.children[0], "x", -120.0);
        let overflow = project(&window);
        assert_eq!(named(&overflow, "Spill").1.bounds().unwrap().x1, 20.0);
    }

    #[test]
    fn z_and_opacity_preserve_accessibility_order_and_actions() {
        let mut window = Node::new("Window", None);
        window.children.extend([
            button("first", "First", 0.0),
            button("second", "Second", 0.0),
        ]);
        let mut projection = Projection::default();
        let mut input = Interaction::default();
        let snapshot = |node: &Node| {
            let scene = Rc::new(node.clone());
            let frame = Rc::new(layout(&scene, Size::new(160.0, 90.0), &mut Text));
            (scene, frame)
        };
        let (scene, frame) = snapshot(&window);
        input.focus_next(&scene, &frame, false);
        let first = projection
            .update(&scene, &frame, &input, 1.0, None)
            .unwrap();
        let (first_id, _) = named(&first, "First");
        let (second_id, _) = named(&first, "Second");

        window.children[0].properties.insert("z", 2.0_f64.into());
        window.children[0]
            .properties
            .insert("opacity", 0.0_f64.into());
        let (scene, frame) = snapshot(&window);
        input.reconcile(&scene, &frame);
        let update = projection
            .update(&scene, &frame, &input, 1.0, None)
            .unwrap();
        let (id, item) = named(&update, "First");
        assert_eq!(id, first_id);
        assert_eq!(update.focus, first_id);
        assert!(item.supports_action(Action::Click));
        let root = update.nodes.iter().find(|(id, _)| *id == ROOT).unwrap();
        assert_eq!(root.1.children(), &[first_id, second_id]);
    }

    #[test]
    fn popup_accessibility_exposes_background_only_when_nonmodal() {
        let mut window = Node::new("Window", None);
        let mut anchor = button("trigger", "Trigger", 130.0);
        let mut popup = Node::new("PopupWindow", Some("popup"));
        popup.properties.insert("open", true.into());
        popup
            .properties
            .insert("accessible-name", "Options".to_owned().into());
        popup.children.push(button("finish", "Finish", 0.0));
        anchor.children.push(popup);
        window
            .children
            .extend([button("background", "Background", 0.0), anchor]);
        let project = |node| {
            let scene = Rc::new(node);
            let frame = Rc::new(layout(&scene, Size::new(320.0, 180.0), &mut Text));
            Projection::default()
                .update(&scene, &frame, &Interaction::default(), 1.0, None)
                .unwrap()
        };
        let nonmodal = project(window.clone());
        assert!(!named(&nonmodal, "Options").1.is_modal());
        assert_eq!(
            nonmodal
                .nodes
                .iter()
                .filter(|(_, node)| node.label() == Some("Options"))
                .count(),
            1
        );
        assert!(
            named(&nonmodal, "Background")
                .1
                .supports_action(Action::Click)
        );
        window.children[1].children[0]
            .properties
            .insert("modal", true.into());
        let modal = project(window);
        assert!(named(&modal, "Options").1.is_modal());
        assert!(
            !modal
                .nodes
                .iter()
                .any(|(_, node)| node.label() == Some("Background"))
        );
    }

    #[test]
    fn an_elided_label_exposes_its_complete_text_to_assistive_technology() {
        let value = "Long label with a tail that is outside its visible width";
        let mut window = Node::new("Window", None);
        let mut label = Node::new("Label", Some("summary"));
        length(&mut label, "width", 30.0);
        label
            .properties
            .insert("overflow", xrack_runtime::TextOverflow::Elide.into());
        label.properties.insert("text", value.to_owned().into());
        window.children.push(label);
        let scene = Rc::new(window);
        let frame = Rc::new(layout(&scene, Size::new(200.0, 100.0), &mut Text));
        let update = Projection::default()
            .update(&scene, &frame, &Interaction::default(), 1.0, None)
            .unwrap();
        assert!(
            update
                .nodes
                .iter()
                .any(|(_, node)| node.role() == Role::Label && node.value() == Some(value))
        );
    }

    #[test]
    fn named_images_have_an_image_role_and_decorative_images_are_omitted() {
        let mut icon = Node::new("Image", Some("named"));
        length(&mut icon, "width", 32.0);
        length(&mut icon, "height", 32.0);
        let mut decorative = icon.clone();
        length(&mut decorative, "x", 40.0);
        icon.properties
            .insert("accessible-name", "Map preview".to_owned().into());
        let mut window = Node::new("Window", None);
        window.children.extend([icon, decorative]);
        let scene = Rc::new(window);
        let frame = Rc::new(layout(&scene, Size::new(100.0, 60.0), &mut Text));
        let update = Projection::default()
            .update(&scene, &frame, &Interaction::default(), 1.0, None)
            .unwrap();
        assert_eq!(named(&update, "Map preview").1.role(), Role::Image);
        assert_eq!(update.nodes.len(), 2);
    }

    #[test]
    fn styled_text_exposes_full_plain_text_through_the_label_role() {
        let mut label = Node::new("StyledText", Some("rich"));
        length(&mut label, "width", 30.0);
        label
            .properties
            .insert("overflow", xrack_runtime::TextOverflow::Elide.into());
        let text = xrack_runtime::StyledText::from_markdown("**Full** `text`").unwrap();
        label.properties.insert("text", text.into());
        let mut window = Node::new("Window", None);
        window.children.push(label);
        let scene = Rc::new(window);
        let frame = Rc::new(layout(&scene, Size::new(200.0, 100.0), &mut Text));
        let update = Projection::default()
            .update(&scene, &frame, &Interaction::default(), 1.0, None)
            .unwrap();
        assert!(
            update.nodes.iter().any(|(_, node)| {
                node.role() == Role::Label && node.value() == Some("Full text")
            })
        );
    }

    #[test]
    fn styled_text_projects_only_visible_links_with_independent_focus_routes() {
        let mut label = Node::new("StyledText", Some("links"));
        length(&mut label, "width", 210.0);
        label.properties.insert(
            "text",
            xrack_runtime::StyledText::from_markdown("[first](#a) [second](#b)\n\n[hidden](#c)")
                .unwrap()
                .into(),
        );
        label.properties.insert("max-lines", 1_i64.into());
        label
            .properties
            .insert("overflow", xrack_runtime::TextOverflow::Elide.into());
        let mut scene = Node::new("Window", None);
        scene.children.push(label);
        let scene = Rc::new(scene);
        let frame = Rc::new(layout(&scene, Size::new(240.0, 120.0), &mut Text));
        let mut input = Interaction::default();
        input.focus_next(&scene, &frame, false);
        input.focus_next(&scene, &frame, false);
        let mut projection = Projection::default();
        let update = projection
            .update(&scene, &frame, &input, 1.0, None)
            .unwrap();
        let links: Vec<_> = update
            .nodes
            .iter()
            .filter(|(_, node)| node.role() == Role::Link)
            .collect();
        assert_eq!(links.len(), 2);
        assert_eq!(links[0].1.url(), Some("#a"));
        assert_eq!(links[1].1.url(), Some("#b"));
        assert_eq!(update.focus, links[1].0);
        assert!(links[1].1.supports_action(Action::Click));
        assert!(links[1].1.supports_action(Action::Focus));
        assert!(
            matches!(projection.route(update.focus), Some(Route::TextLink(target))
            if target.text_link_index() == Some(1))
        );
    }

    #[test]
    fn swipe_delegate_projects_only_revealed_actions() {
        let mut row = Node::new("SwipeDelegate", Some("row"));
        row.properties
            .insert("accessible-name", "Message".to_owned().into());
        let mut actions = Node::new("SwipeRight", None);
        actions.children.push(button("archive", "Archive", 0.0));
        row.children
            .extend([Node::new("SwipeContent", None), actions]);
        let input = Interaction::default();
        let project = |node: Node| {
            let scene = Rc::new(node);
            let frame = Rc::new(layout(&scene, Size::new(300.0, 50.0), &mut Text));
            Projection::default()
                .update(&scene, &frame, &input, 1.0, None)
                .unwrap()
        };
        let closed = project(row.clone());
        let (_, delegate) = named(&closed, "Message");
        assert!(delegate.supports_action(Action::Expand));
        assert!(
            named(&closed, "Show actions")
                .1
                .supports_action(Action::Click)
        );
        assert!(
            !closed
                .nodes
                .iter()
                .any(|(_, node)| node.label() == Some("Archive"))
        );
        row.properties.insert("open-side", SwipeSide::Right.into());
        let open = project(row);
        let (_, delegate) = named(&open, "Message");
        assert!(delegate.supports_action(Action::Collapse));
        assert!(
            named(&open, "Hide actions")
                .1
                .supports_action(Action::Click)
        );
        assert!(named(&open, "Archive").1.supports_action(Action::Click));
    }

    #[test]
    fn accessibility_button_identity_changes_when_its_standard_action_changes() {
        let mut window = Node::new("Window", None);
        let mut button = Node::new("Button", Some("current-action"));
        button
            .properties
            .insert("kind", StandardButtonKind::Save.into());
        window.children.push(button);
        let input = Interaction::default();
        let mut projection = Projection::default();
        let mut project = |window: Node| {
            let frame = Rc::new(layout(&window, Size::new(200.0, 100.0), &mut Text));
            projection
                .update(&Rc::new(window), &frame, &input, 1.0, None)
                .unwrap()
        };
        let old = project(window.clone());
        let (save, _) = named(&old, "Save");
        window.children[0]
            .properties
            .insert("kind", StandardButtonKind::Discard.into());
        let new = project(window);
        let (discard, _) = named(&new, "Discard");
        assert_ne!(save, discard);
        assert!(projection.route(save).is_none());
        assert!(matches!(
            projection.route(discard),
            Some(Route::Element {
                standard_kind: Some(StandardButtonKind::Discard),
                ..
            })
        ));
    }

    #[test]
    fn virtual_table_exposes_absolute_cell_positions_and_sortable_headers() {
        let mut table = Node::new("TableView", Some("inventory"));
        table
            .properties
            .insert("accessible-name", "Inventory".to_owned().into());
        table.list = Some(ListMetrics {
            row_count: 50_000,
            first_row: 200,
        });
        let mut column = Node::new("TableColumn", None);
        column
            .properties
            .insert("title", "Quantity".to_owned().into());
        table.table_header = Some(Box::new(xrack_runtime::TableHeader {
            columns: vec![column],
            sort: Some(xrack_runtime::TableSort {
                column: 0,
                descending: true,
            }),
            column_widths: Vec::new(),
        }));
        let mut row = Node::new("ListRow", None);
        let mut record = Node::new("TableRow", None);
        let mut cell = Node::new("TableCell", None);
        cell.properties.insert("text", "47 units".to_owned().into());
        record.children.push(cell);
        row.children.push(record);
        table.children.push(row);
        let scene = Rc::new(table);
        let offsets = BTreeMap::from([(Vec::new(), 6_400.0)]);
        let frame = Rc::new(xrack_layout::layout_with_scroll(
            &scene,
            Size::new(240.0, 120.0),
            &mut Text,
            &offsets,
        ));
        let mut projection = Projection::default();
        let update = projection
            .update(&scene, &frame, &Interaction::default(), 1.0, None)
            .unwrap();
        let (_, table) = named(&update, "Inventory");
        assert_eq!(table.row_count(), Some(50_000));
        assert_eq!(table.column_count(), Some(1));
        let (id, header) = named(&update, "Quantity");
        assert_eq!(table.children()[0], id);
        assert_eq!(header.role(), Role::ColumnHeader);
        assert_eq!(header.sort_direction(), Some(SortDirection::Descending));
        assert!(matches!(
            projection.route(id),
            Some(Route::ControlPart {
                action: ControlAction::SortColumn(0),
                ..
            })
        ));
        let (_, cell) = named(&update, "47 units");
        assert_eq!(cell.row_index(), Some(200));
        assert_eq!(cell.column_index(), Some(0));
    }

    #[test]
    fn component_table_cell_exposes_coordinates_and_keeps_its_editor_role() {
        let mut table = Node::new("TableView", None);
        table.list = Some(ListMetrics {
            row_count: 1,
            first_row: 0,
        });
        table.table_header = Some(Box::new(xrack_runtime::TableHeader {
            columns: vec![Node::new("TableColumn", None)],
            sort: None,
            column_widths: Vec::new(),
        }));
        let mut row = Node::new("ListRow", None);
        let mut record = Node::new("TableRow", None);
        let mut cell = Node::new("HorizontalLayout", None);
        cell.properties.insert("table-delegate-cell", true.into());
        cell.properties.insert("table-selected", true.into());
        let mut editor = Node::new("LineEdit", Some("entry"));
        editor
            .properties
            .insert("accessible-name", "Delegate editor".to_owned().into());
        cell.children.push(editor);
        record.children.push(cell);
        row.children.push(record);
        table.children.push(row);
        let scene = Rc::new(table);
        let frame = Rc::new(xrack_layout::layout(
            &scene,
            Size::new(240.0, 120.0),
            &mut Text,
        ));
        let mut projection = Projection::default();
        let update = projection
            .update(&scene, &frame, &Interaction::default(), 1.0, None)
            .unwrap();
        let (_, cell) = update
            .nodes
            .iter()
            .find(|(_, node)| node.role() == Role::Cell)
            .unwrap();
        assert_eq!(cell.row_index(), Some(0));
        assert_eq!(cell.column_index(), Some(0));
        assert_eq!(cell.is_selected(), Some(true));
        let (id, editor) = named(&update, "Delegate editor");
        assert_eq!(editor.role(), Role::TextInput);
        assert!(matches!(projection.route(id), Some(Route::Element { .. })));
    }

    #[test]
    fn frozen_columns_hide_scrolled_cells_under_their_pane_from_accessibility() {
        let mut table = Node::new("TableView", Some("table"));
        table.properties.insert("frozen-columns", 2_i64.into());
        table.list = Some(ListMetrics {
            row_count: 1,
            first_row: 0,
        });
        let mut record = Node::new("TableRow", None);
        let columns = ["A", "B", "C", "D"].map(|label| {
            let mut column = Node::new("TableColumn", None);
            length(&mut column, "width", 100.0);
            column.properties.insert("title", label.to_owned().into());
            let mut cell = Node::new("TableCell", None);
            cell.properties
                .insert("text", format!("Cell {label}").into());
            record.children.push(cell);
            column
        });
        let mut row = Node::new("ListRow", None);
        row.children.push(record);
        table.children.push(row);
        table.table_header = Some(Box::new(xrack_runtime::TableHeader {
            columns: columns.into(),
            sort: None,
            column_widths: Vec::new(),
        }));
        let scene = Rc::new(table);
        let size = Size::new(300.0, 130.0);
        let mut input = Interaction::default();
        let mut projection = Projection::default();
        let frame = Rc::new(xrack_layout::layout_with_input(
            &scene, size, &mut Text, &input,
        ));
        let first = projection
            .update(&scene, &frame, &input, 1.0, None)
            .unwrap();
        let (id, frozen) = named(&first, "Cell B");
        let bounds = frozen.bounds();
        assert!(input.accessibility_scroll_to(&frame, &[], Orientation::Horizontal, 100.0));
        let frame = Rc::new(xrack_layout::layout_with_input(
            &scene, size, &mut Text, &input,
        ));
        let update = projection
            .update(&scene, &frame, &input, 1.0, None)
            .unwrap();
        let (after_id, after) = named(&update, "Cell B");
        assert_eq!(after_id, id);
        assert_eq!(after.bounds(), bounds);
        assert!(
            !update
                .nodes
                .iter()
                .any(|(_, node)| matches!(node.label(), Some("C" | "Cell C")))
        );
        assert_eq!(named(&update, "D").1.bounds().unwrap().x0, 200.0);
        assert_eq!(named(&update, "Cell D").1.column_index(), Some(3));
    }

    #[test]
    fn moved_table_columns_keep_accessible_identity_and_report_visual_indices() {
        let mut table = Node::new("TableView", Some("table"));
        table
            .properties
            .insert("accessible-name", "Moved table".to_owned().into());
        table.list = Some(ListMetrics {
            row_count: 1,
            first_row: 0,
        });
        let mut record = Node::new("TableRow", None);
        let columns = ["Name", "Quantity", "Owner"].map(|title| {
            let mut column = Node::new("TableColumn", None);
            column.properties.insert("title", title.to_owned().into());
            let mut cell = Node::new("TableCell", None);
            cell.properties
                .insert("text", format!("{title} value").into());
            record.children.push(cell);
            column
        });
        let mut row = Node::new("ListRow", None);
        row.children.push(record);
        table.children.push(row);
        table.table_header = Some(Box::new(xrack_runtime::TableHeader {
            columns: columns.into(),
            sort: None,
            column_widths: Vec::new(),
        }));
        let layout = |scene: &Node| {
            Rc::new(xrack_layout::layout(
                scene,
                Size::new(300.0, 120.0),
                &mut Text,
            ))
        };
        let input = Interaction::default();
        let mut projection = Projection::default();
        let scene = Rc::new(table.clone());
        let first = projection
            .update(&scene, &layout(&scene), &input, 1.0, None)
            .unwrap();
        let name = named(&first, "Name").0;
        let cell = named(&first, "Name value").0;

        let (_, order) = table.table_column_order.moved(3, 0, 2).unwrap();
        table.table_column_order = order.clone();
        table.children[0].children[0].table_column_order = order;
        let scene = Rc::new(table);
        let update = projection
            .update(&scene, &layout(&scene), &input, 1.0, None)
            .unwrap();
        let (_, table) = named(&update, "Moved table");
        let (header_id, header) = named(&update, "Name");
        assert_eq!(header_id, name);
        assert_eq!(table.children()[2], name);
        assert_eq!(header.column_index(), Some(2));
        let (cell_id, value) = named(&update, "Name value");
        assert_eq!(cell_id, cell);
        assert_eq!(value.column_index(), Some(2));
        assert!(matches!(
            projection.route(name),
            Some(Route::ControlPart {
                action: ControlAction::SortColumn(0),
                ..
            })
        ));
    }

    #[test]
    fn grid_exposes_absolute_item_positions_and_keeps_delegate_controls() {
        let mut grid = Node::new("GridView", Some("grid"));
        grid.properties
            .insert("accessible-name", "Tiles".to_owned().into());
        length(&mut grid, "cell-width", 100.0);
        length(&mut grid, "cell-height", 60.0);
        grid.list = Some(ListMetrics {
            row_count: 50_000,
            first_row: 300,
        });
        let mut row = Node::new("ListRow", None);
        row.properties.insert("selected", true.into());
        let mut card = Node::new("Rectangle", None);
        card.properties
            .insert("accessible-name", "Tile 300".to_owned().into());
        card.children.push(button("pin", "Pin", 0.0));
        row.children.push(card);
        grid.children.push(row);
        let scene = Rc::new(grid);
        let offsets = BTreeMap::from([(Vec::new(), 6_000.0)]);
        let frame = Rc::new(xrack_layout::layout_with_scroll(
            &scene,
            Size::new(300.0, 60.0),
            &mut Text,
            &offsets,
        ));
        let update = Projection::default()
            .update(&scene, &frame, &Interaction::default(), 1.0, None)
            .unwrap();
        let (_, grid) = named(&update, "Tiles");
        assert_eq!(grid.role(), Role::List);
        assert_eq!(grid.size_of_set(), Some(50_000));
        let tile = update
            .nodes
            .iter()
            .find_map(|(_, node)| (node.role() == Role::ListItem).then_some(node))
            .unwrap();
        assert_eq!(tile.label(), Some("Tile 300"));
        assert_eq!(tile.position_in_set(), Some(300));
        assert_eq!(tile.is_selected(), Some(true));
        assert_eq!(named(&update, "Pin").1.role(), Role::Button);
    }

    #[test]
    fn page_accessibility_reads_header_then_body_then_footer() {
        let mut page = Node::new("Page", None);
        page.properties
            .insert("title", "Settings".to_owned().into());
        let mut header = Node::new("PageHeader", None);
        header.children.push(button("back", "Back", 0.0));
        let mut footer = Node::new("PageFooter", None);
        footer.children.push(button("save", "Save", 0.0));
        page.children = vec![footer, button("edit", "Edit", 0.0), header];
        let mut window = Node::new("Window", None);
        window.children.push(page);
        let scene = Rc::new(window);
        let frame = Rc::new(layout(&scene, Size::new(180.0, 120.0), &mut Text));
        let update = Projection::default()
            .update(&scene, &frame, &Interaction::default(), 1.0, None)
            .unwrap();
        let (_, page) = named(&update, "Settings");
        let [header, body, footer] = page.children() else {
            panic!("expected three accessible page regions")
        };
        let by_id = |id| {
            update
                .nodes
                .iter()
                .find(|(node_id, _)| *node_id == id)
                .map(|(_, node)| node)
                .unwrap()
        };
        assert_eq!(by_id(*header).children(), &[named(&update, "Back").0]);
        assert_eq!(*body, named(&update, "Edit").0);
        assert_eq!(by_id(*footer).children(), &[named(&update, "Save").0]);
    }

    #[test]
    fn standard_list_exposes_absolute_positions_for_virtualized_options() {
        let mut view = Node::new("StandardListView", Some("records"));
        view.properties
            .insert("accessible-name", "Records".to_owned().into());
        view.list = Some(ListMetrics {
            row_count: 50_000,
            first_row: 123,
        });
        let mut row = Node::new("ListRow", None);
        row.properties.insert("selected", true.into());
        let mut label = Node::new("StandardListItem", None);
        label.properties.insert("text", "Row 123".to_owned().into());
        row.children.push(label);
        view.children.push(row);
        let scene = Rc::new(view);
        let offsets = BTreeMap::from([(Vec::new(), 123.0 * ListMetrics::row_height(&scene))]);
        let frame = Rc::new(xrack_layout::layout_with_scroll(
            &scene,
            Size::new(180.0, 72.0),
            &mut Text,
            &offsets,
        ));
        let update = Projection::default()
            .update(&scene, &frame, &Interaction::default(), 1.0, None)
            .unwrap();
        let (_, list) = named(&update, "Records");
        assert_eq!(list.role(), Role::ListBox);
        assert_eq!(list.size_of_set(), Some(50_000));
        let (_, option) = named(&update, "Row 123");
        assert_eq!(option.role(), Role::ListBoxOption);
        assert_eq!(option.position_in_set(), Some(123));
        assert_eq!(option.is_selected(), Some(true));
        assert!(option.supports_action(Action::Click));
    }

    #[test]
    fn indicators_project_numeric_progress_and_only_active_busy_status() {
        let mut busy = Node::new("BusyIndicator", Some("busy"));
        busy.properties.insert("animated", false.into());
        let mut spinner = Node::new("Spinner", Some("ring"));
        length(&mut spinner, "x", 50.0);
        spinner.properties.insert("progress", 1.5_f64.into());
        let mut root = Node::new("Window", None);
        root.children = vec![busy, spinner];
        let scene = Rc::new(root);
        let size = Size::new(180.0, 70.0);
        let frame = Rc::new(layout(&scene, size, &mut Text));
        let mut projection = Projection::default();
        let input = Interaction::default();
        let update = projection
            .update(&scene, &frame, &input, 1.0, None)
            .unwrap();
        let (_, loading) = named(&update, "Loading");
        assert_eq!(loading.role(), Role::ProgressIndicator);
        assert!(loading.is_busy());
        assert_eq!(loading.numeric_value(), None);
        let (_, progress) = named(&update, "Progress");
        assert_eq!(progress.numeric_value(), Some(1.0));
        assert!(!progress.is_busy());

        let mut stopped = (*scene).clone();
        stopped.children[0]
            .properties
            .insert("running", false.into());
        let stopped = Rc::new(stopped);
        let stopped_frame = Rc::new(layout(&stopped, size, &mut Text));
        assert_eq!(stopped_frame.children[0].bounds, frame.children[0].bounds);
        assert!(!stopped_frame.children[0].visible);
        let update = projection
            .update(&stopped, &stopped_frame, &input, 1.0, None)
            .unwrap();
        assert!(
            !update
                .nodes
                .iter()
                .any(|(_, node)| node.label() == Some("Loading"))
        );
    }

    #[test]
    fn delay_button_accessibility_requires_confirmation_after_the_deadline() {
        use std::time::{Duration, Instant};
        use xrack_runtime::ControlAction;

        let mut confirm = button("confirm", "Confirm", 10.0);
        confirm.kind = "DelayButton";
        confirm.properties.insert("delay", 200_i64.into());
        let mut root = Node::new("Window", None);
        root.children.push(confirm);
        let scene = Rc::new(root);
        let frame = Rc::new(layout(&scene, Size::new(200.0, 90.0), &mut Text));
        let mut input = Interaction::default();
        let target = input.accessibility_target(&scene, &frame, &[0]).unwrap();
        let mut projection = Projection::default();
        let now = Instant::now();
        let click = |input: &mut Interaction, millis| {
            input.accessibility_delay_click(
                &scene,
                &frame,
                target.clone(),
                now + Duration::from_millis(millis),
            )
        };
        assert!(click(&mut input, 0).is_none());
        let armed = projection
            .update(&scene, &frame, &input, 1.0, None)
            .unwrap();
        assert!(
            named(&armed, "Confirm")
                .1
                .description()
                .unwrap()
                .contains("Waiting")
        );
        assert!(click(&mut input, 50).is_none());
        assert!(input.delay_deadline().is_none());
        assert!(click(&mut input, 100).is_none());
        assert!(
            input
                .advance_delay(&scene, &frame, now + Duration::from_millis(300))
                .is_none()
        );
        assert!(input.delay_deadline().is_none());
        let ready = projection
            .update(&scene, &frame, &input, 1.0, None)
            .unwrap();
        let (_, node) = named(&ready, "Confirm");
        assert!(node.description().unwrap().starts_with("Ready."));
        assert_eq!(node.toggled(), Some(Toggled::False));
        assert!(ready.nodes.iter().any(|(_, status)| {
            status.role() == Role::Status
                && status.live() == Some(Live::Polite)
                && status
                    .label()
                    .is_some_and(|label| label.starts_with("Ready."))
        }));
        assert_eq!(
            click(&mut input, 301).unwrap().action,
            ControlAction::DelayButton(true)
        );
    }

    #[test]
    fn password_projection_publishes_only_bullets_and_maps_utf8_selection() {
        use xrack_runtime::InputType;

        let secret = "e\u{301}👩‍💻";
        for revealed in [false, true] {
            let mut field = Node::new("LineEdit", Some("passphrase"));
            field
                .properties
                .insert("input-type", InputType::Password.into());
            field.properties.insert("text", secret.to_owned().into());
            field.properties.insert("password-visible", revealed.into());
            let scene = Rc::new(field);
            let frame = Rc::new(layout(&scene, Size::new(240.0, 40.0), &mut Text));
            let mut input = Interaction::default();
            input.accessibility_focus(input.accessibility_target(&scene, &frame, &[]).unwrap());
            input.sync_editor(&scene);
            let mut tree = Projection::default();
            let update = tree.update(&scene, &frame, &input, 1.0, None).unwrap();
            let (id, field) = update
                .nodes
                .iter()
                .find(|(_, node)| node.role() == Role::PasswordInput)
                .unwrap();
            assert!(field.value().is_none());
            assert!(field.supports_action(Action::SetTextSelection));
            let run = &update
                .nodes
                .iter()
                .find(|(id, _)| field.children().contains(id))
                .unwrap()
                .1;
            assert_eq!(run.value(), Some("••"));
            assert_eq!(run.character_lengths(), &[3, 3]);
            assert!(run.word_starts().is_empty());
            assert!(!format!("{update:?}").contains(secret));

            let mut selection = *field.text_selection().unwrap();
            selection.anchor.character_index = 1;
            let (anchor, focus) = tree.text_selection(*id, selection, secret).unwrap();
            assert_eq!(anchor, "e\u{301}".len());
            assert_eq!(focus.offset, secret.len());
            assert!(tree.text_selection(*id, selection, "changed").is_none());
        }
    }

    #[test]
    fn tab_bar_reports_the_selected_tab_and_the_overflow_range() {
        let mut bar = Node::new("TabBar", Some("tabs"));
        bar.properties
            .insert("accessible-name", "Pages".to_owned().into());
        for (name, enabled) in [("Overview", true), ("Disabled", false), ("Details", true)] {
            let mut tab = Node::new("TabButton", None);
            length(&mut tab, "width", 80.0);
            tab.properties.insert("text", name.to_owned().into());
            tab.properties.insert("enabled", enabled.into());
            bar.children.push(tab);
        }
        let scene = Rc::new(bar);
        let frame = Rc::new(layout(&scene, Size::new(160.0, 40.0), &mut Text));
        let input = Interaction::default();
        let update = Projection::default()
            .update(&scene, &frame, &input, 1.0, None)
            .unwrap();
        let (_, tabs) = named(&update, "Pages");
        assert_eq!(tabs.role(), Role::TabList);
        assert_eq!(tabs.children().len(), 2);
        assert_eq!(tabs.scroll_x_max(), Some(80.0));
        assert!(tabs.supports_action(Action::ScrollRight));
        let (_, overview) = named(&update, "Overview");
        assert_eq!(overview.role(), Role::Tab);
        assert_eq!(overview.is_selected(), Some(true));
        assert!(overview.supports_action(Action::Click));
        let (_, disabled) = named(&update, "Disabled");
        assert_eq!(disabled.is_selected(), Some(false));
        assert!(!disabled.supports_action(Action::Click));
    }

    #[test]
    fn publishes_directional_selection_and_rejects_foreign_or_stale_text_ranges() {
        let value = "A👩🏽‍💻\r\n中e\u{301}\n";
        let mut editor = Node::new("TextEdit", Some("notes"));
        length(&mut editor, "width", 140.0);
        length(&mut editor, "height", 80.0);
        editor
            .properties
            .insert("accessible-name", "Notes".to_owned().into());
        editor.properties.insert("text", value.to_owned().into());
        editor.properties.insert("read-only", true.into());
        let mut other = Node::new("LineEdit", Some("other"));
        length(&mut other, "y", 88.0);
        other
            .properties
            .insert("accessible-name", "Other".to_owned().into());
        other
            .properties
            .insert("placeholder-text", "Placeholder".to_owned().into());
        let mut root = Node::new("Window", None);
        root.children = vec![editor, other];
        let scene = Rc::new(root);
        let frame = Rc::new(layout(&scene, Size::new(180.0, 150.0), &mut Text));
        let mut input = Interaction::default();
        let target = input.accessibility_target(&scene, &frame, &[0]).unwrap();
        input.accessibility_focus(target);
        input.sync_editor(&scene);
        let mut tree = Projection::default();
        let update = tree.update(&scene, &frame, &input, 1.0, None).unwrap();
        let (field, node) = named(&update, "Notes");
        let [first, second, last] = node.children() else {
            panic!("expected three text lines")
        };
        let [first, second, last] = [*first, *second, *last];
        let (_, other) = named(&update, "Other");
        let other_run = other.children()[0];
        let run = |id| {
            &update
                .nodes
                .iter()
                .find(|(node_id, _)| *node_id == id)
                .unwrap()
                .1
        };
        assert_eq!(run(first).value(), Some("A👩🏽‍💻\r\n"));
        assert_eq!(run(first).character_lengths(), &[1, "👩🏽‍💻".len() as u8, 2]);
        assert_eq!(run(second).character_lengths(), &[3, 3, 1]);
        assert_eq!(run(last).value(), Some(""));
        assert_eq!(run(other_run).value(), Some(""));
        assert!(node.supports_action(Action::SetTextSelection));
        assert!(!node.supports_action(Action::SetValue));

        let anchor = value.len();
        let focus = TextPosition {
            offset: 1,
            trailing: false,
        };
        input
            .editor
            .as_mut()
            .unwrap()
            .set_selection(value, anchor, focus);
        let changed = tree.update(&scene, &frame, &input, 1.0, None).unwrap();
        let (_, updated) = named(&changed, "Notes");
        let selection = *updated.text_selection().unwrap();
        assert_eq!(selection.anchor.node, last);
        assert_eq!(selection.focus.node, first);
        assert_eq!(
            tree.text_selection(field, selection, value),
            Some((anchor, focus))
        );
        assert!(tree.text_selection(field, selection, "changed").is_none());
        let mut invalid = selection;
        invalid.focus.character_index = 4;
        assert!(tree.text_selection(field, invalid, value).is_none());
        invalid.focus.node = other_run;
        invalid.focus.character_index = 0;
        assert!(tree.text_selection(field, invalid, value).is_none());
        assert!(tree.update(&scene, &frame, &input, 1.0, None).is_none());

        input
            .editor
            .as_mut()
            .unwrap()
            .set_preedit("输入中".into(), None);
        let composing = tree.update(&scene, &frame, &input, 1.0, None).unwrap();
        let (_, node) = named(&composing, "Notes");
        assert_eq!(node.value(), Some(value));
        assert!(!node.supports_action(Action::SetTextSelection));
    }

    #[test]
    fn projects_physical_bounds_and_focus_and_excludes_modal_background() {
        let viewport = Size::new(400.0, 260.0);
        let mut root = Node::new("Window", None);
        let main = button("main", "Save", 10.0);
        let mut disabled = button("disabled", "Disabled", 130.0);
        disabled.properties.insert("enabled", Value::Bool(false));
        root.children = vec![main, disabled];
        let mut scene = Rc::new(root);
        let first_layout = Rc::new(layout(&scene, viewport, &mut Text));
        let mut input = Interaction::default();
        let mut tree = Projection::default();
        let update = tree
            .update(&scene, &first_layout, &input, 2.0, None)
            .unwrap();
        let (save, node) = named(&update, "Save");
        assert_eq!(node.role(), Role::Button);
        assert_eq!(
            node.bounds(),
            Some(accesskit::Rect::new(20.0, 0.0, 220.0, 64.0))
        );
        assert!(node.supports_action(Action::Click));
        let (_, disabled) = named(&update, "Disabled");
        assert!(disabled.is_disabled());
        assert!(!disabled.supports_action(Action::Click));
        assert!(
            tree.update(&scene, &first_layout, &input, 2.0, None)
                .is_none()
        );

        let target = input
            .accessibility_target(&scene, &first_layout, &[0])
            .unwrap();
        input.accessibility_focus(target);
        assert_eq!(
            tree.update(&scene, &first_layout, &input, 2.0, None)
                .unwrap()
                .focus,
            save
        );

        let mut modal = Node::new("Dialog", Some("modal"));
        modal.properties.insert("open", Value::Bool(true));
        modal
            .properties
            .insert("title", Value::String("Confirm".into()));
        modal.children.push(button("ok", "OK", 0.0));
        Rc::make_mut(&mut scene).children.push(modal);
        let new_layout = Rc::new(layout(&scene, viewport, &mut Text));
        input.reconcile(&scene, &new_layout);
        let update = tree.update(&scene, &new_layout, &input, 2.0, None).unwrap();
        assert!(named(&update, "Confirm").1.is_modal());
        assert_eq!(update.focus, named(&update, "OK").0);
        assert!(tree.route(save).is_none());
        assert!(
            input
                .accessibility_target(&scene, &new_layout, &[0])
                .is_none()
        );
    }

    #[test]
    fn visible_model_rows_keep_identity_when_reordered() {
        let model = VecModel::from(vec!["Alpha", "Beta"]);
        let make_row = |index, label| {
            let mut row = Node::new("ListRow", None);
            row.row_key = model.row_key(index);
            row.children.push(button("row-button", label, 0.0));
            row
        };
        let mut view = Node::new("ListView", Some("list"));
        length(&mut view, "row-height", 42.0);
        view.list = Some(ListMetrics {
            row_count: 2,
            first_row: 0,
        });
        view.children = vec![make_row(0, "Alpha"), make_row(1, "Beta")];
        let mut scene = Rc::new(view);
        let viewport = Size::new(200.0, 130.0);
        let initial_layout = Rc::new(layout(&scene, viewport, &mut Text));
        let input = Interaction::default();
        let mut tree = Projection::default();
        let update = tree
            .update(&scene, &initial_layout, &input, 1.0, None)
            .unwrap();
        let first = named(&update, "Alpha").0;
        let second = named(&update, "Beta").0;
        Rc::make_mut(&mut scene).children.swap(0, 1);
        let current_layout = Rc::new(layout(&scene, viewport, &mut Text));
        let update = tree
            .update(&scene, &current_layout, &input, 1.0, None)
            .unwrap();
        assert_eq!(named(&update, "Alpha").0, first);
        assert_eq!(named(&update, "Beta").0, second);
        assert!(
            matches!(tree.route(first), Some(Route::Element { target, .. }) if target.path == [1, 0])
        );
    }

    #[test]
    fn nonexclusive_group_members_and_a_mixed_master_keep_their_accessible_roles() {
        let mut scene = Node::new("Window", None);
        for (index, name, kind) in [
            (0, "All options", "CheckBox"),
            (1, "Assets", "CheckBox"),
            (2, "Background", "Switch"),
        ] {
            let mut node = Node::new(kind, None);
            length(&mut node, "y", f64::from(index) * 40.0);
            length(&mut node, "height", 34.0);
            node.properties.insert("text", name.to_owned().into());
            if index == 0 {
                node.properties
                    .insert("check-state", CheckState::PartiallyChecked.into());
            } else {
                node.properties.insert("checked", (index == 1).into());
            }
            scene.children.push(node);
        }
        xrack_runtime::bind_check_group(
            &mut scene,
            "options",
            "Export options".into(),
            &[&[1], &[2]],
            true,
        );
        let scene = Rc::new(scene);
        let layout = Rc::new(layout(&scene, Size::new(350.0, 140.0), &mut Text));
        let mut projection = Projection::default();
        let update = projection
            .update(&scene, &layout, &Interaction::default(), 1.0, None)
            .unwrap();
        let (group, group_node) = named(&update, "Export options");
        assert_eq!(group_node.role(), Role::Group);
        let (_, master) = named(&update, "All options");
        assert_eq!(master.role(), Role::CheckBox);
        assert_eq!(master.toggled(), Some(Toggled::Mixed));
        assert_eq!(master.member_of(), None);
        let (_, assets) = named(&update, "Assets");
        let (_, background) = named(&update, "Background");
        assert_eq!(assets.member_of(), Some(group));
        assert_eq!(background.member_of(), Some(group));
        assert_eq!(assets.toggled(), Some(Toggled::True));
        assert_eq!(background.toggled(), Some(Toggled::False));
    }

    #[test]
    fn attached_radio_groups_publish_membership_without_reparenting_controls() {
        use xrack_runtime::{ButtonGroupSelection, bind_button_group};

        let mut root = Node::new("Window", None);
        for (index, name) in ["Left group", "Right group"].into_iter().enumerate() {
            let mut component = Node::new("Rectangle", None);
            length(&mut component, "x", (index * 160) as f64);
            length(&mut component, "width", 150.0);
            length(&mut component, "height", 100.0);
            for (row, label) in ["One", "Two"].into_iter().enumerate() {
                let mut radio = Node::new("RadioButton", None);
                length(&mut radio, "y", (row * 40) as f64);
                length(&mut radio, "height", 34.0);
                radio
                    .properties
                    .insert("text", format!("{name} {label}").into());
                component.children.push(radio);
            }
            bind_button_group(
                &mut component,
                "options",
                name.into(),
                &[&[0], &[1]],
                ButtonGroupSelection::Index(index as i64),
                index == 0,
            );
            root.children.push(component);
        }
        let scene = Rc::new(root);
        let layout = Rc::new(layout(&scene, Size::new(350.0, 130.0), &mut Text));
        let mut projection = Projection::default();
        let update = projection
            .update(&scene, &layout, &Interaction::default(), 1.0, None)
            .unwrap();
        let (left, group) = named(&update, "Left group");
        let (right, disabled) = named(&update, "Right group");
        assert_eq!(group.role(), Role::RadioGroup);
        assert_ne!(left, right);
        assert!(disabled.is_disabled());
        assert_eq!(named(&update, "Left group One").1.member_of(), Some(left));
        assert_eq!(named(&update, "Left group Two").1.member_of(), Some(left));
        assert_eq!(named(&update, "Right group Two").1.member_of(), Some(right));
        assert_eq!(
            named(&update, "Left group One").1.toggled(),
            Some(Toggled::True)
        );
        assert_eq!(
            named(&update, "Right group One").1.toggled(),
            Some(Toggled::False)
        );
    }

    #[test]
    fn combo_accessibility_keeps_identity_when_a_row_becomes_disabled() {
        let source = VecModel::from(vec![
            ("Online".to_owned(), true),
            ("Archive".to_owned(), true),
        ]);
        let items = xrack_runtime::ComboItems::with_enabled(
            source.clone(),
            |row| &row.0,
            |row| row.0.clone().into(),
            |row| row.1,
        );
        let scene = || {
            let mut root = Node::new("Window", None);
            let mut combo = Node::new("ComboBox", Some("choice"));
            length(&mut combo, "width", 150.0);
            length(&mut combo, "height", 36.0);
            combo
                .properties
                .insert("model", Value::ComboModel(items.snapshot(0, None)));
            root.children.push(combo);
            Rc::new(root)
        };
        let before = scene();
        let layout = Rc::new(layout(&before, Size::new(240.0, 160.0), &mut Text));
        let mut input = Interaction::default();
        let target = input.accessibility_target(&before, &layout, &[0]).unwrap();
        assert!(input.open_choice(&before, &layout, target));
        input.choice_key(&before, &layout, ChoiceKey::End);
        let mut tree = Projection::default();
        let update = tree.update(&before, &layout, &input, 1.0, None).unwrap();
        let (id, option) = named(&update, "Archive");
        assert_eq!(update.focus, id);
        assert!(option.supports_action(Action::Click));
        assert!(source.set_row(1, ("Archive".to_owned(), false)));
        let after = scene();
        input.reconcile(&after, &layout);
        let update = tree.update(&after, &layout, &input, 1.0, None).unwrap();
        let (current, option) = named(&update, "Archive");
        assert_eq!(id, current);
        assert_ne!(update.focus, id);
        assert!(option.is_disabled());
        assert!(!option.supports_action(Action::Click));
        assert!(tree.route(id).is_none());
    }

    #[test]
    fn combo_popup_exposes_visible_options_and_highlighted_focus() {
        let mut root = Node::new("Window", None);
        let mut combo = Node::new("ComboBox", Some("choice"));
        length(&mut combo, "width", 150.0);
        length(&mut combo, "height", 36.0);
        combo
            .properties
            .insert("accessible-name", Value::String("Workspace".into()));
        combo.properties.insert(
            "model",
            Value::StringList(vec!["Alpha".into(), "Beta".into()]),
        );
        root.children.push(combo);
        let scene = Rc::new(root);
        let layout = Rc::new(layout(&scene, Size::new(240.0, 160.0), &mut Text));
        let mut input = Interaction::default();
        let target = input.accessibility_target(&scene, &layout, &[0]).unwrap();
        assert!(input.open_choice(&scene, &layout, target));
        let mut tree = Projection::default();
        let update = tree.update(&scene, &layout, &input, 1.0, None).unwrap();
        let (_, combo) = named(&update, "Workspace");
        assert_eq!(combo.role(), Role::ComboBox);
        assert_eq!(combo.controls().len(), 1);
        let (alpha, option) = named(&update, "Alpha");
        assert_eq!(option.role(), Role::ListBoxOption);
        assert_eq!(update.focus, alpha);
        assert!(
            matches!(tree.route(alpha), Some(Route::Choice { index: 0, value, .. }) if value == "Alpha")
        );
        input.choice_key(&scene, &layout, ChoiceKey::Down);
        let update = tree.update(&scene, &layout, &input, 1.0, None).unwrap();
        assert_eq!(update.focus, named(&update, "Beta").0);
    }

    #[test]
    fn editor_context_menu_publishes_actions_and_expires_its_accessibility_session() {
        let mut root = Node::new("Window", None);
        let mut entry = Node::new("LineEdit", Some("entry"));
        length(&mut entry, "width", 180.0);
        length(&mut entry, "height", 36.0);
        entry
            .properties
            .insert("accessible-name", Value::String("Document".into()));
        entry
            .properties
            .insert("text", Value::String("initial value".into()));
        root.children.push(entry);
        let scene = Rc::new(root);
        let layout = Rc::new(layout(&scene, Size::new(420.0, 280.0), &mut Text));
        let mut input = Interaction::default();
        let trigger = input.accessibility_target(&scene, &layout, &[0]).unwrap();
        input.accessibility_focus(trigger.clone());
        let mut tree = Projection::default();
        let update = tree.update(&scene, &layout, &input, 1.0, None).unwrap();
        assert!(
            named(&update, "Document")
                .1
                .supports_action(Action::ShowContextMenu)
        );
        assert!(input.open_context_menu(&scene, &layout, true));
        let update = tree.update(&scene, &layout, &input, 1.0, None).unwrap();
        assert_eq!(named(&update, "Text editing").1.role(), Role::Menu);
        assert!(named(&update, "Copy").1.is_disabled());
        let old_id = named(&update, "Select All").0;
        let Some(Route::MenuEntry { entry, .. }) = tree.route(old_id) else {
            panic!("editor menu action");
        };
        let action = input
            .accessibility_menu_entry(&scene, &layout, &trigger, &entry, true)
            .unwrap();
        assert_eq!(action.path, [0]);
        assert_eq!(
            action.action,
            ControlAction::TextCommand(xrack_runtime::TextCommand::SelectAll)
        );
        input.apply_text_selection(&scene, xrack_runtime::TextCommand::SelectAll);
        assert!(input.open_context_menu(&scene, &layout, true));
        let update = tree.update(&scene, &layout, &input, 1.0, None).unwrap();
        assert!(named(&update, "Copy").1.supports_action(Action::Click));
        assert_ne!(named(&update, "Select All").0, old_id);
        assert!(tree.route(old_id).is_none());
        assert!(
            input
                .accessibility_menu_entry(&scene, &layout, &trigger, &entry, true)
                .is_none()
        );
    }

    #[test]
    fn menu_popup_exposes_nested_items_and_rejects_replaced_rows() {
        let mut root = Node::new("Window", None);
        let mut bar = Node::new("MenuBar", None);
        let mut file = Node::new("Menu", Some("file"));
        file.properties
            .insert("title", Value::String("File".into()));
        let mut disabled = Node::new("MenuItem", None);
        disabled
            .properties
            .insert("text", Value::String("Import".into()));
        disabled.properties.insert("enabled", Value::Bool(false));
        let mut export = Node::new("Menu", None);
        export
            .properties
            .insert("title", Value::String("Export".into()));
        let mut png = Node::new("MenuItem", None);
        png.properties.insert("text", Value::String("PNG".into()));
        export.children.push(png);
        file.children = vec![disabled, export];
        bar.children.push(file);
        root.children.push(bar);
        let mut scene = Rc::new(root);
        let layout = Rc::new(layout(&scene, Size::new(420.0, 260.0), &mut Text));
        let mut input = Interaction::default();
        let trigger = input
            .accessibility_target(&scene, &layout, &[0, 0])
            .unwrap();
        assert!(input.open_menu_bar(&scene, &layout, trigger.clone()));
        let mut tree = Projection::default();
        let update = tree.update(&scene, &layout, &input, 1.0, None).unwrap();
        let (disabled, item) = named(&update, "Import");
        assert!(item.is_disabled());
        assert!(tree.route(disabled).is_none());
        let (export, item) = named(&update, "Export");
        assert_eq!(update.focus, export);
        assert!(item.supports_action(Action::Expand));
        let Some(Route::MenuEntry { entry, .. }) = tree.route(export) else {
            panic!("submenu route");
        };
        assert!(
            input
                .accessibility_menu_entry(&scene, &layout, &trigger, &entry, true)
                .is_none()
        );
        let update = tree.update(&scene, &layout, &input, 1.0, None).unwrap();
        let (png, item) = named(&update, "PNG");
        assert_eq!(update.focus, png);
        assert!(item.supports_action(Action::Click));
        let (_, export_item) = update.nodes.iter().find(|(id, _)| *id == export).unwrap();
        assert_eq!(export_item.label(), Some("Export"));
        assert_eq!(export_item.children().len(), 1);

        let Some(Route::MenuEntry { entry, .. }) = tree.route(png) else {
            panic!("command route");
        };
        let replacement =
            &mut Rc::make_mut(&mut scene).children[0].children[0].children[1].children[0];
        replacement
            .properties
            .insert("text", Value::String("JPEG".into()));
        assert!(
            input
                .accessibility_menu_entry(&scene, &layout, &trigger, &entry, true)
                .is_none()
        );
        let update = tree.update(&scene, &layout, &input, 1.0, None).unwrap();
        assert!(tree.route(png).is_none());
        let (jpeg, _) = named(&update, "JPEG");
        let Some(Route::MenuEntry { entry, .. }) = tree.route(jpeg) else {
            panic!("replacement route");
        };
        let action = input
            .accessibility_menu_entry(&scene, &layout, &trigger, &entry, true)
            .unwrap();
        assert_eq!(action.path, [0, 0, 1, 0]);
        assert!(!input.menu_open());
    }
}
