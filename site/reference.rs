use pulldown_cmark::{CowStr, Event, Options, Parser, Tag, html};
use std::{collections::BTreeSet, fmt::Write};
use xrack_ir::semantic::{
    builtin_event, builtin_property, combo_state_property, event_parameters, event_return_type,
    media_object_read_type, visual_template_inputs,
};
use xrack_syntax::ast::{Span, Type};
use xrack_syntax::builtins::{
    MESH_LOADER_OUTPUTS, POSITION_CHILD_METHOD, VIEWPORT_MOVE_METHODS, media_methods,
    mesh_loader_output_type, navigation_move_methods, scene_view_methods, scroll_bar_methods,
    text_editor_methods,
};

const TEMPLATE: &str = include_str!("reference-template.html");
const ELEMENT_SOURCE: &str = include_str!("../crates/xrack-syntax/src/builtins.rs");
const CATALOG: &str = include_str!("../crates/xrack-lsp/src/catalog.rs");
const GUIDES: [(&str, &str, &str); 6] = [
    ("scene3d", "3D 场景", include_str!("../docs/scene3d.md")),
    ("language", ".xr 语言", include_str!("../docs/language.md")),
    (
        "controls-guide",
        "控件用法",
        include_str!("../docs/controls.md"),
    ),
    (
        "models",
        "列表、表格与树",
        include_str!("../docs/models.md"),
    ),
    ("desktop", "桌面应用", include_str!("../docs/desktop.md")),
    (
        "editor",
        "VS Code 与预览",
        include_str!("../docs/editor.md"),
    ),
];

fn candidates(name: &str) -> Result<BTreeSet<&'static str>, String> {
    let declaration = format!(r#"const {name}: &str = ""#);
    CATALOG
        .split_once(&declaration)
        .and_then(|(_, rest)| rest.split_once('"'))
        .map(|(words, _)| words.split_whitespace().collect())
        .ok_or_else(|| format!("cannot read {name} from the completion catalog"))
}

fn elements() -> Result<BTreeSet<&'static str>, String> {
    let list = ELEMENT_SOURCE
        .split_once("    elements!(")
        .and_then(|(_, rest)| rest.split_once("\n    )"))
        .map(|(list, _)| list)
        .ok_or("cannot read the shared built-in element registry")?;
    let mut entries = candidates("ELEMENTS")?;
    let mut registered = 0;
    for line in list.lines().map(str::trim).filter(|line| !line.is_empty()) {
        let name = line
            .trim_end_matches(',')
            .strip_prefix('"')
            .and_then(|name| name.strip_suffix('"'))
            .ok_or_else(|| format!("invalid built-in registry entry: {line}"))?;
        entries.insert(name);
        registered += 1;
    }
    if registered == 0 {
        return Err("the built-in element registry is empty".into());
    }
    entries.retain(|kind| xrack_ir::semantic::is_builtin(kind));
    Ok(entries)
}

