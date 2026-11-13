use crate::{
    viewport,
    visual_cache::{VisualBindings, repeat_scope},
    *,
};
use std::{cell::RefCell, collections::HashMap};
use xrack_mlir::{
    AliasGraphs, AnimationRef, ComponentAnalysis, Module, OperationRef, RecordDeclRef, RepeatRef,
    StateCaseRef, TranslationRef, ValueRef,
};
use xrack_resources::visual::{
    ControlDefault, ControlLiteral, control_default, controlled_properties, default_combo_edit_text,
};
use xrack_runtime::{self as runtime, Percent};
use xrack_syntax::ast::{Parameter, Type};

/// Load XR dialect text without consulting the filesystem.
pub fn load_mlir(source: &str) -> Result<Program> {
    Program::from_module(&xrack_mlir::process(source).map_err(Error::Invalid)?)
}

pub(crate) fn program(
    module: &Module,
    translations: Option<TranslationCatalog>,
    fonts: Option<LoadedFonts>,
    images: BTreeMap<String, ImageAsset>,
    paths: BTreeMap<String, xrack_resources::EmbeddedPath>,
) -> Result<Program> {
    module.verify().map_err(Error::Invalid)?;
    let records = Rc::new(
        module
            .operations()
            .filter(|op| op.name() == "xr.record_decl")
            .map(|op| {
                let record =
                    RecordDeclRef::from_operation(op).map_err(|error| invalid(op, error))?;
                let fields = record
                    .fields
                    .into_iter()
                    .map(|field| {
                        Ok(RecordField {
                            name: field.name,
                            ty: kind(op, &field.ty)?,
                        })
                    })
                    .collect::<Result<_>>()?;
                Ok((record.name.to_owned(), fields))
            })
            .collect::<Result<BTreeMap<_, _>>>()?,
    );
    let aliases = AliasGraphs::from_module(module).map_err(Error::Invalid)?;
    let assets = Rc::new(AssetValues {
        images,
        markdown: xrack_resources::collect_markdown(module).map_err(Error::Invalid)?,
        paths: paths
            .into_iter()
            .map(|(commands, path)| {
                let value = LoadedVectorPath::new(commands.clone(), path.bounds, path.segments);
                (commands, value)
            })
            .collect(),
        ..AssetValues::default()
    });
    let mut components = BTreeMap::new();
    let mut exports = BTreeSet::new();
    for op in module.operations() {
        match op.name() {
            "xr.component" => {
                let name = attribute(op, "sym_name")?.to_owned();
                if op.attribute("exported") == Some("true") {
                    exports.insert(name.clone());
                }
                components.insert(
                    name,
                    Loader::definition(
                        op,
                        module.has_visual_cache_plan(),
                        translations.is_some(),
                        records.clone(),
                        assets.clone(),
                        &aliases,
                    ),
                );
            }
            "xr.record_decl" | "xr.translation_catalog" => {}
            "xr.font" if fonts.is_some() => {}
            _ => return Err(unsupported(op, op.name())),
        }
    }
    Ok(Program {
        components: Rc::new(components),
        exports: Rc::new(exports),
        record_validator: crate::host::record_validator(records.clone()),
        records,
        translations: translations.map(Rc::new),
        fonts,
    })
}

type Op<'m> = OperationRef<'m>;

fn location(op: Op<'_>) -> String {
    let span = op.span();
    format!(
        "{}:{}:{}",
        op.source_file().unwrap_or("<MLIR>"),
        span.line,
        span.column
    )
}

fn unsupported(op: Op<'_>, feature: &str) -> Error {
    Error::Unsupported(format!(
        "{}: interpreter does not support {feature}",
        location(op)
    ))
}

fn invalid(op: Op<'_>, message: impl std::fmt::Display) -> Error {
    Error::Invalid(format!("{}: {message}", location(op)))
}

fn attribute<'m>(op: Op<'m>, name: &str) -> Result<&'m str> {
    op.attr(name).map_err(|error| invalid(op, error))
}

fn operand(op: Op<'_>, index: usize) -> Result<ValueRef<'_>> {
    op.operand(index)
        .ok_or_else(|| invalid(op, format_args!("{} missing operand {index}", op.name())))
}

fn kind(op: Op<'_>, ty: &Type) -> Result<ValueType> {
    Ok(match ty {
        Type::EventResult => ValueType::EventResult,
        Type::FocusReason => ValueType::FocusReason,
        Type::DialogRole => ValueType::DialogRole,
        Type::StandardButtonKind => ValueType::StandardButtonKind,
        Type::DrawerEdge => ValueType::DrawerEdge,
        Type::FileDialogMode => ValueType::FileDialogMode,
        Type::MessageLevel => ValueType::MessageLevel,
        Type::MessageButtons => ValueType::MessageButtons,
        Type::KeyEvent => ValueType::KeyEvent,
        Type::WheelEvent => ValueType::WheelEvent,
        Type::PointerEvent => ValueType::PointerEvent,
        Type::PointerEventKind => ValueType::PointerEventKind,
        Type::PointerButton => ValueType::PointerButton,
        Type::PointerButtons => ValueType::PointerButtons,
        Type::CursorShape => ValueType::CursorShape,
        Type::TouchPoint => ValueType::TouchPoint,
        Type::TouchPoints => ValueType::TouchPoints,
        Type::KeyModifiers => ValueType::KeyModifiers,
        Type::Bool => ValueType::Bool,
        Type::Int => ValueType::Int,
        Type::Float => ValueType::Float,
        Type::Length => ValueType::Length,
        Type::Percent => ValueType::Percent,
        Type::Color => ValueType::Color,
        Type::Brush => ValueType::Brush,
        Type::String => ValueType::String,
        Type::StringList => ValueType::StringList,
        Type::Record(name) => ValueType::Record(Rc::from(name.as_ref())),
        Type::RecordList(name) => ValueType::RecordList(Rc::from(name.as_ref())),
        Type::Date => ValueType::Date,
        Type::Time => ValueType::Time,
        Type::Font => ValueType::Font,
        Type::CheckState => ValueType::CheckState,
        Type::StyledText => ValueType::StyledText,
        Type::LayoutAlignment => ValueType::LayoutAlignment,
        Type::Orientation => ValueType::Orientation,
        Type::ScrollSnapMode => ValueType::ScrollSnapMode,
        Type::ScrollAlignment => ValueType::ScrollAlignment,
        Type::SelectionTarget => ValueType::SelectionTarget,
        Type::ScrollBarPolicy => ValueType::ScrollBarPolicy,
        Type::FlickDirection => ValueType::FlickDirection,
        Type::FlickBoundsBehavior => ValueType::FlickBoundsBehavior,
        Type::FlickBoundsMovement => ValueType::FlickBoundsMovement,
        Type::PopupPlacement => ValueType::PopupPlacement,
        Type::FlexboxLayoutDirection => ValueType::FlexboxLayoutDirection,
        Type::FlexboxLayoutWrap => ValueType::FlexboxLayoutWrap,
        Type::CrossAxisAlignment => ValueType::CrossAxisAlignment,
        Type::TextWrap => ValueType::TextWrap,
        Type::TextHorizontalAlignment => ValueType::TextHorizontalAlignment,
        Type::TextVerticalAlignment => ValueType::TextVerticalAlignment,
        Type::TextOverflow => ValueType::TextOverflow,
        Type::ColorScheme => ValueType::ColorScheme,
        Type::InputType => ValueType::InputType,
        Type::DialInputMode => ValueType::DialInputMode,
        Type::SwipeSide => ValueType::SwipeSide,
        Type::DragAction => ValueType::DragAction,
        Type::Image => ValueType::Image,
        Type::ImageFit => ValueType::ImageFit,
        Type::ImageTileMode => ValueType::ImageTileMode,
        Type::ImageHorizontalAlignment => ValueType::ImageHorizontalAlignment,
        Type::ImageVerticalAlignment => ValueType::ImageVerticalAlignment,
        Type::ImageRendering => ValueType::ImageRendering,
        _ => return Err(unsupported(op, &format!("type {}", ty.name()))),
    })
}

fn value_type(op: Op<'_>, value: ValueRef<'_>) -> Result<Type> {
    value.xr_type().map_err(|error| unsupported(op, &error))
}

fn value_kind(op: Op<'_>, value: ValueRef<'_>) -> Result<ValueType> {
    kind(op, &value_type(op, value)?)
}

fn literal(op: Op<'_>, ty: &Type, spelling: &str) -> Result<Value> {
    xrack_resources::literal::decode(ty, spelling)
        .map(xrack_resources::literal::Literal::into_value)
        .map_err(|error| invalid(op, error))
}

fn member(op: Op<'_>, name: &str) -> Result<Member> {
    Ok(Member::new(
        op.attribute("owner").unwrap_or("root"),
        attribute(op, name)?,
    ))
}

fn reference(op: Op<'_>, value: ValueRef<'_>) -> Result<Member> {
    let target = value
        .defining_operation()
        .ok_or_else(|| invalid(op, "missing link endpoint"))?;
    match target.name() {
        "xr.read" | "xr.control_read" => member(target, "value"),
        "xr.check_group_read" => Ok(Member::new(attribute(target, "owner")?, "check-state")),
        _ => Err(unsupported(target, target.name())),
    }
}

#[derive(Default)]
struct AssetValues<'m> {
    images: BTreeMap<String, ImageAsset>,
    markdown: BTreeMap<&'m str, runtime::StyledText>,
    paths: BTreeMap<String, LoadedVectorPath>,
    subtrees: RefCell<BTreeMap<i64, StaticCache<Node>>>,
    gradients: RefCell<BTreeMap<i64, StaticCache<Value>>>,
}

