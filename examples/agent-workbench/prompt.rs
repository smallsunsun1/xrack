use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::path::Path;

const FILE_CONTEXT: &str = "XRack attached file: ";
const FILE_REFERENCE: &str = "XRack local file reference: ";
const FILE_EXCERPT: &str = "XRack selected text: ";
const TERMINAL_CONTEXT: &str = "XRack attached terminal output: ";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExcerptOrigin {
    Source,
    Draft,
    MarkdownPreview,
}

impl ExcerptOrigin {
    fn key(self) -> &'static str {
        match self {
            Self::Source => "source",
            Self::Draft => "draft",
            Self::MarkdownPreview => "renderedMarkdown",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Source => "选段",
            Self::Draft => "草稿选段",
            Self::MarkdownPreview => "Markdown 摘录",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileExcerpt {
    pub contents: String,
    pub lines: Option<(usize, usize)>,
    pub origin: ExcerptOrigin,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalOutput {
    pub title: String,
    pub contents: String,
    pub selected: bool,
    pub history: bool,
    pub truncated: bool,
}

impl TerminalOutput {
    pub fn label(&self) -> &'static str {
        if self.selected {
            "终端选段"
        } else if self.history {
            "终端历史"
        } else {
            "终端输出"
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum AttachmentContent {
    Image,
    Text(String),
    Path,
    Excerpt(FileExcerpt),
    Terminal(TerminalOutput),
    Skill(String),
}

impl AttachmentContent {
    pub fn inline_bytes(&self) -> usize {
        match self {
            Self::Text(text) => text.len(),
            Self::Excerpt(excerpt) => excerpt.contents.len(),
            Self::Terminal(output) => output.contents.len(),
            _ => 0,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Attachment {
    pub path: String,
    pub content: AttachmentContent,
}

impl Attachment {
    pub fn name(&self) -> &str {
        match &self.content {
            AttachmentContent::Terminal(output) => &output.title,
            AttachmentContent::Skill(name) => name,
            _ => path_name(&self.path),
        }
    }

    pub fn display_name(&self) -> String {
        match &self.content {
            AttachmentContent::Excerpt(excerpt) => excerpt_name(&self.path, excerpt.lines),
            AttachmentContent::Skill(name) => format!("${name}"),
            _ => self.name().into(),
        }
    }

    fn display(&self) -> String {
        let kind = match &self.content {
            AttachmentContent::Excerpt(excerpt) => excerpt.origin.label(),
            AttachmentContent::Terminal(output) => output.label(),
            AttachmentContent::Skill(_) => return self.display_name(),
            _ => "附件",
        };
        format!("{kind} · {}", self.display_name())
    }

    fn input(&self) -> Value {
        let text = match &self.content {
            AttachmentContent::Image => return json!({"type":"localImage","path":self.path}),
            AttachmentContent::Skill(name) => {
                return json!({"type":"skill","name":name,"path":self.path});
            }
            AttachmentContent::Text(contents) => format!(
                "{FILE_CONTEXT}{}",
                json!({"path":self.path,"contents":contents})
            ),
            AttachmentContent::Path => format!("{FILE_REFERENCE}{}", json!({"path":self.path})),
            AttachmentContent::Excerpt(excerpt) => {
                let start = excerpt.lines.map(|(start, _)| start);
                let end = excerpt.lines.map(|(_, end)| end);
                format!(
                    "{FILE_EXCERPT}{}",
                    json!({
                        "path":self.path,"contents":excerpt.contents,"origin":excerpt.origin.key(),
                        "startLine":start,"endLine":end,
                    })
                )
            }
            AttachmentContent::Terminal(output) => format!(
                "{TERMINAL_CONTEXT}{}",
                json!({
                    "workspace": self.path, "title": output.title, "contents": output.contents,
                    "selected": output.selected, "history": output.history, "truncated": output.truncated,
                })
            ),
        };
        json!({"type":"text","text":text,
            "text_elements":[{"byteRange":{"start":0,"end":text.len()},
                "placeholder":self.display()}]})
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Prompt {
    pub text: String,
    pub attachments: Vec<Attachment>,
}

impl Prompt {
    pub fn is_empty(&self) -> bool {
        self.text.trim().is_empty() && self.attachments.is_empty()
    }

    pub(crate) fn has_images(&self) -> bool {
        self.attachments
            .iter()
            .any(|file| file.content == AttachmentContent::Image)
    }

    pub(crate) fn input(&self) -> Vec<Value> {
        let mut items = Vec::new();
        if !self.text.trim().is_empty() {
            items.push(json!({"type":"text","text":self.text}));
        }
        items.extend(self.attachments.iter().map(Attachment::input));
        items
    }

    pub(crate) fn display(&self) -> String {
        let mut message = self.text.clone();
        for attachment in &self.attachments {
            append_line(&mut message, &attachment.display());
        }
        message
    }
}

impl From<String> for Prompt {
    fn from(text: String) -> Self {
        Self {
            text,
            attachments: Vec::new(),
        }
    }
}

impl From<&str> for Prompt {
    fn from(text: &str) -> Self {
        Self::from(text.to_owned())
    }
}

fn append_line(message: &mut String, line: &str) {
    if !message.is_empty() {
        message.push('\n');
    }
    message.push_str(line);
}

fn path_name(path: &str) -> &str {
    Path::new(path)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(path)
}

fn excerpt_name(path: &str, lines: Option<(usize, usize)>) -> String {
    let name = path_name(path);
    match lines {
        Some((first, last)) if first < last => format!("{name}:{first}–{last}"),
        Some((line, _)) => format!("{name}:{line}"),
        None => name.into(),
    }
}

pub(crate) fn attachment_label(text: &str) -> Option<String> {
    if let Some(data) = text.strip_prefix(TERMINAL_CONTEXT) {
        let info: Value = serde_json::from_str(data).ok()?;
        info["workspace"].as_str()?;
        let output: TerminalOutput = serde_json::from_value(info).ok()?;
        return Some(format!("{} · {}", output.label(), output.title));
    }
    if let Some(data) = text.strip_prefix(FILE_EXCERPT) {
        let info: Value = serde_json::from_str(data).ok()?;
        let origin = match info["origin"].as_str()? {
            "source" => ExcerptOrigin::Source,
            "draft" => ExcerptOrigin::Draft,
            "renderedMarkdown" => ExcerptOrigin::MarkdownPreview,
            _ => return None,
        };
        info["contents"].as_str()?;
        let lines = info["startLine"]
            .as_u64()
            .zip(info["endLine"].as_u64())
            .and_then(|(first, last)| {
                Some((usize::try_from(first).ok()?, usize::try_from(last).ok()?))
            });
        return Some(format!(
            "{} · {}",
            origin.label(),
            excerpt_name(info["path"].as_str()?, lines)
        ));
    }
    let data = text
        .strip_prefix(FILE_CONTEXT)
        .or_else(|| text.strip_prefix(FILE_REFERENCE))?;
    let info: Value = serde_json::from_str(data).ok()?;
    Some(format!("附件 · {}", path_name(info["path"].as_str()?)))
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UserMessage {
    pub display: String,
    prompt: Option<String>,
}

impl UserMessage {
    pub(crate) fn from_prompt(prompt: &Prompt) -> Self {
        Self::new(prompt.display(), prompt.text.clone())
    }

    fn new(display: String, prompt: String) -> Self {
        let prompt = (prompt != display).then_some(prompt);
        Self { display, prompt }
    }

    pub fn prompt_text(&self) -> &str {
        self.prompt.as_deref().unwrap_or(&self.display)
    }
}

impl From<&str> for UserMessage {
    fn from(text: &str) -> Self {
        Self {
            display: text.into(),
            prompt: None,
        }
    }
}

pub(crate) fn user_message(input: &Value) -> Option<UserMessage> {
    let mut display = String::new();
    let mut prompt = String::new();
    for part in input.as_array()? {
        match part["type"].as_str() {
            Some("localImage" | "localAudio") => {
                if let Some(path) = part["path"].as_str() {
                    append_line(&mut display, &format!("附件 · {}", path_name(path)));
                }
            }
            Some("image" | "audio") => append_line(&mut display, "媒体附件"),
            Some("skill") => {
                if let Some(name) = part["name"].as_str().filter(|name| !name.is_empty()) {
                    append_line(&mut display, &format!("${name}"));
                }
            }
            Some("text") => {
                let Some(text) = part["text"].as_str() else {
                    continue;
                };
                if let Some(label) = attachment_label(text) {
                    append_line(&mut display, &label);
                } else if !text.is_empty() {
                    append_line(&mut display, text);
                    append_line(&mut prompt, text);
                }
            }
            _ => {}
        }
    }
    (!display.is_empty()).then(|| UserMessage::new(display, prompt))
}

pub(crate) fn user_input_text(input: &Value) -> Option<String> {
    user_message(input).map(|message| message.display)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selected_skill_keeps_its_path_in_the_wire_input_and_its_name_in_history() {
        let prompt = Prompt {
            text: "验证设置界面".into(),
            attachments: vec![Attachment {
                path: "/workspace/.agents/skills/ui-check/SKILL.md".into(),
                content: AttachmentContent::Skill("ui-check".into()),
            }],
        };
        let items = prompt.input();
        assert_eq!(items[1]["type"], "skill");
        assert_eq!(items[1]["name"], "ui-check");
        assert_eq!(
            items[1]["path"],
            "/workspace/.agents/skills/ui-check/SKILL.md"
        );
        let message = user_message(&Value::Array(items)).unwrap();
        assert_eq!(message.display, "验证设置界面\n$ui-check");
        assert_eq!(message.display, prompt.display());
        assert_eq!(message.prompt_text(), "验证设置界面");
    }

    #[test]
    fn terminal_context_keeps_the_log_in_the_wire_text_and_only_a_label_in_history() {
        let prompt = Prompt {
            text: "帮我检查构建失败".into(),
            attachments: vec![Attachment {
                path: "/workspace/xrack".into(),
                content: AttachmentContent::Terminal(TerminalOutput {
                    title: "Shell 2".into(),
                    contents: "error: 无法编译\n位置 src/main.rs".into(),
                    selected: false,
                    history: true,
                    truncated: true,
                }),
            }],
        };
        let parts = prompt.input();
        let wire = parts[1]["text"].as_str().unwrap();
        let metadata: Value =
            serde_json::from_str(wire.strip_prefix(TERMINAL_CONTEXT).unwrap()).unwrap();
        assert_eq!(metadata["contents"], "error: 无法编译\n位置 src/main.rs");
        assert_eq!(metadata["workspace"], "/workspace/xrack");
        assert_eq!(metadata["truncated"], true);
        assert_eq!(parts[1]["text_elements"][0]["byteRange"]["end"], wire.len());
        let restored = user_message(&Value::Array(parts)).unwrap();
        assert_eq!(restored.display, "帮我检查构建失败\n终端历史 · Shell 2");
        assert_eq!(restored.prompt_text(), prompt.text);
    }

    #[test]
    fn selected_draft_has_a_scoped_line_reference_in_history_and_exact_wire_text() {
        let prompt = Prompt {
            text: "解释选段".into(),
            attachments: vec![Attachment {
                path: "/workspace/src/计数器.rs".into(),
                content: AttachmentContent::Excerpt(FileExcerpt {
                    contents: "let 数量 = 2;\n数量 += 1;".into(),
                    lines: Some((9, 10)),
                    origin: ExcerptOrigin::Draft,
                }),
            }],
        };
        let items = prompt.input();
        let text = items[1]["text"].as_str().unwrap();
        let metadata: Value =
            serde_json::from_str(text.strip_prefix(FILE_EXCERPT).unwrap()).unwrap();
        assert_eq!(metadata["contents"], "let 数量 = 2;\n数量 += 1;");
        assert_eq!(metadata["path"], "/workspace/src/计数器.rs");
        assert_eq!(metadata["origin"], "draft");
        assert_eq!(
            (metadata["startLine"].as_u64(), metadata["endLine"].as_u64()),
            (Some(9), Some(10))
        );
        assert_eq!(items[1]["text_elements"][0]["byteRange"]["end"], text.len());
        let label = "解释选段\n草稿选段 · 计数器.rs:9–10";
        assert_eq!(prompt.display(), label);
        let restored = user_message(&Value::Array(items.clone())).unwrap();
        assert_eq!(restored.prompt_text(), "解释选段");
        assert_eq!(restored.display, label);
        assert_eq!(
            user_input_text(&Value::Array(items)).as_deref(),
            Some(label)
        );
    }

    #[test]
    fn typed_input_round_trips_attachment_labels_without_showing_file_body() {
        let prompt = Prompt {
            text: "检查附件".into(),
            attachments: vec![
                Attachment {
                    path: "/workspace/counter.png".into(),
                    content: AttachmentContent::Image,
                },
                Attachment {
                    path: "/workspace/记录.md".into(),
                    content: AttachmentContent::Text("inline-context".into()),
                },
                Attachment {
                    path: "/workspace/archive.zip".into(),
                    content: AttachmentContent::Path,
                },
            ],
        };
        let input = prompt.input();
        assert_eq!(
            input[1],
            json!({"type":"localImage","path":"/workspace/counter.png"})
        );
        assert!(
            input[2]["text"]
                .as_str()
                .unwrap()
                .contains("inline-context")
        );
        assert_eq!(
            input[2]["text_elements"][0]["byteRange"]["end"],
            input[2]["text"].as_str().unwrap().len()
        );
        assert_eq!(
            user_input_text(&Value::Array(input)),
            Some(prompt.display())
        );
        let image_only = Prompt {
            text: String::new(),
            attachments: vec![prompt.attachments[0].clone()],
        };
        assert!(!image_only.is_empty());
        assert_eq!(image_only.input().len(), 1);
    }
}
