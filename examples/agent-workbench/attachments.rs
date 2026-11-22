use session::{Attachment, AttachmentContent, FileExcerpt, Prompt, Skill, TerminalOutput};
use std::{
    cell::{Cell, RefCell},
    fs::{self, File},
    io::Read,
    path::Path,
    rc::Rc,
    thread,
};
use tokio::sync::oneshot;
use workbench_ui::{AgentWorkbench, AttachmentRow};
use xrack_runtime::{Image, ImageFormat, LoadedImage, StyledText};

const MAX_FILES: usize = 8;
pub(super) const TEXT_FILE_LIMIT: usize = 12 * 1024;
const TEXT_TOTAL_LIMIT: usize = 24 * 1024;
pub(super) const PNG_PREVIEW_LIMIT: u64 = 4 * 1024 * 1024;
const PNG_PREVIEW_PIXELS: u64 = 16_000_000;

type AfterLoad = Box<dyn FnOnce(&mut AgentWorkbench, &Attachments)>;

#[derive(Clone)]
pub struct DraftFile {
    key: String,
    source: Attachment,
    preview: Option<LoadedImage>,
}

#[derive(Clone, Default)]
pub struct Attachments {
    files: Rc<RefCell<Vec<DraftFile>>>,
    revision: Rc<Cell<u64>>,
    next_excerpt: Rc<Cell<u64>>,
}

impl Attachments {
    pub fn capture(&self) -> Vec<DraftFile> {
        self.files.borrow().clone()
    }

    pub fn restore(&self, ui: &mut AgentWorkbench, files: Vec<DraftFile>) {
        self.revision.set(self.revision.get().wrapping_add(1));
        self.files.replace(files);
        ui.set_attachment_loading(false);
        ui.set_attachment_status(String::new());
        ui.set_attachment_preview_open(false);
        ui.set_attachment_preview_image(Image::Empty);
        ui.set_attachment_preview_is_text(false);
        ui.set_attachment_preview_text(StyledText::from_plain_text(""));
        self.update_rows(ui);
    }

    pub fn restore_saved(&self, ui: &mut AgentWorkbench, sources: Vec<Attachment>) {
        let mut files = Vec::new();
        let mut unavailable = 0;
        for source in sources {
            let path = Path::new(&source.path);
            let bytes = source.content.inline_bytes();
            if !path.is_absolute()
                || files.len() >= MAX_FILES
                || bytes > TEXT_FILE_LIMIT
                || inline_bytes(&files) + bytes > TEXT_TOTAL_LIMIT
                || (matches!(
                    &source.content,
                    AttachmentContent::Image | AttachmentContent::Path
                ) && (!path.is_file() || path.canonicalize().as_deref().ok() != Some(path)))
                || (matches!(&source.content, AttachmentContent::Skill(_)) && !path.is_file())
            {
                unavailable += 1;
                continue;
            }
            let key = if matches!(
                &source.content,
                AttachmentContent::Excerpt(_)
                    | AttachmentContent::Terminal(_)
                    | AttachmentContent::Skill(_)
            ) {
                self.excerpt_key()
            } else {
                source.path.clone()
            };
            files.push(DraftFile {
                key,
                source,
                preview: None,
            });
        }
        self.restore(ui, files);
        if unavailable > 0 {
            ui.set_attachment_status(format!("{unavailable} 个附件无法恢复，请重新选择附件"));
        }
    }

    pub fn prompt(&self, text: String) -> Prompt {
        Prompt {
            text,
            attachments: self
                .files
                .borrow()
                .iter()
                .map(|file| file.source.clone())
                .collect(),
        }
    }