fn static_cache<T>(
    group: Option<i64>,
    groups: &RefCell<BTreeMap<i64, StaticCache<T>>>,
) -> StaticCache<T> {
    group.map_or_else(Rc::default, |group| {
        groups.borrow_mut().entry(group).or_default().clone()
    })
}

#[derive(Default)]
struct Loader<'m> {
    properties: BTreeMap<Member, PropertyDef>,
    callbacks: BTreeMap<Member, Function>,
    expressions: HashMap<(ValueRef<'m>, bool), Expr>,
    hoist_visual_subexpressions: bool,
    visual_bindings: VisualBindings<'m>,
    visual_scope: Option<&'m str>,
    use_visual_plan: bool,
    can_translate: bool,
    selectors: Vec<StateSelector>,
    analysis: ComponentAnalysis<'m>,
    geometry_targets: BTreeSet<String>,
    viewport_targets: BTreeSet<String>,
    source_scroll_targets: BTreeSet<String>,
    focus_targets: BTreeSet<String>,
    row_parameters: Vec<(String, Option<String>)>,
    in_split_handle: bool,
    in_day_cell: bool,
    timer_order: Vec<String>,
    action_keys: BTreeSet<String>,
    native_dialog_keys: BTreeSet<String>,
    records: Rc<RecordSchemas>,
    assets: Rc<AssetValues<'m>>,
}

impl<'m> Loader<'m> {
    fn definition(
        op: Op<'m>,
        use_visual_plan: bool,
        can_translate: bool,
        records: Rc<RecordSchemas>,
        assets: Rc<AssetValues<'m>>,
        aliases: &AliasGraphs<'m>,
    ) -> Result<Definition> {
        let visual_bindings = op
            .children()
            .find(|child| child.name() == "xr.element")
            .map(|root| VisualBindings::new(root, use_visual_plan))
            .unwrap_or_default();
        let mut loader = Self {
            use_visual_plan,
            can_translate,
            visual_bindings,
            analysis: ComponentAnalysis::from_component(
                op,
                &aliases.properties,
                &aliases.callbacks,
                use_visual_plan,
            )
            .map_err(|error| invalid(op, error))?,
            records,
            assets,
            ..Self::default()
        };
        let mut root = None;
        for child in op.children() {
            match child.name() {
                "xr.property" => loader.property("root", child)?,
                "xr.callback" => loader.callback("root", child)?,
                "xr.element" => root = Some(loader.element(child)?),
                "xr.end" => {}
                _ if child.result_count() != 0 => {}
                _ => return Err(unsupported(child, child.name())),
            }
        }
        let component = attribute(op, "sym_name")?;
        for (member, property) in &mut loader.properties {
            if let PropertySource::Alias(target) = &mut property.source {
                let (owner, name) = aliases.properties[component]
                    .resolve(&aliases.properties, (&member.owner, &member.name))
                    .map_err(|error| invalid(op, error))?;
                *target = Member::new(owner, name);
            }
        }
        for (member, callback) in &mut loader.callbacks {
            if callback.alias.is_some() {
                let (owner, name) = aliases.callbacks[component]
                    .resolve(&aliases.callbacks, (&member.owner, &member.name))
                    .map_err(|error| invalid(op, error))?;
                callback.alias = Some(Member::new(owner, name));
            }
        }
        let root = root.ok_or_else(|| invalid(op, "component has no root element"))?;
        let button_groups = group::members(&root);
        Ok(Definition {
            properties: loader.properties,
            property_forwards: loader
                .analysis
                .property_forwards
                .iter()
                .filter(|(path, _)| path.len() > 1)
                .flat_map(|(path, forward)| {
                    let path: Vec<_> = path
                        .iter()
                        .map(|(owner, name)| Member::new(owner, name))
                        .collect();
                    forward
                        .inputs
                        .iter()
                        .map(move |name| ((*name).to_owned(), path.clone()))
                })
                .collect(),
            callbacks: loader.callbacks,
            callback_defaults: loader
                .analysis
                .callback_forwards
                .values()
                .filter_map(|forward| forward.implementation)
                .map(|((owner, name), _)| Member::new(owner, name))
                .collect(),
            root,
            button_groups,
            selectors: loader.selectors,
            geometry_targets: loader.geometry_targets,
            viewport_targets: loader.viewport_targets,
            source_scroll_targets: loader.source_scroll_targets,
            focus_targets: loader.focus_targets,
            timer_order: loader.timer_order,
            action_keys: loader.action_keys,
            native_dialog_keys: loader.native_dialog_keys,
        })
    }

    fn property(&mut self, owner: &str, op: Op<'m>) -> Result<()> {
        let name = attribute(op, "name")?;
        let visibility = attribute(op, "visibility")?;
        let input = operand(op, 0)?;
        let source = if op.attribute("alias") == Some("true") {
            PropertySource::Alias(reference(op, input)?)
        } else {
            PropertySource::Initial(self.expression(input, &[])?)
        };
        let property = PropertyDef {
            ty: value_kind(op, input)?,
            source,
            public: owner == "root" && visibility != "private",
            writable: matches!(visibility, "in" | "in-out"),
        };
        self.properties.insert(Member::new(owner, name), property);
        Ok(())
    }

    fn callback(&mut self, owner: &str, op: Op<'m>) -> Result<()> {
        let mut callback = self.function(op)?;
        if let Some(target) = op.attribute("alias_owner") {
            callback.alias = Some(Member::new(target, attribute(op, "alias_name")?));
        }
        self.callbacks
            .insert(Member::new(owner, attribute(op, "name")?), callback);
        Ok(())
    }

