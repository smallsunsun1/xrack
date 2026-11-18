//! Project the prepared XRack scene into the native accessibility tree.
use crate::accessibility_text::{
    EditorState, TextDocument, accessible_text_value, project_styled_text, project_text,
    supports_ranges,
};
use accesskit::{
    Action, ActionData, CustomAction, HasPopup, Invalid, Live, Node as AccessibleNode, NodeId,
    Role, ScrollUnit, SortDirection, Toggled, TreeId, TreeInfo, TreeUpdate,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    rc::Rc,
};
use xrack_layout::{
    CalendarGeometry, Interaction, LayoutNode, MenuEntry, MenuPanel, NavigationKey, Rect,
    SearchGeometry, SplitHandleRange, Target, TextPosition, swipe_view_selection,
};
use xrack_renderer_femtovg::TextSystem;
use xrack_runtime::{
    CalendarAction, CalendarState, CheckState, ControlAction, DecimalSpinRange, DialogRole,
    IndicatorState, InputType, ListMetrics, Node, Orientation, RangeAction, RowKey, ScrollBarRange,
    SliderInterval, SliderRange, SpinRange, StandardButtonKind, SwipeSide, Value, button_text,
    choice_identity, combo_editable, combo_text, group_index, indicator_state, is_calendar_control,
    is_spin_box, is_window_overlay, spin_editable, text_input_value,
};

pub(crate) const EDIT_CELL_ACTION: i32 = 10;
pub(crate) const SAVE_CELL_ACTION: i32 = 11;
pub(crate) const CANCEL_CELL_ACTION: i32 = 12;

const ROOT: NodeId = NodeId(0);
const SCROLL_DIRECTIONS: [Action; 4] = [
    Action::ScrollUp,
    Action::ScrollDown,
    Action::ScrollLeft,
    Action::ScrollRight,
];

pub(crate) const TABLE_ROW_MOVE_ACTIONS: [(i32, NavigationKey, &str); 2] = [
    (0, NavigationKey::Up, "Move row up"),
    (1, NavigationKey::Down, "Move row down"),
];