    fn update_rows(&self, ui: &mut AgentWorkbench) {
        let model = ui.model_attachment_rows();
        model.clear();
        for file in self.files.borrow().iter() {
            let kind = match &file.source.content {
                AttachmentContent::Image => "图片",
                AttachmentContent::Text(_) => "文本",
                AttachmentContent::Path => "路径",
                AttachmentContent::Excerpt(excerpt) => excerpt.origin.label(),
                AttachmentContent::Terminal(output) => output.label(),
                AttachmentContent::Skill(_) => "skill",
            };
            model.push(AttachmentRow {
                path: file.key.clone(),
                name: file.source.display_name(),
                kind: kind.into(),
                preview: file.preview.clone().map_or(Image::Empty, Image::Loaded),
                has_preview: file.preview.is_some()
                    || matches!(&file.source.content, AttachmentContent::Terminal(_)),
            });
        }
        ui.invoke_draft_files_changed();
    }

    fn excerpt_key(&self) -> String {
        let id = self.next_excerpt.get();
        self.next_excerpt.set(id.wrapping_add(1));
        format!("excerpt-{id}")
    }

    pub(super) fn add_workspace_file(
        &self,
        ui: &mut AgentWorkbench,
        path: String,
        workspace: String,
    ) -> bool {
        self.add_files(ui, vec![path], Some(workspace), None)
    }

    pub(super) fn add_workspace_mention(
        &self,
        ui: &mut AgentWorkbench,
        path: String,
        workspace: String,
        on_attached: impl FnOnce(&mut AgentWorkbench) + 'static,
    ) {
        let key = path.clone();
        let complete = move |ui: &mut AgentWorkbench, attachments: &Attachments| {
            if attachments
                .files
                .borrow()
                .iter()
                .any(|file| file.key == key)
            {
                on_attached(ui);
            }
        };
        self.add_files(ui, vec![path], Some(workspace), Some(Box::new(complete)));
    }

    pub(super) fn add_excerpt(
        &self,
        ui: &mut AgentWorkbench,
        path: String,
        excerpt: FileExcerpt,
    ) -> bool {
        if excerpt.contents.trim().is_empty() {
            return false;
        }
        self.add_inline(
            ui,
            Attachment {
                path,
                content: AttachmentContent::Excerpt(excerpt),
            },
            "该选段已加入草稿",
        )
    }

    pub(super) fn add_terminal_output(
        &self,
        ui: &mut AgentWorkbench,
        workspace: String,
        output: TerminalOutput,
    ) -> bool {
        self.add_inline(
            ui,
            Attachment {
                path: workspace,
                content: AttachmentContent::Terminal(output),
            },
            "该终端片段已加入草稿",
        )
    }

    pub(super) fn add_skill(&self, ui: &mut AgentWorkbench, skill: &Skill) -> bool {
        if !Path::new(&skill.path).is_file() {
            ui.set_attachment_status("技能文件已不可用，请重新选择".into());
            return false;
        }
        self.add_inline(
            ui,
            Attachment {
                path: skill.path.clone(),
                content: AttachmentContent::Skill(skill.name.clone()),
            },
            "",
        )
    }

    fn add_inline(
        &self,
        ui: &mut AgentWorkbench,
        source: Attachment,
        duplicate_message: &str,
    ) -> bool {
        if !can_attach(ui) {
            return false;
        }
        let mut files = self.files.borrow_mut();
        if files.iter().any(|file| file.source == source) {
            ui.set_attachment_status(duplicate_message.into());
            return true;
        }
        if files.len() >= MAX_FILES {
            ui.set_attachment_status(format!("每条消息最多附加 {MAX_FILES} 项"));
            return false;
        }
        let bytes = source.content.inline_bytes();
        if bytes > TEXT_FILE_LIMIT || inline_bytes(&files) + bytes > TEXT_TOTAL_LIMIT {
            ui.set_attachment_status("选段每项最多 12 KiB，随消息附带的文本合计最多 24 KiB".into());
            return false;
        }
        files.push(DraftFile {
            key: self.excerpt_key(),
            source,
            preview: None,
        });
        drop(files);
        self.update_rows(ui);
        ui.set_attachment_status(String::new());
        true
    }