    fn function(&mut self, op: Op<'m>) -> Result<Function> {
        let parameters = op.parameters().map_err(|error| invalid(op, error))?;
        let parameter_types = parameters
            .iter()
            .map(|param| kind(op, &param.ty))
            .collect::<Result<_>>()?;
        let mut body = Vec::new();
        let mut result = None;
        for statement in op.children() {
            let expression =
                |this: &mut Self, index| this.expression(operand(statement, index)?, &parameters);
            match statement.name() {
                "xr.assign" => body.push(Statement::Assign(
                    member(statement, "name")?,
                    expression(self, 0)?,
                )),
                "xr.invoke" => {
                    let arguments = (0..statement.operand_count())
                        .map(|index| expression(self, index))
                        .collect::<Result<_>>()?;
                    body.push(Statement::Call(member(statement, "name")?, arguments));
                }
                "xr.scroll_command" | "xr.cancel_flick" | "xr.return_to_bounds" => {
                    let target = attribute(statement, "owner")?.to_owned();
                    let method = match statement.name() {
                        "xr.cancel_flick" => "cancel-flick",
                        "xr.return_to_bounds" => "return-to-bounds",
                        _ => attribute(statement, "method")?,
                    };
                    let command = runtime::ScrollCommandMethod::parse(method)
                        .ok_or_else(|| unsupported(statement, method))?;
                    let arguments = (0..statement.operand_count())
                        .map(|index| expression(self, index))
                        .collect::<Result<_>>()?;
                    self.viewport_targets.insert(target.clone());
                    if command.uses_source_row() {
                        self.source_scroll_targets.insert(target.clone());
                    }
                    body.push(Statement::Scroll(target, command, arguments));
                }
                "xr.source_selection" => {
                    let column = (attribute(statement, "method")? == "select-source-cell")
                        .then(|| expression(self, 1))
                        .transpose()?;
                    body.push(Statement::SelectSource(
                        attribute(statement, "owner")?.into(),
                        expression(self, 0)?,
                        column,
                    ));
                }
                "xr.set_row_height"
                | "xr.move_column"
                | "xr.move_row"
                | "xr.table_sort_command"
                | "xr.table_selection_command"
                | "xr.table_history_command" => {
                    let method = statement.attribute("method").unwrap_or(statement.name());
                    let command = table::Command::parse(method)
                        .ok_or_else(|| unsupported(statement, method))?;
                    let arguments = (0..statement.operand_count())
                        .map(|index| expression(self, index))
                        .collect::<Result<_>>()?;
                    let owner = attribute(statement, "owner")?.to_owned();
                    body.push(Statement::Table(owner, command, arguments));
                }
                "xr.focus_command" | "xr.text_command" => {
                    let (owner, method) = if statement.name() == "xr.focus_command" {
                        (
                            attribute(statement, "target")?,
                            attribute(statement, "action")?,
                        )
                    } else {
                        (
                            attribute(statement, "owner")?,
                            attribute(statement, "method")?,
                        )
                    };
                    let command = runtime::FocusCommandMethod::parse(method).ok_or_else(|| {
                        invalid(statement, format_args!("unknown focus command {method}"))
                    })?;
                    let arguments = if statement.operand_count() == 0 {
                        None
                    } else {
                        Some([expression(self, 0)?, expression(self, 1)?])
                    };
                    self.focus_targets.insert(owner.into());
                    body.push(Statement::Focus(owner.into(), command, arguments));
                }
                "xr.tree_command" => {
                    let method = attribute(statement, "method")?;
                    let command = runtime::TreeCommandMethod::parse(method)
                        .ok_or_else(|| unsupported(statement, method))?;
                    let key = expression(self, 0)?;
                    let depth = command
                        .requires_depth()
                        .then(|| expression(self, 1))
                        .transpose()?;
                    body.push(Statement::Tree(
                        attribute(statement, "owner")?.into(),
                        command,
                        key,
                        depth,
                    ));
                }
                "xr.timer_command" => {
                    let name = attribute(statement, "command")?;
                    let command = runtime::TimerCommand::parse(name)
                        .ok_or_else(|| unsupported(statement, name))?;
                    body.push(Statement::Timer(
                        attribute(statement, "owner")?.into(),
                        command,
                    ));
                }
                "xr.check_group_command" => body.push(Statement::SetCheckGroup(
                    attribute(statement, "owner")?.into(),
                    expression(self, 0)?,
                )),
                "xr.navigate" => {
                    let method = attribute(statement, "method")?;
                    let command =
                        runtime::StackNavigationMethod::parse(method).ok_or_else(|| {
                            invalid(statement, format_args!("unknown stack command {method}"))
                        })?;
                    let argument = (command != runtime::StackNavigationMethod::Pop)
                        .then(|| expression(self, 0))
                        .transpose()?;
                    body.push(Statement::StackNavigate(
                        attribute(statement, "owner")?.into(),
                        command,
                        argument,
                    ));
                }
                "xr.swipe_cancel" => body.push(Statement::SwipeCancel(
                    attribute(statement, "owner")?.into(),
                )),
                "xr.split_restore" => body.push(Statement::SplitRestore(
                    attribute(statement, "owner")?.into(),
                    expression(self, 0)?,
                )),
                "xr.popup_command" => body.push(Statement::Popup(
                    attribute(statement, "owner")?.into(),
                    attribute(statement, "open")? == "true",
                )),
                "xr.file_dialog_open" | "xr.native_dialog_open" => {
                    body.push(Statement::NativeDialogOpen(
                        attribute(statement, "owner")?.into(),
                    ));
                }
                "xr.window_close" => body.push(Statement::WindowClose),
                "xr.secondary_window_command" => {
                    let command = attribute(statement, "command")?;
                    let command =
                        runtime::SecondaryWindowCommand::parse(command).ok_or_else(|| {
                            invalid(
                                statement,
                                format_args!("unknown secondary window command {command}"),
                            )
                        })?;
                    body.push(Statement::SecondaryWindow(
                        attribute(statement, "owner")?.into(),
                        command,
                    ));
                }
                "xr.window_command" => {
                    let name = attribute(statement, "command")?;
                    let command = runtime::WindowCommandMethod::parse(name).ok_or_else(|| {
                        invalid(statement, format_args!("unknown window command {name}"))
                    })?;
                    let enabled = (command == runtime::WindowCommandMethod::SetFullScreen)
                        .then(|| expression(self, 0))
                        .transpose()?;
                    body.push(Statement::WindowCommand(command, enabled));
                }
                "xr.return" => {
                    result = Some(value_kind(statement, operand(statement, 0)?)?);
                    body.push(Statement::Return(expression(self, 0)?));
                }
                "xr.end" => {}
                _ if statement.result_count() != 0 => {}
                _ => return Err(unsupported(statement, statement.name())),
            }
        }
        Ok(Function {
            parameters: parameter_types,
            result,
            body,
            pure: op.attribute("pure") == Some("true"),
            alias: None,
        })
    }

    fn element(&mut self, op: Op<'m>) -> Result<Element> {
        let repeat = op
            .children()
            .find(|child| child.name() == "xr.repeat")
            .map(|child| {
                let repeat =
                    RepeatRef::from_operation(child).map_err(|error| invalid(child, error))?;
                let model_type = value_kind(child, repeat.model)?;
                if !matches!(
                    model_type,
                    ValueType::Int
                        | ValueType::StringList
                        | ValueType::RecordList(_)
                        | ValueType::TouchPoints
                ) {
                    return Err(unsupported(child, "repeat model"));
                }
                Ok(Repeat {
                    model: self.expression(repeat.model, &[])?,
                    item: repeat.item.to_owned(),
                    index: repeat.index.map(str::to_owned),
                    positioned: xrack_mlir::repeat_is_positioned(op),
                    empty: self.use_visual_plan
                        && child.attribute("xr.empty_repeat") == Some("true"),
                    static_count: self
                        .use_visual_plan
                        .then(|| op.integer_attribute("xr.static_repeat_count"))
                        .flatten(),
                    scene_cache: self
                        .use_visual_plan
                        .then(|| RowSceneCache::from_operation(op))
                        .flatten(),
                })
            })
            .transpose()?;
        if let Some(repeat) = &repeat {
            self.row_parameters
                .push((repeat.item.clone(), repeat.index.clone()));
        }
        let nested = repeat.is_some();
        let previous_scope = self.visual_scope;
        if nested {
            self.visual_scope = Some(repeat_scope(op));
        } else if matches!(op.attribute("kind"), Some("Action" | "Timer")) {
            self.visual_scope = None;
        }
        let in_split_handle = self.in_split_handle;
        let in_day_cell = self.in_day_cell;
        self.in_split_handle |= op.attribute("kind") == Some("SplitHandle");
        self.in_day_cell |= op.attribute("kind") == Some("DayCell");
        let result = self.element_body(op, repeat);
        self.visual_scope = previous_scope;
        self.in_split_handle = in_split_handle;
        self.in_day_cell = in_day_cell;
        if nested {
            self.row_parameters.pop();
        }
        result
    }