fn escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn controls() -> Result<String, Box<dyn std::error::Error>> {
    let mut properties = candidates("PROPERTIES")?;
    properties.extend(MESH_LOADER_OUTPUTS.iter().map(|(name, _)| *name));
    let events = candidates("EVENTS")?;
    let mut rendered = String::from("<div class=\"control-list\">\n");
    for kind in elements()? {
        let name = escape(kind);
        writeln!(
            rendered,
            "<details class=\"control\" id=\"api-{name}\"><summary><code>{name}</code></summary><div class=\"control-body\"><pre><code>{name} {{ … }}</code></pre>"
        )?;
        if matches!(kind, "SceneView3D" | "MeshLoader3D") {
            rendered.push_str("<p><a href=\"#scene3d\">3D 场景入门与可运行示例</a></p>");
        }
        let mut fields = properties
            .iter()
            .filter_map(|&property| {
                builtin_property(kind, property)
                    .or_else(|| media_object_read_type(kind, property))
                    .or_else(|| mesh_loader_output_type(kind, property))
                    .or_else(|| combo_state_property(kind, property))
                    .or_else(|| {
                        xrack_ir::semantic::navigation_active_property(kind, property)
                            .then_some(xrack_syntax::ast::Type::Bool)
                    })
                    .map(|ty| (property, ty))
            })
            .peekable();
        if fields.peek().is_some() {
            rendered.push_str("<h3>属性</h3><table><thead><tr><th scope=\"col\">名称</th><th scope=\"col\">类型</th></tr></thead><tbody>");
            for (property, ty) in fields {
                writeln!(
                    rendered,
                    "<tr><td><code>{}</code>{}</td><td><code>{}</code></td></tr>",
                    escape(property),
                    if media_object_read_type(kind, property).is_some()
                        || mesh_loader_output_type(kind, property).is_some()
                        || property == "acceptable-input"
                        || xrack_ir::semantic::scroll_view_bar_metric(kind, property)
                        || xrack_ir::semantic::scroll_visual_property(kind, property)
                        || xrack_ir::semantic::scroll_state_property(kind, property)
                        || xrack_ir::semantic::flick_motion_property(kind, property).is_some()
                        || xrack_ir::semantic::range_position_property(kind, property)
                        || xrack_ir::semantic::combo_popup_property(kind, property).is_some()
                        || property == "count" && xrack_syntax::builtins::navigation_group(kind)
                        || kind == "StackLayout" && property == "active-index"
                        || xrack_ir::semantic::navigation_active_property(kind, property)
                        || kind == "ComboBox" && matches!(property, "count" | "current-text")
                        || matches!(kind, "Tumbler" | "PathView") && property == "current-text"
                    {
                        "（只读）"
                    } else {
                        ""
                    },
                    if matches!(kind, "ComboBox" | "Tumbler" | "PathView")
                        && property == "current-value"
                    {
                        "由 value-role 决定".into()
                    } else if matches!(kind, "Tumbler" | "PathView") && property == "model" {
                        "[string] 或记录列表".into()
                    } else if kind == "SceneView3D" && property == "model" {
                        "符合场景字段约束的记录列表，或 scenemodel3d".into()
                    } else if kind == "SceneView3D" && matches!(ty, Type::RecordList(_)) {
                        "符合字段约束的记录列表".into()
                    } else if kind == "SceneView3D" && matches!(ty, Type::Record(_)) {
                        "符合字段约束的记录".into()
                    } else {
                        escape(&ty.name())
                    }
                )?;
            }
            rendered.push_str("</tbody></table>");
        }
        if let Some(parameters) = visual_template_inputs(kind) {
            rendered.push_str("<h3>模板参数</h3><ul>");
            for (name, ty) in parameters {
                write!(
                    rendered,
                    "<li><code>{}</code> · {}</li>",
                    escape(name),
                    escape(&ty.name())
                )?;
            }
            if matches!(kind, "ComboOption" | "TumblerOption" | "PathViewOption") {
                rendered.push_str("<li><code>option-data</code> · 由模型项类型决定</li>");
            }
            rendered.push_str("</ul>");
        }
        let mut callbacks = events
            .iter()
            .filter(|event| builtin_event(kind, event))
            .peekable();
        if callbacks.peek().is_some() {
            rendered.push_str("<h3>回调</h3><ul>");
            for event in callbacks {
                let args = event_parameters(kind, event, None, Span::default())
                    .map(|args| {
                        args.iter()
                            .map(|ty| ty.name())
                            .collect::<Vec<_>>()
                            .join(", ")
                    })
                    .unwrap_or_else(|_| "由模型字段类型决定".into());
                let result = event_return_type(kind, event)
                    .map(|ty| format!(" → {}", ty.name()))
                    .unwrap_or_default();
                writeln!(
                    rendered,
                    "<li><code>{}({}){}</code></li>",
                    escape(event),
                    escape(&args),
                    escape(&result)
                )?;
            }
            rendered.push_str("</ul>");
        }
        let viewport_methods = if xrack_ir::semantic::viewport_kind(kind) {
            VIEWPORT_MOVE_METHODS
        } else {
            &[]
        };
        let mut methods = media_methods(kind)
            .iter()
            .chain(scroll_bar_methods(kind))
            .chain(text_editor_methods(kind))
            .chain(viewport_methods)
            .chain(navigation_move_methods(kind))
            .chain(scene_view_methods(kind))
            .chain(xrack_ir::semantic::content_viewport(kind).then_some(&POSITION_CHILD_METHOD))
            .peekable();
        if methods.peek().is_some() {
            rendered.push_str("<h3>方法</h3><ul>");
            for method in methods {
                writeln!(
                    rendered,
                    "<li><code>{}</code></li>",
                    escape(&method.signature())
                )?;
            }
            rendered.push_str("</ul>");
        }
        rendered.push_str("</div></details>\n");
    }
    rendered.push_str("</div>");
    Ok(rendered)
}

fn resolve_link(url: CowStr<'_>) -> CowStr<'_> {
    let site = match url.as_ref() {
        "../README.md" => Some("./"),
        "language.md" => Some("#language"),
        "controls.md" => Some("#controls-guide"),
        "models.md" => Some("#models"),
        "scene3d.md" => Some("#scene3d"),
        "desktop.md" => Some("#desktop"),
        "editor.md" => Some("#editor"),
        _ => None,
    };
    if let Some(site) = site {
        return CowStr::Borrowed(site);
    }
    if let Some(path) = url.strip_prefix("../") {
        let view = if path.ends_with('/') { "tree" } else { "blob" };
        return format!("https://github.com/smallsunsun1/xrack/{view}/main/{path}").into();
    }
    if let Some(path) = url.strip_prefix("./") {
        return format!("https://github.com/smallsunsun1/xrack/blob/main/docs/{path}").into();
    }
    if url
        .split('#')
        .next()
        .is_some_and(|path| path.ends_with(".md"))
        && !url.contains("://")
    {
        return format!("https://github.com/smallsunsun1/xrack/blob/main/docs/{url}").into();
    }
    url
}

fn guides() -> Result<String, std::fmt::Error> {
    let mut rendered = String::new();
    let options =
        Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS;
    for (id, title, markdown) in GUIDES {
        writeln!(
            rendered,
            "<article id=\"{id}\" class=\"guide\"><h2>{title}</h2>"
        )?;
        let body = markdown.split_once('\n').map_or(markdown, |(_, body)| body);
        let events = Parser::new_ext(body, options).map(|event| match event {
            Event::Start(Tag::Link {
                link_type,
                dest_url,
                title,
                id,
            }) => Event::Start(Tag::Link {
                link_type,
                dest_url: resolve_link(dest_url),
                title,
                id,
            }),
            other => other,
        });
        html::push_html(&mut rendered, events);
        rendered.push_str("</article>\n");
    }
    Ok(rendered)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    print!(
        "{}",
        TEMPLATE
            .replace("<!-- XRACK_CONTROLS -->", &controls()?)
            .replace("<!-- XRACK_GUIDES -->", &guides()?)
    );
    Ok(())
}