    fn add_files(
        &self,
        ui: &mut AgentWorkbench,
        paths: Vec<String>,
        workspace: Option<String>,
        after_load: Option<AfterLoad>,
    ) -> bool {
        if paths.is_empty() || !can_attach(ui) {
            return false;
        }
        let existing = self.capture();
        let revision = self.revision.get().wrapping_add(1);
        self.revision.set(revision);
        ui.set_attachment_loading(true);
        ui.set_attachment_status(String::new());

        let (sender, receiver) = oneshot::channel();
        if let Err(error) = thread::Builder::new()
            .name("workbench-attachments".into())
            .spawn(move || {
                let _ = sender.send(load_batch(
                    existing,
                    paths,
                    workspace.as_deref().map(Path::new),
                ));
            })
        {
            ui.set_attachment_loading(false);
            ui.set_attachment_status(format!("读取附件失败（{error}）"));
            return false;
        }

        let weak = ui.as_weak();
        let attachments = self.clone();
        let result = xrack_platform_winit::spawn_local(async move {
            let result = receiver.await;
            if attachments.revision.get() != revision {
                return;
            }
            let Some(mut ui) = weak.upgrade() else { return };
            ui.set_attachment_loading(false);
            match result {
                Ok((files, status)) => {
                    attachments.files.replace(files);
                    attachments.update_rows(&mut ui);
                    ui.set_attachment_status(status);
                    if let Some(after_load) = after_load {
                        after_load(&mut ui, &attachments);
                    }
                }
                Err(_) => ui.set_attachment_status("附件读取已结束".into()),
            }
        });
        if let Err(error) = result {
            ui.set_attachment_loading(false);
            ui.set_attachment_status(error.to_string());
            return false;
        }
        true
    }
}

fn can_attach(ui: &AgentWorkbench) -> bool {
    !ui.get_attachment_loading()
        && !ui.get_sending_prompt()
        && !ui.get_followup_pending()
        && !ui.get_switching()
        && !ui.get_conversation_archived()
        && !ui.get_question_open()
}

fn inline_bytes(files: &[DraftFile]) -> usize {
    files
        .iter()
        .map(|file| file.source.content.inline_bytes())
        .sum()
}

fn load_batch(
    mut files: Vec<DraftFile>,
    paths: Vec<String>,
    workspace: Option<&Path>,
) -> (Vec<DraftFile>, String) {
    let mut text_bytes = inline_bytes(&files);
    let mut status = String::new();
    for path in paths {
        if files.len() == MAX_FILES {
            status = format!("每条消息最多附加 {MAX_FILES} 项");
            break;
        }
        let mut file = match load_file(Path::new(&path), workspace) {
            Ok(file) => file,
            Err(error) => {
                status = format!(
                    "无法读取 {}（{error}）",
                    Path::new(&path)
                        .file_name()
                        .and_then(|name| name.to_str())
                        .unwrap_or(&path)
                );
                continue;
            }
        };
        if files.iter().any(|existing| existing.key == file.key) {
            continue;
        }
        if let AttachmentContent::Text(text) = &file.source.content {
            if text_bytes + text.len() <= TEXT_TOTAL_LIMIT {
                text_bytes += text.len();
            } else {
                file.source.content = AttachmentContent::Path;
            }
        }
        files.push(file);
    }
    (files, status)
}