    fn element_body(&mut self, op: Op<'m>, repeat: Option<Repeat>) -> Result<Element> {
        let id = attribute(op, "id")?;
        let key = op
            .attribute("key")
            .unwrap_or(if id.is_empty() { "root" } else { id });
        let is_instance = op.name() == "xr.instance";
        match op.attribute("kind") {
            Some("Timer") => self.timer_order.push(key.into()),
            Some("Action") => {
                self.action_keys.insert(key.into());
            }
            Some("FileDialog" | "MessageDialog") => {
                self.native_dialog_keys.insert(key.into());
            }
            _ => {}
        }
        let record_combo = self.record_combo(op)?;
        let standard_list = self.standard_list(op)?;
        let field = |name| {
            xrack_mlir::record_role(op, name)
                .map(|path| {
                    path.map(|path| path.split('.').map(str::to_owned).collect::<Rc<[_]>>())
                })
                .map_err(|error| invalid(op, error))
        };
        let column_field = if op.attribute("kind") == Some("TableColumn") {
            field("field")?
        } else {
            None
        };
        let table = match op.attribute("kind") {
            Some("TableView" | "StandardTableView") => {
                let standard = op.attribute("kind") == Some("StandardTableView");
                let model = standard
                    .then(|| {
                        let binding = op
                            .children()
                            .find(|child| {
                                child.name() == "xr.bind"
                                    && child.attribute("name") == Some("model")
                            })
                            .ok_or_else(|| invalid(op, "missing standard table model"))?;
                        self.visual_binding(binding)
                    })
                    .transpose()?;
                Some(table::Definition {
                    model,
                    filter_field: field("filter-field")?,
                    row_header: field("row-header-field")?,
                })
            }
            _ => None,
        };
        let tree = if op.attribute("kind") == Some("TreeView") {
            let row = op
                .children()
                .find(|child| child.attribute("kind") == Some("TreeRow"))
                .ok_or_else(|| invalid(op, "TreeView is missing a row template"))?;
            let role = |name| {
                let binding = row
                    .children()
                    .find(|binding| {
                        binding.name() == "xr.bind" && binding.attribute("name") == Some(name)
                    })
                    .ok_or_else(|| invalid(row, format!("TreeRow is missing {name}")))?;
                let (_, path) = xrack_mlir::record_field_path(operand(binding, 0)?)
                    .map_err(|error| invalid(binding, error))?;
                Ok(path.into_iter().map(str::to_owned).collect::<Rc<[_]>>())
            };
            Some(tree::Definition {
                key: role("key")?,
                parent: role("parent-key")?,
                filter_field: field("filter-field")?,
            })
        } else {
            None
        };
        let (visible_field, height_field) = if matches!(
            op.attribute("kind"),
            Some(
                "ListView"
                    | "GridView"
                    | "StandardListView"
                    | "TableView"
                    | "StandardTableView"
                    | "TreeView"
            )
        ) {
            (field("row-visible-field")?, field("row-height-field")?)
        } else {
            (None, None)
        };
        let mut instance_handlers = BTreeMap::new();
        let instance_bindings = self.analysis.instances.remove(key).unwrap_or_default();
        for handler in instance_bindings.callbacks {
            let callback = handler.operation;
            if callback.attribute("alias_owner").is_some() {
                return Err(unsupported(callback, "instance callback aliases"));
            }
            instance_handlers.insert(
                attribute(callback, "name")?.into(),
                self.function(callback)?,
            );
        }
        let mut clipboard_parser = None;
        let mut clipboard_text = instance_bindings
            .clipboard_text
            .map(|binding| self.expression(operand(binding, 0)?, &[]))
            .transpose()?;
        let mut bindings = Vec::new();
        let mut offsets = Vec::new();
        let mut scroll_target = None;
        let mut initial_focus = None;
        let mut action = None;
        let mut action_group = None;
        let mut button_group = None;
        let mut non_exclusive = false;
        let mut inputs = BTreeMap::<String, Input>::new();
        for input in instance_bindings.inputs {
            let binding = input.operation;
            let value = operand(binding, 0)?;
            let source = if binding.name() == "xr.bind" {
                InputSource::Binding(self.expression(value, &[])?)
            } else if value
                .defining_operation()
                .is_some_and(|op| op.name() == "xr.read")
            {
                InputSource::Linked(reference(binding, value)?)
            } else {
                InputSource::RowLink(self.row_field(binding, value)?)
            };
            let mut decoded = Input::new(input.endpoint);
            decoded.source = Some(source);
            inputs.insert(attribute(binding, "name")?.into(), decoded);
        }
        for input in instance_bindings.state_inputs {
            inputs
                .entry(input.name.into())
                .or_insert_with(|| Input::new(input.endpoint))
                .cases = input
                .cases
                .into_iter()
                .map(|case| self.state_case(case))
                .collect::<Result<_>>()?;
        }
        let mut render_selector = None;
        let mut state_bindings = BTreeMap::<_, Vec<_>>::new();
        let mut conditions = Vec::new();
        let mut static_hidden = false;
        let mut animations = Vec::new();
        let mut state_timings = BTreeMap::<_, Vec<_>>::new();
        let mut handlers = BTreeMap::new();
        let mut links = BTreeMap::new();
        let mut row_links = BTreeMap::new();
        let mut children = Vec::new();
        let mut timers = Vec::new();
        let mut commands = Vec::new();
        let mut native_dialogs = Vec::new();
        let mut secondary_windows = Vec::new();
        let mut split_handle = None;
        let mut day_cell = None;
        let mut split_fill = None;
        for child in op.children() {
            match child.name() {
                "xr.property" => self.property(key, child)?,
                "xr.callback" if is_instance => {}
                "xr.callback" => self.callback(key, child)?,
                "xr.clipboard_paste" => {
                    let parameter = Parameter {
                        name: attribute(child, "text_parameter")?.to_owned(),
                        ty: Type::String,
                    };
                    let result = child
                        .children()
                        .find(|op| op.name() == "xr.return")
                        .ok_or_else(|| invalid(child, "missing clipboard parser result"))?;
                    clipboard_parser = Some(table_edit::ClipboardParser {
                        target: attribute(child, "target")?.into(),
                        expression: self.expression(operand(result, 0)?, &[parameter])?,
                    });
                }
                "xr.handler" => {
                    let name = attribute(child, "name")?;
                    if !matches!(
                        name,
                        "clicked"
                            | "edited"
                            | "toggled"
                            | "check-state-changed"
                            | "selected"
                            | "value-selected"
                            | "accepted"
                            | "link-activated"
                            | "changed"
                            | "released"
                            | "finished"
                            | "scrolled"
                            | "flicked"
                            | "opened"
                            | "closed"
                            | "current-row-changed"
                            | "current-cell-changed"
                            | "selection-changed"
                            | "expanded"
                            | "collapsed"
                            | "resized"
                            | "triggered"
                            | "activated"
                            | "activated-ambiguously"
                            | "rejected"
                            | "applied"
                            | "reset"
                            | "help-requested"
                            | "discarded"
                            | "canceled"
                            | "month-changed"
                            | "leave-requested"
                            | "wheel"
                            | "scroll-event"
                            | "pointer-event"
                            | "press-and-hold"
                            | "double-clicked"
                            | "entered"
                            | "exited"
                            | "moved"
                            | "swiped"
                            | "started"
                            | "updated"
                            | "ended"
                            | "cancelled"
                            | "drag-started"
                            | "drag-finished"
                            | "dropped"
                            | "files-entered"
                            | "files-dropped"
                            | "position-changed"
                            | "deactivated"
                            | "removed"
                            | "failed"
                            | "close-requested"
                            | "sort-ascending"
                            | "sort-descending"
                            | "column-moved"
                            | "row-moved"
                            | "search-triggered"
                    ) && !runtime::is_focus_handler(name)
                        && !runtime::KeyPhase::ALL
                            .iter()
                            .any(|phase| phase.handler() == name)
                    {
                        return Err(unsupported(child, &format!("event {name}")));
                    }
                    if runtime::is_focus_handler(name) {
                        self.focus_targets.insert(key.into());
                    }
                    if (name == "triggered" && op.attribute("kind") != Some("Timer"))
                        || (name == "activated"
                            && !matches!(
                                op.attribute("kind"),
                                Some(
                                    "DelayButton"
                                        | "Shortcut"
                                        | "Action"
                                        | "ActionGroup"
                                        | "MenuItem"
                                        | "MonthGrid"
                                        | "StackPage"
                                )
                            ))
                        || (name == "activated-ambiguously"
                            && op.attribute("kind") != Some("Shortcut"))
                    {
                        return Err(unsupported(child, &format!("event {name}")));
                    }
                    if matches!(name, "scrolled" | "flicked") {
                        self.viewport_targets.insert(key.into());
                    }
                    handlers.insert(name.into(), self.function(child)?);
                }
                "xr.element" if child.attribute("kind") == Some("DayCell") => {
                    day_cell = Some(Box::new(self.element(child)?));
                }
                "xr.element" if child.attribute("kind") == Some("SplitHandle") => {
                    split_handle = Some(Box::new(self.element(child)?));
                }
                "xr.element" if child.attribute("kind") == Some("Timer") => {
                    timers.push(self.element(child)?)
                }
                "xr.element"
                    if matches!(
                        child.attribute("kind"),
                        Some("Action" | "ActionGroup" | "ButtonGroup")
                    ) =>
                {
                    commands.push(self.element(child)?);
                }
                "xr.element"
                    if matches!(
                        child.attribute("kind"),
                        Some("FileDialog" | "MessageDialog")
                    ) =>
                {
                    native_dialogs.push(self.element(child)?);
                }
                "xr.element" if child.attribute("kind") == Some("SecondaryWindow") => {
                    secondary_windows.push(self.element(child)?);
                }
                "xr.element" | "xr.instance" => children.push(self.element(child)?),
                "xr.action_ref" => action = Some(attribute(child, "owner")?.into()),
                "xr.radio_group_ref" | "xr.check_group_ref" => {
                    button_group = Some(group::Membership {
                        key: attribute(child, "group")?.into(),
                        check_property: child.attribute("property").map(str::to_owned),
                    });
                }
                "xr.action_group_ref" => {
                    action_group = Some(action::Membership {
                        group: attribute(child, "group")?.into(),
                        value: attribute(child, "value")?.into(),
                    });
                }
                "xr.focus_target" => {
                    let target = attribute(child, "target")?;
                    self.focus_targets.insert(target.into());
                    initial_focus = Some(target.into());
                }
                "xr.split_fill" => split_fill = Some(attribute(child, "target")?),
                "xr.bind" | "xr.instance_link" if is_instance => {}
                "xr.bind" if child.attribute("name") == Some("clipboard-text") => {
                    clipboard_text = Some(self.expression(operand(child, 0)?, &[])?);
                }
                "xr.state_select" => {
                    if self
                        .analysis
                        .visual_state_selectors
                        .contains(attribute(child, "owner")?)
                    {
                        let result = child
                            .result(0)
                            .ok_or_else(|| invalid(child, "missing selector result"))?;
                        render_selector = Some(self.state_selector(child, result)?);
                    }
                }
                "xr.state_set" => state_bindings
                    .entry(property_name(child)?)
                    .or_default()
                    .push(self.state_case(child)?),
                "xr.instance_state" => {}
                "xr.scroll_target" => {
                    let target = attribute(child, "target")?.to_owned();
                    self.viewport_targets.insert(target.clone());
                    scroll_target = Some(target);
                }
                "xr.bind" | "xr.control_link"
                    if child
                        .attribute("name")
                        .and_then(viewport::offset_axis)
                        .is_some() =>
                {
                    let axis =
                        viewport::offset_axis(attribute(child, "name")?).expect("viewport axis");
                    let input = operand(child, 0)?;
                    let offset = if child.name() == "xr.bind" {
                        viewport::Offset::Binding(self.visual_binding(child)?)
                    } else {
                        viewport::Offset::Linked(reference(child, input)?)
                    };
                    self.viewport_targets.insert(key.into());
                    offsets.push((axis, offset));
                }
                "xr.bind"
                    if child.attribute("name") == Some("row-visible-field")
                        && visible_field.is_some() => {}
                "xr.bind"
                    if child.attribute("name") == Some("row-height-field")
                        && height_field.is_some() => {}
                "xr.bind"
                    if table.is_some()
                        && matches!(
                            child.attribute("name"),
                            Some("model" | "row-header-field")
                        ) => {}
                "xr.bind" if column_field.is_some() && child.attribute("name") == Some("field") => {
                }
                "xr.bind" | "xr.layout_bind" => {
                    let name = property_name(child)?;
                    if name == "exclusive"
                        && matches!(op.attribute("kind"), Some("ActionGroup" | "ButtonGroup"))
                    {
                        non_exclusive =
                            operand(child, 0)?
                                .defining_operation()
                                .is_some_and(|literal| {
                                    literal.name() == "xr.literal"
                                        && literal.attribute("value") == Some("false")
                                });
                    }
                    if name == "bounds-behavior" && op.attribute("kind") == Some("Flickable") {
                        self.viewport_targets.insert(key.into());
                    }
                    if (record_combo.is_some()
                        && matches!(name, "model" | "text-role" | "value-role" | "enabled-role"))
                        || standard_list.is_some() && matches!(name, "model" | "text-role")
                    {
                        continue;
                    }
                    let expression = self.visual_binding(child)?;
                    let vector_path = match expression.as_ref() {
                        Expression::Literal(Value::String(commands))
                            if self.use_visual_plan
                                && child.attribute("xr.precompiled_path") == Some("true") =>
                        {
                            self.assets.paths.get(commands).cloned()
                        }
                        _ => None,
                    };
                    bindings.push(Binding {
                        name,
                        expression,
                        paint_only: self.use_visual_plan
                            && child.attribute("xr.update_effect") == Some("paint"),
                        vector_path,
                    });
                }
                "xr.control_link" => {
                    let name = property_name(child)?;
                    if !matches!(
                        name,
                        "text"
                            | "checked"
                            | "current-index"
                            | "current-row"
                            | "current-key"
                            | "running"
                            | "open-side"
                            | "current-value"
                            | "value"
                            | "check-state"
                            | "edit-text"
                            | "position"
                            | "open"
                            | "lower-value"
                            | "upper-value"
                            | "date"
                            | "display-month"
                            | "time"
                            | "color"
                            | "font"
                    ) && !(name == "visible" && op.attribute("kind") == Some("SecondaryWindow"))
                    {
                        return Err(unsupported(
                            child,
                            &format!("two-way control property {name}"),
                        ));
                    }
                    links.insert(name, reference(child, operand(child, 0)?)?);
                }
                "xr.row_link" => {
                    let name = property_name(child)?;
                    row_links.insert(name, self.row_field(child, operand(child, 0)?)?);
                }
                "xr.animate" => {
                    animations.push((property_name(child)?, self.animation(child)?));
                }
                "xr.state_timing" => {
                    let case = StateCaseRef::from_operation(child)
                        .map_err(|error| invalid(child, error))?;
                    state_timings
                        .entry(property_name(child)?)
                        .or_default()
                        .push(StateTimingSpec {
                            selector: self.state_selector(child, case.selector)?,
                            index: case.index,
                            enter: attribute(child, "direction")? == "in",
                            animation: self.animation(child)?,
                        });
                }
                "xr.show_if" => {
                    static_hidden |=
                        self.use_visual_plan && child.attribute("xr.static_hidden") == Some("true");
                    conditions.push(self.visual_binding(child)?);
                }
                "xr.end" | "xr.repeat" => {}
                _ if child.result_count() != 0 => {}
                _ => return Err(unsupported(child, child.name())),
            }
        }
        let visual = if is_instance {
            Visual::Instance {
                component: attribute(op, "component")?.into(),
                inputs,
            }
        } else {
            let kind = attribute(op, "kind")?;
            Visual::Builtin(static_name(kind, ELEMENTS).ok_or_else(|| unsupported(op, kind))?)
        };
        if matches!(
            visual,
            Visual::Builtin("ListView" | "GridView" | "TableView" | "TreeView")
        ) && children
            .iter()
            .filter(|child| child.repeat.is_some())
            .count()
            != 1
        {
            return Err(unsupported(
                op,
                "virtual view without one repeated template",
            ));
        }
        if table.is_some()
            && (bindings.iter().any(|binding| binding.name == "edit-on-tab")
                || state_bindings.contains_key("edit-on-tab"))
        {
            self.viewport_targets.insert(key.into());
        }
        let split = if matches!(visual, Visual::Builtin("SplitView")) {
            let fill = split_fill
                .map(|id| {
                    children
                        .iter()
                        .position(|pane| pane.id.as_ref() == id)
                        .ok_or_else(|| invalid(op, "split fill target must be a direct pane"))
                })
                .transpose()?;
            Some(split::Definition {
                fill,
                handle: split_handle,
            })
        } else {
            None
        };
        let stack_routes = matches!(visual, Visual::Builtin("StackView"))
            .then(|| children.iter().map(|page| page.id.to_string()).collect());
        let scene_plan = self
            .use_visual_plan
            .then(|| xrack_resources::ScenePlan::from_operation(op))
            .transpose()
            .map_err(|error| invalid(op, error))?
            .unwrap_or_default();
        let controls = controlled_properties(op)
            .map(|(name, ty)| load_control(op, name, ty))
            .collect::<Result<_>>()?;
        Ok(Element {
            key: key.into(),
            id: id.into(),
            repeat,
            delegate: op.attribute("delegate").is_some(),
            record_combo,
            standard_list,
            table,
            tree,
            split,
            stack_routes,
            day_cell,
            column_field,
            clipboard_parser,
            clipboard_text,
            contains_delegate: is_instance
                || children
                    .iter()
                    .any(|child| child.repeat.is_none() && child.contains_delegate),
            visible_field,
            height_field,
            instance_handlers,
            visual,
            grid_plan: scene_plan.grid.map(Arc::from),
            paint_plan: scene_plan.paint_order.map(Arc::from),
            static_subtree: (self.use_visual_plan
                && op.attribute("xr.static_subtree") == Some("true"))
            .then(|| {
                static_cache(
                    op.integer_attribute("xr.static_subtree_group"),
                    &self.assets.subtrees,
                )
            }),
            static_hidden,
            bindings,
            controls,
            offsets,
            scroll_target,
            initial_focus,
            action,
            action_group,
            button_group,
            non_exclusive,
            render_selector,
            state_bindings,
            animations,
            state_timings,
            conditions,
            handlers,
            links,
            row_links,
            children,
            timers,
            commands,
            native_dialogs,
            secondary_windows,
        })
    }