fn overlay_row(scene: &Node, path: &[usize]) -> Option<(RowKey, usize)> {
    let mut node = scene;
    let mut row = None;
    for (depth, &index) in path.iter().enumerate() {
        node = node.child_at(index)?;
        if let Some(key) = node.routing_row_key() {
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
    SplitHandle {
        target: Target,
        trailing_pane: usize,
        orientation: Orientation,
    },
    TextLink(Target),
    TabClose(Target),
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
    TableRowHeader(Target),
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
    id: Option<std::borrow::Cow<'static, str>>,
    option: Option<(usize, String)>,
    row_option: Option<RowKey>,
    text_run: Option<(usize, String)>,
    menu_session: Option<u64>,
    split_handle: Option<(usize, bool)>,
}

impl Key {
    fn element(node: &Node, path: &[usize], row: Option<(RowKey, usize)>) -> Self {
        let location = match row {
            Some((key, depth)) => Location::Row {
                view: if key.has_view_scope() {
                    Vec::new()
                } else {
                    path[..depth - 1].to_vec()
                },
                key,
                child: path[depth..].to_vec(),
            },
            None => Location::Static(path.to_vec()),
        };
        Self {
            location,
            kind: node.kind,
            id: node.element_id_owned(),
            option: node
                .standard_button_kind()
                .map(|kind| (0, kind.label().to_owned())),
            row_option: None,
            text_run: None,
            menu_session: None,
            split_handle: None,
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
    scroll_owners: BTreeMap<(NodeId, Action), NodeId>,
    documents: BTreeMap<NodeId, TextDocument>,
    next_id: u64,
}

impl Projection {
    pub fn clear(&mut self) {
        self.frame = None;
        self.ids.clear();
        self.routes.clear();
        self.scroll_owners.clear();
        self.documents.clear();
    }

    pub fn route(&self, id: NodeId) -> Option<Route> {
        self.routes.get(&id).cloned()
    }

    pub fn route_for_action(&self, id: NodeId, action: Action) -> Option<Route> {
        self.route(self.scroll_owners.get(&(id, action)).copied().unwrap_or(id))
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
        self.update_tree(
            scene,
            layout,
            input,
            scale,
            text_system,
            cfg!(target_os = "ios"),
        )
    }

    fn update_tree(
        &mut self,
        scene: &Rc<Node>,
        layout: &Rc<LayoutNode>,
        input: &Interaction,
        scale: f64,
        text_system: Option<&TextSystem>,
        ios: bool,
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
            ios,
            previous_ids: &self.ids,
            next_id: &mut self.next_id,
            ids: BTreeMap::new(),
            routes: BTreeMap::new(),
            scroll_owners: BTreeMap::new(),
            documents: BTreeMap::new(),
            nodes: Vec::new(),
            radio_groups: Vec::new(),
            lists: Vec::new(),
            ios_overlay: Vec::new(),
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
        if ios {
            builder.expose_ios_scroll_actions(&mut children);
        }
        if let Some(trigger) = &menu_trigger {
            children.extend(builder.menu_popup(&menu_panels, trigger));
        }
        children.extend(builder.radio_groups.iter().copied());
        if ios && builder.focus == ROOT {
            builder.focus_calendar_cancel_if_needed();
        }
        children.extend(builder.ios_overlay);
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
            scroll_owners,
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
        self.scroll_owners = scroll_owners;
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
    ios: bool,
    previous_ids: &'a BTreeMap<Key, NodeId>,
    next_id: &'a mut u64,
    ids: BTreeMap<Key, NodeId>,
    routes: BTreeMap<NodeId, Route>,
    scroll_owners: BTreeMap<(NodeId, Action), NodeId>,
    documents: BTreeMap<NodeId, TextDocument>,
    nodes: Vec<(NodeId, AccessibleNode)>,
    radio_groups: Vec<NodeId>,
    lists: Vec<NodeId>,
    ios_overlay: Vec<NodeId>,
    focus: NodeId,
}

impl Builder<'_> {
    fn focus_calendar_cancel_if_needed(&mut self) {
        let Some(target) = self.input.focused.as_ref() else {
            return;
        };
        if !target
            .node_in(self.scene)
            .is_some_and(|node| is_calendar_control(node.kind))
            || self
                .input
                .dialog_layers(self.layout)
                .next_back()
                .is_none_or(|path| !target.path.starts_with(path))
        {
            return;
        }
        // A bounded calendar can have neither an enabled date nor a month button.
        if let Some((id, _)) = self.nodes.iter().find(|(id, node)| {
            node.supports_action(Action::Click)
                && matches!(self.routes.get(id), Some(Route::Element { target, .. })
                    if self.input.dialog_button(self.scene, self.layout, target)
                        .is_some_and(|input| input.action == ControlAction::Dialog(DialogRole::Reject)))
        }) {
            self.focus = *id;
        }
    }

    #[expect(
        clippy::expect_used,
        reason = "accessibility IDs must remain distinct across rebuilds so stale actions cannot target new nodes"
    )]
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
        key.id = Some(member.group.key.clone());
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
        if node.kind == "SplitHandle" {
            return Vec::new();
        }
        let mut headings = item.list_section_headers().iter().peekable();
        let mut children = Vec::new();
        for index in xrack_layout::child_reading_order(node) {
            while let Some(heading) = headings.next_if(|heading| {
                node.list
                    .is_some_and(|list| heading.first_row <= list.first_row + index)
            }) {
                children.extend(self.list_section_heading(node, item, path, row, heading));
            }
            let Some((child, layout)) = node.child_at(index).zip(item.child_at(index)) else {
                continue;
            };
            if is_window_overlay(child.kind)
                || child.kind == "CalendarCells"
                || child.is_control_part()
            {
                continue;
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
            if self.ios
                && node.kind == "SwipeDelegate"
                && matches!(child.kind, "SwipeLeft" | "SwipeRight")
            {
                self.ios_overlay.extend(id);
            } else {
                children.extend(id);
            }
        }
        for heading in headings {
            children.extend(self.list_section_heading(node, item, path, row, heading));
        }
        children
    }

    fn list_section_heading(
        &mut self,
        node: &Node,
        layout: &LayoutNode,
        path: &[usize],
        row: Option<(RowKey, usize)>,
        heading: &xrack_layout::ListSectionHeader,
    ) -> Option<NodeId> {
        let rect = layout.visible_window_bounds(heading.bounds.intersect(layout.bounds));
        if rect.width <= 0.0 || rect.height <= 0.0 {
            return None;
        }
        let mut key = Key::element(node, path, row);
        key.kind = "ListSectionHeading";
        key.row_option = Some(heading.key);
        let id = self.id(key);
        let mut accessible = AccessibleNode::new(Role::Heading);
        let label = heading
            .content()
            .and_then(|(node, _)| node.text("accessible-name"))
            .filter(|label| !label.is_empty())
            .unwrap_or(&heading.label);
        accessible.set_label(label);
        accessible.set_bounds(bounds(rect, self.scale));
        self.nodes.push((id, accessible));
        Some(id)
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
        if !has_bounds && !item.allows_overflow() && !(node.kind == "Tab" && item.visible)
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
            .routing_row_key()
            .filter(|_| !path.is_empty())
            .map(|key| (key, path.len()))
            .or(row);
        let split = (node.kind == "SplitHandle")
            .then(|| self.layout.split_handle_range(path))
            .flatten();
        let role = match (self.ios, node.kind) {
            (true, "Tumbler") => Role::SpinButton,
            // AccessKit maps SpinButton to UIKit's Adjustable trait.
            (true, "PathView")
                if item
                    .tumbler
                    .as_ref()
                    .is_some_and(|wheel| wheel.selected.is_some()) =>
            {
                Role::SpinButton
            }
            // UIKit's AccessKit adapter derives Adjustable from the slider role.
            (true, "SplitHandle")
                if item.enabled
                    && split.is_some_and(|range| range.can_increase() || range.can_decrease()) =>
            {
                Role::Slider
            }
            _ => role(node, view),
        };
        let primary_pane = split.and_then(|split| {
            let mut pane_path = path.clone();
            *pane_path.last_mut()? = split.leading_pane;
            Some((self.scene.at_path(&pane_path)?, pane_path))
        });
        let child_view = match node.kind {
            "ListView" | "GridView" | "StandardListView" | "TreeView" | "TableView" => {
                Some(node.kind)
            }
            _ => view,
        };
        let first_child_node = self.nodes.len();
        let mut children = self.children(node, item, path, row, child_view);
        let ios_context_container = self.ios
            && node.kind == "ContextMenuArea"
            && self.nodes[first_child_node..]
                .iter()
                .any(|(_, child)| child.supports_action(Action::ShowContextMenu));
        if node.kind == "StyledText" {
            children.extend(self.text_links(node, item, path, row));
        }
        let ios_tree_controls = self.ios
            && role == Role::TreeItem
            && self.nodes[first_child_node..].iter().any(|(_, child)| {
                [Action::Focus, Action::Click, Action::CustomAction]
                    .into_iter()
                    .any(|action| child.supports_action(action))
            });
        let label = node
            .text("accessible-name")
            .or_else(|| {
                primary_pane
                    .as_ref()
                    .and_then(|(pane, _)| pane.text("accessible-name"))
            })
            .or_else(|| match role {
                Role::Button | Role::CheckBox | Role::Switch | Role::RadioButton => {
                    Some(button_text(node))
                }
                Role::Tab if node.kind == "TabButton" => node.text("text"),
                Role::Group if node.kind == "Calendar" => Some("Calendar"),
                Role::Group if self.ios && node.kind == "SwipeView" => Some("Pages"),
                Role::Group if node.kind == "MonthGrid" => Some("Month grid"),
                Role::Group if node.kind == "DayOfWeekRow" => Some("Days of week"),
                Role::Group if node.kind == "WeekNumberColumn" => Some("Week numbers"),
                Role::Group if node.kind == "StyledText" => node.text("text"),
                Role::Group if matches!(node.kind, "GroupBox" | "Expander" | "Page") => {
                    node.text("title")
                }
                Role::ProgressIndicator if node.kind == "BusyIndicator" => Some("Loading"),
                Role::ProgressIndicator => Some("Progress"),
                Role::ScrollBar => Some("Scroll bar"),
                Role::SpinButton if node.kind == "PathView" => Some("Path selection"),
                Role::ListBoxOption if view == Some("StandardListView") => {
                    node.children.first().and_then(|label| label.text("text"))
                }
                Role::ListItem if view == Some("GridView") => node
                    .children
                    .first()
                    .and_then(|delegate| delegate.text("accessible-name")),
                Role::Dialog | Role::MenuItem | Role::RadioGroup | Role::Tab => node.text("title"),
                Role::TreeItem if self.ios => visible_tree_label(node, item),
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
            && node.kind != "Expander"
        {
            return None;
        }

        let mut key = Key::element(node, path, row);
        // A divider can keep its scene slot when its neighbor or axis changes.
        key.split_handle = split.map(|split| {
            (
                split.trailing_pane,
                split.orientation == Orientation::Vertical,
            )
        });
        let id = self.id(key);
        let mut accessible = AccessibleNode::new(role);
        if matches!(node.kind, "StandardListView" | "ListView" | "GridView")
            && node.table_selection.is_some()
        {
            accessible.set_multiselectable();
        }
        if let Some(group) = self.attached_button_group(node, path, row) {
            accessible.set_member_of(group);
        }
        if has_bounds {
            accessible.set_bounds(bounds(rect, self.scale));
        }
        if let Some(label) = label {
            accessible.set_label(label);
        } else if let Some(split) = split {
            accessible.set_label(format!("Pane divider {}", split.leading_pane + 1));
        }
        if let Some(description) = description {
            accessible.set_description(description);
        }
        if !item.enabled {
            accessible.set_disabled();
        }
        if self.ios
            && node.kind == "Drawer"
            && let Some(label) = label
        {
            let title = item.visible_window_bounds(Rect {
                height: item.dialog_title_height(node),
                ..item.bounds
            });
            if title.width > 0.0 && title.height > 0.0 {
                let mut key = Key::element(node, path, row);
                key.kind = "DrawerTitle";
                let title_id = self.id(key);
                let mut heading = AccessibleNode::new(Role::Heading);
                heading.set_label(label);
                heading.set_bounds(bounds(title, self.scale));
                if let Some(description) = description {
                    heading.set_description(description);
                }
                accessible.clear_label();
                accessible.clear_description();
                self.nodes.push((title_id, heading));
                children.insert(0, title_id);
            }
        }
        if is_calendar_control(node.kind) {
            let calendar = CalendarState::from_node(node);
            if !self.ios && (!calendar.fixed_month || calendar.allows(calendar.selected)) {
                accessible.set_value(calendar.selected.to_string());
            }
            children.extend(self.calendar_parts(node, item, path, row));
        } else if matches!(node.kind, "DayOfWeekRow" | "WeekNumberColumn") {
            children.extend(self.calendar_labels(node, item, path, row));
        }
        let mut has_row_header = false;
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
            Role::ListBox | Role::List
                if matches!(node.kind, "ListView" | "StandardListView" | "GridView") =>
            {
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
            Role::Tree => {
                if let Some(list) = node.list {
                    accessible.set_size_of_set(list.row_count);
                }
            }
            Role::ListItem | Role::TreeItem | Role::Row => {
                accessible.set_selected(node.boolean("selected", false));
                if let Some((&slot, parent)) = path.split_last()
                    && let Some(list) = self.scene.at_path(parent).and_then(|view| view.list)
                {
                    if role == Role::Row && view == Some("TableView") {
                        accessible.set_row_index(list.first_row + slot);
                        if let Some(header) =
                            self.table_row_header(node, path, row, list.first_row + slot)
                        {
                            children.insert(0, header);
                            has_row_header = true;
                        }
                    } else if role == Role::TreeItem
                        || role == Role::ListItem && view == Some("ListView")
                    {
                        accessible.set_position_in_set(list.first_row + slot);
                    }
                }
                if node.boolean("tree-has-children", false) {
                    let expanded = node.boolean("tree-expanded", false);
                    accessible.set_expanded(expanded);
                    if self.ios && role == Role::TreeItem {
                        accessible.set_value(if expanded { "Expanded" } else { "Collapsed" });
                    }
                }
            }
            Role::Table => {
                if let Some(list) = node.list {
                    accessible.set_row_count(list.row_count);
                }
                if let Some(geometry) = &item.table {
                    accessible.set_column_count(geometry.visible_column_count());
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
                        .and_then(|geometry| geometry.visible_column_index(*column))
                    {
                        accessible.set_column_index(visual);
                    }
                }
                accessible.set_selected(node.boolean("table-selected", false));
                if node.table_cell_editor.is_some() {
                    if self.input.table_cell_editing(path) {
                        accessible.set_value(self.input.editor_value(node).into_owned());
                        if self.input.table_cell_edit_invalid(node, path) {
                            accessible.set_invalid(Invalid::True);
                        }
                    } else {
                        accessible.set_value(node.text("text").unwrap_or_default());
                    }
                }
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
                if node.text_read_only() {
                    accessible.set_read_only();
                }
                if node.kind == "SearchField" {
                    accessible.set_has_popup(HasPopup::Listbox);
                    accessible.set_expanded(self.input.choice_is_open_for(path));
                }
            }
            Role::ComboBox | Role::EditableComboBox => {
                if !node.is_password() {
                    accessible.set_value(if combo_editable(node) {
                        text_input_value(node)
                    } else {
                        combo_text(node)
                    });
                }
                accessible.set_expanded(self.input.choice_is_open_for(path));
            }
            Role::ProgressIndicator => {
                let bounds = (node.kind == "ProgressIndicator")
                    .then(|| xrack_runtime::ProgressRange::from_node(node));
                accessible.set_min_numeric_value(bounds.map_or(0.0, |bounds| bounds.minimum));
                accessible.set_max_numeric_value(bounds.map_or(1.0, |bounds| bounds.maximum));
                match indicator_state(node) {
                    Some(IndicatorState::Determinate(value)) => {
                        accessible.set_numeric_value(bounds.map_or(value, |bounds| bounds.value))
                    }
                    Some(IndicatorState::Indeterminate) => accessible.set_busy(),
                    _ => {}
                }
            }
            Role::ListBox | Role::SpinButton if matches!(node.kind, "Tumbler" | "PathView") => {
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
                    if self.ios && node.scroll_target.is_some() {
                        // Format viewport offsets at their original layout precision.
                        accessible.set_value(bar.scroll.offset.to_string());
                    }
                    if !node.boolean("interactive", true) || maximum == 0.0 {
                        accessible.set_read_only();
                    }
                }
            }
            Role::Splitter | Role::Slider if node.kind == "SplitHandle" => {
                if let Some(split) = split {
                    if role == Role::Slider {
                        accessible.set_role_description("splitter");
                    }
                    // The splitter itself runs perpendicular to the resize axis.
                    accessible.set_orientation(match split.orientation {
                        Orientation::Horizontal => accesskit::Orientation::Vertical,
                        Orientation::Vertical => accesskit::Orientation::Horizontal,
                    });
                    accessible.set_numeric_value(split.value);
                    accessible.set_min_numeric_value(split.minimum);
                    accessible.set_max_numeric_value(split.maximum);
                    accessible.set_numeric_value_step(SplitHandleRange::STEP);
                    if !item.enabled {
                        accessible.set_read_only();
                    }
                    if let Some((pane, path)) = &primary_pane
                        && let Some(&id) =
                            self.ids
                                .get(&Key::element(pane, path, overlay_row(self.scene, path)))
                    {
                        accessible.set_controls(vec![id]);
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
                let (min, max, value) = if node.kind == "RangeThumb" {
                    let index = path.last().copied().unwrap_or(0).min(1);
                    accessible.set_label(if index == 0 {
                        "Lower value"
                    } else {
                        "Upper value"
                    });
                    let [lower, upper] = SliderInterval::from_node(range_node).values();
                    if index == 0 {
                        (min, upper, lower)
                    } else {
                        (lower, max, upper)
                    }
                } else {
                    (min, max, SliderRange::from_node(node).value())
                };
                accessible.set_min_numeric_value(min);
                accessible.set_max_numeric_value(max);
                accessible.set_numeric_value(value);
                accessible
                    .set_numeric_value_step(range_node.number("step").unwrap_or(1.0).max(0.0));
            }
            _ => {}
        }

        let ios_cell_editing = self.ios && self.input.table_cell_editing(path);
        if ios_cell_editing {
            accessible.set_role(Role::TextInput);
            accessible.set_value(self.input.editor_value(node).as_ref());
            if self.input.table_cell_edit_invalid(node, path) {
                accessible.set_invalid(Invalid::True);
            }
            if node.text("accessible-name").is_none() {
                let column_title = if let [table @ .., _, 0, column] = path.as_slice() {
                    self.scene
                        .at_path(table)
                        .and_then(|node| node.table_header.as_ref())
                        .and_then(|header| header.columns.get(*column))
                        .and_then(|column| column.text("title"))
                        .filter(|title| !title.is_empty())
                } else {
                    None
                };
                accessible.set_label(
                    column_title
                        .map_or_else(|| "Cell editor".into(), |title| format!("{title} editor")),
                );
            }
        }

        let text_editor = if supports_ranges(node) || ios_cell_editing {
            let editor = self.input.displayed_editor(node, path);
            let scale = self.scale;
            let text_system = self.text_system;
            let input = self.input;
            let allocate = |start, value: &str| {
                let mut key = Key::element(node, path, row);
                key.text_run = Some((start, value.into()));
                self.id(key)
            };
            let (document, runs) = if node.selectable_styled_text() {
                project_styled_text(node, item, path, scale, allocate)
            } else {
                project_text(
                    node,
                    item.bounds,
                    path,
                    input,
                    (
                        scale,
                        item.world_transform(),
                        item.layout_direction_override(),
                        item.clip,
                    ),
                    text_system,
                    allocate,
                )
            };
            children.extend(runs.iter().map(|(id, _)| *id));
            self.nodes.extend(runs);
            if let Some(editor) = editor {
                let text = accessible_text_value(node, input, path);
                accessible.set_text_selection(document.selection(editor, &text));
            }
            self.documents.insert(id, document);
            editor.is_none_or(|edit| edit.preedit.is_empty())
        } else {
            false
        };
        if node.group_checkable() || node.kind == "Expander" {
            if self.ios {
                // The header carries the group name and UIKit traverses the controls below it.
                accessible.clear_label();
                accessible.clear_description();
            }
            if let Some(toggle) = self.group_toggle(node, item, path, row) {
                children.insert(0, toggle);
            }
        }
        // UIKit traverses controls inside these containers individually.
        let ios_container = self.ios
            && (matches!(role, Role::Dialog | Role::Table | Role::TabList)
                || node.kind == "SwipeView"
                || is_calendar_control(node.kind)
                || ios_context_container
                || has_row_header)
            && !children.is_empty();
        let mut siblings = Vec::new();
        if !matches!(node.kind, "GroupBox" | "Expander")
            && (!self.input.menu_open() || node.kind == "Menu")
            && let Some(target) = self
                .input
                .accessibility_target(self.scene, self.layout, path)
        {
            if target.focusable && !ios_container {
                accessible.add_action(Action::Focus);
                if self
                    .input
                    .supports_context_menu(self.scene, self.layout, &target)
                {
                    accessible.add_action(Action::ShowContextMenu);
                }
            }
            if self.ios && xrack_runtime::editable_table_cell(node) {
                let actions: &[(i32, &str)] = if ios_cell_editing {
                    &[
                        (SAVE_CELL_ACTION, "Save edit"),
                        (CANCEL_CELL_ACTION, "Cancel edit"),
                    ]
                } else if self
                    .input
                    .focused
                    .as_ref()
                    .is_some_and(|focus| self.input.table_cell_editing(&focus.path))
                {
                    &[]
                } else {
                    &[(EDIT_CELL_ACTION, "Edit cell")]
                };
                if !actions.is_empty() {
                    accessible.add_action(Action::CustomAction);
                    for &(id, description) in actions {
                        accessible.push_custom_action(CustomAction {
                            id,
                            description: description.into(),
                        });
                    }
                }
            }
            if supports_click(node.kind) && !ios_container {
                accessible.add_action(Action::Click);
            }
            if role == Role::TreeItem {
                let expanded = node.boolean("tree-expanded", false);
                if self
                    .input
                    .tree_row_expansion(self.scene, self.layout, &target, !expanded)
                    .is_some()
                {
                    accessible.add_action(if expanded {
                        Action::Collapse
                    } else {
                        Action::Expand
                    });
                }
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
                    if self.ios {
                        self.ios_overlay.push(id);
                    } else {
                        children.push(id);
                    }
                }
            }
            if role == Role::Slider && split.is_none()
                || supports_text_value(node)
                || node.kind == "DoubleSpinBox" && spin_editable(node)
            {
                accessible.add_action(Action::SetValue);
            }
            if text_editor && target.focusable {
                accessible.add_action(Action::SetTextSelection);
            }
            if role == Role::Slider && split.is_none()
                || role == Role::SpinButton
                    && is_spin_box(node.kind)
                    && !node.boolean("read-only", false)
            {
                accessible.add_action(Action::Increment);
                accessible.add_action(Action::Decrement);
            }
            if role == Role::ScrollBar {
                accessible.add_action(Action::SetValue);
                if !self.ios {
                    accessible.add_action(Action::Increment);
                    accessible.add_action(Action::Decrement);
                } else if let (Some(value), Some(maximum), Some(step)) = (
                    accessible.numeric_value(),
                    accessible.max_numeric_value(),
                    accessible.numeric_value_step(),
                ) && maximum > 0.0
                    && step > 0.0
                    && !accessible.is_read_only()
                {
                    // The slider role exposes UIKit's Adjustable trait.
                    accessible.set_role(Role::Slider);
                    accessible.set_role_description("scroll bar");
                    if value < maximum {
                        accessible.add_action(Action::Increment);
                    }
                    if value > 0.0 {
                        accessible.add_action(Action::Decrement);
                    }
                }
            }
            scroll_actions(&mut accessible, item, self.scale);
            if self.ios && node.kind == "SwipeView" {
                for action in SCROLL_DIRECTIONS {
                    if swipe_scroll_selection(action, node, item).is_some() {
                        accessible.add_action(action);
                    }
                }
            }
            if !ios_container && self.input.focused.as_ref() == Some(&target) {
                self.focus = id;
            }
            if node.kind == "SearchField" {
                let parts = self.search_parts(node, item, &target, row);
                if self.ios && !parts.is_empty() {
                    // UIKit traverses the editor and its buttons as siblings.
                    let entry = SearchGeometry::for_layout(item).entry;
                    accessible.set_bounds(bounds(item.visible_window_bounds(entry), self.scale));
                    siblings = parts;
                } else {
                    children.extend(parts);
                }
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
                if !self.ios {
                    children.extend(self.tumbler_parts(node, item, &target, row));
                }
            }
            if matches!(node.kind, "ComboBox" | "SearchField")
                && self.input.choice_is_open_for(path)
                && let Some((options, highlight)) = self.choice_popup(node, &target, row)
            {
                if self.ios {
                    self.ios_overlay.extend(options.iter().copied());
                } else {
                    children.extend(options.iter().copied());
                }
                accessible.set_controls(options);
                if let Some(highlight) = highlight {
                    if node.kind == "SearchField" {
                        accessible.set_active_descendant(highlight);
                    } else {
                        self.focus = highlight;
                    }
                }
            }
            let route = if let Some(split) = split {
                accessible.add_action(Action::SetValue);
                if split.can_increase() {
                    accessible.add_action(Action::Increment);
                }
                if split.can_decrease() {
                    accessible.add_action(Action::Decrement);
                }
                Route::SplitHandle {
                    target,
                    trailing_pane: split.trailing_pane,
                    orientation: split.orientation,
                }
            } else {
                Route::Element {
                    target,
                    kind: node.kind,
                    standard_kind: node.standard_button_kind(),
                }
            };
            self.routes.insert(id, route);
        }
        if self.ios && role == Role::TabList {
            self.forward_tab_scroll(first_child_node, id, path, &accessible);
        }
        if let Some(close) = self.tab_close_button(node, item, path, row) {
            children.insert(0, close);
        }
        if self.ios
            && (matches!(role, Role::Table | Role::TabList | Role::Tab)
                || role == Role::Row && view == Some("TableView") && !has_row_header
                || ios_tree_controls)
        {
            // UIKit traverses the controls alongside their selectable container.
            siblings = std::mem::take(&mut children);
        }
        accessible.set_children(children);
        if matches!(
            node.kind,
            "ListView" | "GridView" | "StandardListView" | "TreeView"
        ) && node.list.is_some()
        {
            self.lists.push(id);
        }
        self.nodes.push((id, accessible));
        if siblings.is_empty() {
            return Some(id);
        }
        let mut key = Key::element(node, path, row);
        key.kind = match role {
            Role::Table => "TableGroup",
            Role::Row => "TableRowGroup",
            Role::TreeItem => "TreeRowGroup",
            Role::TabList => "TabListGroup",
            Role::Tab => "TabGroup",
            _ => "SearchFieldGroup",
        };
        let group_id = self.id(key);
        let mut group = AccessibleNode::new(Role::Group);
        siblings.insert(0, id);
        group.set_children(siblings);
        self.nodes.push((group_id, group));
        Some(group_id)
    }

    fn forward_tab_scroll(
        &mut self,
        first_child: usize,
        strip: NodeId,
        path: &[usize],
        accessible: &AccessibleNode,
    ) {
        let actions: Vec<_> = SCROLL_DIRECTIONS
            .into_iter()
            .filter(|&action| accessible.supports_action(action))
            .collect();
        if actions.is_empty() {
            return;
        }
        for (id, node) in &mut self.nodes[first_child..] {
            let target = match self.routes.get(id) {
                Some(Route::Element { target, .. }) if node.role() == Role::Tab => target,
                Some(Route::TabClose(target)) => target,
                _ => continue,
            };
            if target
                .path
                .split_last()
                .is_some_and(|(_, parent)| parent == path)
            {
                for &action in &actions {
                    node.add_action(action);
                    self.scroll_owners.insert((*id, action), strip);
                }
            }
        }
    }

    fn expose_list_items(&mut self, window: &mut Vec<NodeId>) {
        if self.lists.is_empty() {
            return;
        }
        // UIKit accessibility elements with their own focus or activation
        // actions are leaves. Publish visible items as window children and
        // keep the list as the owner of scrolling.
        let mut nodes: BTreeMap<_, _> = std::mem::take(&mut self.nodes).into_iter().collect();
        let lists = std::mem::take(&mut self.lists);
        let mut items_by_list = BTreeMap::new();
        for &id in &lists {
            let Some(list) = nodes.get_mut(&id) else {
                continue;
            };
            let children = list.children().to_vec();
            let size = list.size_of_set();
            list.set_children(Vec::new());
            let mut items = Vec::new();
            for child in children {
                self.expose_list_child(&mut nodes, child, &mut items);
            }
            if let Some(list) = nodes.get_mut(&id) {
                list.set_controls(items.clone());
            }
            for &item in &items {
                let Some(node) = nodes.get_mut(&item) else {
                    continue;
                };
                if let Some(size) = size
                    && node.position_in_set().is_some()
                {
                    node.set_size_of_set(size);
                }
            }
            if !items.is_empty() {
                items_by_list.insert(id, items);
            }
        }
        let parents: BTreeMap<_, _> = nodes
            .iter()
            .flat_map(|(&parent, node)| node.children().iter().map(move |&child| (child, parent)))
            .collect();
        let mut list_ancestors = BTreeSet::new();
        for id in items_by_list.keys() {
            let mut current = *id;
            while let Some(&parent) = parents.get(&current) {
                if !list_ancestors.insert(parent) {
                    break;
                }
                current = parent;
            }
        }
        for id in std::mem::take(window) {
            place_list_items(id, &mut nodes, &mut items_by_list, &list_ancestors, window);
        }
        self.routes.retain(|id, _| nodes.contains_key(id));
        self.scroll_owners
            .retain(|(source, _), target| nodes.contains_key(source) && nodes.contains_key(target));
        self.nodes = nodes.into_iter().collect();
    }

    fn expose_ios_scroll_actions(&mut self, roots: &mut Vec<NodeId>) {
        // UIKit starts scrolling at the focused element, including static text.
        // Walk the hierarchy before list items are reparented for UIKit.
        let by_id: BTreeMap<_, _> = self.nodes.iter().map(|(id, node)| (*id, node)).collect();
        let mut pending: Vec<_> = roots.iter().map(|&id| (id, [None; 4])).collect();
        while let Some((id, mut parents)) = pending.pop() {
            let Some(node) = by_id.get(&id) else {
                continue;
            };
            let viewport = matches!(self.routes.get(&id), Some(Route::Element { kind, .. })
                if !matches!(*kind, "TabBar" | "TabWidget"));
            for (action, parent) in SCROLL_DIRECTIONS.into_iter().zip(&mut parents) {
                if node.supports_action(action) {
                    if viewport && !self.scroll_owners.contains_key(&(id, action)) {
                        *parent = Some(id);
                    }
                } else if let Some(owner) = *parent {
                    self.scroll_owners.insert((id, action), owner);
                }
            }
            pending.extend(node.children().iter().map(|&child| (child, parents)));
        }

        self.expose_list_items(roots);
        for (id, node) in &mut self.nodes {
            for action in SCROLL_DIRECTIONS {
                if self.scroll_owners.contains_key(&(*id, action)) {
                    node.add_action(action);
                }
            }
        }
    }

    fn expose_list_child(
        &mut self,
        nodes: &mut BTreeMap<NodeId, AccessibleNode>,
        id: NodeId,
        exposed: &mut Vec<NodeId>,
    ) {
        let Some(node) = nodes.get_mut(&id) else {
            return;
        };
        if !matches!(node.role(), Role::ListItem | Role::Group) {
            exposed.push(id);
            return;
        }
        let children = node.children().to_vec();
        let position = node.position_in_set();
        let selected = node.is_selected();
        let keep = node.label().is_some() || node.description().is_some();
        if keep {
            node.set_children(Vec::new());
            exposed.push(id);
        } else {
            nodes.remove(&id);
        }
        let first = exposed.len();
        for child in children {
            self.expose_list_child(nodes, child, exposed);
        }
        if !keep && self.focus == id {
            self.focus = exposed.get(first).copied().unwrap_or(ROOT);
        }
        if !keep && let Some(&first) = exposed.get(first) {
            if let Some(node) = nodes.get_mut(&first) {
                if let Some(position) = position {
                    node.set_position_in_set(position);
                }
                if selected == Some(true) {
                    node.set_selected(true);
                }
            }
        }
    }

    fn tab_close_button(
        &mut self,
        node: &Node,
        item: &LayoutNode,
        path: &[usize],
        row: Option<(RowKey, usize)>,
    ) -> Option<NodeId> {
        let rect = item.visible_window_bounds(item.tab_close?);
        if rect.width <= 0.0 || rect.height <= 0.0 {
            return None;
        }
        let mut key = Key::element(node, path, row);
        key.kind = "TabClose";
        let id = self.id(key);
        let mut close = AccessibleNode::new(Role::Button);
        let title = node
            .text("accessible-name")
            .or_else(|| node.text(if node.kind == "Tab" { "title" } else { "text" }))
            .filter(|title| !title.is_empty());
        close.set_label(
            title.map_or_else(|| "Close tab".to_owned(), |title| format!("Close {title}")),
        );
        close.set_bounds(bounds(rect, self.scale));
        if !item.enabled {
            close.set_disabled();
        } else if let Some(target) =
            self.input
                .accessibility_tab_close_target(self.scene, self.layout, path)
        {
            close.add_action(Action::Click);
            self.routes.insert(id, Route::TabClose(target));
        }
        self.nodes.push((id, close));
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
            .visible_columns()
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
                let next_sort = header.next_sort(index);
                let current_sort = header.sort.filter(|sort| sort.column == index);
                let mut part = AccessibleNode::new(if self.ios && next_sort.is_some() {
                    Role::Button
                } else {
                    Role::ColumnHeader
                });
                part.set_bounds(bounds(item.visible_window_bounds(rect), self.scale));
                part.set_column_index(visual);
                if let Some(title) = column.text("title") {
                    part.set_label(title);
                }
                if let Some(sort) = current_sort {
                    part.set_sort_direction(if sort.descending {
                        SortDirection::Descending
                    } else {
                        SortDirection::Ascending
                    });
                }
                if self.ios && (next_sort.is_some() || current_sort.is_some()) {
                    part.set_value(match current_sort {
                        Some(sort) if sort.descending => "Sorted descending",
                        Some(_) => "Sorted ascending",
                        None => "Unsorted",
                    });
                }
                if !item.enabled {
                    part.set_disabled();
                }
                if let (Some(target), Some(next_sort)) = (&target, next_sort) {
                    part.add_action(Action::Click);
                    if self.ios {
                        part.set_description(if next_sort.descending {
                            "Sort descending"
                        } else {
                            "Sort ascending"
                        });
                    }
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

    fn table_row_header(
        &mut self,
        node: &Node,
        path: &[usize],
        row: Option<(RowKey, usize)>,
        index: usize,
    ) -> Option<NodeId> {
        let (&slot, parent) = path.split_last()?;
        let table = self.scene.at_path(parent)?;
        let item = self.layout.at_path(parent)?;
        let geometry = item.table.as_ref()?;
        let label = node.text("row-header-text")?;
        let rect = item.visible_window_bounds(
            geometry
                .row_header_cell(item.children.get(slot)?.bounds)
                .intersect(geometry.row_header),
        );
        if rect.width <= 0.0 || rect.height <= 0.0 {
            return None;
        }
        let mut key = Key::element(node, path, row);
        key.kind = "TableRowHeader";
        let id = self.id(key);
        let mut part = AccessibleNode::new(Role::RowHeader);
        part.set_bounds(bounds(rect, self.scale));
        part.set_row_index(index);
        part.set_label(label);
        if let Some((row, view)) = node.row_key.zip(table.table_selection.as_deref()) {
            part.set_selected(view.row_header_selected(row));
        }
        if !item.enabled {
            part.set_disabled();
        } else if let Some(target) = self
            .input
            .accessibility_target(self.scene, self.layout, path)
        {
            part.add_action(Action::Click);
            for (id, direction, description) in TABLE_ROW_MOVE_ACTIONS {
                if self
                    .input
                    .table_row_header_move(self.scene, self.layout, &target, direction)
                    .is_some()
                {
                    part.add_action(Action::CustomAction);
                    part.push_custom_action(CustomAction {
                        id,
                        description: description.into(),
                    });
                }
            }
            self.routes.insert(id, Route::TableRowHeader(target));
        }
        self.nodes.push((id, part));
        Some(id)
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
        let expander = node.kind == "Expander";
        key.kind = if expander {
            "ExpanderHeader"
        } else {
            "GroupBoxToggle"
        };
        let id = self.id(key);
        let mut toggle = AccessibleNode::new(match (expander, self.ios) {
            (true, true) => Role::DisclosureTriangle,
            (true, false) => Role::Button,
            (false, _) => Role::CheckBox,
        });
        toggle.set_bounds(bounds(item.visible_window_bounds(rect), self.scale));
        toggle.set_label(
            node.text("accessible-name")
                .or_else(|| node.text("title"))
                .filter(|label| !label.is_empty())
                .unwrap_or(if expander { "Details" } else { "Enable group" }),
        );
        if self.ios
            && let Some(description) = node.text("accessible-description")
        {
            toggle.set_description(description);
        }
        if expander {
            let expanded = !node.expander_collapsed();
            toggle.set_expanded(expanded);
            if self.ios {
                toggle.set_value(if expanded { "Expanded" } else { "Collapsed" });
            }
        } else {
            toggle.set_toggled(if node.boolean("checked", true) {
                Toggled::True
            } else {
                Toggled::False
            });
        }
        if !item.enabled {
            toggle.set_disabled();
        } else if !self.input.menu_open()
            && let Some(target) = self
                .input
                .accessibility_target(self.scene, self.layout, path)
        {
            toggle.add_action(Action::Click);
            if expander {
                toggle.add_action(if node.expander_collapsed() {
                    Action::Expand
                } else {
                    Action::Collapse
                });
            }
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
        let geometry = CalendarGeometry::for_layout(node, item);
        let target = (!self.input.menu_open())
            .then(|| {
                self.input
                    .accessibility_target(self.scene, self.layout, path)
            })
            .flatten();
        let focused = self.ios
            && target
                .as_ref()
                .is_some_and(|target| self.input.focused.as_ref() == Some(target));
        let cursor = if focused { geometry.cursor() } else { None };
        let mut day_focus = None;
        let mut month_focus = None;
        let navigation = geometry
            .navigation()
            .into_iter()
            .zip(["Previous month", "Next month"])
            .map(|((rect, action), label)| {
                (rect, action, label.to_owned(), None, false, None, None)
            });
        let days = geometry.days().map(|day| {
            let range = match (
                day.highlight.start,
                day.highlight.end,
                day.highlight.in_range,
            ) {
                (true, true, _) => Some("Range start and end"),
                (true, false, _) => Some("Range start"),
                (false, true, _) => Some("Range end"),
                (false, false, true) => Some("In range"),
                _ => None,
            };
            (
                day.bounds,
                day.enabled.then_some(geometry.day_action(day.date)),
                day.date.to_string(),
                Some(day.selected),
                day.today,
                range,
                node.calendar_cells()
                    .and_then(|cells| cells.get(day.slot))
                    .and_then(|cell| cell.text("accessible-description"))
                    .filter(|description| !description.is_empty()),
            )
        });
        let children = navigation
            .chain(days)
            .enumerate()
            .filter_map(
                |(index, (rect, action, label, selected, today, range, description))| {
                    let rect = rect.intersect(item.clip);
                    if rect.width <= 0.0 || rect.height <= 0.0 {
                        return None;
                    }
                    let mut key = Key::element(node, path, row);
                    let identity = match action {
                        Some(CalendarAction::Browse(month)) => month.to_string(),
                        _ => label.clone(),
                    };
                    key.option = Some((if node.kind == "MonthGrid" { 0 } else { index }, identity));
                    let id = self.id(key);
                    let mut part = AccessibleNode::new(if selected.is_some() {
                        Role::RadioButton
                    } else {
                        Role::Button
                    });
                    part.set_label(label);
                    let descriptions = [today.then_some("Today"), range, description];
                    if descriptions.iter().any(Option::is_some) {
                        part.set_description(
                            descriptions
                                .into_iter()
                                .flatten()
                                .collect::<Vec<_>>()
                                .join(". "),
                        );
                    }
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
                        if focused {
                            match action {
                                CalendarAction::Select(day) | CalendarAction::Activate(day)
                                    if cursor == Some(day) =>
                                {
                                    day_focus = Some(id)
                                }
                                CalendarAction::Browse(_) => month_focus = Some(id),
                                _ => {}
                            }
                        }
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
                },
            )
            .collect();
        if let Some(id) = day_focus.or(month_focus) {
            self.focus = id;
        }
        children
    }

    fn calendar_labels(
        &mut self,
        node: &Node,
        item: &LayoutNode,
        path: &[usize],
        row: Option<(RowKey, usize)>,
    ) -> Vec<NodeId> {
        let geometry = CalendarGeometry::for_layout(node, item);
        let days = geometry
            .weekdays(node)
            .map(|(rect, name)| (rect, name.to_owned()));
        let weeks = geometry.weeks().map(|week| {
            (
                week.bounds,
                format!("Week {} of {}", week.number, week.year),
            )
        });
        days.chain(weeks)
            .enumerate()
            .filter_map(|(index, (rect, label))| {
                let rect = item.visible_window_bounds(rect);
                if rect.width <= 0.0 || rect.height <= 0.0 {
                    return None;
                }
                let mut key = Key::element(node, path, row);
                key.option = Some((index, label.clone()));
                let id = self.id(key);
                let mut part = AccessibleNode::new(Role::Label);
                part.set_value(label);
                part.set_bounds(bounds(rect, self.scale));
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
                key.kind = if node.kind == "PathView" {
                    "PathViewOption"
                } else {
                    "TumblerOption"
                };
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
        SearchGeometry::for_layout(item)
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
                part.set_label(match action {
                    ControlAction::ClearSearch => "Clear search",
                    _ if self.ios => "Submit search",
                    _ => "Search",
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
    ) -> Option<(Vec<NodeId>, Option<NodeId>)> {
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
        // UIKit treats the combo box as a leaf. Its popup rows are exposed
        // directly under the window so each row is reachable through UIKit.
        #[cfg(not(target_os = "ios"))]
        let options = {
            let mut key = Key::element(node, &target.path, row);
            key.kind = "ChoicePopup";
            let id = self.id(key);
            let mut popup = AccessibleNode::new(Role::ListBox);
            popup.set_bounds(bounds(menu.bounds, self.scale));
            popup.set_children(options);
            self.nodes.push((id, popup));
            vec![id]
        };
        Some((options, highlight))
    }

    fn menu_popup(&mut self, panels: &[MenuPanel<'_>], trigger: &Target) -> Vec<NodeId> {
        let row = (1..=trigger.path.len()).rev().find_map(|depth| {
            self.scene
                .at_path(&trigger.path[..depth])?
                .row_key
                .map(|key| (key, depth))
        });
        let mut nested: Option<(&[usize], NodeId)> = None;
        let mut focus = None;
        #[cfg(target_os = "ios")]
        let mut exposed_panels = Vec::new();
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
                        #[cfg(not(target_os = "ios"))]
                        accessible.set_children(vec![child]);
                        #[cfg(target_os = "ios")]
                        accessible.set_controls(vec![child]);
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
            #[cfg(not(target_os = "ios"))]
            popup.set_children(rows);
            // UIKit treats the actionable panel as a leaf. Its rows remain
            // independent window children so they can be reached directly.
            #[cfg(target_os = "ios")]
            exposed_panels.push((id, rows));
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
        if let Some(focus) = focus {
            self.focus = focus;
        }
        #[cfg(target_os = "ios")]
        return exposed_panels
            .into_iter()
            .rev()
            .flat_map(|(id, rows)| std::iter::once(id).chain(rows))
            .collect();
        #[cfg(not(target_os = "ios"))]
        nested.into_iter().map(|(_, id)| id).collect()
    }
}

fn place_list_items(
    id: NodeId,
    nodes: &mut BTreeMap<NodeId, AccessibleNode>,
    items: &mut BTreeMap<NodeId, Vec<NodeId>>,
    ancestors: &BTreeSet<NodeId>,
    window: &mut Vec<NodeId>,
) {
    let flatten = ancestors.contains(&id)
        && nodes.get(&id).is_some_and(|node| {
            node.role() == Role::Group
                && node.label().is_none()
                && node.description().is_none()
                && ![Action::Focus, Action::Click]
                    .into_iter()
                    .chain(SCROLL_DIRECTIONS)
                    .any(|action| node.supports_action(action))
        });
    if flatten && let Some(node) = nodes.remove(&id) {
        for &child in node.children() {
            place_list_items(child, nodes, items, ancestors, window);
        }
    } else {
        prepend_list_items(id, nodes, items, ancestors, window);
        window.push(id);
    }
}

fn prepend_list_items(
    id: NodeId,
    nodes: &mut BTreeMap<NodeId, AccessibleNode>,
    items: &mut BTreeMap<NodeId, Vec<NodeId>>,
    ancestors: &BTreeSet<NodeId>,
    window: &mut Vec<NodeId>,
) {
    if let Some(children) = items.remove(&id) {
        // UIKit's point query must encounter each row before the overlapping
        // list viewport. Keep the viewport itself available for scrolling.
        for child in children {
            place_list_items(child, nodes, items, ancestors, window);
        }
    }
    if ancestors.contains(&id) {
        let children = nodes
            .get(&id)
            .map_or_else(Vec::new, |node| node.children().to_vec());
        for child in children {
            prepend_list_items(child, nodes, items, ancestors, window);
        }
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

fn visible_tree_label<'a>(node: &'a Node, item: &LayoutNode) -> Option<&'a str> {
    if !item.visible || node.is_control_part() {
        return None;
    }
    ["accessible-name", "text"]
        .into_iter()
        .filter_map(|name| node.text(name))
        .find(|text| !text.is_empty())
        .or_else(|| {
            node.materialized_children()
                .find_map(|(index, child)| visible_tree_label(child, item.child_at(index)?))
        })
}

fn role(node: &Node, view: Option<&str>) -> Role {
    if xrack_runtime::is_exclusive_push_option(node) {
        return Role::RadioButton;
    }
    match node.kind {
        "Button" | "ToolButton" | "RoundButton" | "DelayButton" | "DialogButton" | "TouchArea"
        | "SwipeDelegate" => Role::Button,
        "CheckBox" => Role::CheckBox,
        "Switch" => Role::Switch,
        "RadioButton" => Role::RadioButton,
        "RadioGroup" => Role::RadioGroup,
        "StyledText" if node.selectable_styled_text() => Role::MultilineTextInput,
        "StyledText"
            if node
                .styled_text("text")
                .is_some_and(|text| !text.links().is_empty()) =>
        {
            Role::Group
        }
        "Text" | "Label" | "StyledText" => Role::Label,
        "Image" | "AnimatedImage" | "MediaPlayer"
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
        // AccessKit's Windows range provider treats Splitter as read-only.
        "SplitHandle" if cfg!(target_os = "windows") => Role::Slider,
        "SplitHandle" => Role::Splitter,
        "SpinBox" | "DoubleSpinBox" => Role::SpinButton,
        "ProgressIndicator" | "Spinner" | "BusyIndicator" => Role::ProgressIndicator,
        "MenuBar" => Role::MenuBar,
        "Menu" => Role::MenuItem,
        "ToolBar" => Role::Toolbar,
        "Dialog" | "Drawer" | "PopupWindow" => Role::Dialog,
        "TabWidget" | "TabBar" => Role::TabList,
        "Tab" | "TabButton" => Role::Tab,
        "Tumbler" | "PathView" | "StandardListView" => Role::ListBox,
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

fn mirrored_scroll(item: &LayoutNode) -> bool {
    item.attached_scroll_geometry()
        .is_some_and(|bar| bar.reversed())
        || item.scroll_axis_reversed(Orientation::Horizontal)
}

pub(crate) fn swipe_scroll_selection(
    action: Action,
    view: &Node,
    item: &LayoutNode,
) -> Option<usize> {
    let key = match action {
        Action::ScrollUp => NavigationKey::Up,
        Action::ScrollDown => NavigationKey::Down,
        Action::ScrollLeft => NavigationKey::Left,
        Action::ScrollRight => NavigationKey::Right,
        _ => return None,
    };
    swipe_view_selection(view, item, key)
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
        let offset = if mirrored_scroll(item) {
            scroll.max_offset - scroll.offset
        } else {
            scroll.offset
        };
        node.set_scroll_x_min(0.0);
        node.set_scroll_x_max(f64::from(scroll.max_offset) * scale);
        node.set_scroll_x(f64::from(offset) * scale);
        if offset > 0.0 {
            node.add_action(Action::ScrollLeft);
        }
        if offset < scroll.max_offset {
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
        let sign = if bar.reversed() { -1.0 } else { 1.0 };
        let line = f64::from(bar.step);
        let step = if matches!(data, Some(ActionData::ScrollUnit(ScrollUnit::Item))) {
            line
        } else {
            f64::from(bar.page)
        };
        let value = match (action, data, bar.orientation) {
            (Action::SetValue, Some(ActionData::NumericValue(value)), _) => *value,
            (Action::SetScrollOffset, Some(ActionData::SetScrollOffset(point)), _) => {
                let offset = [point.x, point.y][index] / scale;
                if bar.reversed() {
                    f64::from(bar.scroll.max_offset) - offset
                } else {
                    offset
                }
            }
            (Action::Increment, _, _) => current + line,
            (Action::Decrement, _, _) => current - line,
            (Action::ScrollLeft, _, Orientation::Horizontal)
            | (Action::ScrollUp, _, Orientation::Vertical) => current - step * sign,
            (Action::ScrollRight, _, Orientation::Horizontal)
            | (Action::ScrollDown, _, Orientation::Vertical) => current + step * sign,
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
            Some(ActionData::SetScrollOffset(point)) => {
                let offset = |value: f64| {
                    value
                        .is_finite()
                        .then(|| (value / scale).clamp(0.0, f64::from(f32::MAX)) as f32)
                };
                let x = offset(point.x).map(|x| {
                    if item.scroll_axis_reversed(Orientation::Horizontal)
                        && let Some(scroll) = item.scroll_on(Orientation::Horizontal)
                    {
                        scroll.max_offset - x
                    } else {
                        x
                    }
                });
                [x, offset(point.y)]
            }
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
        .map_or_else(
            || item.scroll_viewport(axis),
            |table| match axis {
                Orientation::Horizontal => table.scrollable_body(),
                Orientation::Vertical => table.body,
            },
        )
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
    let direction = if item.scroll_axis_reversed(axis) {
        -direction
    } else {
        direction
    };
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
            | "RoundButton"
            | "DelayButton"
            | "GroupBox"
            | "Expander"
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
    (xrack_runtime::is_text_input(node.kind)
        || combo_editable(node)
        || node.table_cell_editor.is_some() && xrack_runtime::editable_table_cell(node))
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
    fn empty_view_content_exposes_its_button_without_creating_an_accessible_row() {
        let mut window = Node::new("Window", None);
        let mut view = Node::new("StandardListView", Some("results"));
        view.properties
            .insert("accessible-name", "Results".to_owned().into());
        length(&mut view, "width", 180.0);
        length(&mut view, "height", 100.0);
        view.list = Some(ListMetrics::default());
        let mut empty = Node::new("EmptyState", None);
        empty.children.push(button("retry", "Retry", 10.0));
        view.children.push(empty);
        window.children.push(view);

        let scene = Rc::new(window);
        let frame = Rc::new(layout(&scene, Size::new(200.0, 120.0), &mut Text));
        let mut projection = Projection::default();
        let update = projection
            .update(&scene, &frame, &Interaction::default(), 1.0, None)
            .unwrap();
        let (id, retry) = named(&update, "Retry");
        assert_eq!(retry.role(), Role::Button);
        assert!(retry.supports_action(Action::Click));
        assert!(
            matches!(projection.route(id), Some(Route::Element { target, .. })
            if target.path == [0, 0, 0])
        );
        assert!(
            !update
                .nodes
                .iter()
                .any(|(_, node)| node.role() == Role::ListBoxOption)
        );

        let mut restored = (*scene).clone();
        restored.children[0].list.as_mut().unwrap().row_count = 1;
        restored.children[0].children.clear();
        let restored = Rc::new(restored);
        let frame = Rc::new(layout(&restored, Size::new(200.0, 120.0), &mut Text));
        projection
            .update(&restored, &frame, &Interaction::default(), 1.0, None)
            .unwrap();
        assert!(projection.route(id).is_none());
    }

    #[test]
    fn sparse_table_cells_keep_accessible_routes_when_dense_positions_change() {
        use xrack_runtime::{TableColumnWindow, TableHeader};

        let model = VecModel::from(vec![0]);
        let mut table = Node::new("TableView", None);
        table.list = Some(ListMetrics {
            row_count: 1,
            first_row: 0,
        });
        let mut column = Node::new("TableColumn", None);
        length(&mut column, "width", 48.0);
        table.table_header = Some(Box::new(TableHeader {
            columns: vec![column; 12],
            sort: None,
            column_widths: Vec::new(),
        }));
        let (_, order) = table.table_column_order.moved(12, 11, 1).unwrap();
        table.table_column_order = order.clone();
        let mut row = Node::new("ListRow", None);
        row.row_key = model.row_key(0);
        let mut cells = Node::new("TableRow", None);
        cells.table_column_order = order;
        let cell = |label: &'static str| {
            let mut cell = Node::new("TableCell", None);
            cell.properties.insert("text", label.to_owned().into());
            cell
        };
        cells.table_column_window =
            TableColumnWindow::new((0..12).map(|column| matches!(column, 0 | 11)));
        cells.children = vec![cell("First value"), cell("Last value")];
        row.children.push(cells);
        table.children.push(row);
        let mut scene = Rc::new(table);
        let viewport = Size::new(210.0, 100.0);
        let input = Interaction::default();
        let mut projection = Projection::default();
        let frame = Rc::new(layout(&scene, viewport, &mut Text));
        let update = projection
            .update(&scene, &frame, &input, 1.0, None)
            .unwrap();
        let (last, last_cell) = named(&update, "Last value");
        assert_eq!(last_cell.column_index(), Some(1));
        assert!(
            matches!(projection.route(last), Some(Route::Element { target, .. })
            if target.path == [0, 0, 11])
        );
        let cells = &mut Rc::make_mut(&mut scene).children[0].children[0];
        cells.table_column_window =
            TableColumnWindow::new((0..12).map(|column| matches!(column, 0 | 1 | 11)));
        cells.children.insert(1, cell("New visible value"));
        let frame = Rc::new(layout(&scene, viewport, &mut Text));
        let update = projection
            .update(&scene, &frame, &input, 1.0, None)
            .unwrap();
        assert_eq!(named(&update, "Last value").0, last);
        assert!(
            matches!(projection.route(last), Some(Route::Element { target, .. })
            if target.path == [0, 0, 11])
        );
        let table = Rc::make_mut(&mut scene);
        table.table_header.as_mut().unwrap().columns[0]
            .properties
            .insert("visible", false.into());
        let cells = &mut table.children[0].children[0];
        cells.table_column_window =
            TableColumnWindow::new((0..12).map(|column| matches!(column, 1 | 11)));
        cells.children.remove(0);
        let frame = Rc::new(layout(&scene, viewport, &mut Text));
        let update = projection
            .update(&scene, &frame, &input, 1.0, None)
            .unwrap();
        let (id, cell) = named(&update, "Last value");
        assert_eq!(id, last);
        assert_eq!(cell.column_index(), Some(0));
        assert!(
            update
                .nodes
                .iter()
                .any(|(_, node)| node.role() == Role::Table && node.column_count() == Some(11))
        );
        assert!(
            !update
                .nodes
                .iter()
                .any(|(_, node)| node.label() == Some("First value"))
        );
    }

    #[test]
    fn nested_rows_of_a_shared_model_keep_distinct_ids_when_outer_slots_swap() {
        let model = VecModel::from(vec!["shared".to_owned()]);
        let shared = model.row_key(0).unwrap();
        let outer = VecModel::from(vec![0, 1]);
        let mut view = Node::new("ListView", None);
        view.list = Some(ListMetrics {
            row_count: 2,
            first_row: 0,
        });
        length(&mut view, "row-height", 64.0);
        for (index, label) in ["Left scope", "Right scope"].into_iter().enumerate() {
            let mut nested = Node::new("ListView", None);
            length(&mut nested, "height", 48.0);
            nested.list = Some(ListMetrics {
                row_count: 1,
                first_row: 0,
            });
            let mut row = Node::new("ListRow", None);
            row.row_key = Some(shared);
            row.row_scope = xrack_runtime::ListRowRenderer::new(|_| Vec::new()).scope();
            row.children.push(button("shared", label, 0.0));
            nested.children.push(row);
            let mut group = Node::new("ListRow", None);
            group.row_key = outer.row_key(index);
            group.children.push(nested);
            view.children.push(group);
        }
        let mut scene = Rc::new(view);
        let viewport = Size::new(180.0, 128.0);
        let input = Interaction::default();
        let mut projection = Projection::default();
        let frame = Rc::new(layout(&scene, viewport, &mut Text));
        let update = projection
            .update(&scene, &frame, &input, 1.0, None)
            .unwrap();
        let left = named(&update, "Left scope").0;
        let right = named(&update, "Right scope").0;
        assert_ne!(left, right);
        Rc::make_mut(&mut scene).children.swap(0, 1);
        let frame = Rc::new(layout(&scene, viewport, &mut Text));
        let update = projection
            .update(&scene, &frame, &input, 1.0, None)
            .unwrap();
        assert_eq!(named(&update, "Left scope").0, left);
        assert_eq!(named(&update, "Right scope").0, right);
        assert!(
            matches!(projection.route(left), Some(Route::Element { target, .. })
            if target.path == [1, 0, 0, 0])
        );
    }

    #[test]
    fn ios_splitter_publishes_an_adjustable_range_only_when_the_panes_can_move() {
        let mut view = Node::new("SplitView", None);
        for width in [120.0, 180.0] {
            let mut pane = Node::new("Rectangle", None);
            length(&mut pane, "min-width", 80.0);
            length(&mut pane, "preferred-width", width);
            view.children.push(pane);
        }
        view.add_split_handles();
        for (width, role) in [(308.0, Role::Slider), (168.0, Role::Splitter)] {
            let scene = Rc::new(view.clone());
            let layout = Rc::new(layout(&scene, Size::new(width, 180.0), &mut Text));
            let mut projection = Projection::default();
            let update = projection
                .update_tree(&scene, &layout, &Interaction::default(), 1.0, None, true)
                .unwrap();
            let (id, handle) = named(&update, "Pane divider 1");
            assert_eq!(handle.role(), role);
            if role == Role::Slider {
                assert_eq!(handle.numeric_value(), Some(120.0));
                assert_eq!(handle.numeric_value_step(), Some(SplitHandleRange::STEP));
                assert!(handle.supports_action(Action::Increment));
                assert!(handle.supports_action(Action::Decrement));
                assert!(matches!(
                    projection.route(id),
                    Some(Route::SplitHandle { .. })
                ));
            } else {
                assert!(!handle.supports_action(Action::Increment));
                assert!(!handle.supports_action(Action::Decrement));
            }
        }
    }

    #[test]
    fn a_constrained_vertical_splitter_reports_its_horizontal_orientation_and_fixed_value() {
        let mut view = Node::new("SplitView", None);
        view.properties
            .insert("orientation", Orientation::Vertical.into());
        for height in [80.0, 112.0] {
            let mut pane = Node::new("Rectangle", None);
            length(&mut pane, "min-height", height);
            length(&mut pane, "max-height", height);
            view.children.push(pane);
        }
        view.add_split_handles();
        let scene = Rc::new(view);
        let layout = Rc::new(layout(&scene, Size::new(160.0, 200.0), &mut Text));
        let mut projection = Projection::default();
        let update = projection
            .update(&scene, &layout, &Interaction::default(), 1.0, None)
            .unwrap();
        let (id, splitter) = named(&update, "Pane divider 1");
        assert_eq!(
            splitter.orientation(),
            Some(accesskit::Orientation::Horizontal)
        );
        assert_eq!(splitter.numeric_value(), Some(80.0));
        assert_eq!(splitter.min_numeric_value(), Some(80.0));
        assert_eq!(splitter.max_numeric_value(), Some(80.0));
        assert!(splitter.is_disabled());
        assert!(splitter.is_read_only());
        assert!(!splitter.supports_action(Action::Focus));
        assert!(!splitter.supports_action(Action::SetValue));
        assert!(projection.route(id).is_none());
    }

    #[test]
    fn round_button_exposes_button_actions_and_skips_decorative_separators() {
        let mut window = Node::new("Window", None);
        let mut round = Node::new("RoundButton", Some("round"));
        round
            .properties
            .insert("accessible-name", "Toggle grid".to_owned().into());
        length(&mut round, "width", 48.0);
        length(&mut round, "height", 48.0);
        let mut separator = Node::new("ToolSeparator", Some("separator"));
        length(&mut separator, "x", 54.0);
        window.children.extend([round, separator]);
        let scene = Rc::new(window);
        let layout = Rc::new(layout(&scene, Size::new(200.0, 150.0), &mut Text));
        let input = Interaction::default();
        let update = Projection::default()
            .update(&scene, &layout, &input, 1.0, None)
            .unwrap();
        let (_, round) = named(&update, "Toggle grid");
        assert_eq!(round.role(), Role::Button);
        assert!(round.supports_action(Action::Focus));
        assert!(round.supports_action(Action::Click));
        assert_eq!(update.nodes.len(), 2);
    }

    #[test]
    fn checkable_push_buttons_keep_the_button_role_and_publish_their_state() {
        let mut window = Node::new("Window", None);
        for (index, kind) in ["Button", "ToolButton", "RoundButton"]
            .into_iter()
            .enumerate()
        {
            let mut node = Node::new(kind, None);
            node.properties.insert("text", kind.to_owned().into());
            node.properties.insert("checkable", true.into());
            node.properties.insert("checked", (index == 1).into());
            length(&mut node, "y", index as f64 * 40.0);
            length(&mut node, "width", 130.0);
            length(&mut node, "height", 32.0);
            window.children.push(node);
        }
        let input = Interaction::default();
        let scene = Rc::new(window);
        let layout = Rc::new(layout(&scene, Size::new(160.0, 140.0), &mut Text));
        let update = Projection::default()
            .update(&scene, &layout, &input, 1.0, None)
            .unwrap();
        for (kind, state) in [
            ("Button", Toggled::False),
            ("ToolButton", Toggled::True),
            ("RoundButton", Toggled::False),
        ] {
            let (_, button) = named(&update, kind);
            assert_eq!(button.role(), Role::Button);
            assert_eq!(button.toggled(), Some(state));
            assert!(button.supports_action(Action::Click));
        }
    }

    #[test]
    fn expander_header_exposes_expansion_and_hides_collapsed_content() {
        let mut panel = Node::new("Expander", Some("panel"));
        panel
            .properties
            .insert("title", "More options".to_owned().into());
        panel.children.push(button("save", "Save preferences", 0.0));
        let mut projection = Projection::default();
        let size = Size::new(220.0, 140.0);
        let check = |panel: Node, expanded: bool, projection: &mut Projection| {
            let scene = Rc::new(panel);
            let frame = Rc::new(layout(&scene, size, &mut Text));
            let update = projection
                .update(&scene, &frame, &Interaction::default(), 1.0, None)
                .unwrap();
            let (id, header) = update
                .nodes
                .iter()
                .find(|(_, node)| {
                    node.role() == Role::Button && node.label() == Some("More options")
                })
                .unwrap();
            assert_eq!(header.is_expanded(), Some(expanded));
            assert_eq!(header.supports_action(Action::Expand), !expanded);
            assert_eq!(header.supports_action(Action::Collapse), expanded);
            assert!(matches!(
                projection.route(*id),
                Some(Route::Element {
                    kind: "Expander",
                    ..
                })
            ));
            assert_eq!(
                update
                    .nodes
                    .iter()
                    .any(|(_, node)| node.label() == Some("Save preferences")),
                expanded
            );
        };
        check(panel.clone(), false, &mut projection);
        panel.properties.insert("expanded", true.into());
        check(panel, true, &mut projection);
    }

    #[test]
    fn frame_groups_accessible_children_and_inherits_disabled_state() {
        let mut window = Node::new("Window", None);
        let mut frame = Node::new("ControlFrame", Some("group"));
        frame
            .properties
            .insert("accessible-name", "Actions".to_owned().into());
        frame.children.push(button("child", "Apply", 0.0));
        window.children.push(frame);
        let input = Interaction::default();
        let mut projection = Projection::default();
        for enabled in [true, false] {
            window.children[0]
                .properties
                .insert("enabled", enabled.into());
            let scene = Rc::new(window.clone());
            let layout = Rc::new(layout(&scene, Size::new(200.0, 150.0), &mut Text));
            let update = projection
                .update(&scene, &layout, &input, 1.0, None)
                .unwrap();
            let (_, frame) = named(&update, "Actions");
            let (child_id, child) = named(&update, "Apply");
            assert_eq!(frame.role(), Role::Group);
            assert!(frame.children().contains(&child_id));
            assert_eq!(frame.is_disabled(), !enabled);
            assert_eq!(child.supports_action(Action::Click), enabled);
        }
    }

    #[test]
    fn replacement_visuals_keep_the_control_accessibility_role_actions_and_focus() {
        let mut window = Node::new("Window", None);
        let mut apply = button("apply", "Apply settings", 0.0);
        let mut content = Node::new("ButtonContent", None);
        content
            .children
            .push(button("decoration", "Decoration", 0.0));
        apply.children.push(content);

        let mut slider = Node::new("Slider", Some("volume"));
        length(&mut slider, "x", 110.0);
        length(&mut slider, "width", 100.0);
        length(&mut slider, "height", 30.0);
        slider
            .properties
            .insert("accessible-name", "Volume".to_owned().into());
        slider.properties.insert("value", 40.0.into());
        slider.properties.insert("maximum", 100.0.into());
        slider.children.push(Node::new("SliderHandle", None));
        let mut checkbox = Node::new("CheckBox", Some("master"));
        length(&mut checkbox, "y", 42.0);
        length(&mut checkbox, "width", 100.0);
        length(&mut checkbox, "height", 34.0);
        checkbox
            .properties
            .insert("text", "Include settings".to_owned().into());
        checkbox
            .properties
            .insert("check-state", CheckState::PartiallyChecked.into());
        let mut indicator = Node::new("ToggleIndicator", None);
        indicator
            .children
            .push(button("mark", "Decorative checkbox mark", 0.0));
        checkbox.children.push(indicator);
        let mut switch = Node::new("Switch", Some("network"));
        length(&mut switch, "x", 110.0);
        length(&mut switch, "y", 42.0);
        length(&mut switch, "width", 100.0);
        length(&mut switch, "height", 34.0);
        switch
            .properties
            .insert("text", "Network".to_owned().into());
        switch.properties.insert("checked", true.into());
        let mut thumb = Node::new("SwitchThumb", None);
        thumb
            .children
            .push(button("ornament", "Thumb decoration", 0.0));
        switch.children.push(thumb);
        window.children.extend([apply, slider, checkbox, switch]);

        let scene = Rc::new(window);
        let frame = Rc::new(layout(&scene, Size::new(220.0, 100.0), &mut Text));
        let mut input = Interaction::default();
        input.focus_next(&scene, &frame, false);
        let mut projection = Projection::default();
        let update = projection
            .update(&scene, &frame, &input, 1.0, None)
            .unwrap();
        let (button_id, apply) = named(&update, "Apply settings");
        assert_eq!(apply.role(), Role::Button);
        assert!(apply.supports_action(Action::Click));
        assert_eq!(update.focus, button_id);
        assert!(apply.children().is_empty());
        let (slider_id, slider) = named(&update, "Volume");
        assert_eq!(slider.role(), Role::Slider);
        assert_eq!(slider.numeric_value(), Some(40.0));
        assert!(slider.supports_action(Action::SetValue));
        assert!(slider.children().is_empty());
        input.focus_next(&scene, &frame, false);
        let update = projection
            .update(&scene, &frame, &input, 1.0, None)
            .unwrap();
        assert_eq!(update.focus, slider_id);
        let (check_id, checkbox) = named(&update, "Include settings");
        assert_eq!(checkbox.role(), Role::CheckBox);
        assert!(checkbox.supports_action(Action::Click));
        assert_eq!(checkbox.toggled(), Some(Toggled::Mixed));
        assert!(checkbox.children().is_empty());
        input.focus_next(&scene, &frame, false);
        let update = projection
            .update(&scene, &frame, &input, 1.0, None)
            .unwrap();
        assert_eq!(update.focus, check_id);
        let (switch_id, switch) = named(&update, "Network");
        assert_eq!(switch.role(), Role::Switch);
        assert!(switch.supports_action(Action::Click));
        assert_eq!(switch.toggled(), Some(Toggled::True));
        assert!(switch.children().is_empty());
        input.focus_next(&scene, &frame, false);
        let update = projection
            .update(&scene, &frame, &input, 1.0, None)
            .unwrap();
        assert_eq!(update.focus, switch_id);
    }

    #[test]
    fn single_line_drag_scroll_moves_accessible_text_without_a_caret_change() {
        let mut node = Node::new("LineEdit", Some("input"));
        length(&mut node, "width", 140.0);
        length(&mut node, "height", 32.0);
        node.properties
            .insert("text", "long text ".repeat(16).into());
        let scene = Rc::new(node);
        let frame = Rc::new(layout(&scene, Size::new(140.0, 32.0), &mut Text));
        let text = TextSystem::system().unwrap();
        let mut input = Interaction::default();
        input.accessibility_focus(input.accessibility_target(&scene, &frame, &[]).unwrap());
        input.editor = Some(Default::default());
        let mut projection = Projection::default();
        let first_run_x = |update: TreeUpdate| {
            update
                .nodes
                .into_iter()
                .find(|(_, node)| node.role() == Role::TextRun)
                .unwrap()
                .1
                .bounds()
                .unwrap()
                .x0
        };
        let initial = first_run_x(
            projection
                .update(&scene, &frame, &input, 1.0, Some(&text))
                .unwrap(),
        );
        input
            .editor
            .as_mut()
            .unwrap()
            .set_horizontal_drag_scroll(Some(24.0));
        let scrolled = first_run_x(
            projection
                .update(&scene, &frame, &input, 1.0, Some(&text))
                .unwrap(),
        );
        assert!(
            (initial - scrolled - 24.0).abs() < 0.01,
            "{initial} -> {scrolled}"
        );
    }

    #[test]
    fn search_buttons_and_editor_text_follow_the_rtl_layout() {
        let mut search = Node::new("SearchField", Some("query"));
        length(&mut search, "width", 220.0);
        length(&mut search, "height", 36.0);
        search
            .properties
            .insert("accessible-name", "Query".to_owned().into());
        search.properties.insert(
            "layout-direction",
            xrack_runtime::LayoutDirection::RightToLeft.into(),
        );
        search.properties.insert("text", "report".to_owned().into());
        let scene = Rc::new(search);
        let frame = Rc::new(layout(&scene, Size::new(220.0, 36.0), &mut Text));
        let geometry = SearchGeometry::for_layout(&frame);
        let input = Interaction::default();
        let text = TextSystem::system().unwrap();
        let update = Projection::default()
            .update(&scene, &frame, &input, 1.0, Some(&text))
            .unwrap();
        let (_, submit) = named(&update, "Search");
        let (_, clear) = named(&update, "Clear search");
        assert_eq!(clear.bounds().unwrap().x0, f64::from(geometry.clear.x));
        assert_eq!(submit.bounds().unwrap().x0, f64::from(geometry.submit.x));
        assert!(clear.supports_action(Action::Click));
        assert!(submit.supports_action(Action::Click));
        let run = update
            .nodes
            .iter()
            .find_map(|(_, node)| {
                (node.role() == Role::TextRun && node.value() == Some("report")).then_some(node)
            })
            .unwrap()
            .bounds()
            .unwrap();
        assert!(run.x0 > f64::from(geometry.entry.x + geometry.entry.width * 0.5));
        assert!(run.x1 < f64::from(geometry.submit.x));
    }

    #[test]
    fn ios_search_field_exposes_actions_beside_the_editable_text() {
        let mut search = Node::new("SearchField", Some("query"));
        length(&mut search, "width", 220.0);
        length(&mut search, "height", 36.0);
        search
            .properties
            .insert("accessible-name", "Query".to_owned().into());
        search.properties.insert(
            "layout-direction",
            xrack_runtime::LayoutDirection::RightToLeft.into(),
        );
        search.properties.insert("text", "report".to_owned().into());
        let mut projection = Projection::default();
        let mut stable_field_id = None;
        for (read_only, text, has_clear) in [
            (false, "report", true),
            (false, "", false),
            (true, "report", false),
        ] {
            search.properties.insert("read-only", read_only.into());
            search.properties.insert("text", text.to_owned().into());
            let scene = Rc::new(search.clone());
            let frame = Rc::new(layout(&scene, Size::new(220.0, 36.0), &mut Text));
            let geometry = SearchGeometry::for_layout(&frame);
            let mut input = Interaction::default();
            input.accessibility_focus(input.accessibility_target(&scene, &frame, &[]).unwrap());
            let update = projection
                .update_tree(&scene, &frame, &input, 2.0, None, true)
                .unwrap();
            let (field_id, field) = named(&update, "Query");
            let (submit_id, submit) = named(&update, "Submit search");
            assert_eq!(*stable_field_id.get_or_insert(field_id), field_id);
            assert_eq!(field.role(), Role::SearchInput);
            assert_eq!(field.value(), Some(text));
            assert_eq!(update.focus, field_id);
            assert_eq!(field.bounds(), Some(bounds(geometry.entry, 2.0)));
            assert_eq!(submit.bounds(), Some(bounds(geometry.submit, 2.0)));
            let (_, root) = update.nodes.iter().find(|(id, _)| *id == ROOT).unwrap();
            let group_id = root.children()[0];
            let (_, group) = update.nodes.iter().find(|(id, _)| *id == group_id).unwrap();
            assert_eq!(group.role(), Role::Group);
            assert_eq!(&group.children()[..2], &[field_id, submit_id]);
            assert!(!field.children().contains(&submit_id));
            assert!(matches!(
                projection.route(submit_id),
                Some(Route::ControlPart {
                    action: ControlAction::SubmitSearch,
                    ..
                })
            ));
            if has_clear {
                let (clear_id, clear) = named(&update, "Clear search");
                assert_eq!(group.children(), &[field_id, submit_id, clear_id]);
                assert_eq!(clear.bounds(), Some(bounds(geometry.clear, 2.0)));
                assert!(matches!(
                    projection.route(clear_id),
                    Some(Route::ControlPart {
                        action: ControlAction::ClearSearch,
                        ..
                    })
                ));
            } else {
                assert_eq!(group.children(), &[field_id, submit_id]);
            }
        }
    }

    #[test]
    fn spin_indicator_art_keeps_numeric_accessibility_on_the_parent() {
        let mut window = Node::new("Window", None);
        window.properties.insert(
            "layout-direction",
            xrack_runtime::LayoutDirection::RightToLeft.into(),
        );
        let mut spin = Node::new("DoubleSpinBox", Some("price"));
        length(&mut spin, "width", 180.0);
        length(&mut spin, "height", 48.0);
        spin.properties
            .insert("accessible-name", "Price".to_owned().into());
        spin.properties.insert("value", 1.25_f64.into());
        spin.properties.insert("step-size", 0.25_f64.into());
        let mut up = Node::new("SpinUpIndicator", None);
        length(&mut up, "width", 64.0);
        up.children
            .push(button("decoration", "Decorative up arrow", 0.0));
        spin.children.push(up);
        window.children.push(spin);

        let scene = Rc::new(window);
        let frame = Rc::new(layout(&scene, Size::new(220.0, 100.0), &mut Text));
        let item = frame.child_at(0).unwrap();
        let parts = xrack_layout::SpinGeometry::for_layout(item, scene.child_at(0).unwrap());
        assert_eq!(parts.up.x, item.bounds.x);
        assert_eq!(item.child_at(0).unwrap().bounds, parts.up);
        let mut input = Interaction::default();
        input.focus_next(&scene, &frame, false);
        let update = Projection::default()
            .update(&scene, &frame, &input, 1.0, None)
            .unwrap();
        let (parent, spin) = named(&update, "Price");
        assert_eq!(update.focus, parent);
        assert_eq!(spin.role(), Role::SpinButton);
        assert_eq!(spin.numeric_value(), Some(1.25));
        assert_eq!(spin.numeric_value_step(), Some(0.25));
        assert!(spin.supports_action(Action::Increment));
        assert!(spin.supports_action(Action::Decrement));
        assert!(spin.children().is_empty());
    }

    #[test]
    fn range_handle_art_preserves_two_independent_accessible_sliders() {
        let mut window = Node::new("Window", None);
        let mut range = Node::new("RangeSlider", Some("budget"));
        length(&mut range, "width", 200.0);
        length(&mut range, "height", 40.0);
        range.properties.insert("lower-value", 20.0_f64.into());
        range.properties.insert("upper-value", 80.0_f64.into());
        range.add_range_thumbs();
        let mut part = Node::new("RangeLowerHandle", None);
        length(&mut part, "width", 40.0);
        part.children
            .push(button("decoration", "Decorative grip", 0.0));
        range.children.push(part);
        window.children.push(range);
        let scene = Rc::new(window);
        let frame = Rc::new(layout(&scene, Size::new(220.0, 90.0), &mut Text));
        let mut input = Interaction::default();
        input.focus_next(&scene, &frame, false);
        let mut projection = Projection::default();
        let update = projection
            .update(&scene, &frame, &input, 1.0, None)
            .unwrap();
        let (lower_id, lower) = named(&update, "Lower value");
        let (upper_id, upper) = named(&update, "Upper value");
        assert_eq!(update.focus, lower_id);
        for (node, value) in [(lower, 20.0), (upper, 80.0)] {
            assert_eq!(node.role(), Role::Slider);
            assert_eq!(node.numeric_value(), Some(value));
            assert!(node.supports_action(Action::SetValue));
            assert!(node.children().is_empty());
        }
        assert_eq!(frame.at_path(&[0, 0]).unwrap().bounds.width, 40.0);
        input.focus_next(&scene, &frame, false);
        assert_eq!(
            projection
                .update(&scene, &frame, &input, 1.0, None)
                .unwrap()
                .focus,
            upper_id,
        );
    }

    #[test]
    fn disabled_touch_area_keeps_child_accessibility_actions() {
        let mut window = Node::new("Window", None);
        let mut area = Node::new("TouchArea", Some("area"));
        length(&mut area, "width", 160.0);
        length(&mut area, "height", 80.0);
        area.properties
            .insert("accessible-name", "Overlay".to_owned().into());
        area.properties.insert("enabled", false.into());
        area.children.push(button("child", "Child", 8.0));
        window.children.push(area);
        let scene = Rc::new(window);
        let frame = Rc::new(layout(&scene, Size::new(200.0, 150.0), &mut Text));
        let input = Interaction::default();
        let update = Projection::default()
            .update(&scene, &frame, &input, 1.0, None)
            .unwrap();
        let (_, area) = named(&update, "Overlay");
        let (child_id, child) = named(&update, "Child");
        assert!(area.is_disabled());
        assert!(!area.supports_action(Action::Click));
        assert!(area.children().contains(&child_id));
        assert!(!child.is_disabled());
        assert!(child.supports_action(Action::Click));
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
        let mut thumb = Node::new("ScrollThumb", Some("bar-artwork"));
        let mut label = Node::new("Label", None);
        label
            .properties
            .insert("text", "artwork decoration".to_owned().into());
        thumb.children.push(label);
        bar.children.push(thumb);
        window.children.push(bar);
        let scene = Rc::new(window);
        let frame = Rc::new(layout(&scene, Size::new(200.0, 150.0), &mut Text));
        let input = Interaction::default();
        let update = Projection::default()
            .update(&scene, &frame, &input, 2.0, None)
            .unwrap();
        assert!(
            update
                .nodes
                .iter()
                .all(|(_, node)| node.label() != Some("artwork decoration"))
        );
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
    fn ios_standalone_scroll_bar_adjustments_follow_range_and_interactivity() {
        let input = Interaction::default();
        for (position, size, step, interactive, enabled, increment, decrement) in [
            (0.0, 0.25, 0.125, true, true, true, false),
            (0.375, 0.25, 0.125, true, true, true, true),
            (0.75, 0.25, 0.125, true, true, false, true),
            (0.375, 0.25, 0.125, false, true, false, false),
            (0.375, 0.25, 0.125, true, false, false, false),
            (0.375, 0.25, 0.0, true, true, false, false),
            (0.0, 1.0, 0.125, true, true, false, false),
        ] {
            let mut window = Node::new("Window", None);
            let mut bar = Node::new("ScrollBar", Some("position"));
            length(&mut bar, "width", 14.0);
            length(&mut bar, "height", 120.0);
            bar.properties.insert("position", position.into());
            bar.properties.insert("size", size.into());
            bar.properties.insert("step-size", step.into());
            bar.properties.insert("interactive", interactive.into());
            bar.properties.insert("enabled", enabled.into());
            bar.properties
                .insert("policy", xrack_runtime::ScrollBarPolicy::AlwaysOn.into());
            window.children.push(bar);
            let scene = Rc::new(window);
            let frame = Rc::new(layout(&scene, Size::new(200.0, 150.0), &mut Text));
            let mut projection = Projection::default();
            let update = projection
                .update_tree(&scene, &frame, &input, 3.0, None, true)
                .unwrap();
            let (id, bar) = named(&update, "Scroll bar");
            assert_eq!(
                bar.role(),
                if increment || decrement {
                    Role::Slider
                } else {
                    Role::ScrollBar
                }
            );
            assert_eq!(bar.numeric_value(), Some(position));
            assert_eq!(bar.max_numeric_value(), Some(1.0 - size));
            assert_eq!(bar.numeric_value_step(), Some(step));
            assert_eq!(bar.is_disabled(), !enabled);
            assert_eq!(bar.supports_action(Action::Increment), increment);
            assert_eq!(bar.supports_action(Action::Decrement), decrement);
            assert_eq!(
                bar.supports_action(Action::SetValue),
                interactive && enabled && size < 1.0
            );
            if increment || decrement {
                assert!(matches!(
                    projection.route(id),
                    Some(Route::Element {
                        kind: "ScrollBar",
                        ..
                    })
                ));
            }
        }
    }

    #[test]
    fn viewport_accessibility_page_uses_the_height_excluding_scrollbar_gutters() {
        let mut view = Node::new("ScrollView", Some("view"));
        length(&mut view, "content-height", 420.0);
        view.properties
            .insert("reserve-scroll-bar-space", true.into());
        view.properties.insert(
            "horizontal-scroll-bar-policy",
            xrack_runtime::ScrollBarPolicy::AlwaysOn.into(),
        );
        let frame = layout(&view, Size::new(190.0, 120.0), &mut Text);
        let offset = requested_scroll(Action::ScrollDown, None, &view, &frame, 1.0);
        assert_eq!(offset, [None, Some(110.0)]);
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

        let ios = Projection::default()
            .update_tree(&scene, &frame, &input, 2.0, None, true)
            .unwrap();
        let (_, bar) = named(&ios, "Bar");
        assert_eq!(bar.role(), Role::Slider);
        assert_eq!(bar.orientation(), Some(accesskit::Orientation::Vertical));
        assert_eq!(bar.numeric_value(), Some(0.0));
        assert_eq!(bar.max_numeric_value(), Some(200.0));
        assert_eq!(bar.numeric_value_step(), Some(75.0));
        assert!(bar.supports_action(Action::Increment));
        assert!(!bar.supports_action(Action::Decrement));
        assert!(bar.supports_action(Action::ScrollDown));
        assert!(bar.supports_action(Action::SetScrollOffset));
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
    fn a_fixed_label_refreshes_accessible_text_with_a_reused_layout() {
        use crate::frame::FrameCache;

        let make_scene = |value: &str| {
            let mut window = Node::new("Window", None);
            let mut label = Node::new("Label", Some("status"));
            label.mark_paint_only("text");
            length(&mut label, "width", 120.0);
            length(&mut label, "height", 24.0);
            label.properties.insert("text", value.to_owned().into());
            window.children.push(label);
            Rc::new(window)
        };
        let input = Interaction::default();
        let viewport = Size::new(180.0, 100.0);
        let mut cache = FrameCache::default();
        let mut projection = Projection::default();
        let mut resolves = 0;
        let mut prepare = |scene| {
            cache.prepare(scene, viewport, &input, |scene| {
                resolves += 1;
                let frame = layout(&scene, viewport, &mut Text);
                (scene, frame)
            })
        };
        let (before, original) = prepare(make_scene("Ready"));
        let first = projection
            .update(&before, &original, &input, 1.0, None)
            .unwrap();
        let (id, _) = first
            .nodes
            .iter()
            .find(|(_, node)| node.value() == Some("Ready"))
            .unwrap();

        let (after, reused) = prepare(make_scene("Downloading an update"));
        assert!(Rc::ptr_eq(&original, &reused));
        let update = projection
            .update(&after, &reused, &input, 1.0, None)
            .unwrap();
        assert!(update.nodes.iter().any(|(updated, node)| {
            updated == id && node.value() == Some("Downloading an update")
        }));
        assert_eq!(resolves, 1);
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
    fn selectable_styled_text_exposes_read_only_ranges_and_link_actions() {
        let mut label = Node::new("StyledText", Some("document"));
        let styled =
            xrack_runtime::StyledText::from_markdown("**Alpha** [link](#a) é.\n\nمرحبا").unwrap();
        let plain = styled.text().to_owned();
        label.properties.insert("text", styled.into());
        label.properties.insert("selectable", true.into());
        label
            .properties
            .insert("accessible-name", "Document".to_owned().into());
        let scene = Rc::new(label);
        let mut text = TextSystem::system().unwrap();
        let frame = Rc::new(layout(&scene, Size::new(280.0, 120.0), &mut text));
        let mut input = Interaction::default();
        input.accessibility_focus(input.accessibility_target(&scene, &frame, &[]).unwrap());
        input.sync_editor(&scene);
        input.apply_text_selection(&scene, xrack_runtime::TextCommand::SelectAll);
        let mut tree = Projection::default();
        let update = tree
            .update(&scene, &frame, &input, 1.0, Some(&text))
            .unwrap();
        let (id, node) = named(&update, "Document");
        assert!(node.is_read_only());
        assert!(node.supports_action(Action::SetTextSelection));
        assert!(!node.supports_action(Action::SetValue));
        let (anchor, focus) = tree
            .text_selection(id, *node.text_selection().unwrap(), &plain)
            .unwrap();
        assert_eq!((anchor, focus.offset), (0, plain.len()));
        assert!(
            update
                .nodes
                .iter()
                .any(|(_, node)| node.role() == Role::Link && node.supports_action(Action::Click))
        );
        assert!(
            update
                .nodes
                .iter()
                .any(|(_, node)| node.role() == Role::TextRun && node.bounds().is_some())
        );
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
    fn ios_swipe_row_actions_are_accessible_siblings_of_the_row() {
        let mut row = Node::new("SwipeDelegate", Some("row"));
        row.properties
            .insert("accessible-name", "Task".to_owned().into());
        let mut actions = Node::new("SwipeRight", None);
        actions
            .children
            .push(button("complete", "Complete task", 0.0));
        row.children
            .extend([Node::new("SwipeContent", None), actions]);
        for side in [SwipeSide::Closed, SwipeSide::Right] {
            row.properties.insert("open-side", side.into());
            let scene = Rc::new(row.clone());
            let layout = Rc::new(layout(&scene, Size::new(300.0, 66.0), &mut Text));
            let mut projection = Projection::default();
            let update = projection
                .update_tree(&scene, &layout, &Interaction::default(), 1.0, None, true)
                .unwrap();
            let (row_id, delegate) = named(&update, "Task");
            let root = update.nodes.iter().find(|(id, _)| *id == ROOT).unwrap();
            let (toggle, _) = named(
                &update,
                if side == SwipeSide::Closed {
                    "Show actions"
                } else {
                    "Hide actions"
                },
            );
            assert!(root.1.children().contains(&row_id));
            assert!(root.1.children().contains(&toggle));
            assert!(!delegate.children().contains(&toggle));
            if side == SwipeSide::Right {
                let (complete, _) = named(&update, "Complete task");
                assert!(matches!(
                    projection.route(complete),
                    Some(Route::Element { kind: "Button", .. })
                ));
                assert!(root.1.children().iter().any(|id| {
                    update
                        .nodes
                        .iter()
                        .any(|(parent, node)| parent == id && node.children().contains(&complete))
                }));
            } else {
                assert!(
                    update
                        .nodes
                        .iter()
                        .all(|(_, node)| node.label() != Some("Complete task"))
                );
            }
        }
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
    fn month_grid_exposes_date_actions_and_calendar_headings() {
        use xrack_runtime::{Date, Length};
        let date = |year, month, day| Date::new(year, month, day).unwrap();
        let mut scene = Node::new("Window", None);
        let part = |kind, x, y, width, height| {
            let mut part = Node::new(kind, None);
            for (name, value) in [("x", x), ("y", y), ("width", width), ("height", height)] {
                part.properties.insert(name, Length(value).into());
            }
            part.properties.insert("week-start", 7_i64.into());
            part
        };
        let mut grid = part("MonthGrid", 44.0, 28.0, 280.0, 216.0);
        grid.properties
            .insert("display-month", date(2021, 1, 1).into());
        grid.properties.insert("date", date(2021, 1, 8).into());
        grid.properties.insert("today", date(2021, 1, 8).into());
        grid.properties
            .insert("highlight-start", date(2021, 1, 7).into());
        grid.properties
            .insert("highlight-end", date(2021, 1, 11).into());
        grid.properties
            .insert("minimum-date", date(2020, 12, 28).into());
        grid.properties.insert(
            "disabled-weekdays",
            xrack_runtime::Value::IntList(vec![6, 7]),
        );
        grid.properties.insert(
            "disabled-dates",
            xrack_runtime::Value::DateList(vec![date(2021, 1, 11)]),
        );
        let cells = xrack_runtime::CalendarCells::default().render(&grid, |day| {
            if [date(2021, 1, 9), date(2021, 1, 11)].contains(&day.date) {
                assert!(!day.enabled);
            }
            let mut cell = Node::new("Rectangle", None);
            if day.today {
                cell.properties
                    .insert("accessible-description", "Release review".to_owned().into());
                let mut text = Node::new("Text", None);
                text.properties
                    .insert("text", "Template label".to_owned().into());
                cell.children.push(text);
            }
            cell
        });
        grid.children.push(cells);
        let weekdays = part("DayOfWeekRow", 44.0, 0.0, 280.0, 28.0);
        let mut weeks = part("WeekNumberColumn", 0.0, 28.0, 44.0, 216.0);
        weeks
            .properties
            .insert("display-month", date(2021, 1, 1).into());
        scene.children.extend([grid, weekdays, weeks]);
        let scene = Rc::new(scene);
        let frame = Rc::new(layout(&scene, Size::new(324.0, 244.0), &mut Text));
        let mut projection = Projection::default();
        let update = projection
            .update(&scene, &frame, &Interaction::default(), 1.0, None)
            .unwrap();
        let (id, selected) = named(&update, "2021-01-08");
        let selected_id = id;
        assert_eq!(selected.toggled(), Some(Toggled::True));
        assert_eq!(
            selected.description(),
            Some("Today. In range. Release review")
        );
        assert!(
            !update
                .nodes
                .iter()
                .any(|(_, node)| node.value() == Some("Template label"))
        );
        assert!(matches!(
            projection.route(id),
            Some(Route::ControlPart {
                action: ControlAction::Calendar(CalendarAction::Activate(day)),
                ..
            }) if day == date(2021, 1, 8)
        ));
        let (id, disabled) = named(&update, "2020-12-27");
        assert!(disabled.is_disabled());
        assert!(projection.route(id).is_none());
        assert_eq!(
            named(&update, "2021-01-07").1.description(),
            Some("Range start")
        );
        for (label, description) in [("2021-01-09", "In range"), ("2021-01-11", "Range end")] {
            let (id, closed) = named(&update, label);
            assert!(closed.is_disabled());
            assert_eq!(closed.description(), Some(description));
            assert!(!closed.supports_action(Action::Click));
            assert!(projection.route(id).is_none());
        }
        for value in ["Sun", "Week 53 of 2020", "Week 1 of 2021"] {
            assert!(
                update
                    .nodes
                    .iter()
                    .any(|(_, node)| node.role() == Role::Label && node.value() == Some(value))
            );
        }

        let mut rtl = scene.as_ref().clone();
        rtl.children[0]
            .properties
            .insert("fixed-month", true.into());
        rtl.properties.insert(
            "layout-direction",
            xrack_runtime::LayoutDirection::RightToLeft.into(),
        );
        let scene = Rc::new(rtl);
        let frame = Rc::new(layout(&scene, Size::new(324.0, 244.0), &mut Text));
        let update = projection
            .update(&scene, &frame, &Interaction::default(), 1.0, None)
            .unwrap();
        let (id, selected) = named(&update, "2021-01-08");
        assert_eq!(id, selected_id);
        let (adjacent, day) = named(&update, "2020-12-28");
        assert!(day.is_disabled());
        assert!(projection.route(adjacent).is_none());
        let geometry =
            CalendarGeometry::for_layout(scene.child_at(0).unwrap(), frame.child_at(0).unwrap());
        let day = geometry.days().find(|day| day.selected).unwrap();
        assert_eq!(frame.at_path(&[0, 0, day.slot]).unwrap().bounds, day.bounds);
        assert_eq!(selected.bounds().unwrap().x0, f64::from(day.bounds.x));
        let x = |label| {
            update
                .nodes
                .iter()
                .find_map(|(_, node)| {
                    (node.value() == Some(label)).then(|| node.bounds().unwrap().x0)
                })
                .unwrap()
        };
        assert!(x("Sun") > x("Mon"));
        assert_eq!(
            selected.description(),
            Some("Today. In range. Release review")
        );
    }

    #[test]
    fn ios_date_picker_focus_tracks_the_displayed_month_and_returns_to_the_trigger() {
        use xrack_runtime::{Date, DateBounds, DatePopupState};

        let date = |month, day| Date::new(2024, month, day).unwrap();
        let popup = DatePopupState::default();
        let mut dialog = Node::new("Dialog", Some("picker"));
        dialog.properties.insert("minimum-date", date(2, 1).into());
        dialog.properties.insert("maximum-date", date(4, 30).into());
        dialog
            .properties
            .insert("disabled-weekdays", Value::IntList(vec![1, 6, 7]));
        let bounds = DateBounds::new(date(2, 1), date(4, 30)).with_disabled_weekdays(
            xrack_runtime::calendar_weekday_mask(&VecModel::from(vec![1, 6, 7])),
        );
        let chosen = date(2, 29);
        let prepare = |open: bool| {
            let mut window = Node::new("Window", None);
            window.children.push(button("choose", "Choose date", 0.0));
            let mut dialog = dialog.clone();
            dialog.properties.insert("open", open.into());
            popup.populate(&mut dialog, || chosen);
            window.children.push(dialog);
            let scene = Rc::new(window);
            let layout = Rc::new(layout(&scene, Size::new(402.0, 812.0), &mut Text));
            (scene, layout)
        };
        let (scene, layout) = prepare(false);
        let mut input = Interaction::default();
        input.accessibility_focus(input.accessibility_target(&scene, &layout, &[0]).unwrap());
        let mut projection = Projection::default();
        let (mut scene, mut layout) = prepare(true);
        for (month, expected_day) in [(2, 29), (3, 1), (4, 2)] {
            if month != 2 {
                assert!(popup.edit(
                    bounds.clone(),
                    scene.at_path(&[1, 0]).unwrap(),
                    ControlAction::Calendar(CalendarAction::Browse(date(month, 1)))
                ));
                (scene, layout) = prepare(true);
            }
            input.reconcile(&scene, &layout);
            let update = projection
                .update_tree(&scene, &layout, &input, 1.0, None, true)
                .unwrap();
            let (active, day) = named(&update, &date(month, expected_day).to_string());
            assert_eq!(update.focus, active, "accessibility focus in month {month}");
            assert!(day.supports_action(Action::Click));
            assert_eq!(
                day.toggled(),
                Some(if month == 2 {
                    Toggled::True
                } else {
                    Toggled::False
                })
            );
            assert_eq!(scene.at_path(&[1, 0]).unwrap().date("date"), Some(chosen));
        }
        let focused = input.focused.as_ref().unwrap();
        assert_eq!(
            input
                .calendar_activate(&scene, &layout, focused)
                .unwrap()
                .action,
            ControlAction::Calendar(CalendarAction::Select(date(4, 2)))
        );
        let (scene, layout) = prepare(false);
        input.reconcile(&scene, &layout);
        let update = projection
            .update_tree(&scene, &layout, &input, 1.0, None, true)
            .unwrap();
        assert_eq!(update.focus, named(&update, "Choose date").0);
    }

    #[test]
    fn ios_date_picker_focus_stays_inside_the_modal_when_every_date_is_disabled() {
        use xrack_runtime::{Date, DatePopupState};
        let mut popup = Node::new("Dialog", Some("picker"));
        let day = |day| Date::new(2024, 2, day).unwrap();
        popup.properties.insert("open", true.into());
        popup.properties.insert("minimum-date", day(1).into());
        popup
            .properties
            .insert("disabled-weekdays", Value::IntList((1..=7).collect()));
        popup
            .properties
            .insert("cancel-text", "Cancel".to_owned().into());
        for (maximum, expected) in [
            (day(29), "Cancel"),
            (Date::new(2024, 3, 31).unwrap(), "Next month"),
        ] {
            let mut popup = popup.clone();
            popup.properties.insert("maximum-date", maximum.into());
            DatePopupState::default().populate(&mut popup, || day(29));
            let mut scene = Node::new("Window", None);
            scene.children.push(popup);
            let scene = Rc::new(scene);
            let layout = Rc::new(layout(&scene, Size::new(402.0, 812.0), &mut Text));
            let mut input = Interaction::default();
            input.reconcile(&scene, &layout);
            assert!(input.is_focused(&[0, 0]));
            let update = Projection::default()
                .update_tree(&scene, &layout, &input, 1.0, None, true)
                .unwrap();
            let (id, node) = named(&update, expected);
            assert!(node.supports_action(Action::Click));
            assert_eq!(update.focus, id);
        }
    }

    #[test]
    fn ios_picker_dialogs_expose_days_and_adjustable_time_wheels() {
        use xrack_runtime::{Date, DatePopupState, Time, TimePopupState};

        let mut date = Node::new("Dialog", Some("date"));
        date.properties.insert("open", true.into());
        date.properties
            .insert("title", "Inspection date".to_owned().into());
        date.properties
            .insert("accept-text", "Save date".to_owned().into());
        let leap_day = Date::new(2024, 2, 29).unwrap();
        DatePopupState::default().populate(&mut date, || leap_day);
        let mut window = Node::new("Window", None);
        window.children.push(date);

        let scene = Rc::new(window);
        let layout = Rc::new(xrack_layout::layout(
            &scene,
            Size::new(320.0, 520.0),
            &mut Text,
        ));
        let mut input = Interaction::default();
        input.reconcile(&scene, &layout);
        let mut projection = Projection::default();
        let update = projection
            .update_tree(&scene, &layout, &input, 1.0, None, true)
            .unwrap();
        let (_, dialog) = named(&update, "Inspection date");
        let (calendar_id, calendar) = named(&update, "Calendar");
        let (day_id, day) = named(&update, "2024-02-29");
        let (_, save) = named(&update, "Save date");
        assert!(dialog.children().contains(&calendar_id));
        assert!(!dialog.supports_action(Action::Focus));
        assert!(!calendar.supports_action(Action::Focus));
        assert!(calendar.value().is_none());
        assert!(calendar.children().contains(&day_id));
        assert!(day.supports_action(Action::Click));
        assert_eq!(day.toggled(), Some(Toggled::True));
        assert_eq!(update.focus, day_id);
        assert!(save.supports_action(Action::Click));

        let mut time = Node::new("Dialog", Some("time"));
        time.properties.insert("open", true.into());
        time.properties
            .insert("title", "Inspection time".to_owned().into());
        TimePopupState::default().populate(&mut time, || Time::new(12, 30, 0).unwrap());
        let mut window = Node::new("Window", None);
        window.children.push(time);
        let scene = Rc::new(window);
        let layout = Rc::new(xrack_layout::layout(
            &scene,
            Size::new(320.0, 520.0),
            &mut Text,
        ));
        let update = projection
            .update_tree(&scene, &layout, &Interaction::default(), 1.0, None, true)
            .unwrap();
        let (minute, wheel) = named(&update, "Minute");
        assert_eq!(wheel.role(), Role::SpinButton);
        assert_eq!(wheel.value(), Some("30"));
        assert!(wheel.supports_action(Action::Increment));
        assert!(wheel.supports_action(Action::Decrement));
        assert!(matches!(
            projection.route(minute),
            Some(Route::Element {
                kind: "Tumbler",
                ..
            })
        ));
    }

    #[test]
    fn ios_path_view_exposes_one_adjustable_selection_with_directional_steps() {
        let mut window = Node::new("Window", None);
        let mut cards = Node::new("PathView", Some("cards"));
        length(&mut cards, "width", 240.0);
        length(&mut cards, "height", 90.0);
        cards
            .properties
            .insert("accessible-name", "Route cards".to_owned().into());
        cards.properties.insert("current-index", 1_i64.into());
        cards.properties.insert("wrap", false.into());
        cards.tumbler_items = Some(xrack_runtime::TumblerItems::new(VecModel::from(vec![
            "Port".to_owned(),
            "Buoy".to_owned(),
            "Reef".to_owned(),
        ])));
        window.children.push(cards);

        let input = Interaction::default();
        let mut projection = Projection::default();
        let mut card_id = None;
        for (index, enabled, value, increment, decrement) in [
            (1, true, "Buoy", true, true),
            (0, true, "Port", true, false),
            (2, true, "Reef", false, true),
            (2, false, "Reef", false, false),
        ] {
            let mut current = window.clone();
            current.children[0]
                .properties
                .insert("current-index", i64::from(index).into());
            current.children[0]
                .properties
                .insert("enabled", enabled.into());
            let scene = Rc::new(current);
            let frame = Rc::new(layout(&scene, Size::new(300.0, 150.0), &mut Text));
            let update = projection
                .update_tree(&scene, &frame, &input, 1.0, None, true)
                .unwrap();
            let (id, picker) = named(&update, "Route cards");
            assert_eq!(*card_id.get_or_insert(id), id);
            assert_eq!(picker.role(), Role::SpinButton);
            assert_eq!(picker.value(), Some(value));
            assert_eq!(picker.is_disabled(), !enabled);
            assert_eq!(picker.supports_action(Action::Increment), increment);
            assert_eq!(picker.supports_action(Action::Decrement), decrement);
            assert!(picker.children().is_empty());
            if enabled {
                assert!(matches!(
                    projection.route_for_action(id, Action::Focus),
                    Some(Route::Element {
                        kind: "PathView",
                        ..
                    })
                ));
            }
        }

        let scene = Rc::new(window);
        let frame = Rc::new(layout(&scene, Size::new(300.0, 150.0), &mut Text));
        let desktop = Projection::default()
            .update_tree(&scene, &frame, &input, 1.0, None, false)
            .unwrap();
        let (_, list) = named(&desktop, "Route cards");
        let (selected_id, selected) = named(&desktop, "Buoy");
        assert_eq!(list.role(), Role::ListBox);
        assert!(list.children().contains(&selected_id));
        assert_eq!(selected.is_selected(), Some(true));
        assert!(selected.supports_action(Action::Click));
    }

    #[test]
    fn section_headings_keep_order_and_identity_when_pinned() {
        use xrack_runtime::{
            Component, ListRowRenderer, ListSectionProjection, ListSectionRenderer,
        };

        struct View;
        impl Component for View {
            fn render(&self) -> Node {
                unreachable!("the test supplies the base scene")
            }

            fn dispatch(&mut self, _: &str, _: &str) -> bool {
                false
            }
        }

        let model = VecModel::from(vec![
            ("A", "First"),
            ("A", "Second"),
            ("B", "Third"),
            ("B", "Fourth"),
        ]);
        let mut window = Node::new("Window", None);
        let mut view = Node::new("StandardListView", Some("agenda"));
        view.properties
            .insert("accessible-name", "Agenda".to_owned().into());
        view.properties
            .insert("sticky-section-headers", true.into());
        length(&mut view, "width", 180.0);
        length(&mut view, "height", 110.0);
        length(&mut view, "row-height", 30.0);
        length(&mut view, "section-height", 26.0);
        view.list = Some(ListMetrics {
            row_count: model.len(),
            first_row: 0,
        });
        view.list_sections =
            Some(ListSectionProjection::default().snapshot(&model, None, |row| row.0));
        view.list_section_renderer = Some(ListSectionRenderer::new(|input| {
            let mut header = Node::new("SectionHeader", None);
            let prefix = if input.pinned { "Pinned " } else { "" };
            header.properties.insert(
                "accessible-name",
                format!("{prefix}{} · {}", input.label, input.count).into(),
            );
            let mut text = Node::new("Text", None);
            text.properties
                .insert("text", "Decorative content".to_owned().into());
            header.children.push(text);
            Some(header)
        }));
        let rows: Vec<_> = model
            .snapshot()
            .into_iter()
            .enumerate()
            .map(|(index, (_, label))| {
                let mut row = Node::new("StandardListItem", None);
                row.row_key = model.row_key(index);
                row.properties.insert("text", label.to_owned().into());
                row
            })
            .collect();
        view.list_row_renderer = Some(ListRowRenderer::new(move |range| rows[range].to_vec()));
        window.children.push(view);
        let base = Rc::new(window);
        let size = Size::new(220.0, 120.0);
        let mut input = Interaction::default();
        let render = |input: &Interaction| {
            let (scene, layout) =
                xrack_layout::prepare_scene_with_input(&View, base.clone(), size, &mut Text, input);
            (scene, Rc::new(layout))
        };
        let (scene, frame) = render(&input);
        let mut projection = Projection::default();
        let before = projection
            .update(&scene, &frame, &input, 1.0, None)
            .unwrap();
        let (_, list) = named(&before, "Agenda");
        let (a, heading) = named(&before, "A · 2");
        let (first, _) = named(&before, "First");
        let (b, _) = named(&before, "B · 2");
        assert_eq!(heading.role(), Role::Heading);
        assert_eq!(list.children()[..2], [a, first]);
        assert_eq!(list.size_of_set(), Some(4));

        assert!(input.accessibility_scroll_to(&frame, &[0], Orientation::Vertical, 42.0));
        let (scene, frame) = render(&input);
        let after = projection
            .update(&scene, &frame, &input, 1.0, None)
            .unwrap();
        let (new_a, a_heading) = named(&after, "Pinned A · 2");
        let (new_b, _) = named(&after, "B · 2");
        let (_, list) = named(&after, "Agenda");
        assert_eq!((new_a, new_b), (a, b));
        assert_eq!(list.children().first(), Some(&a));
        assert_eq!(a_heading.bounds().unwrap().y0, 0.0);
        assert!(
            !after
                .nodes
                .iter()
                .any(|(_, node)| node.label() == Some("Decorative content"))
        );
        assert!(
            !after
                .nodes
                .iter()
                .any(|(_, node)| node.label() == Some("First"))
        );
    }

    #[test]
    fn rtl_scroll_view_and_attached_bar_translate_native_physical_actions() {
        use xrack_runtime::{LayoutDirection, ViewportHandle};

        let mut window = Node::new("Window", None);
        window
            .properties
            .insert("layout-direction", LayoutDirection::RightToLeft.into());
        let mut view = Node::new("ScrollView", Some("canvas"));
        let handle = ViewportHandle::with_scroll_notifications();
        view.viewport_handle = Some(handle.clone());
        view.properties
            .insert("accessible-name", "Canvas".to_owned().into());
        length(&mut view, "width", 120.0);
        length(&mut view, "height", 80.0);
        length(&mut view, "content-width", 360.0);
        let mut bar = Node::new("ScrollBar", Some("bar"));
        bar.scroll_target = Some(handle);
        bar.properties
            .insert("accessible-name", "Position".to_owned().into());
        bar.properties
            .insert("orientation", Orientation::Horizontal.into());
        bar.properties
            .insert("layout-direction", LayoutDirection::LeftToRight.into());
        length(&mut bar, "y", 90.0);
        length(&mut bar, "width", 120.0);
        length(&mut bar, "height", 12.0);
        window.children.extend([view, bar]);

        let scene = Rc::new(window);
        let frame = Rc::new(layout(&scene, Size::new(160.0, 120.0), &mut Text));
        let scale = 2.0;
        let update = Projection::default()
            .update(&scene, &frame, &Interaction::default(), scale, None)
            .unwrap();
        for (index, label) in [(0, "Canvas"), (1, "Position")] {
            let (_, accessible) = named(&update, label);
            assert_eq!(accessible.scroll_x(), Some(480.0));
            assert!(accessible.supports_action(Action::ScrollLeft));
            assert!(!accessible.supports_action(Action::ScrollRight));
            let left = requested_scroll(
                Action::ScrollLeft,
                Some(&ActionData::ScrollUnit(ScrollUnit::Item)),
                &scene.children[index],
                &frame.children[index],
                scale,
            );
            assert!(left[0].is_some_and(|offset| offset > 0.0));
            let set = requested_scroll(
                Action::SetScrollOffset,
                Some(&ActionData::SetScrollOffset(accesskit::Point::new(
                    300.0, 0.0,
                ))),
                &scene.children[index],
                &frame.children[index],
                scale,
            );
            assert_eq!(set[0], Some(90.0));
        }
    }

    #[test]
    fn rtl_table_reports_physical_scroll_position_and_left_scroll_action() {
        let mut table = Node::new("TableView", None);
        table.properties.insert(
            "layout-direction",
            xrack_runtime::LayoutDirection::RightToLeft.into(),
        );
        let columns = (0..2)
            .map(|_| {
                let mut column = Node::new("TableColumn", None);
                length(&mut column, "width", 160.0);
                column
            })
            .collect();
        table.table_header = Some(Box::new(xrack_runtime::TableHeader {
            columns,
            sort: None,
            column_widths: vec![],
        }));
        let item = layout(&table, Size::new(200.0, 120.0), &mut Text);
        let mut accessible = AccessibleNode::new(Role::Table);
        scroll_actions(&mut accessible, &item, 1.0);
        assert_eq!(accessible.scroll_x(), Some(120.0));
        assert!(accessible.supports_action(Action::ScrollLeft));
        assert!(!accessible.supports_action(Action::ScrollRight));
        let offset = requested_scroll(
            Action::ScrollLeft,
            Some(&ActionData::ScrollUnit(ScrollUnit::Item)),
            &table,
            &item,
            1.0,
        );
        assert_eq!(offset, [Some(28.0), None]);
    }

    #[test]
    fn rtl_tree_keeps_accessible_row_identity_and_routes_expand_and_collapse() {
        use xrack_runtime::{LayoutDirection, TreeAction, TreeState};

        let model = Rc::new(VecModel::from(vec![(1_i64, -1_i64), (2, 1)]));
        let state = TreeState::default();
        let scene = |direction| {
            let mut window = Node::new("Window", None);
            window
                .properties
                .insert("layout-direction", Value::LayoutDirection(direction));
            let mut view = Node::new("TreeView", Some("directory"));
            let snapshot = state.snapshot(|| model.clone(), |row| *row);
            view.list = Some(ListMetrics {
                row_count: snapshot.rows().len(),
                first_row: 0,
            });
            for branch in snapshot.rows() {
                let mut row = Node::new("ListRow", None);
                row.row_key = Some(branch.row);
                row.properties
                    .insert("tree-depth", (branch.depth as i64).into());
                row.properties
                    .insert("tree-has-children", branch.has_children.into());
                row.properties
                    .insert("tree-expanded", branch.expanded.into());
                let mut cell = Node::new("TreeCell", None);
                cell.properties
                    .insert("accessible-name", format!("Branch {}", branch.key).into());
                let mut glyph = Node::new("Text", None);
                glyph
                    .properties
                    .insert("text", "Decorative glyph".to_owned().into());
                let mut disclosure = Node::new("TreeDisclosure", None);
                disclosure.children.push(glyph);
                let mut delegate = Node::new("TreeRow", None);
                delegate.children.extend([disclosure, cell]);
                row.children.push(delegate);
                view.children.push(row);
            }
            view.tree = Some(snapshot);
            window.children.push(view);
            Rc::new(window)
        };
        let mut projection = Projection::default();
        let input = Interaction::default();
        let size = Size::new(220.0, 110.0);
        let rtl = scene(LayoutDirection::RightToLeft);
        let frame = Rc::new(layout(&rtl, size, &mut Text));
        let update = projection.update(&rtl, &frame, &input, 1.0, None).unwrap();
        let (row_id, row) = update
            .nodes
            .iter()
            .find(|(_, node)| node.role() == Role::TreeItem)
            .unwrap();
        let row_id = *row_id;
        assert_eq!(row.is_expanded(), Some(false));
        assert!(row.supports_action(Action::Expand));
        assert!(
            update
                .nodes
                .iter()
                .all(|(_, node)| node.label() != Some("Decorative glyph"))
        );
        let (cell_id, cell) = named(&update, "Branch 1");
        let rtl_bounds = cell.bounds().unwrap();
        let Some(Route::Element { target, .. }) = projection.route(row_id) else {
            panic!("branch has no action route");
        };
        let expand = input
            .tree_row_expansion(&rtl, &frame, &target, true)
            .unwrap();
        let ControlAction::Tree(TreeAction::SetExpanded { key, row, expanded }) = expand.action
        else {
            panic!("wrong tree action");
        };
        assert!(state.set_expanded(rtl.children[0].tree.as_ref().unwrap(), key, row, expanded));

        let ltr = scene(LayoutDirection::LeftToRight);
        let frame = Rc::new(layout(&ltr, size, &mut Text));
        let update = projection.update(&ltr, &frame, &input, 1.0, None).unwrap();
        let row = update
            .nodes
            .iter()
            .find(|(id, _)| *id == row_id)
            .unwrap()
            .1
            .clone();
        assert_eq!(row.is_expanded(), Some(true));
        assert!(row.supports_action(Action::Collapse));
        let (updated_cell, cell) = named(&update, "Branch 1");
        assert_eq!(cell_id, updated_cell);
        assert!(cell.bounds().unwrap().x0 > rtl_bounds.x0);
        let collapse = input
            .tree_row_expansion(&ltr, &frame, &target, false)
            .unwrap();
        assert_eq!(
            collapse.action,
            ControlAction::Tree(TreeAction::SetExpanded {
                key,
                row: target.row_key.unwrap(),
                expanded: false
            })
        );
    }

    #[test]
    fn ios_cell_editor_projects_each_draft_and_preserves_the_unheaded_row() {
        use xrack_layout::{EditAction, EditSource};
        use xrack_runtime::{Component, ListModel, TableCellEditor};

        struct View;
        impl Component for View {
            fn render(&self) -> Node {
                unreachable!("the accessibility projection uses the supplied scene")
            }

            fn dispatch(&mut self, _: &str, _: &str) -> bool {
                false
            }
        }

        let mut table = Node::new("TableView", None);
        table.list = Some(ListMetrics {
            row_count: 1,
            first_row: 0,
        });
        let mut column = Node::new("TableColumn", None);
        column
            .properties
            .insert("title", "Quantity".to_owned().into());
        table.table_header = Some(Box::new(xrack_runtime::TableHeader {
            columns: vec![column],
            sort: None,
            column_widths: Vec::new(),
        }));
        let mut row = Node::new("ListRow", None);
        row.row_key = Some(1_i64.row_key(0));
        let mut record = Node::new("TableRow", None);
        let mut cell = Node::new("TableCell", None);
        cell.properties.insert("text", "12".to_owned().into());
        cell.properties.insert("editable", true.into());
        cell.table_cell_editor = Some(TableCellEditor::Int);
        record.children.push(cell);
        row.children.push(record);
        table.children.push(row);

        let scene = Rc::new(table);
        let frame = Rc::new(layout(&scene, Size::new(260.0, 140.0), &mut Text));
        let mut input = Interaction::default();
        let mut projection = Projection::default();
        let original = projection
            .update_tree(&scene, &frame, &input, 1.0, None, true)
            .unwrap();
        let (id, original_cell) = named(&original, "12");
        assert_eq!(original_cell.custom_actions()[0].id, EDIT_CELL_ACTION);
        let (_, row) = original
            .nodes
            .iter()
            .find(|(_, node)| node.role() == Role::Row)
            .unwrap();
        assert!(row.children().is_empty());

        let path = [0, 0, 0];
        let target = input.accessibility_target(&scene, &frame, &path).unwrap();
        assert!(input.begin_accessible_table_edit(&scene, &frame, target));
        for (text, invalid) in [("15", None), ("no", Some(Invalid::True))] {
            assert!(input.edit_text(
                &mut View,
                &scene,
                EditAction::ReplaceAll(text),
                EditSource::Atomic,
                std::time::Instant::now(),
            ));
            let update = projection
                .update_tree(&scene, &frame, &input, 1.0, None, true)
                .unwrap();
            let (editor_id, editor) = named(&update, "Quantity editor");
            assert_eq!(editor_id, id);
            assert_eq!(update.focus, id);
            assert_eq!(editor.role(), Role::TextInput);
            assert_eq!(editor.value(), Some(text));
            assert_eq!(editor.invalid(), invalid);
            assert!(editor.text_selection().is_some());
        }
        assert!(input.cancel_table_edit());
        let cancelled = projection
            .update_tree(&scene, &frame, &input, 1.0, None, true)
            .unwrap();
        assert_eq!(cancelled.focus, id);
        assert_eq!(named(&cancelled, "12").0, id);
    }

    #[test]
    fn ios_sortable_column_reports_order_and_the_next_action() {
        let mut table = Node::new("TableView", Some("inventory"));
        let mut instrument = Node::new("TableColumn", None);
        instrument
            .properties
            .insert("title", "Instrument".to_owned().into());
        let mut location = Node::new("TableColumn", None);
        location
            .properties
            .insert("title", "Location".to_owned().into());
        location.properties.insert("sortable", false.into());
        table.table_header = Some(Box::new(xrack_runtime::TableHeader {
            columns: vec![instrument, location],
            sort: None,
            column_widths: Vec::new(),
        }));

        let mut projection = Projection::default();
        let mut instrument_id = None;
        for (descending, enabled, value, hint) in [
            (None, true, "Unsorted", Some("Sort ascending")),
            (
                Some(false),
                true,
                "Sorted ascending",
                Some("Sort descending"),
            ),
            (
                Some(true),
                true,
                "Sorted descending",
                Some("Sort ascending"),
            ),
            (Some(true), false, "Sorted descending", None),
        ] {
            let mut current = table.clone();
            current.properties.insert("enabled", enabled.into());
            current.table_header.as_mut().unwrap().sort =
                descending.map(|descending| xrack_runtime::TableSort {
                    column: 0,
                    descending,
                });
            let scene = Rc::new(current);
            let frame = Rc::new(layout(&scene, Size::new(360.0, 160.0), &mut Text));
            let update = projection
                .update_tree(&scene, &frame, &Interaction::default(), 1.0, None, true)
                .unwrap();
            let (id, header) = named(&update, "Instrument");
            assert_eq!(*instrument_id.get_or_insert(id), id);
            assert_eq!(header.role(), Role::Button);
            assert_eq!(header.value(), Some(value));
            assert_eq!(header.description().as_deref(), hint);
            assert_eq!(header.supports_action(Action::Click), enabled);
            assert_eq!(header.is_disabled(), !enabled);
            assert_eq!(
                header.sort_direction(),
                descending.map(|descending| if descending {
                    SortDirection::Descending
                } else {
                    SortDirection::Ascending
                })
            );
            let (_, other) = named(&update, "Location");
            assert_eq!(other.role(), Role::ColumnHeader);
            assert!(other.value().is_none());
            assert!(!other.supports_action(Action::Click));
        }
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
        table
            .properties
            .insert("row-header-width", xrack_runtime::Length(64.0).into());
        table.properties.insert("movable-rows", true.into());
        table.standard_list_stamp = Some((0, 1));
        let model = 50_000_i64;
        table.table_selection = Some(Box::new(xrack_runtime::TableSelection::default().snapshot(
            &model,
            Some(200),
            xrack_runtime::SelectionTarget::Rows,
            1,
            true,
        )));
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
        row.row_key = Some(xrack_runtime::ListModel::row_key(&model, 200));
        row.properties
            .insert("row-header-text", "Device 200".to_owned().into());
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
        let (id, header) = named(&update, "Device 200");
        assert_eq!(header.role(), Role::RowHeader);
        assert_eq!(header.row_index(), Some(200));
        assert_eq!(header.is_selected(), Some(true));
        assert!(header.supports_action(Action::Click));
        assert!(header.supports_action(Action::CustomAction));
        assert_eq!(
            header.custom_actions(),
            [
                CustomAction {
                    id: 0,
                    description: "Move row up".into()
                },
                CustomAction {
                    id: 1,
                    description: "Move row down".into()
                },
            ]
        );
        let Some(Route::TableRowHeader(target)) = projection.route(id) else {
            panic!("expected a row header action");
        };
        assert_eq!(target.row_key, scene.children[0].row_key);
        let action = Interaction::default()
            .table_row_header_selection(
                &scene,
                &frame,
                &target,
                xrack_runtime::RowSelection::Replace,
            )
            .unwrap();
        assert_eq!(
            action.action,
            ControlAction::TableSelection(xrack_runtime::TableSelectionAction::Row {
                index: 200,
                operation: xrack_runtime::RowSelection::Replace,
            })
        );
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
            if label == "B" {
                cell.properties.insert("editable", true.into());
                cell.table_cell_editor = Some(xrack_runtime::TableCellEditor::String);
            }
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
        assert_eq!(frozen.value(), Some("Cell B"));
        assert!(frozen.supports_action(Action::SetValue));
        assert!(!named(&first, "Cell A").1.supports_action(Action::SetValue));
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
    fn ios_list_items_keep_visible_row_identity_and_forward_scrolling() {
        let model = VecModel::from(vec![0_i32, 1, 2, 3, 4]);
        let titles = ["Pump", "Valve", "Cabinet", "Meter", "Seal"];
        let project = |first: usize| {
            let mut list = Node::new("ListView", Some("tasks"));
            list.properties
                .insert("accessible-name", "Tasks".to_owned().into());
            length(&mut list, "row-height", 32.0);
            length(&mut list, "y", 32.0);
            length(&mut list, "width", 200.0);
            length(&mut list, "height", 64.0);
            list.list = Some(ListMetrics {
                row_count: titles.len(),
                first_row: first,
            });
            for (slot, title) in titles.iter().enumerate().skip(first).take(3) {
                let mut row = Node::new("ListRow", None);
                row.row_key = model.row_key(slot);
                let mut content = Node::new("Rectangle", None);
                content.children.push(button("task", title, 0.0));
                row.children.push(content);
                list.children.push(row);
            }
            let mut footer = button("footer", "After tasks", 0.0);
            length(&mut footer, "y", 102.0);
            let mut group = Node::new("Rectangle", None);
            length(&mut group, "width", 200.0);
            length(&mut group, "height", 140.0);
            group
                .children
                .extend([button("header", "Before tasks", 0.0), list, footer]);
            let mut root = Node::new("Window", None);
            root.children.push(group);
            let scene = Rc::new(root);
            let bounds = Size::new(200.0, 140.0);
            let offsets = BTreeMap::from([(vec![0, 1], first as f32 * 32.0)]);
            let layout = Rc::new(xrack_layout::layout_with_scroll(
                &scene, bounds, &mut Text, &offsets,
            ));
            (scene, layout)
        };
        let mut projection = Projection::default();
        let input = Interaction::default();
        let (scene, layout) = project(0);
        let update = projection
            .update_tree(&scene, &layout, &input, 1.0, None, true)
            .unwrap();
        let (list, list_node) = named(&update, "Tasks");
        let (pump, _) = named(&update, "Pump");
        let (valve, item) = named(&update, "Valve");
        let window = &update.nodes.iter().find(|(id, _)| *id == ROOT).unwrap().1;
        assert_eq!(
            window.children(),
            &[
                named(&update, "Before tasks").0,
                pump,
                valve,
                list,
                named(&update, "After tasks").0
            ],
        );
        assert!(list_node.children().is_empty());
        assert_eq!(list_node.controls(), &[pump, valve]);
        assert_eq!(item.position_in_set(), Some(1));
        assert_eq!(item.size_of_set(), Some(5));
        assert!(item.supports_action(Action::Click));
        assert!(item.supports_action(Action::ScrollDown));
        assert!(matches!(
            projection.route_for_action(valve, Action::Click),
            Some(Route::Element { target, kind: "Button", .. }) if target.row_key == model.row_key(1)
        ));
        assert!(matches!(
            projection.route_for_action(valve, Action::ScrollDown),
            Some(Route::Element {
                kind: "ListView",
                ..
            })
        ));

        let (scene, layout) = project(1);
        let update = projection
            .update_tree(&scene, &layout, &input, 1.0, None, true)
            .unwrap();
        assert_eq!(named(&update, "Valve").0, valve);
        assert_eq!(named(&update, "Cabinet").1.position_in_set(), Some(2));
        assert!(projection.route_for_action(pump, Action::Click).is_none());
        assert!(
            projection
                .route_for_action(pump, Action::ScrollDown)
                .is_none()
        );
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
        grid.properties.insert("multi-select", true.into());
        grid.properties.insert("current-index", 300_i64.into());
        xrack_runtime::TableSelection::default().populate_list_scene(&mut grid, &50_000_i64);
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
        assert!(grid.is_multiselectable());
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
    fn application_window_accessibility_uses_the_visual_band_order() {
        let mut window = Node::new("Window", None);
        window.application_window = true;
        window
            .properties
            .insert("title", "Editor".to_owned().into());
        let mut footer = Node::new("PageFooter", None);
        footer
            .properties
            .insert("accessible-name", "Footer".to_owned().into());
        footer.children.push(button("save", "Save", 0.0));
        let mut header = Node::new("PageHeader", None);
        header
            .properties
            .insert("accessible-name", "Header".to_owned().into());
        header.children.push(button("back", "Back", 0.0));
        let mut menu = Node::new("MenuBar", None);
        menu.properties
            .insert("accessible-name", "Menu".to_owned().into());
        let mut file = Node::new("Menu", None);
        file.properties.insert("title", "File".to_owned().into());
        menu.children.push(file);
        window.children = vec![footer, button("body", "Content", 0.0), menu, header];
        let scene = Rc::new(window);
        let frame = Rc::new(layout(&scene, Size::new(240.0, 180.0), &mut Text));
        let update = Projection::default()
            .update(&scene, &frame, &Interaction::default(), 1.0, None)
            .unwrap();
        let (_, root) = named(&update, "Editor");
        assert_eq!(
            root.children(),
            &["Menu", "Header", "Content", "Footer"].map(|label| named(&update, label).0)
        );
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
        let mut bar = Node::new("ProgressIndicator", None);
        length(&mut bar, "x", 102.0);
        length(&mut bar, "width", 60.0);
        length(&mut bar, "height", 24.0);
        bar.properties
            .insert("accessible-name", "Transfer".to_owned().into());
        bar.properties.insert("minimum", 100.0_f64.into());
        bar.properties.insert("maximum", 900.0_f64.into());
        bar.properties.insert("progress", 380.0_f64.into());
        let mut track = Node::new("ProgressTrack", None);
        let mut decoration = Node::new("Label", None);
        decoration
            .properties
            .insert("text", "Artwork".to_owned().into());
        track.children.push(decoration);
        bar.children.push(track);
        let mut root = Node::new("Window", None);
        root.children = vec![busy, spinner, bar];
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
        let (_, bar) = named(&update, "Transfer");
        assert_eq!(bar.role(), Role::ProgressIndicator);
        assert_eq!(bar.min_numeric_value(), Some(100.0));
        assert_eq!(bar.max_numeric_value(), Some(900.0));
        assert_eq!(bar.numeric_value(), Some(380.0));
        assert!(
            !update
                .nodes
                .iter()
                .any(|(_, node)| node.value() == Some("Artwork"))
        );

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
    fn password_projection_publishes_only_masked_text_and_maps_utf8_selection() {
        use xrack_runtime::InputType;

        let secret = "e\u{301}👩‍💻";
        for (kind, text, role, revealed, marker) in [
            ("LineEdit", "text", Role::PasswordInput, false, "•"),
            ("LineEdit", "text", Role::PasswordInput, true, "x"),
            ("ComboBox", "edit-text", Role::EditableComboBox, false, "🇨🇳"),
        ] {
            let mut field = Node::new(kind, Some("passphrase"));
            field
                .properties
                .insert("input-type", InputType::Password.into());
            field.properties.insert(text, secret.to_owned().into());
            if kind == "ComboBox" {
                field.properties.insert("editable", true.into());
            }
            field.properties.insert("password-visible", revealed.into());
            field
                .properties
                .insert("password-character", marker.to_owned().into());
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
                .find(|(_, node)| node.role() == role)
                .unwrap();
            assert!(field.value().is_none());
            assert!(field.supports_action(Action::SetTextSelection));
            let run = &update
                .nodes
                .iter()
                .find(|(id, _)| field.children().contains(id))
                .unwrap()
                .1;
            assert_eq!(run.value(), Some(marker.repeat(2).as_str()));
            assert_eq!(run.character_lengths(), &[marker.len() as u8; 2]);
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
    fn text_run_bounds_follow_the_editors_asymmetric_insets() {
        use xrack_runtime::Length;
        let text_system = xrack_renderer_femtovg::TextSystem::bundled_only();
        let mut editor = Node::new("LineEdit", Some("entry"));
        editor.properties.insert("text", "WW".to_owned().into());
        let text_bounds = |node: Node| {
            let scene = Rc::new(node);
            let frame = Rc::new(layout(&scene, Size::new(240.0, 80.0), &mut Text));
            let update = Projection::default()
                .update(
                    &scene,
                    &frame,
                    &Interaction::default(),
                    1.0,
                    Some(&text_system),
                )
                .unwrap();
            update
                .nodes
                .iter()
                .find(|(_, node)| node.role() == Role::TextRun)
                .and_then(|(_, node)| node.bounds())
                .unwrap()
        };
        let uniform = text_bounds(editor.clone());
        editor.properties.insert("padding-top", Length(26.0).into());
        editor
            .properties
            .insert("padding-left", Length(28.0).into());
        let asymmetric = text_bounds(editor);
        assert_eq!(asymmetric.x0 - uniform.x0, 22.0);
        assert_eq!(asymmetric.y0 - uniform.y0, 10.0);
    }

    #[test]
    fn password_projection_stays_masked_during_timed_echo() {
        use xrack_layout::{EditAction, EditSource, PasswordMask};
        use xrack_runtime::{Component, InputType};

        struct Editor(Node);
        impl Component for Editor {
            fn render(&self) -> Node {
                self.0.clone()
            }

            fn dispatch(&mut self, _: &str, _: &str) -> bool {
                false
            }

            fn edit_at(&mut self, _: &Node, _: &[usize], value: String) -> bool {
                self.0.properties.insert("text", value.into());
                true
            }
        }

        let mut field = Node::new("LineEdit", Some("passphrase"));
        field
            .properties
            .insert("input-type", InputType::Password.into());
        field
            .properties
            .insert("password-mask-delay", 1000_i64.into());
        let mut editor = Editor(field);
        let old_scene = editor.render();
        let size = Size::new(240.0, 40.0);
        let old_frame = layout(&old_scene, size, &mut Text);
        let mut input = Interaction::default();
        input.accessibility_focus(
            input
                .accessibility_target(&old_scene, &old_frame, &[])
                .unwrap(),
        );
        input.sync_editor(&old_scene);
        assert!(input.edit_text(
            &mut editor,
            &old_scene,
            EditAction::Insert("🔒"),
            EditSource::Keyboard,
            std::time::Instant::now()
        ));
        let scene = Rc::new(editor.render());
        let frame = Rc::new(layout(&scene, size, &mut Text));
        let visual = PasswordMask::for_editor(&scene, "🔒", input.displayed_editor(&scene, &[]));
        assert_eq!(visual.display(), "🔒");
        let update = Projection::default()
            .update(&scene, &frame, &input, 1.0, None)
            .unwrap();
        assert!(!format!("{update:?}").contains("🔒"));
        assert!(
            update
                .nodes
                .iter()
                .any(|(_, node)| node.role() == Role::TextRun && node.value() == Some("•"))
        );
    }

    #[test]
    fn tab_widget_keeps_selected_page_children_when_its_header_scrolls_offscreen() {
        let mut tabs = Node::new("TabWidget", Some("tabs"));
        tabs.properties
            .insert("accessible-name", "Pages".to_owned().into());
        for index in 0..6 {
            let mut tab = Node::new("Tab", None);
            tab.properties
                .insert("title", format!("Page {index}").into());
            if index == 0 {
                tab.children.push(button("editor", "Page editor", 0.0));
            }
            tabs.children.push(tab);
        }
        let scene = Rc::new(tabs);
        let size = Size::new(166.0, 160.0);
        let mut input = Interaction::default();
        let initial = xrack_layout::layout_with_input(&scene, size, &mut Text, &input);
        let scroll = initial.scroll_on(Orientation::Horizontal).unwrap();
        assert!(input.accessibility_scroll_to(
            &initial,
            &[],
            Orientation::Horizontal,
            scroll.max_offset
        ));
        let frame = Rc::new(xrack_layout::layout_with_input(
            &scene, size, &mut Text, &input,
        ));
        let header = frame.children[0].tab_header.unwrap();
        assert_eq!(frame.children[0].visible_window_bounds(header).width, 0.0);
        let tree = Projection::default()
            .update(&scene, &frame, &input, 1.0, None)
            .unwrap();
        let (_, tabs) = named(&tree, "Pages");
        assert_eq!(tabs.scroll_x_max(), Some(f64::from(scroll.max_offset)));
        assert!(tabs.supports_action(Action::ScrollLeft));
        let (_, editor) = named(&tree, "Page editor");
        assert_eq!(editor.role(), Role::Button);
        assert!(editor.bounds().unwrap().y0 >= 34.0);
    }

    #[test]
    fn tab_close_affordances_are_named_and_route_to_their_own_tab() {
        let mut bar = Node::new("TabBar", Some("tab-strip"));
        bar.properties.insert("tabs-closable", true.into());
        for (name, enabled, closable) in [
            ("Pinned", true, false),
            ("Notes", true, true),
            ("Disabled", false, true),
        ] {
            let mut tab = Node::new("TabButton", None);
            tab.properties.insert("text", name.to_owned().into());
            tab.properties.insert("value", name.to_lowercase().into());
            tab.properties.insert("enabled", enabled.into());
            tab.properties.insert("closable", closable.into());
            bar.children.push(tab);
        }
        let scene = Rc::new(bar);
        let frame = Rc::new(layout(&scene, Size::new(400.0, 42.0), &mut Text));
        let input = Interaction::default();
        let mut projection = Projection::default();
        let update = projection
            .update(&scene, &frame, &input, 1.0, None)
            .unwrap();
        let (id, close) = named(&update, "Close Notes");
        assert_eq!(close.role(), Role::Button);
        assert!(close.supports_action(Action::Click));
        let Some(Route::TabClose(target)) = projection.route(id) else {
            panic!("missing close route");
        };
        assert_eq!(target.path, [1]);
        let close = input.tab_close_action(&scene, &frame, &target).unwrap();
        assert_eq!(
            close.action,
            ControlAction::CloseTab(scene.tab_close(1).unwrap())
        );
        assert!(close.path.is_empty());

        let mut replaced = scene.as_ref().clone();
        replaced.children[1]
            .properties
            .insert("value", "replacement".to_owned().into());
        let replacement_frame = layout(&replaced, Size::new(400.0, 42.0), &mut Text);
        assert!(
            input
                .tab_close_action(&replaced, &replacement_frame, &target)
                .is_none()
        );
        assert!(
            !named(&update, "Close Disabled")
                .1
                .supports_action(Action::Click)
        );
        assert!(
            update
                .nodes
                .iter()
                .all(|(_, node)| node.label() != Some("Close Pinned"))
        );
    }

    #[test]
    fn ios_static_content_inherits_only_its_own_viewport_directions() {
        let mut window = Node::new("Window", None);
        let mut viewport = Node::new("ScrollView", Some("log"));
        length(&mut viewport, "width", 240.0);
        length(&mut viewport, "height", 100.0);
        length(&mut viewport, "content-height", 260.0);
        let mut heading = Node::new("Text", Some("log-heading"));
        heading
            .properties
            .insert("text", "Service notes".to_owned().into());
        let mut sibling = Node::new("Text", Some("outside"));
        length(&mut sibling, "y", 120.0);
        sibling
            .properties
            .insert("text", "Outside the viewport".to_owned().into());
        viewport.children.push(heading);
        window.children.extend([viewport, sibling]);
        let scene = Rc::new(window);
        let frame = Rc::new(layout(&scene, Size::new(280.0, 160.0), &mut Text));
        let mut projection = Projection::default();
        let tree = projection
            .update_tree(&scene, &frame, &Interaction::default(), 1.0, None, true)
            .unwrap();
        let (heading_id, heading) = named(&tree, "Service notes");
        assert!(heading.supports_action(Action::ScrollDown));
        assert!(!heading.supports_action(Action::ScrollUp));
        assert!(!heading.supports_action(Action::Click));
        assert!(matches!(
            projection.route_for_action(heading_id, Action::ScrollDown),
            Some(Route::Element {
                kind: "ScrollView",
                ..
            })
        ));
        assert!(
            !named(&tree, "Outside the viewport")
                .1
                .supports_action(Action::ScrollDown)
        );
    }

    #[test]
    fn ios_pager_routes_each_direction_to_the_closest_available_owner() {
        let mut window = Node::new("Window", None);
        let mut pager = Node::new("SwipeView", Some("pager"));
        length(&mut pager, "width", 240.0);
        length(&mut pager, "height", 120.0);
        pager
            .properties
            .insert("accessible-name", "Inspection pages".to_owned().into());
        let mut viewport = Node::new("ScrollView", Some("viewport"));
        length(&mut viewport, "width", 240.0);
        length(&mut viewport, "height", 120.0);
        length(&mut viewport, "content-height", 240.0);
        viewport.children.push(button("details", "Details", 0.0));
        let mut end = button("end", "End of details", 0.0);
        length(&mut end, "y", 188.0);
        viewport.children.push(end);
        let mut first = Node::new("SwipePage", None);
        first.children.push(viewport);
        let mut hidden = Node::new("SwipePage", None);
        hidden.properties.insert("visible", false.into());
        let mut last = Node::new("SwipePage", None);
        last.children.push(button("done", "Done", 0.0));
        pager.children.extend([first, hidden, last]);
        window.children.push(pager);

        let check = |scene: Rc<Node>, forward| {
            let mut input = Interaction::default();
            let frame = |input: &Interaction| {
                Rc::new(xrack_layout::layout_with_input(
                    &scene,
                    Size::new(280.0, 160.0),
                    &mut Text,
                    input,
                ))
            };
            let initial = frame(&input);
            let mut projection = Projection::default();
            let update = projection
                .update_tree(&scene, &initial, &input, 1.0, None, true)
                .unwrap();
            let (_, pager) = named(&update, "Inspection pages");
            assert!(!pager.supports_action(Action::Focus));
            let (details, element) = named(&update, "Details");
            assert!(element.supports_action(Action::Click));
            assert!(element.supports_action(Action::ScrollDown));
            assert!(!element.supports_action(Action::ScrollUp));
            assert!(matches!(
                projection.route_for_action(details, Action::ScrollDown),
                Some(Route::Element {
                    kind: "ScrollView",
                    ..
                })
            ));
            if forward != Action::ScrollDown {
                assert!(element.supports_action(forward));
                assert!(matches!(
                    projection.route_for_action(details, forward),
                    Some(Route::Element {
                        kind: "SwipeView",
                        ..
                    })
                ));
            }
            assert!(input.accessibility_scroll_to(
                &initial,
                &[0, 0, 0],
                Orientation::Vertical,
                120.0,
            ));
            let end = frame(&input);
            let update = projection
                .update_tree(&scene, &end, &input, 1.0, None, true)
                .unwrap();
            let (details, element) = named(&update, "End of details");
            assert!(element.supports_action(Action::ScrollUp));
            assert!(element.supports_action(forward));
            assert!(matches!(
                projection.route_for_action(details, forward),
                Some(Route::Element {
                    kind: "SwipeView",
                    ..
                })
            ));
            assert_eq!(
                swipe_scroll_selection(forward, &scene.children[0], end.child_at(0).unwrap()),
                Some(2)
            );
        };

        check(Rc::new(window.clone()), Action::ScrollRight);
        let mut locked = window.clone();
        locked.children[0]
            .properties
            .insert("interactive", false.into());
        let locked = Rc::new(locked);
        let frame = Rc::new(layout(&locked, Size::new(280.0, 160.0), &mut Text));
        let update = Projection::default()
            .update_tree(&locked, &frame, &Interaction::default(), 1.0, None, true)
            .unwrap();
        let (_, details) = named(&update, "Details");
        assert!(!details.supports_action(Action::ScrollRight));
        assert!(details.supports_action(Action::ScrollDown));

        let mut rtl = window.clone();
        rtl.children[0].properties.insert(
            "layout-direction",
            xrack_runtime::LayoutDirection::RightToLeft.into(),
        );
        check(Rc::new(rtl), Action::ScrollLeft);
        window.children[0]
            .properties
            .insert("orientation", Orientation::Vertical.into());
        check(Rc::new(window), Action::ScrollDown);
    }

    #[test]
    fn ios_tab_headers_forward_scroll_to_their_strip_in_each_direction() {
        use xrack_runtime::LayoutDirection;

        for (kind, axis, direction, forward, backward) in [
            (
                "TabBar",
                Orientation::Horizontal,
                LayoutDirection::LeftToRight,
                Action::ScrollRight,
                Action::ScrollLeft,
            ),
            (
                "TabBar",
                Orientation::Horizontal,
                LayoutDirection::RightToLeft,
                Action::ScrollLeft,
                Action::ScrollRight,
            ),
            (
                "TabWidget",
                Orientation::Vertical,
                LayoutDirection::LeftToRight,
                Action::ScrollDown,
                Action::ScrollUp,
            ),
        ] {
            let mut strip = Node::new(kind, Some("pages"));
            strip.properties.insert("orientation", axis.into());
            strip
                .properties
                .insert("layout-direction", direction.into());
            strip.properties.insert("tabs-closable", true.into());
            for index in 0..5 {
                let widget = kind == "TabWidget";
                let mut tab = Node::new(if widget { "Tab" } else { "TabButton" }, None);
                length(&mut tab, "width", 84.0);
                tab.properties.insert(
                    if widget { "title" } else { "text" },
                    format!("Page {index}").into(),
                );
                if index == 0 && widget {
                    tab.children.push(button("page", "Page action", 0.0));
                }
                strip.children.push(tab);
            }
            let scene = Rc::new(strip);
            let mut input = Interaction::default();
            let size = Size::new(178.0, 98.0);
            let frame = Rc::new(xrack_layout::layout_with_input(
                &scene, size, &mut Text, &input,
            ));
            let mut projection = Projection::default();
            let tree = projection
                .update_tree(&scene, &frame, &input, 2.0, None, true)
                .unwrap();
            let (tab_id, tab) = named(&tree, "Page 0");
            let (close_id, close) = named(&tree, "Close Page 0");
            for (id, node) in [(tab_id, tab), (close_id, close)] {
                assert!(node.supports_action(forward));
                assert!(!node.supports_action(backward));
                assert!(matches!(projection.route_for_action(id, forward),
                    Some(Route::Element { target, .. }) if target.path.is_empty()));
            }
            assert!(
                matches!(projection.route_for_action(close_id, Action::Click),
                Some(Route::TabClose(target)) if target.path == [0])
            );
            let scroll = frame.scroll_on(axis).unwrap();
            let view = frame.scroll_viewport(axis).intersect(frame.clip);
            let page = match axis {
                Orientation::Horizontal => view.width,
                Orientation::Vertical => view.height,
            };
            let (expected, end) =
                if axis == Orientation::Horizontal && direction == LayoutDirection::RightToLeft {
                    ((scroll.offset - page).max(0.0), 0.0)
                } else {
                    (
                        (scroll.offset + page).min(scroll.max_offset),
                        scroll.max_offset,
                    )
                };
            assert_eq!(
                requested_scroll(forward, None, &scene, &frame, 2.0)
                    [usize::from(axis == Orientation::Vertical)],
                Some(expected)
            );

            assert!(input.accessibility_scroll_to(&frame, &[], axis, end));
            let frame = Rc::new(xrack_layout::layout_with_input(
                &scene, size, &mut Text, &input,
            ));
            let tree = projection
                .update_tree(&scene, &frame, &input, 2.0, None, true)
                .unwrap();
            let (_, last) = named(&tree, "Page 4");
            assert!(last.supports_action(backward));
            assert!(!last.supports_action(forward));
            if kind == "TabWidget" {
                let (_, button) = named(&tree, "Page action");
                assert!(button.supports_action(Action::Click));
                assert!(!button.supports_action(backward));
            }
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
            let mut content = Node::new("ButtonContent", None);
            content
                .children
                .push(button("decoration", "Decorative badge", 0.0));
            tab.children.push(content);
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
        assert!(overview.children().is_empty());
        let (_, disabled) = named(&update, "Disabled");
        assert_eq!(disabled.is_selected(), Some(false));
        assert!(!disabled.supports_action(Action::Click));

        let mut mirrored = scene.as_ref().clone();
        mirrored.properties.insert(
            "layout-direction",
            xrack_runtime::LayoutDirection::RightToLeft.into(),
        );
        let mirrored = Rc::new(mirrored);
        let frame = Rc::new(layout(&mirrored, Size::new(160.0, 40.0), &mut Text));
        let update = Projection::default()
            .update(&mirrored, &frame, &input, 1.0, None)
            .unwrap();
        let (_, tabs) = named(&update, "Pages");
        assert_eq!(tabs.scroll_x(), Some(80.0));
        assert!(tabs.supports_action(Action::ScrollLeft));
        let (_, overview) = named(&update, "Overview");
        assert_eq!(overview.is_selected(), Some(true));
        assert_eq!(overview.bounds().unwrap().x0, 80.0);
        assert_eq!(overview.bounds().unwrap().x1, 160.0);
    }

    #[test]
    fn accessibility_keeps_the_opted_in_selection_on_an_unfocused_editor() {
        let text = "e\u{301}clair";
        let mut editor = Node::new("TextArea", Some("editor"));
        length(&mut editor, "width", 170.0);
        length(&mut editor, "height", 60.0);
        editor
            .properties
            .insert("accessible-name", "Editor".to_owned().into());
        editor.properties.insert("text", text.to_owned().into());
        editor
            .properties
            .insert("persistent-selection", true.into());
        let mut button = Node::new("Button", Some("toolbar"));
        length(&mut button, "y", 72.0);
        button
            .properties
            .insert("text", "Toolbar".to_owned().into());
        let mut root = Node::new("Window", None);
        root.children = vec![editor, button];
        let scene = Rc::new(root);
        let frame = Rc::new(layout(&scene, Size::new(200.0, 150.0), &mut Text));
        let mut input = Interaction::default();
        for path in [[0], [1]] {
            input.accessibility_focus(input.accessibility_target(&scene, &frame, &path).unwrap());
            input.sync_editor(&scene);
            if path == [0] {
                input
                    .editor
                    .as_mut()
                    .unwrap()
                    .set_selection(text, 3, TextPosition::default());
            }
        }
        let mut tree = Projection::default();
        let update = tree.update(&scene, &frame, &input, 1.0, None).unwrap();
        let (editor_id, editor) = named(&update, "Editor");
        let (button_id, _) = named(&update, "Toolbar");
        assert_eq!(update.focus, button_id);
        let selection = editor.text_selection().unwrap();
        assert_eq!(
            tree.text_selection(editor_id, *selection, text),
            Some((3, TextPosition::default()))
        );
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
    fn radio_group_announces_its_title_and_keeps_an_explicit_accessible_name() {
        let mut root = Node::new("Window", None);
        let mut group = Node::new("RadioGroup", Some("options"));
        group
            .properties
            .insert("title", "Rendering quality".to_owned().into());
        let mut option = Node::new("RadioButton", None);
        option.properties.insert("text", "High".to_owned().into());
        group.children.push(option);
        root.children.push(group);
        let input = Interaction::default();
        let mut projection = Projection::default();
        for accessible_name in [None, Some("Graphics quality")] {
            if let Some(name) = accessible_name {
                root.children[0]
                    .properties
                    .insert("accessible-name", name.to_owned().into());
            }
            let scene = Rc::new(root.clone());
            let layout = Rc::new(layout(&scene, Size::new(200.0, 130.0), &mut Text));
            let update = projection
                .update(&scene, &layout, &input, 1.0, None)
                .unwrap();
            let (_, group) = named(&update, accessible_name.unwrap_or("Rendering quality"));
            assert_eq!(group.role(), Role::RadioGroup);
            let (id, radio) = named(&update, "High");
            assert!(group.children().contains(&id));
            assert_eq!(radio.role(), Role::RadioButton);
            assert!(radio.supports_action(Action::Click));
        }
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
                let kind = if row == 0 {
                    "RadioButton"
                } else {
                    "ToolButton"
                };
                let mut radio = Node::new(kind, None);
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
        assert_eq!(named(&update, "Left group Two").1.role(), Role::RadioButton);
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
        let mut indicator = Node::new("ComboIndicator", None);
        length(&mut indicator, "width", 44.0);
        let mut artwork = Node::new("Label", None);
        artwork
            .properties
            .insert("text", "Dropdown artwork".to_owned().into());
        indicator.children.push(artwork);
        combo.children.push(indicator);
        let mut content = Node::new("ComboContent", None);
        let mut heading = Node::new("Label", None);
        heading
            .properties
            .insert("text", "Custom selection label".to_owned().into());
        content.children.push(heading);
        combo.children.push(content);
        let mut style = Node::new("ComboPopup", None);
        length(&mut style, "row-height", 36.0);
        length(&mut style, "padding", 6.0);
        combo.children.push(style);
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
        assert_eq!(combo.value(), Some("Alpha"));
        assert_eq!(combo.controls().len(), 1);
        assert!(
            update
                .nodes
                .iter()
                .all(|(_, node)| node.label() != Some("Dropdown artwork")
                    && node.value() != Some("Custom selection label"))
        );
        let (alpha, option) = named(&update, "Alpha");
        assert_eq!(option.role(), Role::ListBoxOption);
        let accessible = option.bounds().unwrap();
        assert_eq!(
            (accessible.x0, accessible.y0, accessible.y1),
            (6.0, 42.0, 78.0)
        );
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
        root.properties.insert(
            "layout-direction",
            xrack_runtime::LayoutDirection::RightToLeft.into(),
        );
        let mut bar = Node::new("MenuBar", None);
        length(&mut bar, "width", 420.0);
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
        assert!(item.bounds().unwrap().x1 <= export_item.bounds().unwrap().x0);
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