fn load_file(selected: &Path, workspace: Option<&Path>) -> Result<DraftFile, String> {
    let path = selected.canonicalize().map_err(|error| error.to_string())?;
    if let Some(workspace) = workspace {
        let scope = workspace
            .canonicalize()
            .map_err(|error| error.to_string())?;
        if path != selected
            || !path
                .strip_prefix(scope)
                .is_ok_and(crate::workspace_files::allowed_relative)
        {
            return Err("文件位置已变化，请从工作区重新打开".into());
        }
    }
    let metadata = fs::metadata(&path).map_err(|error| error.to_string())?;
    if !metadata.is_file() {
        return Err("请选择普通文件".into());
    }
    let path_str = path.to_str().ok_or("文件路径必须为 UTF-8")?.to_owned();
    let extension = path
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let mut preview = None;
    let content = match extension.as_str() {
        "png" | "jpeg" | "jpg" | "gif" | "webp" => {
            if extension == "png" && metadata.len() <= PNG_PREVIEW_LIMIT {
                let mut bytes = Vec::new();
                File::open(path)
                    .and_then(|file| file.take(PNG_PREVIEW_LIMIT + 1).read_to_end(&mut bytes))
                    .map_err(|error| error.to_string())?;
                if bytes.len() as u64 <= PNG_PREVIEW_LIMIT {
                    preview = preview_png(bytes)?;
                }
            }
            AttachmentContent::Image
        }
        _ if metadata.len() <= TEXT_FILE_LIMIT as u64 => {
            let mut bytes = Vec::new();
            File::open(path)
                .and_then(|file| {
                    file.take(TEXT_FILE_LIMIT as u64 + 1)
                        .read_to_end(&mut bytes)
                })
                .map_err(|error| error.to_string())?;
            if bytes.len() <= TEXT_FILE_LIMIT {
                String::from_utf8(bytes)
                    .ok()
                    .filter(|text| {
                        text.chars().all(|letter| {
                            !letter.is_control() || matches!(letter, '\n' | '\r' | '\t')
                        })
                    })
                    .map_or(AttachmentContent::Path, AttachmentContent::Text)
            } else {
                AttachmentContent::Path
            }
        }
        _ => AttachmentContent::Path,
    };
    Ok(DraftFile {
        key: path_str.clone(),
        source: Attachment {
            path: path_str,
            content,
        },
        preview,
    })
}

pub(super) fn preview_png(bytes: Vec<u8>) -> Result<Option<LoadedImage>, String> {
    let dimensions = bytes
        .get(16..24)
        .filter(|_| bytes.starts_with(b"\x89PNG\r\n\x1a\n"))
        .ok_or("PNG 文件头无效")?;
    let ([width, height], []) = dimensions.as_chunks::<4>() else {
        return Err("PNG 尺寸无效".into());
    };
    let pixels = u64::from(u32::from_be_bytes(*width)) * u64::from(u32::from_be_bytes(*height));
    if pixels > PNG_PREVIEW_PIXELS {
        return Ok(None);
    }
    let size = xrack_assets::image_size(ImageFormat::Png, &bytes)?;
    Ok(Some(LoadedImage::new(ImageFormat::Png, bytes, size)))
}

pub fn attach(ui: &mut AgentWorkbench) -> Attachments {
    let state = Attachments::default();
    let attachments = state.clone();
    let weak = ui.as_weak();
    ui.on_attach_files(move |paths| {
        if let Some(mut ui) = weak.upgrade() {
            attachments.add_files(&mut ui, paths, None, None);
        }
    });
    let attachments = state.clone();
    let weak = ui.as_weak();
    ui.on_remove_attachment(move |path| {
        if let Some(mut ui) = weak.upgrade()
            && !ui.get_attachment_loading()
            && !ui.get_sending_prompt()
            && !ui.get_followup_pending()
        {
            attachments
                .files
                .borrow_mut()
                .retain(|file| file.key != path);
            attachments.update_rows(&mut ui);
            ui.set_attachment_status(String::new());
        }
    });
    let attachments = state.clone();
    let weak = ui.as_weak();
    ui.on_preview_attachment(move |path| {
        if let Some(mut ui) = weak.upgrade()
            && let Some(file) = attachments
                .files
                .borrow()
                .iter()
                .find(|file| file.key == path)
        {
            if let Some(preview) = &file.preview {
                ui.set_attachment_preview_is_text(false);
                ui.set_attachment_preview_title(file.source.name().into());
                ui.set_attachment_preview_image(Image::Loaded(preview.clone()));
            } else if let AttachmentContent::Terminal(output) = &file.source.content {
                ui.set_attachment_preview_is_text(true);
                ui.set_attachment_preview_title(format!("{} · {}", output.label(), output.title));
                let text = if output.truncated {
                    format!("（较早的内容已省略）\n\n{}", output.contents)
                } else {
                    output.contents.clone()
                };
                ui.set_attachment_preview_text(StyledText::from_plain_text(text));
            } else {
                return;
            }
            ui.set_attachment_preview_open(true);
        }
    });
    let attachments = state.clone();
    let weak = ui.as_weak();
    ui.on_clear_attachments(move || {
        if let Some(mut ui) = weak.upgrade() {
            attachments.restore(&mut ui, Vec::new());
        }
    });
    state
}