    fn standard_list(&mut self, op: Op<'m>) -> Result<Option<StandardList>> {
        if op.attribute("kind") != Some("StandardListView") {
            return Ok(None);
        }
        let model = op
            .children()
            .find(|child| child.name() == "xr.bind" && child.attribute("name") == Some("model"))
            .map(|binding| self.visual_binding(binding))
            .transpose()?;
        let label = xrack_mlir::record_role(op, "text-role")
            .map_err(|error| invalid(op, error))?
            .unwrap_or_else(|| "text".into())
            .split('.')
            .map(str::to_owned)
            .collect();
        Ok(Some(StandardList { model, label }))
    }

    fn record_combo(&mut self, op: Op<'m>) -> Result<Option<RecordCombo>> {
        if op.attribute("kind") != Some("ComboBox") {
            return Ok(None);
        }
        let Some(binding) = op
            .children()
            .find(|child| child.name() == "xr.bind" && child.attribute("name") == Some("model"))
        else {
            return Ok(None);
        };
        let model = operand(binding, 0)?;
        if !matches!(value_kind(binding, model)?, ValueType::RecordList(_)) {
            return Ok(None);
        }
        let role = |name| {
            xrack_mlir::record_role(op, name)
                .map(|role| role.map(|role| role.split('.').map(str::to_owned).collect::<Vec<_>>()))
                .map_err(|error| invalid(op, error))
        };
        let label = role("text-role")?.unwrap_or_else(|| vec!["text".to_owned()]);
        let value = role("value-role")?.unwrap_or_else(|| label.clone());
        Ok(Some(RecordCombo {
            model: self.visual_binding(binding)?,
            label,
            value,
            enabled: role("enabled-role")?,
        }))
    }

    fn row_field(&self, op: Op<'m>, value: ValueRef<'m>) -> Result<RowField> {
        let (base, path) =
            xrack_mlir::record_field_path(value).map_err(|error| invalid(op, error))?;
        let parameter = base
            .defining_operation()
            .filter(|operation| operation.name() == "xr.param")
            .ok_or_else(|| unsupported(op, "row link target"))?;
        if !matches!(
            value_kind(op, base)?,
            ValueType::Record(_) | ValueType::String
        ) {
            return Err(unsupported(op, "row link outside a record model"));
        }
        let item = attribute(parameter, "value")?.to_owned();
        if !self.row_parameters.iter().any(|(name, _)| *name == item) {
            return Err(invalid(op, "row link missing its repeated item"));
        }
        Ok(RowField {
            item,
            path: path.into_iter().map(str::to_owned).collect(),
        })
    }

    fn state_selector(&mut self, op: Op<'m>, value: ValueRef<'m>) -> Result<usize> {
        match self.expression(value, &[])?.as_ref() {
            Expression::StateSelect(index) => Ok(*index),
            _ => Err(invalid(op, "expected a state selector")),
        }
    }

    fn state_case(&mut self, op: Op<'m>) -> Result<StateCase> {
        let case = StateCaseRef::from_operation(op).map_err(|error| invalid(op, error))?;
        Ok(StateCase {
            selector: self.state_selector(op, case.selector)?,
            index: case.index,
            value: self.expression(case.value, &[])?,
        })
    }

    fn animation(&mut self, op: Op<'m>) -> Result<AnimationSpec> {
        use xrack_runtime::Easing;

        let timing = AnimationRef::from_operation(op).map_err(|error| invalid(op, error))?;
        let easing = match timing.curve {
            Some([x1, y1, x2, y2]) => Easing::cubic_bezier(x1, y1, x2, y2)
                .ok_or_else(|| invalid(op, "invalid animation easing"))?,
            None => Easing::LINEAR,
        };
        Ok(AnimationSpec {
            timing: Animation::new(timing.duration_ms, timing.delay_ms, easing),
            enabled: timing
                .enabled
                .map(|value| self.expression(value, &[]))
                .transpose()?,
        })
    }

    fn record_fields(&self, op: Op<'m>, name: &str) -> Result<&[RecordField]> {
        self.records
            .get(name)
            .map(Vec::as_slice)
            .ok_or_else(|| invalid(op, format_args!("unknown record {name}")))
    }

    fn default_record(&self, op: Op<'m>, name: Rc<str>) -> Result<Value> {
        default_value(&self.records, &ValueType::Record(name)).map_err(|error| invalid(op, error))
    }

    fn visual_binding(&mut self, binding: Op<'m>) -> Result<Expr> {
        let value = operand(binding, 0)?;
        let slot = self.visual_bindings.slot(binding, self.visual_scope);
        let hoist = self.use_visual_plan
            && !self.row_parameters.is_empty()
            && !slot.is_some_and(|slot| slot.uses_component_scope());
        let previous = std::mem::replace(&mut self.hoist_visual_subexpressions, hoist);
        let expression = self.expression(value, &[]);
        self.hoist_visual_subexpressions = previous;
        let expression = expression?;
        Ok(match slot {
            Some(slot) => Rc::new(Expression::CachedVisual(slot, expression)),
            None => expression,
        })
    }

    fn expression(&mut self, value: ValueRef<'m>, parameters: &[Parameter]) -> Result<Expr> {
        let key = (value, self.hoist_visual_subexpressions);
        if let Some(expression) = self.expressions.get(&key) {
            return Ok(expression.clone());
        }
        let op = value
            .defining_operation()
            .ok_or_else(|| Error::Invalid("value has no XR defining operation".into()))?;
        if self.hoist_visual_subexpressions
            && let Some(group) = VisualCacheGroup::for_subexpression(op)
        {
            // A planned subexpression already owns the full invariant graph.
            // Decode its operands without applying a second level of hoisting.
            self.hoist_visual_subexpressions = false;
            let expression = self.expression(value, parameters);
            self.hoist_visual_subexpressions = true;
            let expression = Rc::new(Expression::CachedVisual(
                VisualSlot::Planned(group),
                expression?,
            ));
            self.expressions.insert(key, expression.clone());
            return Ok(expression);
        }
        let ty = value_type(op, value)?;
        let argument = |this: &mut Self, index| this.expression(operand(op, index)?, parameters);
        let arguments = |this: &mut Self| {
            (0..op.operand_count())
                .map(|index| argument(this, index))
                .collect::<Result<_>>()
        };
        let spelling = || attribute(op, "value");
        let expression = match op.name() {
            "xr.literal" => Expression::Literal(literal(op, &ty, spelling()?)?),
            "xr.read" => Expression::Read(member(op, "value")?),
            "xr.geometry_read" => {
                let target = attribute(op, "owner")?.to_owned();
                let metric = match spelling()? {
                    "layout-ready" => None,
                    name => Some(runtime::GeometryMetric::from_name(name).ok_or_else(|| {
                        invalid(op, format_args!("unknown geometry metric {name}"))
                    })?),
                };
                self.geometry_targets.insert(target.clone());
                Expression::GeometryRead(target, metric)
            }
            "xr.viewport_read" | "xr.flick_motion_read" => {
                let target = attribute(op, "owner")?.to_owned();
                let name = spelling()?;
                let metric = viewport::Metric::parse(name)
                    .ok_or_else(|| invalid(op, format_args!("unknown viewport metric {name}")))?;
                self.viewport_targets.insert(target.clone());
                Expression::ViewportRead(target, metric)
            }
            "xr.check_group_read" => {
                Expression::Read(Member::new(attribute(op, "owner")?, "check-state"))
            }
            "xr.action_group_read" => Expression::ActionGroupRead(attribute(op, "owner")?.into()),
            "xr.native_dialog_read" => {
                let name = spelling()?;
                let field = native_dialog::Read::parse(name).ok_or_else(|| {
                    invalid(op, format_args!("unknown native dialog field {name}"))
                })?;
                Expression::NativeDialogRead(attribute(op, "owner")?.into(), field)
            }
            "xr.window_read" => {
                let name = spelling()?;
                let metric = runtime::WindowMetric::parse(name)
                    .ok_or_else(|| invalid(op, format_args!("unknown window metric {name}")))?;
                Expression::WindowRead(metric)
            }
            "xr.swipe_read" | "xr.scale_rotate_read" => {
                let name = spelling()?;
                let read = gesture::Metric::parse(op.name(), name)
                    .ok_or_else(|| invalid(op, format_args!("unknown gesture metric {name}")))?;
                Expression::GestureRead(attribute(op, "owner")?.into(), read)
            }
            "xr.hover_read" => {
                let name = spelling()?;
                let read = pointer::HoverRead::parse(name)
                    .ok_or_else(|| invalid(op, format_args!("unknown hover property {name}")))?;
                Expression::HoverRead(attribute(op, "owner")?.into(), read)
            }
            "xr.touch_area_read" => {
                let name = spelling()?;
                let read = pointer::Read::parse(name)
                    .ok_or_else(|| invalid(op, format_args!("unknown pointer property {name}")))?;
                Expression::PointerRead(attribute(op, "owner")?.into(), read)
            }
            "xr.focus_read" => {
                let owner = attribute(op, "owner")?;
                self.focus_targets.insert(owner.into());
                Expression::FocusRead(owner.into(), spelling()? == "focus-within")
            }
            "xr.stack_read" => {
                let name = spelling()?;
                let read = stack::Read::parse(name)
                    .ok_or_else(|| invalid(op, format_args!("unknown stack property {name}")))?;
                let route = matches!(read, stack::Read::Active | stack::Read::Index)
                    .then(|| argument(self, 0))
                    .transpose()?;
                Expression::StackRead(attribute(op, "owner")?.into(), read, route)
            }
            "xr.split_state_read" => Expression::SplitSave(attribute(op, "owner")?.into()),
            "xr.split_resizing" => Expression::SplitResizing(attribute(op, "owner")?.into()),
            "xr.tree_query" => {
                let method = spelling()?;
                let query = tree::Query::parse(method).ok_or_else(|| unsupported(op, method))?;
                Expression::TreeQuery(attribute(op, "owner")?.into(), query, argument(self, 0)?)
            }
            "xr.view_index" => {
                let method = spelling()?;
                let direction = runtime::ViewIndex::parse(method).ok_or_else(|| {
                    invalid(op, format_args!("unknown view index query {method}"))
                })?;
                Expression::ViewIndex(
                    attribute(op, "owner")?.into(),
                    argument(self, 0)?,
                    direction,
                )
            }
            "xr.table_selection_read" | "xr.table_sort_read" | "xr.table_history_read" => {
                Expression::TableRead(member(op, "value")?)
            }
            "xr.scroll_control_read" => Expression::ScrollControlRead(member(op, "value")?),
            "xr.image_url" => {
                let url = spelling()?;
                let image = if url.is_empty() {
                    ImageAsset::default()
                } else {
                    self.assets.images.get(url).cloned().ok_or_else(|| {
                        unsupported(op, &format!("unresolved image asset {url:?}"))
                    })?
                };
                Expression::Literal(Value::ImageAsset(image))
            }
            "xr.image_dimension" => {
                let axis = match spelling()? {
                    "width" => 0,
                    "height" => 1,
                    _ => return Err(invalid(op, "invalid image dimension")),
                };
                Expression::ImageDimension(argument(self, 0)?, axis)
            }
            "xr.image_playback_read" => Expression::ImagePlaybackRead(member(op, "value")?),
            "xr.nine_slice" => {
                let edges =
                    xrack_resources::nine_slice_edges(op).map_err(|error| invalid(op, error))?;
                Expression::NineSlice(argument(self, 0)?, edges)
            }
            "xr.param" => {
                let name = spelling()?;
                if let Some(index) = parameters
                    .iter()
                    .position(|parameter| parameter.name == name)
                {
                    Expression::Param(index)
                } else if self
                    .row_parameters
                    .iter()
                    .any(|(item, index)| item == name || index.as_deref() == Some(name))
                {
                    Expression::RowParam(name.to_owned())
                } else if self.in_split_handle
                    && let Some(parameter) = split::Parameter::parse(name)
                {
                    Expression::SplitHandleParam(parameter)
                } else if self.in_day_cell
                    && let Some(parameter) = calendar::Parameter::parse(name)
                {
                    Expression::CalendarCellParam(parameter)
                } else {
                    return Err(invalid(op, format_args!("unknown parameter {name}")));
                }
            }
            "xr.unary" => {
                let operator =
                    UnaryOperator::from_operation(op).map_err(|error| invalid(op, error))?;
                let input = argument(self, 0)?;
                if operator == UnaryOperator::Positive {
                    return Ok(input);
                }
                Expression::Unary(operator, input)
            }
            "xr.binary" => Expression::Binary(
                BinaryOperator::from_operation(op).map_err(|error| invalid(op, error))?,
                argument(self, 0)?,
                argument(self, 1)?,
            ),
            "xr.select" => {
                Expression::Select(argument(self, 0)?, argument(self, 1)?, argument(self, 2)?)
            }
            "xr.cast" | "xr.to_ratio" => Expression::Cast(argument(self, 0)?),
            "xr.stringify" => Expression::Stringify(argument(self, 0)?),
            "xr.solid_brush" => Expression::SolidBrush(argument(self, 0)?),
            "xr.linear_gradient" | "xr.radial_gradient" => {
                let linear = op.name() == "xr.linear_gradient";
                let angle = linear.then(|| argument(self, 0)).transpose()?;
                let stops = (usize::from(linear)..op.operand_count())
                    .map(|index| {
                        let stop = operand(op, index)?
                            .defining_operation()
                            .filter(|stop| stop.name() == "xr.gradient_stop")
                            .ok_or_else(|| invalid(op, "expected a gradient stop"))?;
                        Ok((
                            self.expression(operand(stop, 0)?, parameters)?,
                            self.expression(operand(stop, 1)?, parameters)?,
                        ))
                    })
                    .collect::<Result<_>>()?;
                let cache = (self.use_visual_plan
                    && op.attribute("xr.static_gradient") == Some("true"))
                .then(|| {
                    static_cache(
                        op.integer_attribute("xr.static_gradient_group"),
                        &self.assets.gradients,
                    )
                });
                Expression::Gradient {
                    angle,
                    stops,
                    cache,
                }
            }
            "xr.state_select" => {
                let conditions = arguments(self)?;
                let index = self.selectors.len();
                self.selectors.push(StateSelector {
                    conditions,
                    persistent: self
                        .analysis
                        .persistent_state_selectors
                        .contains_key(attribute(op, "owner")?),
                });
                Expression::StateSelect(index)
            }
            "xr.translate" => {
                if !self.can_translate {
                    return Err(unsupported(op, "translations without a loaded catalog"));
                }
                let translation =
                    TranslationRef::from_operation(op).map_err(|error| invalid(op, error))?;
                Expression::Translate {
                    id: translation.id.to_owned(),
                    arguments: translation
                        .arguments
                        .into_iter()
                        .map(|(name, value)| {
                            Ok((name.to_owned(), self.expression(value, parameters)?))
                        })
                        .collect::<Result<_>>()?,
                }
            }
            "xr.markdown" => Expression::Literal(Value::StyledText(
                self.assets
                    .markdown
                    .get(spelling()?)
                    .ok_or_else(|| invalid(op, "unresolved Markdown literal"))?
                    .clone(),
            )),
            "xr.make_record" => {
                let Type::Record(name) = ty else {
                    return Err(invalid(op, "expected record result"));
                };
                let names: Vec<_> = self
                    .record_fields(op, &name)?
                    .iter()
                    .map(|field| field.name.clone())
                    .collect();
                let values: Vec<_> = arguments(self)?;
                if names.len() != values.len() {
                    return Err(invalid(op, "record field count mismatch"));
                }
                Expression::Record {
                    name: name.into(),
                    fields: names.into_iter().zip(values).collect(),
                }
            }
            "xr.default_record" => {
                let Type::Record(name) = ty else {
                    return Err(invalid(op, "expected record result"));
                };
                Expression::Literal(self.default_record(op, name.into())?)
            }
            "xr.record_array" => {
                let Type::RecordList(name) = ty else {
                    return Err(invalid(op, "expected record list result"));
                };
                Expression::RecordArray(name.into(), arguments(self)?)
            }
            "xr.get_field" => Expression::RecordField(argument(self, 0)?, spelling()?.to_owned()),
            "xr.date_field"
            | "xr.time_field"
            | "xr.font_field"
            | "xr.key_field"
            | "xr.wheel_field"
            | "xr.pointer_field"
            | "xr.touch_point_field" => Expression::BuiltinField(
                argument(self, 0)?,
                BuiltinField::from_operation(op).map_err(|error| invalid(op, error))?,
            ),
            "xr.make_date" => {
                Expression::MakeDate([argument(self, 0)?, argument(self, 1)?, argument(self, 2)?])
            }
            "xr.make_time" => {
                Expression::MakeTime([argument(self, 0)?, argument(self, 1)?, argument(self, 2)?])
            }
            "xr.make_font" => Expression::MakeFont([
                argument(self, 0)?,
                argument(self, 1)?,
                argument(self, 2)?,
                argument(self, 3)?,
            ]),
            "xr.pointer_buttons" => Expression::PointerButtons(arguments(self)?),
            "xr.array" => Expression::Array(arguments(self)?),
            "xr.call" => Expression::Call(member(op, "value")?, arguments(self)?),
            _ => return Err(unsupported(op, op.name())),
        };
        let expression = Rc::new(expression);
        self.expressions.insert(key, expression.clone());
        Ok(expression)
    }
}