#[cfg(test)]
pub(super) const PNG_FIXTURE: &[u8] = &[
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x02, 0x08, 0x02, 0x00, 0x00, 0x00, 0xfd, 0xd4, 0x9a,
    0x73, 0x00, 0x00, 0x00, 0x13, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9c, 0x63, 0xb0, 0xcd, 0xd9, 0x02,
    0x44, 0x0c, 0x4e, 0x8b, 0x9b, 0x80, 0x08, 0x00, 0x25, 0x12, 0x05, 0x89, 0xd2, 0x9d, 0xd6, 0x65,
    0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selected_skill_is_removable_and_can_be_restored_from_a_saved_draft() -> std::io::Result<()> {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("SKILL.md");
        fs::write(&path, "---\nname: ui-check\n---\n")?;
        let skill = Skill {
            name: "ui-check".into(),
            path: path.to_string_lossy().into(),
            description: String::new(),
        };
        let mut ui = AgentWorkbench::new();
        let attachments = attach(&mut ui);
        assert!(attachments.add_skill(&mut ui, &skill));
        assert!(attachments.add_skill(&mut ui, &skill));
        let saved = attachments.prompt(String::new()).attachments;
        assert_eq!(saved.len(), 1);

        attachments.restore_saved(&mut ui, saved.clone());
        let row = ui.model_attachment_rows().row(0).unwrap();
        assert_eq!(row.name, "$ui-check");
        assert_eq!(row.kind, "skill");
        ui.invoke_remove_attachment(row.path);
        assert!(attachments.prompt(String::new()).attachments.is_empty());

        fs::remove_file(path)?;
        attachments.restore_saved(&mut ui, saved);
        assert_eq!(ui.model_attachment_rows().len(), 0);
        assert!(!ui.get_attachment_status().is_empty());
        Ok(())
    }
    use session::ExcerptOrigin;

    #[test]
    fn excerpts_are_independent_removable_items_and_share_the_text_budget()
    -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let workspace = temp.path().canonicalize()?;
        let path = workspace.join("source.rs");
        fs::write(&path, vec![b'a'; TEXT_FILE_LIMIT])?;
        let path = path.to_str().unwrap();
        let mut ui = AgentWorkbench::new();
        let attachments = attach(&mut ui);
        let (files, _) = load_batch(Vec::new(), vec![path.into()], Some(&workspace));
        attachments.restore(&mut ui, files);

        let excerpt = |contents: String, line| FileExcerpt {
            contents,
            lines: Some((line, line)),
            origin: ExcerptOrigin::Draft,
        };
        assert!(attachments.add_excerpt(&mut ui, path.into(), excerpt("未保存".into(), 2)));
        assert!(attachments.add_excerpt(&mut ui, path.into(), excerpt("未保存".into(), 2)));
        assert!(attachments.add_excerpt(&mut ui, path.into(), excerpt("another slice".into(), 8)));
        assert_eq!(ui.model_attachment_rows().len(), 3);
        assert!(!attachments.add_excerpt(
            &mut ui,
            path.into(),
            excerpt("a".repeat(TEXT_TOTAL_LIMIT), 1)
        ));
        assert!(ui.get_attachment_status().contains("24 KiB"));
        let snapshot = attachments.capture();
        attachments.restore(&mut ui, Vec::new());
        assert!(ui.model_attachment_rows().is_empty());
        attachments.restore(&mut ui, snapshot);
        let key = ui.model_attachment_rows().row(1).unwrap().path;
        ui.invoke_remove_attachment(key);
        let output = attachments.prompt(String::new());
        assert_eq!(output.attachments.len(), 2);
        assert!(matches!(
            &output.attachments[0].content,
            AttachmentContent::Text(_)
        ));
        assert!(
            matches!(&output.attachments[1].content, AttachmentContent::Excerpt(excerpt) if excerpt.lines == Some((8,8)))
        );
        Ok(())
    }

    #[test]
    fn automatic_workspace_attachment_respects_the_selected_worktree()
    -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path().canonicalize()?;
        let worktree = root.join("worktree");
        fs::create_dir(&worktree)?;
        let outside = root.join("private.txt");
        fs::write(&outside, "outside")?;
        let inside = worktree.join("source.rs");
        fs::write(&inside, "local source")?;
        let (files, status) = load_batch(
            Vec::new(),
            vec![
                outside.to_str().unwrap().into(),
                inside.to_str().unwrap().into(),
            ],
            Some(&worktree),
        );
        assert!(status.contains("无法读取 private.txt"));
        assert_eq!(files.len(), 1);
        assert_eq!(
            files[0].source.content,
            AttachmentContent::Text("local source".into())
        );
        Ok(())
    }

    #[test]
    fn classifies_selected_files_and_manages_preview_and_removal()
    -> Result<(), Box<dyn std::error::Error>> {
        let tmp = tempfile::tempdir()?;
        let image = tmp.path().join("图像.png");
        let text = tmp.path().join("记录.md");
        let binary = tmp.path().join("bundle.bin");
        let large = tmp.path().join("large.txt");
        fs::write(&image, PNG_FIXTURE)?;
        fs::write(&text, "设计说明")?;
        fs::write(&binary, [0, 1, 2])?;
        fs::write(&large, vec![b'a'; TEXT_FILE_LIMIT + 1])?;
        let (files, status) = load_batch(
            Vec::new(),
            [&image, &text, &binary, &large, &image]
                .into_iter()
                .map(|path| path.to_str().unwrap().to_owned())
                .collect(),
            None,
        );
        assert!(status.is_empty());
        assert_eq!(files.len(), 4);
        assert_eq!(files[0].source.content, AttachmentContent::Image);
        assert!(
            matches!(files[1].source.content, AttachmentContent::Text(ref value) if value == "设计说明")
        );
        assert_eq!(files[2].source.content, AttachmentContent::Path);
        assert_eq!(files[3].source.content, AttachmentContent::Path);

        let mut window = AgentWorkbench::new();
        let attachments = attach(&mut window);
        let selected = files[0].source.path.clone();
        attachments.restore(&mut window, files);
        window.invoke_preview_attachment(selected.clone());
        assert!(window.get_attachment_preview_open());
        assert_eq!(window.get_attachment_preview_title(), "图像.png");
        assert_eq!(window.get_attachment_preview_image().size(), [2, 2]);
        window.invoke_remove_attachment(selected);
        assert_eq!(attachments.prompt(String::new()).attachments.len(), 3);
        let saved = attachments.capture();
        window.invoke_clear_attachments();
        assert!(window.model_attachment_rows().is_empty());
        attachments.restore(&mut window, saved);
        assert_eq!(window.model_attachment_rows().len(), 3);
        assert!(!window.get_attachment_preview_open());
        Ok(())
    }

    #[test]
    fn oversize_plain_context_is_sent_by_path_and_invalid_png_is_reported()
    -> Result<(), Box<dyn std::error::Error>> {
        let tmp = tempfile::tempdir()?;
        let paths = (0..3)
            .map(|i| tmp.path().join(format!("{i}.txt")))
            .collect::<Vec<_>>();
        for path in &paths {
            fs::write(path, vec![b'a'; TEXT_FILE_LIMIT])?;
        }
        let invalid = tmp.path().join("invalid.png");
        fs::write(&invalid, b"no signature")?;
        let (files, status) = load_batch(
            Vec::new(),
            paths
                .iter()
                .chain([&invalid])
                .map(|path| path.to_str().unwrap().to_owned())
                .collect(),
            None,
        );
        assert_eq!(files.len(), 3);
        assert_eq!(files[2].source.content, AttachmentContent::Path);
        assert!(status.contains("invalid.png"));
        Ok(())
    }
}