fn load_control(op: Op<'_>, name: &'static str, ty: Type) -> Result<Control> {
    let kind_name = attribute(op, "kind")?;
    let default = control_default(kind_name, name, &ty).ok_or_else(|| {
        invalid(
            op,
            format_args!("unsupported control default {kind_name}.{name}"),
        )
    })?;
    let fallback = |value| match value {
        ControlLiteral::Literal(value) => literal(op, &ty, value),
        ControlLiteral::TypeDefault => runtime_default(&kind(op, &ty)?)
            .ok_or_else(|| invalid(op, "missing control type default")),
    };
    Ok(Control {
        name,
        default: match default {
            ControlDefault::Value(value) => ControlDefault::Value(fallback(value)?),
            ControlDefault::Binding(property, value) => {
                ControlDefault::Binding(property, fallback(value)?)
            }
            ControlDefault::Control(property) => ControlDefault::Control(property),
        },
        omit_default: name == "edit-text" && default_combo_edit_text(op),
    })
}

pub(super) fn default_value(
    records: &RecordSchemas,
    ty: &ValueType,
) -> std::result::Result<Value, String> {
    match ty {
        ValueType::Record(name) => {
            let fields = records
                .get(name.as_ref())
                .ok_or_else(|| format!("unknown record {name}"))?
                .iter()
                .map(|field| Ok((field.name.clone(), default_value(records, &field.ty)?)))
                .collect::<std::result::Result<_, String>>()?;
            Ok(Value::Record(RecordValue::new(name.clone(), fields)))
        }
        ValueType::RecordList(name) => {
            Ok(Value::RecordList(RecordList::new(name.clone(), Vec::new())))
        }
        ValueType::StringList => Ok(Value::StringList(Vec::new())),
        _ => runtime_default(ty).ok_or_else(|| format!("no runtime default for {ty:?}")),
    }
}

/// Match the defaults of the concrete runtime fields in generated Rust records.
fn runtime_default(kind: &ValueType) -> Option<Value> {
    macro_rules! defaults {
        ($($variant:ident),* $(,)?) => {
            match kind {
                $(ValueType::$variant => Value::$variant(Default::default()),)*
                ValueType::Image => Value::ImageAsset(ImageAsset::default()),
                ValueType::Percent => Value::Percent(Percent(0.0)),
                ValueType::Record(_) | ValueType::RecordList(_) | ValueType::StringList => return None,
            }
        };
    }
    Some(defaults!(
        EventResult,
        FocusReason,
        DialogRole,
        StandardButtonKind,
        DrawerEdge,
        FileDialogMode,
        MessageLevel,
        MessageButtons,
        KeyEvent,
        WheelEvent,
        PointerEvent,
        PointerEventKind,
        PointerButton,
        PointerButtons,
        CursorShape,
        SwipeSide,
        DragAction,
        TouchPoint,
        TouchPoints,
        KeyModifiers,
        Bool,
        Int,
        Float,
        Length,
        Color,
        Brush,
        String,
        Date,
        Time,
        Font,
        CheckState,
        StyledText,
        LayoutAlignment,
        Orientation,
        ScrollSnapMode,
        ScrollAlignment,
        SelectionTarget,
        ScrollBarPolicy,
        FlickDirection,
        FlickBoundsBehavior,
        FlickBoundsMovement,
        PopupPlacement,
        FlexboxLayoutDirection,
        FlexboxLayoutWrap,
        CrossAxisAlignment,
        TextWrap,
        TextHorizontalAlignment,
        TextVerticalAlignment,
        TextOverflow,
        ColorScheme,
        InputType,
        DialInputMode,
        ImageFit,
        ImageTileMode,
        ImageHorizontalAlignment,
        ImageVerticalAlignment,
        ImageRendering,
    ))
}

fn static_name(value: &str, names: &'static [&'static str]) -> Option<&'static str> {
    names.iter().copied().find(|&name| name == value)
}

fn property_name(op: Op<'_>) -> Result<&'static str> {
    let name = attribute(op, "name")?;
    static_name(name, PROPERTIES).ok_or_else(|| unsupported(op, &format!("visual property {name}")))
}

const ELEMENTS: &[&str] = &[
    "Window",
    "ApplicationWindow",
    "SecondaryWindow",
    "SystemTrayIcon",
    "WindowMoveArea",
    "Page",
    "PageHeader",
    "PageFooter",
    "FileDialog",
    "MessageDialog",
    "Rectangle",
    "Pane",
    "ControlFrame",
    "PopupWindow",
    "Image",
    "AnimatedImage",
    "Path",
    "Text",
    "Label",
    "StyledText",
    "TouchArea",
    "WheelHandler",
    "HoverHandler",
    "SwipeGestureHandler",
    "ScaleRotateGestureHandler",
    "SwipeDelegate",
    "SwipeLeft",
    "SwipeContent",
    "SwipeRight",
    "DragArea",
    "DropArea",
    "Timer",
    "DelayButton",
    "FocusScope",
    "Shortcut",
    "Action",
    "ActionGroup",
    "ButtonGroup",
    "RadioButton",
    "RadioGroup",
    "TabWidget",
    "Tab",
    "TabBar",
    "TabButton",
    "SwipeView",
    "SwipePage",
    "StackView",
    "StackPage",
    "PageIndicator",
    "MenuBar",
    "Menu",
    "MenuItem",
    "MenuSeparator",
    "ContextMenuArea",
    "ToolBar",
    "ToolButton",
    "ToolSeparator",
    "RoundButton",
    "Tooltip",
    "GroupBox",
    "Dialog",
    "DialogButton",
    "DialogButtonBox",
    "StandardButton",
    "Drawer",
    "ProgressIndicator",
    "BusyIndicator",
    "Spinner",
    "Button",
    "TextInput",
    "LineEdit",
    "TextEdit",
    "TextArea",
    "CheckBox",
    "ComboBox",
    "SpinBox",
    "DoubleSpinBox",
    "Slider",
    "Dial",
    "RangeSlider",
    "Calendar",
    "MonthGrid",
    "DayCell",
    "DayOfWeekRow",
    "WeekNumberColumn",
    "DatePickerPopup",
    "TimePicker",
    "TimePickerPopup",
    "ColorPickerPopup",
    "FontPickerPopup",
    "Tumbler",
    "Switch",
    "HorizontalBox",
    "VerticalBox",
    "HorizontalLayout",
    "VerticalLayout",
    "GridLayout",
    "FlowLayout",
    "FlexboxLayout",
    "Row",
    "ScrollView",
    "Flickable",
    "ScrollIndicator",
    "ScrollBar",
    "ListView",
    "StandardListView",
    "StandardTableView",
    "TableView",
    "TableColumn",
    "TableRow",
    "TableCell",
    "TreeView",
    "TreeRow",
    "TreeCell",
    "SplitView",
    "SplitHandle",
    "SearchField",
    "GridView",
];

const PROPERTIES: &[&str] = &[
    "x",
    "y",
    "width",
    "height",
    "min-width",
    "min-height",
    "max-width",
    "max-height",
    "preferred-width",
    "preferred-height",
    "horizontal-stretch",
    "vertical-stretch",
    "row",
    "col",
    "rowspan",
    "colspan",
    "spacing",
    "dot-size",
    "padding",
    "padding-left",
    "padding-right",
    "padding-top",
    "handle-size",
    "handle-hit-size",
    "handle-color",
    "interval",
    "repeat",
    "running",
    "triggered-on-start",
    "delay",
    "timeout",
    "tooltip",
    "progress",
    "progress-color",
    "indeterminate",
    "animated",
    "track-color",
    "padding-bottom",
    "visible",
    "source",
    "icon",
    "icon-template",
    "icon-size",
    "image-fit",
    "image-rendering",
    "horizontal-tile-mode",
    "vertical-tile-mode",
    "source-clip-x",
    "source-clip-y",
    "source-clip-width",
    "source-clip-height",
    "colorize",
    "playing",
    "paused",
    "speed",
    "loops",
    "commands",
    "viewbox-x",
    "viewbox-y",
    "viewbox-width",
    "viewbox-height",
    "fill",
    "stroke",
    "stroke-width",
    "fit",
    "opacity",
    "z",
    "clip",
    "title",
    "no-frame",
    "resize-border-width",
    "resizable",
    "mode",
    "directory",
    "file-name",
    "name-filters",
    "level",
    "buttons",
    "informative-text",
    "enabled",
    "highlighted",
    "text",
    "checked",
    "check-state",
    "tristate",
    "read-only",
    "placeholder-text",
    "max-length",
    "font-size",
    "font-family",
    "font",
    "default-font",
    "default-font-family",
    "default-font-size",
    "default-font-weight",
    "default-font-italic",
    "font-weight",
    "font-italic",
    "letter-spacing",
    "line-height",
    "background",
    "color",
    "current-color",
    "border-width",
    "border-radius",
    "border-top-left-radius",
    "border-top-right-radius",
    "border-bottom-left-radius",
    "border-bottom-right-radius",
    "alignment",
    "cross-axis-line-alignment",
    "horizontal-spacing",
    "vertical-spacing",
    "spacing-horizontal",
    "spacing-vertical",
    "flex-direction",
    "flex-wrap",
    "layout-order",
    "max-lines",
    "row-height",
    "header-height",
    "row-header-width",
    "row-header-text",
    "current-row",
    "current-key",
    "root-key",
    "parent-key",
    "key",
    "indentation",
    "follow-current",
    "frozen-columns",
    "multi-select",
    "selection-target",
    "drag-select",
    "resizable-columns",
    "resizable-rows",
    "movable-columns",
    "movable-rows",
    "resizable",
    "sortable",
    "copyable",
    "pasteable",
    "undoable",
    "edit-on-click",
    "edit-on-selected-click",
    "edit-on-double-click",
    "edit-on-key",
    "edit-on-typing",
    "edit-on-tab",
    "clipboard-text",
    "auto-sort",
    "filter-text",
    "filter-field",
    "cell-width",
    "cell-height",
    "column-width",
    "preserve-scroll-anchor",
    "link-color",
    "model",
    "text-role",
    "value-role",
    "enabled-role",
    "edit-text",
    "auto-complete",
    "current-index",
    "value",
    "minimum",
    "maximum",
    "step-size",
    "lower-value",
    "upper-value",
    "date",
    "display-month",
    "minimum-date",
    "maximum-date",
    "today",
    "week-start",
    "weekday-labels",
    "time",
    "use-24-hour-format",
    "show-seconds",
    "show-alpha",
    "show-color-plane",
    "preview-text",
    "accept-text",
    "cancel-text",
    "visible-item-count",
    "count",
    "input-mode",
    "start-angle",
    "end-angle",
    "decimals",
    "decimal-separator",
    "live",
    "wheel-enabled",
    "cursor-shape",
    "blocking",
    "accepted-buttons",
    "multi-touch",
    "press-and-hold-interval",
    "password-visible",
    "maximum-length",
    "suggestions",
    "suggestions-for",
    "step",
    "editable",
    "orientation",
    "snap-mode",
    "current-value",
    "cross-axis-alignment",
    "cross-axis-self-alignment",
    "horizontal-alignment",
    "vertical-alignment",
    "wrap",
    "overflow",
    "input-type",
    "color-scheme",
    "accent-color",
    "content-width",
    "content-height",
    "mouse-drag-pan-enabled",
    "handle-swipe-left",
    "handle-swipe-right",
    "handle-swipe-up",
    "handle-swipe-down",
    "open-side",
    "data",
    "drag-key",
    "allow-copy",
    "allow-move",
    "allow-link",
    "drop-action",
    "keys",
    "accept-files",
    "file-extensions",
    "viewport-height",
    "vertical-scroll-bar-policy",
    "horizontal-scroll-bar-policy",
    "policy",
    "minimum-size",
    "flick-direction",
    "bounds-behavior",
    "bounds-movement",
    "open",
    "close-on-escape",
    "close-on-outside",
    "placement",
    "anchor-gap",
    "modal",
    "focus",
    "dim",
    "rebound-duration",
    "interactive",
    "initial-page",
    "transition-duration",
    "size",
    "position",
    "accessible-name",
    "accessible-description",
    "focus-on-click",
    "restore-focus",
    "focus-on-tab-navigation",
    "context-menu-enabled",
    "sequence",
    "shortcut",
    "exclusive",
    "checkable",
    "icon-only",
    "radius",
    "content-padding",
    "role",
    "kind",
    "primary",
    "edge",
    "drag-margin",
    "sequences",
    "focus-within",
    "auto-repeat",
    "border-color",
    "drop-shadow-color",
    "drop-shadow-blur",
    "drop-shadow-spread",
    "drop-shadow-offset-x",
    "drop-shadow-offset-y",
    "inner-shadow-color",
    "inner-shadow-blur",
    "inner-shadow-spread",
    "inner-shadow-offset-x",
    "inner-shadow-offset-y",
    "transform-rotation",
    "transform-scale",
    "transform-scale-x",
    "transform-scale-y",
    "transform-origin-x",
    "transform-origin-y",
];
