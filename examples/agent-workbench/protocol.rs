use crate::code_review::ReviewTarget;
use crate::context::ContextUsage;
use crate::drafts::{DraftKey, DraftStore};
use crate::followup::QueuedMessage;
use crate::models::{ModelInfo, TurnOptions};
use crate::project_index::ProjectIndex;
use crate::prompt::{Prompt, UserMessage, user_input_text, user_message};
use crate::questions::{PendingInput, QuestionPage};
use crate::skills::{SkillCatalog, SkillCatalogState, parse_catalog};
use crate::tab_store::{SavedTab, SavedTabs, TabStore};
use crate::thread_index::ThreadIndex;
use crate::turn_diff::TurnDiff;
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, HashMap, HashSet, VecDeque},
    ops::Range,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

const RESPONSE_TIMEOUT: Duration = Duration::from_secs(30);
const COMMAND_OUTPUT_LIMIT: usize = 8 * 1024;
const SESSION_PAGE_SIZE: usize = 30;
const RECENT_TURN_LIMIT: usize = 16;

#[derive(Clone)]
pub enum Entry {
    User(UserMessage),
    Assistant { text: String, finalized: bool },
    Proposal { text: String, finalized: bool },
    Command(CommandActivity),
    Plan(PlanProgress),
    Changes(TurnDiff),
    Compaction,
    ReviewReport(String),
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ActivityState {
    Pending,
    Running,
    Completed,
    Failed,
    Declined,
    Interrupted,
    Unfinished,
}

#[derive(Clone)]
pub struct CommandActivity {
    pub command: String,
    pub state: ActivityState,
    pub exit_code: Option<i64>,
    pub output: String,
    pub output_truncated: bool,
}

impl CommandActivity {
    fn new(command: &str) -> Self {
        Self {
            command: command.to_owned(),
            state: ActivityState::Running,
            exit_code: None,
            output: String::new(),
            output_truncated: false,
        }
    }

    fn append_output(&mut self, output: &str) {
        if self.output_truncated {
            return;
        }
        let count = output.floor_char_boundary(COMMAND_OUTPUT_LIMIT - self.output.len());
        self.output.push_str(&output[..count]);
        self.output_truncated = count < output.len();
    }

    fn complete(&mut self, command: &str, item: &Value) {
        self.command = command.to_owned();
        self.exit_code = item["exitCode"].as_i64();
        self.state = match item["status"].as_str() {
            Some("completed") if self.exit_code.is_none_or(|code| code == 0) => {
                ActivityState::Completed
            }
            Some("failed" | "completed") => ActivityState::Failed,
            Some("declined") => ActivityState::Declined,
            _ => ActivityState::Unfinished,
        };
        if let Some(output) = item["aggregatedOutput"].as_str() {
            self.output.clear();
            self.output_truncated = false;
            self.append_output(output);
        }
    }
}

#[derive(Clone)]
pub struct PlanStep {
    pub text: String,
    pub state: ActivityState,
}

#[derive(Clone)]
pub struct PlanProgress {
    pub explanation: Option<String>,
    pub steps: Vec<PlanStep>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConversationSummary {
    pub id: i64,
    pub workspace: String,
    pub title: String,
    pub archived: bool,
    pub pinned: bool,
    pub project_workspace: Option<String>,
    pub activity: ConversationActivity,
}

impl ConversationSummary {
    pub fn project(&self) -> &str {
        self.project_workspace.as_deref().unwrap_or(&self.workspace)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ConversationActivity {
    #[default]
    Idle,
    Running,
    NeedsInput,
    Updated,
}

impl ConversationActivity {
    pub fn key(self) -> &'static str {
        match self {
            Self::Idle => "",
            Self::Running => "running",
            Self::NeedsInput => "needs-input",
            Self::Updated => "updated",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ForkPoint {
    pub id: String,
    pub label: String,
    pub response: Option<usize>,
    pub prompt: Option<usize>,
    pub entries: Range<usize>,
}

/// A session status or a diagnostic supplied by a tool or the host.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Status {
    Detail(String),
    Connecting,
    Disconnected,
    ChooseWorkspace,
    Archived,
    ReadyWrite,
    ReadyRead,
    AwaitingReply,
    Stopping,
    Stopped,
    Reviewing,
    Reviewed,
    Compacting,
    Compacted,
    WorkingOnQueuedMessage,
    AwaitingAnswer,
    Resuming,
    LoadingHistory,
    Completed,
    Queued,
    Steered,
}

impl Status {
    pub fn presentation(&self) -> (&'static str, &str) {
        match self {
            Self::Detail(detail) => ("", detail),
            Self::Connecting => ("connecting", ""),
            Self::Disconnected => ("disconnected", ""),
            Self::ChooseWorkspace => ("choose-workspace", ""),
            Self::Archived => ("archived", ""),
            Self::ReadyWrite => ("ready-write", ""),
            Self::ReadyRead => ("ready-read", ""),
            Self::AwaitingReply => ("awaiting-reply", ""),
            Self::Stopping => ("stopping", ""),
            Self::Stopped => ("stopped", ""),
            Self::Reviewing => ("reviewing", ""),
            Self::Reviewed => ("reviewed", ""),
            Self::Compacting => ("compacting", ""),
            Self::Compacted => ("compacted", ""),
            Self::WorkingOnQueuedMessage => ("working-on-queued-message", ""),
            Self::AwaitingAnswer => ("awaiting-answer", ""),
            Self::Resuming => ("resuming", ""),
            Self::LoadingHistory => ("loading-history", ""),
            Self::Completed => ("completed", ""),
            Self::Queued => ("queued", ""),
            Self::Steered => ("steered", ""),
        }
    }

    fn is_empty(&self) -> bool {
        matches!(self, Self::Detail(detail) if detail.is_empty())
    }
}

impl Default for Status {
    fn default() -> Self {
        Self::Detail(String::new())
    }
}

impl From<String> for Status {
    fn from(detail: String) -> Self {
        Self::Detail(detail)
    }
}

impl From<&str> for Status {
    fn from(detail: &str) -> Self {
        Self::Detail(detail.into())
    }
}

#[derive(Clone, Default)]
pub struct ConversationView {
    pub conversation: i64,
    pub draft_key: DraftKey,
    pub restoring: bool,
    pub renamable: bool,
    pub rename_pending: bool,
    pub rename_error: String,
    pub archive_pending: bool,
    pub turn_options: TurnOptions,
    pub plan_mode: bool,
    pub question: Option<Arc<QuestionPage>>,
    pub context_usage: Option<ContextUsage>,
    pub compacting: bool,
    pub reviewing: bool,
    pub fork_points: Arc<Vec<ForkPoint>>,
    pub initial_draft: Option<Arc<str>>,
    pub active_turn_id: String,
    pub prompt_pending: bool,
    pub sent_prompts: u64,
    pub followup_pending: bool,
    pub followup_error: String,
    pub queued: Arc<Vec<QueuedMessage>>,
    pub queue_loading: bool,
    pub queue_pending: bool,
    pub queue_resumed: bool,
    pub queue_edit_saved: u64,
    pub queue_edit_error: String,
    pub earlier_turns: bool,
    pub busy: bool,
    pub handoff_pending: bool,
    pub handoff_error: String,
    pub associated_worktree: Option<String>,
    pub stopping: bool,
    pub status: Status,
    pub workspace: String,
    project_workspace: String,
    pub entries: Vec<Arc<Entry>>,
    unread: bool,
}

impl ConversationView {
    fn new(conversation: i64, workspace: String, project: Option<&str>) -> Self {
        let project_workspace = project.unwrap_or(&workspace).to_owned();
        Self {
            conversation,
            workspace,
            project_workspace,
            ..Self::default()
        }
    }

    fn activity(&self) -> ConversationActivity {
        if self.question.is_some() {
            ConversationActivity::NeedsInput
        } else if self.busy {
            ConversationActivity::Running
        } else if self.unread {
            ConversationActivity::Updated
        } else {
            ConversationActivity::Idle
        }
    }
}

#[derive(Clone)]
pub struct Snapshot {
    pub view: ConversationView,
    pub(crate) acknowledged: u64,
    pub conversations: Vec<ConversationSummary>,
    pub connected: bool,
    pub composer_ready: bool,
    pub tabs_restoring: bool,
    pub restored_tabs: Option<Arc<Vec<i64>>>,
    pub navigation_back: bool,
    pub navigation_forward: bool,
    pub history_loading: bool,
    pub history_more: bool,
    pub history_status: String,
    pub history_query: String,
    history_matches: HashSet<i64>,
    pub archive_view: bool,
    pub pin_status: String,
    pub models: Arc<Vec<ModelInfo>>,
    pub models_loading: bool,
    pub models_status: String,
    pub skills: Option<Arc<SkillCatalog>>,
    pub plan_available: bool,
    pub fork_pending: bool,
    pub fork_error: String,
    pub forks_created: u64,
    pub projects: Arc<Vec<String>>,
    pub project_status: String,
    pub project_histories: Arc<HashMap<String, ProjectHistory>>,
    pub project_search: Arc<ProjectSearch>,
    pub conversation_tools: Arc<HashMap<i64, ConversationTools>>,
    pub sidebar_status: String,
}

#[derive(Clone, Default, PartialEq, Eq)]
pub struct ConversationTools {
    pub rename: bool,
    pub archive: bool,
    pub pin: bool,
    pub rename_pending: bool,
    pub rename_error: String,
}

#[derive(Clone, Default)]
pub struct ProjectSearch {
    pub query: String,
    pub archived: bool,
    pub loading: bool,
    pub status: String,
    pub matches: Vec<i64>,
    cursor: Option<String>,
    workspaces: BTreeMap<String, String>,
}

impl ProjectSearch {
    pub fn more(&self) -> bool {
        self.cursor.is_some()
    }
}

#[derive(Clone, Default, PartialEq, Eq)]
pub struct ProjectHistory {
    pub loading: bool,
    pub loaded: bool,
    pub status: String,
    cursor: Option<String>,
    workspaces: Vec<String>,
}

impl ProjectHistory {
    pub fn more(&self) -> bool {
        self.cursor.is_some()
    }
}

struct SessionListScope<'a> {
    workspaces: &'a BTreeMap<String, String>,
    archived: bool,
    searching: bool,
    refresh_existing: bool,
    linked_read: bool,
    restore_tab: bool,
}

impl Snapshot {
    pub fn connecting(workspace: String) -> Self {
        let conversations = if workspace.is_empty() {
            Vec::new()
        } else {
            vec![ConversationSummary {
                id: 0,
                workspace: workspace.clone(),
                title: "新会话".into(),
                archived: false,
                pinned: false,
                project_workspace: None,
                activity: ConversationActivity::Idle,
            }]
        };
        let projects = Arc::new(
            (!workspace.is_empty())
                .then(|| workspace.clone())
                .into_iter()
                .collect(),
        );
        let mut view = ConversationView::new(0, workspace, None);
        view.status = Status::Connecting;
        Self {
            view,
            projects,
            project_status: String::new(),
            project_histories: Arc::default(),
            project_search: Arc::default(),
            conversation_tools: Arc::default(),
            sidebar_status: String::new(),
            acknowledged: 0,
            conversations,
            connected: false,
            composer_ready: false,
            tabs_restoring: false,
            restored_tabs: None,
            navigation_back: false,
            navigation_forward: false,
            history_loading: false,
            history_more: false,
            history_status: String::new(),
            history_query: String::new(),
            history_matches: HashSet::new(),
            archive_view: false,
            pin_status: String::new(),
            models: Arc::default(),
            models_loading: false,
            models_status: String::new(),
            skills: None,
            plan_available: false,
            fork_pending: false,
            fork_error: String::new(),
            forks_created: 0,
        }
    }

    pub fn shows_conversation(&self, row: &ConversationSummary) -> bool {
        let query = self.history_query.trim();
        row.archived == self.archive_view
            && row.project() == self.view.project_workspace
            && (query.is_empty() || self.history_matches.contains(&row.id))
    }

    pub fn project_workspace(&self) -> &str {
        &self.view.project_workspace
    }

    pub fn current_archived(&self) -> bool {
        self.conversations
            .iter()
            .any(|row| row.id == self.view.conversation && row.archived)
    }
}

pub enum Input {
    Workspace(String),
    Reset,
    Select(i64),
    MarkActivityRead,
    Navigate {
        forward: bool,
    },
    History {
        more: bool,
    },
    ProjectHistory {
        path: String,
        more: bool,
    },
    ProjectSearch {
        query: String,
        archived: bool,
        more: bool,
    },
    Search(String),
    Rename(String),
    RenameConversation {
        conversation: i64,
        name: String,
    },
    ArchiveView(bool),
    SetArchived(bool),
    SetConversationArchived {
        conversation: i64,
        archived: bool,
    },
    SetPinned {
        conversation: i64,
        pinned: bool,
    },
    LoadModels,
    LoadSkills(String),
    ChooseModel(String),
    ChooseEffort(String),
    ChoosePlanMode(bool),
    AnswerQuestion {
        token: i64,
        index: i64,
        value: Option<String>,
    },
    EarlierTurns,
    SaveOpenTabs {
        open: Vec<i64>,
        active: Option<i64>,
    },
    Compact,
    Review(ReviewTarget),
    Fork {
        title: String,
        last_turn: String,
        worktree: Option<String>,
    },
    ForkBeforePrompt {
        conversation: i64,
        turn: String,
        draft: String,
    },
    Followup {
        conversation: i64,
        turn: Option<String>,
        prompt: Prompt,
    },
    RemoveQueued(String),
    StartQueued(String),
    UpdateQueued {
        conversation: i64,
        original: QueuedMessage,
        text: String,
    },
    MoveQueued {
        conversation: i64,
        id: String,
        step: i64,
    },
    ResumeQueue,
    Handoff {
        conversation: i64,
        original: String,
        workspace: String,
        worktree: String,
    },
    SendInWorktree {
        conversation: i64,
        original: String,
        workspace: String,
        prompt: Prompt,
    },
    Send(Prompt),
    Interrupt(i64),
}

enum Request {
    Initialize,
    Models {
        cursor: Option<String>,
    },
    Skills,
    Modes,
    Thread {
        attempt: u64,
        prompt: Prompt,
    },
    Resume {
        attempt: u64,
        prompt: Prompt,
    },
    Sessions {
        workspace: String,
        more: bool,
    },
    ProjectSessions {
        project: String,
        workspaces: Vec<String>,
    },
    SearchProjects,
    Rename {
        conversation: i64,
        name: String,
    },
    SetArchived {
        conversation: i64,
        archived: bool,
    },
    History {
        conversation: i64,
        earlier: bool,
    },
    Turn(u64),
    Interrupt(u64),
    Compact {
        conversation: i64,
        resume: bool,
    },
    Review {
        conversation: i64,
        prepare: Option<ReviewTarget>,
    },
    Fork {
        source: i64,
        title: String,
        worktree: Option<String>,
        initial_draft: Option<String>,
    },
    ForkTitle {
        conversation: i64,
        title: String,
    },
    IndexedThread {
        workspace: String,
        thread_id: String,
        actual_workspace: String,
    },
    RestoreTab(SavedTab),
    Handoff {
        thread: String,
        workspace: String,
        worktree: String,
        resume: bool,
    },
    Followup {
        turn: Option<String>,
    },
    QueueList {
        conversation: i64,
        collected: Vec<QueuedMessage>,
        cursors: HashSet<String>,
    },
    QueueAction {
        conversation: i64,
        action: QueueAction,
    },
}

impl Request {
    fn is_shared(&self) -> bool {
        matches!(
            self,
            Self::Initialize
                | Self::Models { .. }
                | Self::Skills
                | Self::Modes
                | Self::Sessions { .. }
                | Self::ProjectSessions { .. }
                | Self::SearchProjects
                | Self::Rename { .. }
                | Self::SetArchived { .. }
                | Self::IndexedThread { .. }
                | Self::RestoreTab(_)
                | Self::Fork { .. }
                | Self::ForkTitle { .. }
        )
    }
}

struct PendingRequest {
    sent: Instant,
    owner: Option<i64>,
    kind: Request,
}

enum QueueAction {
    Remove,
    Start,
    Resume,
    Update(String),
    Reorder,
}

struct ActiveTurn {
    attempt: u64,
    id: Option<String>,
    sent: bool,
    interrupt_sent: bool,
    items: HashMap<String, usize>,
    plan_index: Option<usize>,
    diff_index: Option<usize>,
    options: TurnOptions,
    mode: Option<bool>,
    local_client_id: Option<String>,
    local_prompt: Option<String>,
    user_items: HashSet<String>,
    first_entry: usize,
}

#[derive(Default)]
struct ConversationRuntime {
    thread: Option<Thread>,
    turn: Option<ActiveTurn>,
    pending_input: VecDeque<PendingInput>,
    last_finished_turn: Option<String>,
}

struct Conversation {
    view: ConversationView,
    runtime: ConversationRuntime,
}

impl Conversation {
    fn new(
        id: i64,
        workspace: String,
        project: Option<&str>,
        thread: Option<Thread>,
        plan_mode: bool,
    ) -> Self {
        let mut view = ConversationView::new(id, workspace, project);
        if let Some(thread) = &thread {
            view.turn_options = thread.reported_options.clone();
            view.draft_key = DraftKey::Thread(thread.id.clone());
        }
        view.plan_mode = plan_mode;
        Self {
            view,
            runtime: ConversationRuntime {
                thread,
                ..ConversationRuntime::default()
            },
        }
    }
}

struct Thread {
    id: String,
    resumed: bool,
    history_loaded: bool,
    history_cursor: Option<String>,
    reported_options: TurnOptions,
}

#[derive(Clone, Copy)]
enum TextItem {
    Assistant,
    Proposal,
}

fn conversation_title(text: &str) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(72)
        .collect()
}

fn fork_error_with_worktree(message: String, worktree: Option<&str>) -> String {
    if let Some(path) = worktree {
        format!("{message}。工作树保留在 {path}")
    } else {
        message
    }
}

fn history_entry(item: &Value) -> Option<Entry> {
    match item["type"].as_str()? {
        "userMessage" => user_message(&item["content"]).map(Entry::User),
        "agentMessage" => Some(Entry::Assistant {
            text: item["text"].as_str()?.to_owned(),
            finalized: true,
        }),
        "plan" => Some(Entry::Proposal {
            text: item["text"].as_str()?.to_owned(),
            finalized: true,
        }),
        "contextCompaction" => Some(Entry::Compaction),
        "exitedReviewMode" => item["review"]
            .as_str()
            .filter(|review| !review.trim().is_empty())
            .map(|review| Entry::ReviewReport(review.into())),
        "commandExecution" => {
            let command = item["command"].as_str()?;
            let mut activity = CommandActivity::new(command);
            activity.complete(command, item);
            Some(Entry::Command(activity))
        }
        _ => None,
    }
}

fn history_fork_point(turn: &Value) -> Option<ForkPoint> {
    let id = turn["id"].as_str().filter(|id| !id.is_empty())?;
    if turn["status"] == "inProgress" {
        return None;
    }
    let items = turn["items"].as_array()?;
    let label = items
        .iter()
        .find_map(|item| {
            (item["type"] == "userMessage")
                .then(|| user_input_text(&item["content"]))
                .flatten()
        })
        .map_or_else(
            || {
                if items.iter().any(|item| item["type"] == "contextCompaction") {
                    "上下文压缩".into()
                } else {
                    "会话活动".into()
                }
            },
            |text| conversation_title(&text),
        );
    Some(ForkPoint {
        id: id.into(),
        label,
        response: None,
        prompt: None,
        entries: 0..0,
    })
}

fn first_prompt(entries: &[Arc<Entry>], start: usize) -> Option<usize> {
    entries
        .get(start..)?
        .iter()
        .position(|entry| matches!(entry.as_ref(), Entry::User(_)))
        .map(|index| start + index)
}

enum ForkBoundary {
    Through(String),
    BeforePrompt { turn: String, draft: String },
}

fn last_response(entries: &[Arc<Entry>], start: usize) -> Option<usize> {
    entries
        .get(start..)?
        .iter()
        .rposition(|entry| {
            matches!(entry.as_ref(), Entry::Assistant { text, finalized: true } if !text.trim().is_empty())
        })
        .map(|response| start + response)
}

pub(crate) struct Protocol {
    snapshot: Snapshot,
    runtime: ConversationRuntime,
    workspace_write: bool,
    fork_index: ThreadIndex,
    pin_index: ThreadIndex,
    temporary_pins: HashSet<String>,
    project_index: ProjectIndex,
    drafts: DraftStore,
    tabs: TabStore,
    startup_tabs: Option<SavedTabs>,
    keep_launch_workspace: bool,
    recent_project_conversations: HashMap<String, i64>,
    visits: Vec<i64>,
    visit: usize,
    inactive: HashMap<i64, Conversation>,
    history_cursor: Option<String>,
    discovered_models: Vec<ModelInfo>,
    plan_effort: String,
    next_question_token: i64,
    next_conversation: i64,
    next_id: u64,
    attempt: u64,
    pending: BTreeMap<u64, PendingRequest>,
    outbound: Vec<Value>,
    dirty: bool,
}

impl Protocol {
    pub(crate) fn new(
        workspace: String,
        workspace_write: bool,
        fork_index: Option<PathBuf>,
    ) -> Self {
        let visits = if workspace.is_empty() {
            Vec::new()
        } else {
            vec![0]
        };
        let mut session = Self {
            snapshot: Snapshot::connecting(workspace),
            runtime: ConversationRuntime::default(),
            workspace_write,
            fork_index: ThreadIndex::new(fork_index),
            pin_index: ThreadIndex::new(None),
            temporary_pins: HashSet::new(),
            project_index: ProjectIndex::default(),
            drafts: DraftStore::default(),
            tabs: TabStore::default(),
            startup_tabs: None,
            keep_launch_workspace: false,
            recent_project_conversations: HashMap::new(),
            visits,
            visit: 0,
            inactive: HashMap::new(),
            history_cursor: None,
            discovered_models: Vec::new(),
            plan_effort: String::new(),
            next_question_token: 1,
            next_conversation: 1,
            next_id: 1,
            attempt: 0,
            pending: BTreeMap::new(),
            outbound: Vec::new(),
            dirty: false,
        };
        session.request(
            Request::Initialize,
            "initialize",
            json!({
                "clientInfo":{"name":"xrack_agent_workbench","title":"XRack Agent Workbench","version":"0.1.0"},
                "capabilities":{"experimentalApi":true},
            }),
        );
        session
    }

    pub(crate) fn restore_workspace_state(
        &mut self,
        state_directory: Option<PathBuf>,
        keep_launch_workspace: bool,
    ) {
        self.pin_index = ThreadIndex::new(
            state_directory
                .as_ref()
                .map(|directory| directory.join("pins")),
        );
        self.drafts = DraftStore::new(state_directory.clone());
        self.tabs = TabStore::new(state_directory.clone());
        self.keep_launch_workspace = keep_launch_workspace;
        self.project_index = ProjectIndex::new(state_directory);
        if self.snapshot.view.project_workspace.is_empty() {
            match self.project_index.load() {
                Ok(projects) => self.snapshot.projects = Arc::new(projects),
                Err(error) => self.snapshot.project_status = format!("读取项目列表失败：{error}"),
            }
            self.dirty = true;
        } else {
            self.remember_project();
        }
        self.restore_local_drafts();
        match self.tabs.load() {
            Ok(saved) if !saved.open.is_empty() => self.startup_tabs = Some(saved),
            Ok(_) => {}
            Err(error) => self.snapshot.project_status = format!("读取会话标签失败（{error}）"),
        }
        self.snapshot.tabs_restoring = self.startup_tabs.is_some();
        self.snapshot.composer_ready = true;
        self.dirty = true;
    }

    fn restore_local_drafts(&mut self) {
        let Ok(records) = self.drafts.records() else {
            return;
        };
        let mut reused_startup = false;
        for record in records.into_iter().rev() {
            if !matches!(record.key, DraftKey::Local(_))
                || !Path::new(&record.workspace).is_dir()
                || !Path::new(&record.project).is_dir()
            {
                continue;
            }
            let mut title = conversation_title(&record.content.text);
            if title.is_empty() {
                title = "未发送的草稿".into();
            }
            if !reused_startup
                && record.workspace == self.snapshot.view.workspace
                && record.project == self.snapshot.view.project_workspace
            {
                reused_startup = true;
                self.snapshot.view.draft_key = record.key;
                if let Some(row) = self.snapshot.conversations.first_mut() {
                    row.title = title;
                }
                continue;
            }
            let id = self.next_conversation;
            self.next_conversation += 1;
            let mut conversation = Conversation::new(
                id,
                record.workspace.clone(),
                Some(&record.project),
                None,
                false,
            );
            conversation.view.draft_key = record.key;
            self.inactive.insert(id, conversation);
            self.recent_project_conversations
                .entry(record.project.clone())
                .or_insert(id);
            self.snapshot.conversations.push(ConversationSummary {
                id,
                workspace: record.workspace,
                project_workspace: Some(record.project),
                title,
                archived: false,
                pinned: false,
                activity: ConversationActivity::Idle,
            });
        }
    }

    fn save_open_tabs(&mut self, open: &[i64], active: Option<i64>) {
        if !self.snapshot.connected || self.snapshot.tabs_restoring {
            return;
        }
        let mut seen = HashSet::new();
        let mut saved = SavedTabs::default();
        for &id in open {
            let Some(summary) = self.snapshot.conversations.iter().find(|row| row.id == id) else {
                continue;
            };
            let view = if id == self.snapshot.view.conversation {
                Some(&self.snapshot.view)
            } else {
                self.inactive.get(&id).map(|state| &state.view)
            };
            if let Some(view) = view
                && !summary.workspace.is_empty()
                && seen.insert(id)
            {
                if active == Some(id) {
                    saved.active = Some(view.draft_key.clone());
                }
                saved.open.push(SavedTab {
                    key: view.draft_key.clone(),
                    workspace: summary.workspace.clone(),
                    project: summary.project().to_owned(),
                    archived: summary.archived,
                });
            }
        }
        if let Err(error) = self.tabs.save(&saved) {
            self.status(format!("保存会话标签失败（{error}）"));
        }
    }

    fn begin_tab_restore(&mut self) {
        let Some(saved) = &self.startup_tabs else {
            return;
        };
        for tab in saved.open.clone() {
            if self.restored_tab_id(&tab).is_some()
                || !Path::new(&tab.workspace).is_dir()
                || !Path::new(&tab.project).is_dir()
            {
                continue;
            }
            match &tab.key {
                DraftKey::Thread(thread) => {
                    let params = json!({"threadId":thread,"includeTurns":false});
                    self.request(Request::RestoreTab(tab), "thread/read", params);
                }
                DraftKey::Local(_) => {
                    let id = self.next_conversation;
                    self.next_conversation += 1;
                    let project = (tab.project != tab.workspace).then_some(tab.project);
                    let mut conversation = Conversation::new(
                        id,
                        tab.workspace.clone(),
                        project.as_deref(),
                        None,
                        false,
                    );
                    conversation.view.draft_key = tab.key;
                    self.inactive.insert(id, conversation);
                    self.snapshot.conversations.push(ConversationSummary {
                        id,
                        title: "新会话".into(),
                        workspace: tab.workspace,
                        project_workspace: project,
                        archived: false,
                        pinned: false,
                        activity: ConversationActivity::Idle,
                    });
                }
            }
        }
        self.finish_tab_restore();
    }

    fn restored_tab_id(&self, tab: &SavedTab) -> Option<i64> {
        let id = match &tab.key {
            DraftKey::Thread(thread) => self.conversation_for_thread(thread),
            DraftKey::Local(_) if self.snapshot.view.draft_key == tab.key => {
                Some(self.snapshot.view.conversation)
            }
            DraftKey::Local(_) => self.inactive.iter().find_map(|(&id, conversation)| {
                (conversation.view.draft_key == tab.key).then_some(id)
            }),
        }?;
        self.snapshot
            .conversations
            .iter()
            .find(|row| {
                row.id == id && row.workspace == tab.workspace && row.project() == tab.project
            })
            .map(|row| row.id)
    }

    fn restore_native_tab(&mut self, tab: SavedTab, result: &Value) {
        let thread = &result["thread"];
        if let DraftKey::Thread(id) = &tab.key
            && thread["id"] == id.as_str()
            && (thread["cwd"] == tab.workspace.as_str()
                || self
                    .fork_index
                    .handoff(id)
                    .is_some_and(|location| location.workspace == tab.workspace))
        {
            self.merge_session_rows(
                std::slice::from_ref(thread),
                SessionListScope {
                    workspaces: &BTreeMap::from([(tab.workspace, tab.project)]),
                    archived: thread["archived"].as_bool().unwrap_or(tab.archived),
                    searching: false,
                    refresh_existing: false,
                    linked_read: true,
                    restore_tab: true,
                },
            );
        }
        self.finish_tab_restore();
    }

    fn finish_tab_restore(&mut self) {
        if self
            .pending
            .values()
            .any(|request| matches!(request.kind, Request::RestoreTab(_)))
        {
            return;
        }
        let Some(saved) = self.startup_tabs.take() else {
            return;
        };
        let restored: Vec<_> = saved
            .open
            .iter()
            .filter_map(|tab| self.restored_tab_id(tab))
            .collect();
        let active = saved
            .active
            .as_ref()
            .and_then(|key| saved.open.iter().find(|tab| &tab.key == key))
            .filter(|tab| {
                !self.keep_launch_workspace || tab.workspace == self.snapshot.view.workspace
            })
            .and_then(|tab| self.restored_tab_id(tab));
        self.snapshot.restored_tabs = Some(Arc::new(restored));
        self.snapshot.tabs_restoring = false;
        // A user who already visited or sent a message owns the current view.
        if let Some(id) = active
            && self.visits.len() <= 1
            && self.snapshot.view.conversation == 0
            && self.snapshot.view.entries.is_empty()
            && !self.snapshot.view.busy
            && self.can_switch_conversation()
        {
            self.select_conversation(id, None);
        }
        self.dirty = true;
    }

    fn remember_project(&mut self) {
        self.snapshot.project_status = self
            .project_index
            .remember(
                &self.snapshot.view.project_workspace,
                Arc::make_mut(&mut self.snapshot.projects),
            )
            .err()
            .map_or_else(String::new, |error| format!("保存项目列表失败：{error}"));
        self.dirty = true;
    }

    fn request(&mut self, request: Request, method: &str, params: Value) {
        let id = self.next_id;
        self.next_id += 1;
        self.pending.insert(
            id,
            PendingRequest {
                sent: Instant::now(),
                owner: (!request.is_shared()).then_some(self.snapshot.view.conversation),
                kind: request,
            },
        );
        self.outbound
            .push(json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}));
    }

    pub(crate) fn drain_outbound(&mut self) -> impl Iterator<Item = Value> + '_ {
        self.outbound.drain(..)
    }

    pub(crate) fn expired(&mut self) -> bool {
        let expired: Vec<_> = self
            .pending
            .iter()
            .filter_map(|(&id, request)| (request.sent.elapsed() >= RESPONSE_TIMEOUT).then_some(id))
            .collect();
        let mut disconnected = false;
        for id in expired {
            let Some(request) = self.pending.remove(&id) else {
                continue;
            };
            if let Some(owner) = request.owner {
                self.with_conversation(owner, |session| session.request_expired(request.kind));
            } else if matches!(request.kind, Request::Initialize) {
                disconnected = true;
            } else {
                self.request_expired(request.kind);
            }
        }
        disconnected
    }

    fn request_expired(&mut self, request: Request) {
        match request {
            Request::RestoreTab(_) => self.finish_tab_restore(),
            Request::Handoff { .. } => {
                self.handoff_failed("工作目录更新超时，可在核对后重试".into())
            }
            Request::SearchProjects => self.project_search_failed("搜索超时，可重试".into()),
            Request::ProjectSessions { project, .. } => {
                self.project_history_failed(&project, "读取超时，可重试".into());
            }
            Request::Sessions { .. } | Request::IndexedThread { .. } => {
                self.settle_session_list();
                self.snapshot.history_status = "读取会话列表超时，可重试".into();
            }
            Request::History { .. } => {
                self.snapshot.view.restoring = false;
                self.status("读取会话历史超时，可重新选择会话");
            }
            Request::Rename { conversation, .. } => {
                self.rename_failed(conversation, "重命名请求超时，可重试".into())
            }
            Request::SetArchived { conversation, .. } => {
                self.archive_failed(conversation, "归档操作超时，可重试".into());
            }
            Request::Models { .. } => self.models_failed("读取模型列表超时，可重试".into()),
            Request::Skills => self.set_skill_state(SkillCatalogState::Failed(String::new())),
            Request::Followup { .. } => self.followup_failed("发送状态未确认，草稿已保留".into()),
            Request::QueueAction { action, .. } => {
                self.queue_action_failed(&action, "队列操作超时，可刷新后重试".into());
            }
            Request::QueueList { .. } => {
                self.snapshot.view.queue_loading = false;
                self.dirty = true;
            }
            Request::Compact { .. } if self.snapshot.view.compacting => {
                self.finish("等待压缩响应超时", ActivityState::Unfinished);
            }
            Request::Review { .. } if self.snapshot.view.reviewing => {
                self.finish("等待审阅响应超时", ActivityState::Unfinished);
            }
            Request::Fork { worktree, .. } => self.fork_failed(fork_error_with_worktree(
                "分支创建响应超时".into(),
                worktree.as_deref(),
            )),
            Request::ForkTitle { .. } => self.fork_failed("分支已创建，名称保存超时".into()),
            Request::Thread { attempt, .. }
            | Request::Resume { attempt, .. }
            | Request::Turn(attempt)
                if self.is_current(attempt)
                    && self
                        .runtime
                        .turn
                        .as_ref()
                        .is_some_and(|turn| turn.id.is_none()) =>
            {
                self.finish("等待轮次响应超时", ActivityState::Unfinished);
            }
            Request::Interrupt(attempt) if self.is_current(attempt) => {
                self.snapshot.view.stopping = false;
                if let Some(turn) = &mut self.runtime.turn {
                    turn.interrupt_sent = false;
                }
                self.status("停止响应超时，可重试");
            }
            _ => {}
        }
    }

    pub(crate) fn take_snapshot(&mut self) -> Option<Snapshot> {
        if !std::mem::take(&mut self.dirty) {
            return None;
        }
        self.snapshot.navigation_back = self.navigation_target(false).is_some();
        self.snapshot.navigation_forward = self.navigation_target(true).is_some();
        for row in &mut self.snapshot.conversations {
            row.activity = if row.id == self.snapshot.view.conversation {
                self.snapshot.view.activity()
            } else {
                self.inactive
                    .get(&row.id)
                    .map_or(ConversationActivity::Idle, |state| state.view.activity())
            };
        }
        let tools = self
            .snapshot
            .conversations
            .iter()
            .filter_map(|row| self.tools_for(row.id).map(|tools| (row.id, tools)))
            .collect();
        if *self.snapshot.conversation_tools != tools {
            self.snapshot.conversation_tools = Arc::new(tools);
        }
        self.snapshot.view.renamable = self.runtime.thread.is_some();
        self.snapshot.view.queue_resumed = self
            .runtime
            .thread
            .as_ref()
            .is_some_and(|thread| thread.resumed);
        self.snapshot.view.active_turn_id = self
            .runtime
            .turn
            .as_ref()
            .filter(|turn| turn.sent)
            .and_then(|turn| turn.id.clone())
            .unwrap_or_default();
        Some(self.snapshot.clone())
    }

    pub(crate) fn acknowledge(&mut self, sequence: u64) {
        self.snapshot.acknowledged = sequence;
        self.dirty = true;
    }

    fn status(&mut self, value: impl Into<Status>) {
        self.snapshot.view.status = value.into();
        self.dirty = true;
    }

    fn idle_status(&mut self) {
        self.status(if !self.snapshot.connected {
            Status::Disconnected
        } else if self.snapshot.view.workspace.is_empty() {
            Status::ChooseWorkspace
        } else if self.snapshot.current_archived() {
            Status::Archived
        } else if self.workspace_write {
            Status::ReadyWrite
        } else {
            Status::ReadyRead
        });
    }

    fn finish(&mut self, status: impl Into<Status>, incomplete: ActivityState) {
        self.snapshot.view.prompt_pending = false;
        self.runtime.pending_input.clear();
        self.snapshot.view.question = None;
        self.snapshot.view.compacting = false;
        self.snapshot.view.reviewing = false;
        if let Some(turn) = self.runtime.turn.take() {
            if let Some(id) = &turn.id {
                self.runtime.last_finished_turn = Some(id.clone());
            }
            for index in turn.items.values() {
                let entry = &mut self.snapshot.view.entries[*index];
                if matches!(entry.as_ref(), Entry::Command(command) if command.state == ActivityState::Running)
                    && let Entry::Command(command) = Arc::make_mut(entry)
                {
                    command.state = incomplete;
                }
            }
            if let Some(index) = turn.plan_index
                && let Entry::Plan(plan) = Arc::make_mut(&mut self.snapshot.view.entries[index])
            {
                for step in &mut plan.steps {
                    if step.state == ActivityState::Running {
                        step.state = incomplete;
                    }
                }
            }
            self.pending.retain(|_, pending| match &pending.kind {
                Request::Initialize => true,
                Request::Thread { attempt, .. }
                | Request::Resume { attempt, .. }
                | Request::Turn(attempt)
                | Request::Interrupt(attempt) => *attempt != turn.attempt,
                Request::Sessions { .. }
                | Request::ProjectSessions { .. }
                | Request::SearchProjects
                | Request::History { .. }
                | Request::Rename { .. }
                | Request::SetArchived { .. }
                | Request::Models { .. }
                | Request::Skills
                | Request::Modes
                | Request::Followup { .. }
                | Request::QueueList { .. }
                | Request::QueueAction { .. } => true,
                Request::Compact { .. } => true,
                Request::Review { .. } => true,
                Request::Fork { .. } | Request::ForkTitle { .. } => true,
                Request::IndexedThread { .. } => true,
                Request::RestoreTab(_) => true,
                Request::Handoff { .. } => true,
            });
        }
        self.snapshot.view.busy = false;
        self.snapshot.view.stopping = false;
        self.status(status);
    }

    pub(crate) fn disconnected(&mut self, reason: String) {
        self.snapshot.connected = false;
        self.invalidate_skills();
        self.snapshot.tabs_restoring = false;
        if self.snapshot.project_search.loading {
            self.project_search_failed("Codex 未连接".into());
        }
        for history in Arc::make_mut(&mut self.snapshot.project_histories).values_mut() {
            if history.loading {
                history.loading = false;
                history.status = "Codex 未连接".into();
            }
        }
        self.snapshot.view.restoring = false;
        self.snapshot.history_loading = false;
        self.snapshot.view.archive_pending = false;
        self.snapshot.models_loading = false;
        if self.snapshot.fork_pending {
            self.fork_failed("连接已关闭，分支操作未确认".into());
        }
        let parked: Vec<_> = self.inactive.keys().copied().collect();
        for id in parked {
            self.with_conversation(id, |session| {
                session.finish_disconnected_conversation(&reason)
            });
        }
        self.finish_disconnected_conversation(&reason);
    }

    fn finish_disconnected_conversation(&mut self, reason: &str) {
        self.snapshot.view.restoring = false;
        self.snapshot.view.archive_pending = false;
        self.snapshot.view.queue_loading = false;
        self.snapshot.view.queue_pending = false;
        if self.snapshot.view.handoff_pending {
            self.handoff_failed("连接已关闭，工作目录更新未确认".into());
        }
        if self.snapshot.view.followup_pending {
            self.followup_failed("连接已关闭，草稿已保留".into());
        }
        if self.snapshot.view.rename_pending {
            self.rename_failed(self.snapshot.view.conversation, "Codex 连接已关闭".into());
        }
        self.finish(reason, ActivityState::Unfinished);
    }

    pub(crate) fn input(&mut self, input: Input) {
        match input {
            Input::Workspace(path) if self.can_switch_conversation() => self.activate_project(path),
            Input::Reset
                if self.can_switch_conversation() && !self.snapshot.view.workspace.is_empty() =>
            {
                let reset_list =
                    self.snapshot.archive_view || !self.snapshot.history_query.is_empty();
                let workspace = self.snapshot.view.workspace.clone();
                let project = (workspace != self.snapshot.view.project_workspace)
                    .then(|| self.snapshot.view.project_workspace.clone());
                self.new_conversation(workspace, project);
                if reset_list {
                    self.snapshot.archive_view = false;
                    self.clear_search();
                    self.list_sessions(false);
                }
            }
            Input::Select(id) if self.can_switch_conversation() => {
                self.select_conversation(id, None)
            }
            Input::Navigate { forward } if self.can_switch_conversation() => {
                if let Some((visit, id)) = self.navigation_target(forward) {
                    self.select_conversation(id, Some(visit));
                }
            }
            Input::MarkActivityRead => {
                let clear = |view: &mut ConversationView| std::mem::take(&mut view.unread);
                self.dirty |= clear(&mut self.snapshot.view);
                for conversation in self.inactive.values_mut() {
                    self.dirty |= clear(&mut conversation.view);
                }
            }
            Input::History { more } if !self.snapshot.history_loading => self.list_sessions(more),
            Input::ProjectHistory { path, more } => self.list_project_history(path, more),
            Input::ProjectSearch {
                query,
                archived,
                more,
            } => self.search_projects(query, archived, more),
            Input::ArchiveView(archived) if self.snapshot.archive_view != archived => {
                self.snapshot.archive_view = archived;
                self.list_sessions(false);
            }
            Input::SetArchived(archived) => {
                self.set_archived(self.snapshot.view.conversation, archived)
            }
            Input::SetConversationArchived {
                conversation,
                archived,
            } => self.set_archived(conversation, archived),
            Input::SetPinned {
                conversation,
                pinned,
            } => self.set_pinned(conversation, pinned),
            Input::LoadModels if self.snapshot.connected && !self.snapshot.models_loading => {
                self.request_models(None);
            }
            Input::LoadSkills(workspace)
                if self.snapshot.connected
                    && !workspace.is_empty()
                    && workspace == self.snapshot.view.workspace =>
            {
                self.list_skills(workspace);
            }
            Input::ChooseModel(model)
                if !self.snapshot.view.busy && !self.snapshot.view.restoring =>
            {
                if model != self.snapshot.view.turn_options.model
                    && (model.is_empty()
                        || self.snapshot.models.iter().any(|entry| entry.id == model))
                {
                    self.snapshot.view.turn_options = TurnOptions {
                        model,
                        effort: String::new(),
                    }
                    .supported(&self.snapshot.models);
                    self.dirty = true;
                }
            }
            Input::ChooseEffort(effort)
                if !self.snapshot.view.busy && !self.snapshot.view.restoring =>
            {
                if let Some(model) = self
                    .snapshot
                    .models
                    .iter()
                    .find(|model| model.id == self.snapshot.view.turn_options.model)
                    && (model.efforts.contains(&effort)
                        || effort.is_empty() && model.default_effort.is_empty())
                {
                    self.snapshot.view.turn_options.effort = effort;
                    self.dirty = true;
                }
            }
            Input::ChoosePlanMode(plan)
                if self.snapshot.plan_available
                    && !self.snapshot.view.busy
                    && !self.snapshot.view.restoring =>
            {
                self.snapshot.view.plan_mode = plan;
                self.dirty = true;
            }
            Input::AnswerQuestion {
                token,
                index,
                value,
            } if !self.snapshot.view.stopping => {
                self.answer_question(token, index, value);
            }
            Input::Search(query) if query != self.snapshot.history_query => {
                self.snapshot.history_query = query;
                self.snapshot.history_matches.clear();
                self.list_sessions(false);
            }
            Input::Rename(name)
                if !self.snapshot.view.busy
                    && !self.snapshot.view.restoring
                    && !self.snapshot.view.rename_pending
                    && !self.snapshot.fork_pending
                    && !self.snapshot.view.archive_pending =>
            {
                self.rename(self.snapshot.view.conversation, name);
            }
            Input::RenameConversation { conversation, name } => self.rename(conversation, name),
            Input::EarlierTurns if !self.snapshot.view.busy && !self.snapshot.view.restoring => {
                self.load_history(true);
            }
            Input::SaveOpenTabs { open, active } => self.save_open_tabs(&open, active),
            Input::Compact => self.compact(),
            Input::Review(target) => self.review(target),
            Input::Fork {
                title,
                last_turn,
                worktree,
            } => self.fork(title, ForkBoundary::Through(last_turn), worktree),
            Input::ForkBeforePrompt {
                conversation,
                turn,
                draft,
            } if conversation == self.snapshot.view.conversation => self.fork(
                String::new(),
                ForkBoundary::BeforePrompt { turn, draft },
                None,
            ),
            Input::Followup {
                conversation,
                turn,
                prompt,
            } => self.followup(conversation, turn, prompt),
            Input::RemoveQueued(id) => self.queue_action(QueueAction::Remove, &id),
            Input::StartQueued(id) => self.queue_action(QueueAction::Start, &id),
            Input::UpdateQueued {
                conversation,
                original,
                text,
            } if conversation == self.snapshot.view.conversation => {
                self.update_queued(original, text);
            }
            Input::MoveQueued {
                conversation,
                id,
                step,
            } if conversation == self.snapshot.view.conversation => {
                self.move_queued(&id, step);
            }
            Input::ResumeQueue => self.queue_action(QueueAction::Resume, ""),
            Input::Handoff {
                conversation,
                original,
                workspace,
                worktree,
            } if conversation == self.snapshot.view.conversation
                && original == self.snapshot.view.workspace =>
            {
                self.handoff(workspace, worktree);
            }
            Input::SendInWorktree {
                conversation,
                original,
                workspace,
                prompt,
            } => {
                self.send_in_worktree(conversation, &original, workspace, prompt);
            }
            Input::Send(prompt)
                if !self.snapshot.view.busy
                    && !self.snapshot.view.handoff_pending
                    && !self.snapshot.view.restoring
                    && !self.snapshot.view.archive_pending
                    && !self.snapshot.view.queue_pending
                    && !self.snapshot.view.followup_pending
                    && !self.snapshot.fork_pending
                    && !self.snapshot.current_archived()
                    && self.snapshot.connected =>
            {
                if self
                    .runtime
                    .thread
                    .as_ref()
                    .is_some_and(|thread| !thread.resumed)
                    && (self.snapshot.view.queue_loading || !self.snapshot.view.queued.is_empty())
                {
                    self.status("请先接回待发队列");
                    return;
                }
                if self.snapshot.view.workspace.is_empty() || prompt.is_empty() {
                    return;
                }
                let model = if self.snapshot.view.turn_options.model.is_empty() {
                    self.runtime
                        .thread
                        .as_ref()
                        .map(|thread| thread.reported_options.model.as_str())
                        .unwrap_or_default()
                } else {
                    self.snapshot.view.turn_options.model.as_str()
                };
                if !self.model_accepts_prompt(model, &prompt) {
                    self.status("当前模型接受文本，请选择支持图片的模型");
                    return;
                }
                self.snapshot.fork_error.clear();
                let message = UserMessage::from_prompt(&prompt);
                let display = message.display.clone();
                if self.runtime.thread.is_none()
                    && self.snapshot.view.entries.is_empty()
                    && let Some(summary) = self
                        .snapshot
                        .conversations
                        .iter_mut()
                        .find(|row| row.id == self.snapshot.view.conversation)
                {
                    let title = if prompt.text.trim().is_empty() {
                        prompt
                            .attachments
                            .first()
                            .map(|attachment| attachment.name())
                            .unwrap_or_default()
                    } else {
                        &prompt.text
                    };
                    summary.title = conversation_title(title);
                }
                let first_entry = self.snapshot.view.entries.len();
                self.snapshot
                    .view
                    .entries
                    .push(Arc::new(Entry::User(message)));
                self.snapshot.view.prompt_pending = true;
                self.snapshot.view.busy = true;
                self.attempt += 1;
                let options = self
                    .snapshot
                    .view
                    .turn_options
                    .supported(&self.snapshot.models);
                let model = (!options.model.is_empty()).then(|| options.model.clone());
                self.runtime.turn = Some(ActiveTurn {
                    attempt: self.attempt,
                    id: None,
                    sent: false,
                    interrupt_sent: false,
                    items: HashMap::new(),
                    plan_index: None,
                    diff_index: None,
                    options,
                    mode: self
                        .snapshot
                        .plan_available
                        .then_some(self.snapshot.view.plan_mode),
                    local_client_id: Some(self.client_message_id()),
                    local_prompt: Some(display),
                    user_items: HashSet::new(),
                    first_entry,
                });
                self.status(Status::AwaitingReply);
                match &self.runtime.thread {
                    Some(thread) if thread.resumed => self.start_turn(prompt),
                    Some(thread) => self.request(
                        Request::Resume {
                            attempt: self.attempt,
                            prompt,
                        },
                        "thread/resume",
                        json!({
                            "threadId": thread.id,
                            "excludeTurns": true,
                            "cwd": self.snapshot.view.workspace,
                            "sandbox": self.sandbox(),
                            "approvalPolicy": "never",
                            "model": model,
                        }),
                    ),
                    None => self.request(
                        Request::Thread {
                            attempt: self.attempt,
                            prompt,
                        },
                        "thread/start",
                        json!({
                            "cwd": self.snapshot.view.workspace,
                            "sandbox": self.sandbox(),
                            "approvalPolicy": "never",
                            "ephemeral": false,
                            "model": model,
                        }),
                    ),
                }
            }
            Input::Interrupt(conversation)
                if conversation == self.snapshot.view.conversation && self.snapshot.view.busy =>
            {
                if self.runtime.turn.as_ref().is_some_and(|turn| !turn.sent) {
                    self.finish(Status::Stopped, ActivityState::Interrupted);
                } else {
                    self.snapshot.view.stopping = true;
                    self.status(Status::Stopping);
                    self.interrupt_if_ready();
                }
            }
            _ => {}
        }
    }

    fn client_message_id(&self) -> String {
        let clock = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        format!("xrack-{}-{clock}-{}", std::process::id(), self.next_id)
    }

    fn send_in_worktree(
        &mut self,
        conversation: i64,
        original: &str,
        workspace: String,
        prompt: Prompt,
    ) {
        if conversation != self.snapshot.view.conversation
            || original != self.snapshot.view.workspace
        {
            return;
        }
        if !self.snapshot.connected
            || self.snapshot.view.busy
            || !self.can_switch_conversation()
            || self.snapshot.current_archived()
            || self.runtime.thread.is_some()
            || !self.snapshot.view.entries.is_empty()
            || prompt.is_empty()
            || !Path::new(&workspace).is_absolute()
            || workspace == original
        {
            self.status(format!(
                "工作树已在 {workspace} 创建，当前会话未能切换到该目录"
            ));
            return;
        }
        let key = &self.snapshot.view.draft_key;
        if let Err(error) = self
            .drafts
            .relocate(key, &workspace)
            .and_then(|()| self.tabs.relocate(key, &workspace))
        {
            self.snapshot.project_status = format!("工作树草稿位置保存失败（{error}）");
        }
        self.snapshot.view.workspace = workspace.clone();
        let project = self.snapshot.project_workspace().to_owned();
        if let Some(row) = self
            .snapshot
            .conversations
            .iter_mut()
            .find(|row| row.id == conversation)
        {
            row.workspace = workspace;
            row.project_workspace = Some(project);
        }
        self.dirty = true;
        self.input(Input::Send(prompt));
    }

    fn handoff(&mut self, workspace: String, worktree: String) {
        let Some(thread) = &self.runtime.thread else {
            return;
        };
        if !self.snapshot.connected
            || self.snapshot.view.busy
            || !self.can_switch_conversation()
            || self.snapshot.current_archived()
            || self.snapshot.view.queue_loading
            || !self.snapshot.view.queued.is_empty()
            || workspace == self.snapshot.view.workspace
            || !Path::new(&workspace).is_absolute()
            || !Path::new(&worktree).is_absolute()
            || !Path::new(&workspace).is_dir()
        {
            self.snapshot.view.handoff_error = "请等会话和待发队列空闲后再交接".into();
            self.dirty = true;
            return;
        }
        let resume = !thread.resumed;
        let id = thread.id.clone();
        let (method, params) = if resume {
            (
                "thread/resume",
                json!({
                    "threadId": id, "cwd": workspace, "runtimeWorkspaceRoots": [&workspace],
                    "excludeTurns": true, "sandbox": self.sandbox(), "approvalPolicy": "never",
                }),
            )
        } else {
            (
                "thread/settings/update",
                json!({"threadId": id, "cwd": workspace}),
            )
        };
        self.snapshot.view.handoff_pending = true;
        self.snapshot.view.handoff_error.clear();
        self.dirty = true;
        self.request(
            Request::Handoff {
                thread: id,
                workspace,
                worktree,
                resume,
            },
            method,
            params,
        );
    }

    fn handoff_failed(&mut self, message: String) {
        self.snapshot.view.handoff_pending = false;
        self.snapshot.view.handoff_error = message;
        self.dirty = true;
    }

    fn handoff_confirmed(&mut self, thread_id: &str, workspace: String, worktree: String) {
        let Some(thread) = self
            .runtime
            .thread
            .as_mut()
            .filter(|thread| thread.id == thread_id)
        else {
            self.handoff_failed("会话已变化，无法确认交接".into());
            return;
        };
        thread.resumed = true;
        let project = self.snapshot.project_workspace().to_owned();
        let key = &self.snapshot.view.draft_key;
        let persisted = self
            .fork_index
            .remember_handoff(thread_id, &workspace, &project, &worktree)
            .and_then(|()| self.drafts.relocate(key, &workspace))
            .and_then(|()| self.tabs.relocate(key, &workspace));
        if let Err(error) = persisted {
            self.snapshot.project_status = format!("工作目录已更新，交接记录保存失败（{error}）");
        }
        self.snapshot.view.workspace = workspace.clone();
        self.snapshot.view.associated_worktree = Some(worktree);
        self.snapshot.view.handoff_pending = false;
        self.snapshot.view.handoff_error.clear();
        if let Some(row) = self
            .snapshot
            .conversations
            .iter_mut()
            .find(|row| row.id == self.snapshot.view.conversation)
        {
            row.project_workspace = (project != workspace).then_some(project);
            row.workspace = workspace;
        }
        self.idle_status();
    }

    fn can_switch_conversation(&self) -> bool {
        !self.snapshot.view.restoring
            && !self.snapshot.view.handoff_pending
            && !self.snapshot.view.queue_pending
            && !self.snapshot.view.followup_pending
            && !self.snapshot.fork_pending
            && !self.snapshot.view.archive_pending
            && !self.snapshot.view.rename_pending
    }

    fn fork(&mut self, title: String, boundary: ForkBoundary, worktree: Option<String>) {
        let Some(thread) = &self.runtime.thread else {
            return;
        };
        if self.snapshot.fork_pending {
            return;
        }
        if self.snapshot.current_archived() {
            self.fork_failed(fork_error_with_worktree(
                "请先恢复归档会话，再创建分支".into(),
                worktree.as_deref(),
            ));
            return;
        }
        if worktree
            .as_deref()
            .is_some_and(|path| !Path::new(path).is_absolute())
        {
            self.fork_failed("工作树需要绝对路径".into());
            return;
        }
        if !self.snapshot.connected
            || self.snapshot.view.busy
            || !self.can_switch_conversation()
            || self.snapshot.view.rename_pending
            || self.snapshot.view.archive_pending
        {
            self.fork_failed(fork_error_with_worktree(
                "请等当前操作完成后再创建分支".into(),
                worktree.as_deref(),
            ));
            return;
        }
        let points = &self.snapshot.view.fork_points;
        let (last_turn, before_turn, initial_draft) = match boundary {
            ForkBoundary::Through(turn)
                if turn.is_empty() || points.iter().any(|p| p.id == turn) =>
            {
                (Some(turn).filter(|turn| !turn.is_empty()), None, None)
            }
            ForkBoundary::BeforePrompt { turn, draft }
                if !draft.trim().is_empty()
                    && points
                        .iter()
                        .any(|point| point.id == turn && point.prompt.is_some()) =>
            {
                (None, Some(turn), Some(draft))
            }
            _ => {
                self.fork_failed(fork_error_with_worktree(
                    "所选轮次已不可用，请重新选择".into(),
                    worktree.as_deref(),
                ));
                return;
            }
        };
        let name = conversation_title(&title);
        let name = if name.is_empty() {
            let current = self
                .snapshot
                .conversations
                .iter()
                .find(|row| row.id == self.snapshot.view.conversation)
                .map_or("会话", |row| row.title.as_str());
            format!("{} · 分支", current.chars().take(65).collect::<String>())
        } else {
            name
        };
        let mut params = json!({
            "threadId":thread.id,
            "lastTurnId":last_turn,
            "cwd":worktree.as_deref().unwrap_or(&self.snapshot.view.workspace),
            "sandbox":self.sandbox(),
            "approvalPolicy":"never",
            "ephemeral":false,
            "excludeTurns":true,
            "deferGoalContinuation":true,
        });
        if let Some(turn) = before_turn {
            params["beforeTurnId"] = turn.into();
        }
        self.snapshot.fork_pending = true;
        self.snapshot.fork_error.clear();
        self.dirty = true;
        self.request(
            Request::Fork {
                source: self.snapshot.view.conversation,
                title: name,
                worktree,
                initial_draft,
            },
            "thread/fork",
            params,
        );
    }

    fn fork_failed(&mut self, message: String) {
        self.snapshot.fork_pending = false;
        self.snapshot.fork_error = message;
        self.dirty = true;
    }

    fn fork_created(
        &mut self,
        result: &Value,
        title: String,
        worktree: Option<String>,
        initial_draft: Option<String>,
    ) {
        let Some(thread_id) = result["thread"]["id"].as_str().filter(|id| !id.is_empty()) else {
            self.fork_failed(fork_error_with_worktree(
                "Codex 未返回分支会话 ID".into(),
                worktree.as_deref(),
            ));
            return;
        };
        if self
            .runtime
            .thread
            .as_ref()
            .is_some_and(|thread| thread.id == thread_id)
            || self.inactive.values().any(|conversation| {
                conversation
                    .runtime
                    .thread
                    .as_ref()
                    .is_some_and(|thread| thread.id == thread_id)
            })
        {
            self.fork_failed(fork_error_with_worktree(
                "Codex 返回了已存在的会话 ID".into(),
                worktree.as_deref(),
            ));
            return;
        }
        let reported = result["thread"]["cwd"]
            .as_str()
            .or_else(|| result["cwd"].as_str());
        if let Some(expected) = worktree.as_deref()
            && reported != Some(expected)
        {
            self.fork_failed(fork_error_with_worktree(
                "Codex 返回的分支目录与工作树不一致".into(),
                Some(expected),
            ));
            return;
        }

        let id = self.next_conversation;
        self.next_conversation += 1;
        let options = TurnOptions::from_thread(result);
        let native_title = result["thread"]["name"]
            .as_str()
            .or_else(|| result["thread"]["preview"].as_str())
            .filter(|title| !title.trim().is_empty())
            .map_or_else(|| "会话分支".into(), conversation_title);
        let workspace = worktree.unwrap_or_else(|| self.snapshot.view.workspace.clone());
        let project = (workspace != self.snapshot.project_workspace())
            .then(|| self.snapshot.project_workspace().to_owned());
        let mut conversation = Conversation::new(
            id,
            workspace.clone(),
            project.as_deref(),
            Some(Thread {
                id: thread_id.into(),
                resumed: true,
                history_loaded: false,
                history_cursor: None,
                reported_options: options,
            }),
            self.snapshot.view.plan_mode,
        );
        conversation.view.initial_draft = initial_draft.map(Arc::from);
        self.inactive.insert(id, conversation);
        let same_workspace = workspace == self.snapshot.view.workspace;
        self.snapshot.conversations.insert(
            0,
            ConversationSummary {
                id,
                workspace: workspace.clone(),
                project_workspace: project.clone(),
                title: native_title,
                archived: false,
                pinned: false,
                activity: ConversationActivity::Idle,
            },
        );
        self.snapshot.forks_created = self.snapshot.forks_created.wrapping_add(1);
        if let Err(error) =
            self.fork_index
                .remember(thread_id, &workspace, false, project.as_deref())
        {
            self.snapshot.fork_error = format!("分支已创建，侧栏索引保存失败（{error}）");
        }
        self.snapshot.archive_view = false;
        self.clear_search();
        if !self.snapshot.view.busy {
            self.select_conversation(id, None);
        }
        if same_workspace || self.snapshot.view.busy {
            self.list_sessions(false);
        }
        let params = json!({"threadId":thread_id,"name":title});
        self.request(
            Request::ForkTitle {
                conversation: id,
                title,
            },
            "thread/name/set",
            params,
        );
    }

    fn record_fork_point(&mut self, id: &str) {
        let Some(turn) = &self.runtime.turn else {
            return;
        };
        let items = self
            .snapshot
            .view
            .entries
            .get(turn.first_entry..)
            .unwrap_or_default();
        let label = items
            .iter()
            .find_map(|entry| match entry.as_ref() {
                Entry::User(message) => Some(conversation_title(&message.display)),
                _ => None,
            })
            .unwrap_or_else(|| {
                if items
                    .iter()
                    .any(|entry| matches!(entry.as_ref(), Entry::Compaction))
                {
                    "上下文压缩".into()
                } else {
                    "会话活动".into()
                }
            });
        let points = Arc::make_mut(&mut self.snapshot.view.fork_points);
        if !points.iter().any(|point| point.id == id) {
            points.push(ForkPoint {
                id: id.into(),
                label,
                response: last_response(&self.snapshot.view.entries, turn.first_entry),
                prompt: first_prompt(&self.snapshot.view.entries, turn.first_entry),
                entries: turn.first_entry..self.snapshot.view.entries.len(),
            });
        }
    }

    fn can_start_native_task(&self) -> bool {
        self.snapshot.connected
            && !self.snapshot.view.busy
            && self.can_switch_conversation()
            && !self.snapshot.view.workspace.is_empty()
            && !self.snapshot.current_archived()
            && !self.snapshot.view.rename_pending
            && !self.snapshot.view.archive_pending
            && !self.snapshot.view.queue_loading
            && self.snapshot.view.queued.is_empty()
    }

    fn review(&mut self, target: ReviewTarget) {
        if !self.can_start_native_task() {
            return;
        }
        self.snapshot.view.reviewing = true;
        self.snapshot.view.busy = true;
        self.status(Status::Reviewing);
        let (method, params) = match &self.runtime.thread {
            Some(thread) if thread.resumed => {
                self.send_review(target);
                return;
            }
            Some(thread) => (
                "thread/resume",
                json!({
                    "threadId":thread.id,"excludeTurns":true,"cwd":self.snapshot.view.workspace,
                    "sandbox":self.sandbox(),"approvalPolicy":"never",
                }),
            ),
            None => (
                "thread/start",
                json!({
                    "ephemeral":false,"cwd":self.snapshot.view.workspace,
                    "sandbox":self.sandbox(),"approvalPolicy":"never",
                }),
            ),
        };
        self.request(
            Request::Review {
                conversation: self.snapshot.view.conversation,
                prepare: Some(target),
            },
            method,
            params,
        );
    }

    fn send_review(&mut self, target: ReviewTarget) {
        let Some(thread) = &self.runtime.thread else {
            return;
        };
        let params = json!({"threadId":thread.id,"delivery":"inline","target":target.params()});
        self.request(
            Request::Review {
                conversation: self.snapshot.view.conversation,
                prepare: None,
            },
            "review/start",
            params,
        );
    }

    fn compact(&mut self) {
        let Some(thread) = &self.runtime.thread else {
            return;
        };
        if !self.can_start_native_task() {
            return;
        }
        let resume = !thread.resumed;
        let (method, params) = if resume {
            (
                "thread/resume",
                json!({"threadId":thread.id,"excludeTurns":true,"cwd":self.snapshot.view.workspace,
                       "sandbox":self.sandbox(),"approvalPolicy":"never"}),
            )
        } else {
            ("thread/compact/start", json!({"threadId":thread.id}))
        };
        self.snapshot.view.compacting = true;
        self.snapshot.view.busy = true;
        self.status(Status::Compacting);
        self.request(
            Request::Compact {
                conversation: self.snapshot.view.conversation,
                resume,
            },
            method,
            params,
        );
    }

    fn model_accepts_prompt(&self, model: &str, prompt: &Prompt) -> bool {
        !prompt.has_images()
            || !self
                .snapshot
                .models
                .iter()
                .any(|choice| choice.id == model && choice.allows_images == Some(false))
    }

    fn followup(&mut self, conversation: i64, turn: Option<String>, prompt: Prompt) {
        if self.snapshot.view.followup_pending {
            return;
        }
        let Some(thread) = &self.runtime.thread else {
            self.followup_failed("当前会话还未连接，草稿已保留".into());
            return;
        };
        if conversation != self.snapshot.view.conversation
            || !self.snapshot.connected
            || self.snapshot.current_archived()
            || self.snapshot.view.stopping
            || self.snapshot.view.compacting
            || self.snapshot.view.reviewing
            || !thread.resumed
            || prompt.is_empty()
        {
            self.followup_failed("当前无法发送追加消息，草稿已保留".into());
            return;
        }
        if !self.model_accepts_prompt(&thread.reported_options.model, &prompt) {
            self.followup_failed("当前轮次的模型接受文本，图片草稿已保留".into());
            return;
        }
        let mut params = json!({
            "threadId": thread.id,
            "input": prompt.input(),
            "clientUserMessageId": self.client_message_id(),
        });
        let method = if let Some(turn_id) = &turn {
            if !self
                .runtime
                .turn
                .as_ref()
                .is_some_and(|active| active.id.as_deref() == Some(turn_id.as_str()))
            {
                self.followup_failed("这轮已结束，草稿已保留".into());
                return;
            }
            params["expectedTurnId"] = json!(turn_id);
            "turn/steer"
        } else {
            "thread/queue/add"
        };
        self.snapshot.view.followup_error.clear();
        self.snapshot.view.followup_pending = true;
        self.dirty = true;
        self.request(Request::Followup { turn }, method, params);
    }

    fn followup_failed(&mut self, message: String) {
        self.snapshot.view.followup_pending = false;
        self.snapshot.view.followup_error = message.clone();
        self.status(message);
    }

    fn read_queue(&mut self) {
        let Some(thread) = &self.runtime.thread else {
            return;
        };
        if !self.snapshot.connected || self.snapshot.current_archived() {
            return;
        }
        let thread = thread.id.clone();
        self.pending
            .retain(|_, pending| !matches!(&pending.kind, Request::QueueList { conversation, .. } if *conversation == self.snapshot.view.conversation));
        self.snapshot.view.queue_loading = true;
        self.dirty = true;
        self.read_queue_page(&thread, None, Vec::new(), HashSet::new());
    }

    fn read_queue_page(
        &mut self,
        thread: &str,
        cursor: Option<String>,
        collected: Vec<QueuedMessage>,
        cursors: HashSet<String>,
    ) {
        self.request(
            Request::QueueList {
                conversation: self.snapshot.view.conversation,
                collected,
                cursors,
            },
            "thread/queue/list",
            json!({"threadId":thread,"limit":100,"cursor":cursor}),
        );
    }

    fn can_update_queue(&self) -> bool {
        self.snapshot.connected
            && !self.snapshot.current_archived()
            && !self.snapshot.view.restoring
            && !self.snapshot.view.queue_pending
            && !self.snapshot.view.followup_pending
    }

    fn queue_action(&mut self, action: QueueAction, id: &str) {
        let Some(thread) = &self.runtime.thread else {
            return;
        };
        if !self.can_update_queue() {
            return;
        }
        if !matches!(action, QueueAction::Resume)
            && !self.snapshot.view.queued.iter().any(|item| item.id == id)
        {
            return;
        }
        let (method, params) = match action {
            QueueAction::Remove => (
                "thread/queue/delete",
                json!({"threadId":thread.id,"queuedSubmissionId":id}),
            ),
            QueueAction::Start if !self.snapshot.view.busy && thread.resumed => (
                "thread/queue/start",
                json!({"threadId":thread.id,"queuedSubmissionId":id}),
            ),
            QueueAction::Resume
                if !self.snapshot.view.busy
                    && !thread.resumed
                    && !self.snapshot.view.queued.is_empty() =>
            {
                (
                    "thread/resume",
                    json!({"threadId":thread.id,"excludeTurns":true,"cwd":self.snapshot.view.workspace,
                       "sandbox":self.sandbox(),"approvalPolicy":"never"}),
                )
            }
            _ => return,
        };
        self.send_queue_action(action, method, params);
    }

    fn send_queue_action(&mut self, action: QueueAction, method: &str, params: Value) {
        self.snapshot.view.queue_pending = true;
        self.snapshot.view.queue_edit_error.clear();
        self.dirty = true;
        self.request(
            Request::QueueAction {
                conversation: self.snapshot.view.conversation,
                action,
            },
            method,
            params,
        );
    }

    fn queue_action_failed(&mut self, action: &QueueAction, message: String) {
        self.snapshot.view.queue_pending = false;
        if matches!(action, QueueAction::Update(_)) {
            self.snapshot.view.queue_edit_error = message;
            self.dirty = true;
        } else {
            self.status(message);
        }
        self.read_queue();
    }

    fn update_queued(&mut self, original: QueuedMessage, text: String) {
        if !self.can_update_queue() || self.snapshot.view.queue_loading {
            return;
        }
        let Some(thread) = &self.runtime.thread else {
            return;
        };
        if !self
            .snapshot
            .view
            .queued
            .iter()
            .any(|item| item == &original)
        {
            self.snapshot.view.queue_edit_error = "待发消息已变动，编辑内容已保留".into();
            self.dirty = true;
            return;
        }
        let Some(input) = original.with_text(text) else {
            return;
        };
        let params = json!({"threadId":thread.id,"queuedSubmissionId":original.id,"input":input});
        self.send_queue_action(
            QueueAction::Update(original.id),
            "thread/queue/update",
            params,
        );
    }

    fn move_queued(&mut self, id: &str, step: i64) {
        if !self.can_update_queue() || self.snapshot.view.queue_loading || !matches!(step, -1 | 1) {
            return;
        }
        let Some(thread) = &self.runtime.thread else {
            return;
        };
        let mut ids = self
            .snapshot
            .view
            .queued
            .iter()
            .map(|item| item.id.as_str())
            .collect::<Vec<_>>();
        let Some(index) = ids.iter().position(|item| *item == id) else {
            return;
        };
        let Some(target) = index
            .checked_add_signed(step as isize)
            .filter(|target| *target < ids.len())
        else {
            return;
        };
        ids.swap(index, target);
        let params = json!({"threadId":thread.id,"queuedSubmissionIds":ids});
        self.send_queue_action(QueueAction::Reorder, "thread/queue/reorder", params);
    }

    fn native_turn_started(&mut self, id: &str) {
        if self.runtime.last_finished_turn.as_deref() == Some(id)
            || self.snapshot.current_archived()
        {
            return;
        }
        if self.runtime.turn.is_some() {
            self.set_turn_id(id);
            return;
        }
        let resuming_queue = self.pending.values().any(|pending| {
            matches!(&pending.kind, Request::QueueAction { conversation, action: QueueAction::Resume }
                if *conversation == self.snapshot.view.conversation)
        });
        let resuming_task = self.pending.values().any(|pending| {
            matches!(&pending.kind, Request::Compact { conversation, resume: true }
                | Request::Review { conversation, prepare: Some(_) }
                if *conversation == self.snapshot.view.conversation)
        });
        if !resuming_queue
            && !resuming_task
            && !self
                .runtime
                .thread
                .as_ref()
                .is_some_and(|thread| thread.resumed)
        {
            return;
        }
        if resuming_task {
            // Resuming can start a queued turn before the requested operation.
            self.snapshot.view.compacting = false;
            self.snapshot.view.reviewing = false;
        }
        self.attempt += 1;
        self.runtime.turn = Some(ActiveTurn {
            attempt: self.attempt,
            id: Some(id.into()),
            sent: true,
            interrupt_sent: false,
            items: HashMap::new(),
            plan_index: None,
            diff_index: None,
            options: TurnOptions::default(),
            mode: None,
            local_client_id: None,
            local_prompt: None,
            user_items: HashSet::new(),
            first_entry: self.snapshot.view.entries.len(),
        });
        self.remember_draft_thread();
        self.snapshot.view.busy = true;
        self.status(if self.snapshot.view.compacting {
            Status::Compacting
        } else if self.snapshot.view.reviewing {
            Status::Reviewing
        } else {
            Status::WorkingOnQueuedMessage
        });
        self.interrupt_if_ready();
    }

    fn user_item(&mut self, item: &Value) {
        let Some(turn) = &mut self.runtime.turn else {
            return;
        };
        let Some(id) = item["id"].as_str() else {
            return;
        };
        if !turn.user_items.insert(id.to_owned()) {
            return;
        }
        let Some(message) = user_message(&item["content"]) else {
            return;
        };
        let client_id = item["clientId"].as_str();
        if client_id.is_some() && client_id == turn.local_client_id.as_deref()
            || client_id.is_none() && Some(message.display.as_str()) == turn.local_prompt.as_deref()
        {
            turn.local_prompt = None;
            return;
        }
        self.snapshot
            .view
            .entries
            .push(Arc::new(Entry::User(message)));
        self.dirty = true;
    }

    fn conversation_state(&self, id: i64) -> Option<(&ConversationView, &ConversationRuntime)> {
        if id == self.snapshot.view.conversation {
            Some((&self.snapshot.view, &self.runtime))
        } else {
            self.inactive
                .get(&id)
                .map(|entry| (&entry.view, &entry.runtime))
        }
    }

    fn conversation_view_mut(&mut self, id: i64) -> Option<&mut ConversationView> {
        if id == self.snapshot.view.conversation {
            Some(&mut self.snapshot.view)
        } else {
            self.inactive.get_mut(&id).map(|entry| &mut entry.view)
        }
    }

    fn tools_for(&self, id: i64) -> Option<ConversationTools> {
        let (view, runtime) = self.conversation_state(id)?;
        let saved = self.snapshot.connected && runtime.thread.is_some() && !view.restoring;
        let idle = saved
            && !view.busy
            && !view.rename_pending
            && !view.archive_pending
            && (id != self.snapshot.view.conversation || !self.snapshot.fork_pending);
        Some(ConversationTools {
            rename: idle,
            archive: idle && !view.queue_pending && !view.followup_pending,
            pin: saved && !view.archive_pending,
            rename_pending: view.rename_pending,
            rename_error: view.rename_error.clone(),
        })
    }

    fn rename(&mut self, conversation: i64, name: String) {
        let Some((_, runtime)) = self.conversation_state(conversation) else {
            return;
        };
        let Some(thread) = &runtime.thread else {
            self.rename_failed(conversation, "会话尚未保存".into());
            return;
        };
        if !self
            .tools_for(conversation)
            .is_some_and(|tools| tools.rename)
        {
            if !self
                .conversation_state(conversation)
                .is_some_and(|(view, _)| view.rename_pending)
            {
                self.rename_failed(conversation, "当前无法重命名，可稍后重试".into());
            }
            return;
        }
        let name = conversation_title(&name);
        if name.is_empty() {
            self.rename_failed(conversation, "请输入会话名称".into());
            return;
        }
        let params = json!({"threadId":thread.id,"name":name});
        if let Some(view) = self.conversation_view_mut(conversation) {
            view.rename_pending = true;
            view.rename_error.clear();
        }
        self.dirty = true;
        self.request(
            Request::Rename { conversation, name },
            "thread/name/set",
            params,
        );
    }

    fn rename_failed(&mut self, conversation: i64, message: String) {
        if let Some(view) = self.conversation_view_mut(conversation) {
            view.rename_pending = false;
            view.rename_error = message;
            self.dirty = true;
        }
    }

    fn set_archived(&mut self, conversation: i64, archived: bool) {
        if !self
            .tools_for(conversation)
            .is_some_and(|tools| tools.archive)
            || !self
                .snapshot
                .conversations
                .iter()
                .any(|row| row.id == conversation && row.archived != archived)
        {
            return;
        }
        let Some(thread) = self
            .conversation_state(conversation)
            .and_then(|(_, runtime)| runtime.thread.as_ref())
        else {
            return;
        };
        let params = json!({"threadId":thread.id});
        if let Some(view) = self.conversation_view_mut(conversation) {
            view.archive_pending = true;
        }
        let (method, status) = if archived {
            ("thread/archive", "正在归档…")
        } else {
            ("thread/unarchive", "正在恢复…")
        };
        self.manage_status(conversation, status.into());
        self.request(
            Request::SetArchived {
                conversation,
                archived,
            },
            method,
            params,
        );
    }

    fn manage_status(&mut self, conversation: i64, message: String) {
        if conversation == self.snapshot.view.conversation {
            self.status(message);
        } else {
            self.snapshot.sidebar_status = message;
            self.dirty = true;
        }
    }

    fn is_pinned(&self, id: &str) -> bool {
        self.temporary_pins.contains(id) || self.pin_index.project(id).is_some()
    }

    fn set_pinned(&mut self, conversation: i64, pinned: bool) {
        if !self.tools_for(conversation).is_some_and(|tools| tools.pin) {
            return;
        }
        let Some(thread_id) = self
            .conversation_state(conversation)
            .and_then(|(_, runtime)| runtime.thread.as_ref())
            .map(|thread| thread.id.clone())
        else {
            return;
        };
        let Some(row) = self
            .snapshot
            .conversations
            .iter_mut()
            .find(|row| row.id == conversation)
        else {
            return;
        };
        if row.archived || row.pinned == pinned {
            return;
        }
        let result = if pinned {
            self.pin_index
                .remember(&thread_id, &row.workspace, false, Some(row.project()))
        } else {
            self.pin_index.remove(&thread_id)
        };
        match result {
            Ok(()) => {
                if pinned && !self.pin_index.is_persistent() {
                    self.temporary_pins.insert(thread_id.clone());
                } else {
                    self.temporary_pins.remove(&thread_id);
                }
                row.pinned = pinned;
                self.snapshot.pin_status.clear();
                self.snapshot.sidebar_status.clear();
            }
            Err(error) => {
                self.snapshot.sidebar_status.clear();
                self.snapshot.pin_status = format!("无法更新置顶状态（{error}）");
            }
        }
        self.dirty = true;
    }

    fn update_archive_state(&mut self, id: i64, archived: bool) {
        let Some(row) = self
            .snapshot
            .conversations
            .iter_mut()
            .find(|row| row.id == id)
        else {
            return;
        };
        if row.archived == archived {
            return;
        }
        row.archived = archived;
        // Archiving can unload the server-side thread. Resume it after restore.
        let thread = if id == self.snapshot.view.conversation {
            self.runtime.thread.as_mut()
        } else {
            self.inactive
                .get_mut(&id)
                .and_then(|entry| entry.runtime.thread.as_mut())
        };
        if let Some(thread) = thread {
            thread.resumed = false;
            if row.pinned
                && let Err(error) = self.pin_index.archive(&thread.id, archived)
            {
                self.snapshot.pin_status = format!("无法更新置顶索引（{error}）");
            }
        }
        self.dirty = true;
    }

    fn archive_failed(&mut self, conversation: i64, message: String) {
        if let Some(view) = self.conversation_view_mut(conversation) {
            view.archive_pending = false;
        }
        self.manage_status(conversation, message);
    }

    fn set_title(&mut self, conversation: i64, name: String) {
        if let Some(row) = self
            .snapshot
            .conversations
            .iter_mut()
            .find(|row| row.id == conversation)
        {
            row.title = name;
            self.snapshot.history_matches.remove(&conversation);
        }
        self.dirty = true;
    }

    fn clear_search(&mut self) {
        self.snapshot.history_query.clear();
        self.snapshot.history_matches.clear();
    }

    fn sandbox(&self) -> &'static str {
        if self.workspace_write {
            "workspace-write"
        } else {
            "read-only"
        }
    }

    fn list_skills(&mut self, workspace: String) {
        if self.snapshot.skills.as_ref().is_some_and(|catalog| {
            catalog.workspace == workspace && !matches!(catalog.state, SkillCatalogState::Failed(_))
        }) {
            return;
        }
        self.invalidate_skills();
        self.request(
            Request::Skills,
            "skills/list",
            json!({"cwds": [&workspace]}),
        );
        self.snapshot.skills = Some(Arc::new(SkillCatalog {
            workspace,
            state: SkillCatalogState::Loading,
        }));
        self.dirty = true;
    }

    fn invalidate_skills(&mut self) {
        self.pending
            .retain(|_, pending| !matches!(pending.kind, Request::Skills));
        self.dirty |= self.snapshot.skills.take().is_some();
    }

    fn set_skill_state(&mut self, state: SkillCatalogState) {
        if let Some(catalog) = &mut self.snapshot.skills {
            Arc::make_mut(catalog).state = state;
            self.dirty = true;
        }
    }

    fn request_models(&mut self, cursor: Option<String>) {
        if cursor.is_none() {
            self.discovered_models.clear();
        }
        self.snapshot.models_loading = true;
        self.snapshot.models_status.clear();
        self.dirty = true;
        self.request(
            Request::Models {
                cursor: cursor.clone(),
            },
            "model/list",
            json!({"cursor":cursor,"limit":50,"includeHidden":false}),
        );
    }

    fn models_failed(&mut self, message: String) {
        self.discovered_models.clear();
        self.snapshot.models_loading = false;
        self.snapshot.models_status = message;
        self.dirty = true;
    }

    fn models_listed(&mut self, result: &Value, requested_cursor: Option<String>) {
        let Some(data) = result["data"].as_array() else {
            self.models_failed("无法读取模型列表".into());
            return;
        };
        let models = &mut self.discovered_models;
        for model in data.iter().filter_map(ModelInfo::parse) {
            if let Some(known) = models.iter_mut().find(|known| known.id == model.id) {
                *known = model;
            } else {
                models.push(model);
            }
        }
        let next = result["nextCursor"]
            .as_str()
            .filter(|cursor| !cursor.is_empty() && requested_cursor.as_deref() != Some(*cursor));
        if let Some(next) = next {
            self.request_models(Some(next.into()));
        } else {
            self.snapshot.models = Arc::new(std::mem::take(models));
            if self.snapshot.models.is_empty() {
                self.models_failed("当前没有可选择的模型".into());
            } else {
                self.snapshot.models_loading = false;
                self.dirty = true;
            }
        }
    }

    fn install_thread(&mut self, result: &Value) -> Result<(), String> {
        let Some(thread_id) = result["thread"]["id"].as_str().filter(|id| !id.is_empty()) else {
            return Err("Codex 没有返回会话 ID".into());
        };
        if self.conversation_for_thread(thread_id).is_some() {
            return Err("Codex 返回了已存在的会话 ID".into());
        }
        self.runtime.thread = Some(Thread {
            id: thread_id.to_owned(),
            resumed: true,
            history_loaded: true,
            history_cursor: None,
            reported_options: TurnOptions::default(),
        });
        let project = self.snapshot.project_workspace();
        if project != self.snapshot.view.workspace
            && let Err(error) = self.fork_index.remember(
                thread_id,
                &self.snapshot.view.workspace,
                false,
                Some(project),
            )
        {
            self.status(format!("会话已创建，工作树索引保存失败（{error}）"));
        }
        self.remember_thread_defaults(result);
        Ok(())
    }

    fn remember_draft_thread(&mut self) {
        let Some(thread) = &self.runtime.thread else {
            return;
        };
        let key = DraftKey::Thread(thread.id.clone());
        if self.snapshot.view.draft_key == key {
            return;
        }
        if let Err(error) = self.drafts.rekey(&self.snapshot.view.draft_key, &key) {
            self.status(format!("迁移会话草稿失败（{error}）"));
        } else {
            if let Err(error) = self.tabs.rekey(&self.snapshot.view.draft_key, &key) {
                self.snapshot.project_status = format!("迁移会话标签失败（{error}）");
            }
            self.snapshot.view.draft_key = key;
            self.dirty = true;
        }
    }

    fn remember_thread_defaults(&mut self, result: &Value) {
        let reported = TurnOptions::from_thread(result);
        if reported.model.is_empty() {
            return;
        }
        if let Some(thread) = &mut self.runtime.thread {
            thread.reported_options = reported.clone();
        }
        if self.snapshot.view.turn_options.model.is_empty() {
            self.snapshot.view.turn_options = reported;
        }
        self.dirty = true;
    }

    fn modes_listed(&mut self, result: &Value) {
        let Some(modes) = result["data"].as_array() else {
            return;
        };
        let Some(plan) = modes.iter().find(|mode| mode["mode"] == "plan") else {
            return;
        };
        if modes.iter().any(|mode| mode["mode"] == "default") {
            self.plan_effort = plan["reasoning_effort"].as_str().unwrap_or_default().into();
            self.snapshot.plan_available = true;
            self.dirty = true;
        }
    }

    fn request_user_input(&mut self, id: &Value, params: &Value) {
        let same_thread = self
            .runtime
            .thread
            .as_ref()
            .is_some_and(|thread| params["threadId"].as_str() == Some(thread.id.as_str()));
        let active_turn = self.runtime.turn.as_ref().is_some_and(|turn| {
            turn.id
                .as_deref()
                .is_some_and(|turn_id| params["turnId"] == turn_id)
        });
        if !same_thread || !active_turn || self.snapshot.view.stopping {
            self.outbound.push(json!({"jsonrpc":"2.0","id":id,
                "error":{"code":-32602,"message":"No matching active turn"}}));
            return;
        }
        if self
            .runtime
            .pending_input
            .iter()
            .any(|pending| pending.request == *id)
        {
            return;
        }
        if let Some(request) = PendingInput::parse(id.clone(), self.next_question_token, params) {
            self.next_question_token = self.next_question_token.wrapping_add(1);
            self.runtime.pending_input.push_back(request);
            self.show_question();
        } else {
            self.outbound.push(json!({"jsonrpc":"2.0","id":id,
                "error":{"code":-32602,"message":"Invalid user input questions"}}));
        }
    }

    fn answer_question(&mut self, token: i64, index: i64, value: Option<String>) {
        let Some(current) = self.runtime.pending_input.front_mut() else {
            return;
        };
        if !current.answer(token, index, value) {
            return;
        }
        if current.current().is_none()
            && let Some(done) = self.runtime.pending_input.pop_front()
        {
            self.outbound.push(done.response());
        }
        self.show_question();
    }

    fn show_question(&mut self) {
        let waiting = self.snapshot.view.question.is_some();
        self.snapshot.view.question = self
            .runtime
            .pending_input
            .front()
            .and_then(PendingInput::current)
            .cloned()
            .map(Arc::new);
        if self.snapshot.view.stopping {
            self.dirty = true;
        } else if self.snapshot.view.question.is_some() {
            self.status(Status::AwaitingAnswer);
        } else if waiting {
            self.status(Status::Resuming);
        }
    }

    fn list_sessions(&mut self, more: bool) {
        if !self.snapshot.connected || self.snapshot.view.workspace.is_empty() {
            return;
        }
        let cursor = if more {
            let Some(cursor) = self.history_cursor.clone() else {
                return;
            };
            Some(cursor)
        } else {
            self.history_cursor = None;
            self.snapshot.history_more = false;
            self.snapshot.history_matches.clear();
            None
        };
        self.pending.retain(|_, pending| {
            !matches!(
                &pending.kind,
                Request::Sessions { .. } | Request::IndexedThread { .. }
            )
        });
        self.snapshot.history_loading = true;
        self.snapshot.history_status.clear();
        self.dirty = true;
        let project = self.snapshot.project_workspace();
        let cwd = if project == self.snapshot.view.workspace {
            json!(self.snapshot.view.workspace)
        } else {
            json!([project, self.snapshot.view.workspace])
        };
        self.request(
            Request::Sessions {
                workspace: self.snapshot.view.workspace.clone(),
                more,
            },
            "thread/list",
            json!({
                "cwd": cwd,
                "cursor": cursor,
                "limit": SESSION_PAGE_SIZE,
                "archived": self.snapshot.archive_view,
                "searchTerm": Some(self.snapshot.history_query.trim()).filter(|query| !query.is_empty()),
                "sourceKinds": ["cli", "vscode", "appServer"],
                "modelProviders": [],
            }),
        );
    }

    fn list_project_history(&mut self, project: String, more: bool) {
        if !self.snapshot.connected || !self.snapshot.projects.contains(&project) {
            return;
        }
        let history = Arc::make_mut(&mut self.snapshot.project_histories)
            .entry(project.clone())
            .or_default();
        if history.loading || (!more && history.loaded) || (more && history.cursor.is_none()) {
            return;
        }
        history.loading = true;
        history.status.clear();
        let cursor = history.cursor.clone();
        let workspaces = if more {
            history.workspaces.clone()
        } else {
            let mut workspaces: Vec<String> = self
                .snapshot
                .conversations
                .iter()
                .filter(|row| row.project() == project && !row.archived)
                .map(|row| row.workspace.clone())
                .collect();
            workspaces.push(project.clone());
            for index in [&self.fork_index, &self.pin_index] {
                if let Ok(records) = index.records(&project, false) {
                    workspaces.extend(records.into_iter().map(|entry| entry.workspace));
                }
            }
            workspaces.sort_unstable();
            workspaces.dedup();
            history.workspaces.clone_from(&workspaces);
            workspaces
        };
        let cwd = if workspaces.len() == 1 {
            json!(project)
        } else {
            json!(workspaces)
        };
        self.request(
            Request::ProjectSessions {
                project,
                workspaces,
            },
            "thread/list",
            json!({
                "cwd": cwd, "cursor": cursor, "limit": SESSION_PAGE_SIZE, "archived": false,
                "sourceKinds": ["cli", "vscode", "appServer"], "modelProviders": [],
            }),
        );
        self.dirty = true;
    }

    fn project_history_failed(&mut self, project: &str, status: String) {
        if let Some(history) = Arc::make_mut(&mut self.snapshot.project_histories).get_mut(project)
        {
            history.loading = false;
            history.status = status;
            self.dirty = true;
        }
    }

    fn project_sessions_listed(&mut self, project: &str, workspaces: &[String], result: &Value) {
        let Some(threads) = result["data"].as_array() else {
            self.project_history_failed(project, "无法读取会话，可重试".into());
            return;
        };
        let workspaces = workspaces
            .iter()
            .map(|path| (path.clone(), project.to_owned()))
            .collect();
        self.merge_session_rows(
            threads,
            SessionListScope {
                workspaces: &workspaces,
                archived: false,
                searching: false,
                refresh_existing: false,
                linked_read: false,
                restore_tab: false,
            },
        );
        if let Some(history) = Arc::make_mut(&mut self.snapshot.project_histories).get_mut(project)
        {
            let next = result["nextCursor"]
                .as_str()
                .filter(|cursor| !cursor.is_empty());
            history.cursor = next
                .filter(|cursor| Some(*cursor) != history.cursor.as_deref())
                .map(str::to_owned);
            history.loading = false;
            history.loaded = true;
        }
        self.dirty = true;
    }

    fn search_projects(&mut self, query: String, archived: bool, more: bool) {
        let query = query.trim().to_owned();
        if more {
            let state = &self.snapshot.project_search;
            if state.loading || state.query != query || state.archived != archived || !state.more()
            {
                return;
            }
        } else {
            self.pending
                .retain(|_, request| !matches!(request.kind, Request::SearchProjects));
            self.snapshot.project_search = Arc::new(ProjectSearch {
                query: query.clone(),
                archived,
                ..ProjectSearch::default()
            });
            self.dirty = true;
        }
        if query.is_empty() {
            return;
        }
        if !self.snapshot.connected {
            self.project_search_failed("Codex 未连接".into());
            return;
        }
        if !more {
            let mut workspaces: BTreeMap<_, _> = self
                .snapshot
                .projects
                .iter()
                .map(|path| (path.clone(), path.clone()))
                .collect();
            let registered: HashSet<_> =
                self.snapshot.projects.iter().map(String::as_str).collect();
            for row in &self.snapshot.conversations {
                if registered.contains(row.project()) {
                    workspaces
                        .entry(row.workspace.clone())
                        .or_insert_with(|| row.project().to_owned());
                }
            }
            for index in [&self.fork_index, &self.pin_index] {
                if let Ok(links) = index.project_workspaces() {
                    for (project, workspace) in links {
                        if registered.contains(project.as_str()) {
                            workspaces.entry(workspace).or_insert(project);
                        }
                    }
                }
            }
            Arc::make_mut(&mut self.snapshot.project_search).workspaces = workspaces;
        }
        let search = Arc::make_mut(&mut self.snapshot.project_search);
        if search.workspaces.is_empty() {
            self.project_search_failed("还没有可搜索的项目".into());
            return;
        }
        search.loading = true;
        search.status.clear();
        let cwd: Vec<_> = search.workspaces.keys().collect();
        let params = json!({
            "cwd": cwd, "cursor": search.cursor, "limit": SESSION_PAGE_SIZE, "archived": archived,
            "searchTerm": query, "sourceKinds": ["cli", "vscode", "appServer"], "modelProviders": [],
        });
        self.request(Request::SearchProjects, "thread/list", params);
        self.dirty = true;
    }

    fn project_search_failed(&mut self, status: String) {
        let search = Arc::make_mut(&mut self.snapshot.project_search);
        search.loading = false;
        search.status = status;
        self.dirty = true;
    }

    fn project_search_listed(&mut self, result: &Value) {
        let Some(threads) = result["data"].as_array() else {
            self.project_search_failed("搜索结果无法读取，可重试".into());
            return;
        };
        let request = self.snapshot.project_search.clone();
        let matches = self.merge_session_rows(
            threads,
            SessionListScope {
                workspaces: &request.workspaces,
                archived: request.archived,
                searching: false,
                refresh_existing: false,
                linked_read: false,
                restore_tab: false,
            },
        );
        let search = Arc::make_mut(&mut self.snapshot.project_search);
        let mut seen: HashSet<_> = search.matches.iter().copied().collect();
        search
            .matches
            .extend(matches.into_iter().filter(|id| seen.insert(*id)));
        search.cursor = result["nextCursor"]
            .as_str()
            .filter(|cursor| !cursor.is_empty() && Some(*cursor) != request.cursor.as_deref())
            .map(str::to_owned);
        search.loading = false;
        self.dirty = true;
    }

    fn sessions_listed(&mut self, result: &Value, more: bool) {
        let threads = result["data"].as_array().map_or(&[][..], Vec::as_slice);
        if !result["data"].is_array() {
            self.snapshot.history_status = "无法读取历史会话".into();
        }
        self.merge_sessions(threads, None);
        let native_ids = threads
            .iter()
            .filter_map(|thread| {
                if (thread["cwd"] != self.snapshot.view.workspace
                    && thread["cwd"] != self.snapshot.project_workspace())
                    || thread["ephemeral"] == true
                {
                    return None;
                }
                thread["id"].as_str()
            })
            .collect::<HashSet<_>>();
        if !self.snapshot.archive_view {
            for id in &native_ids {
                self.fork_index.forget(id);
            }
        }
        if !more {
            self.lookup_indexed_threads(&native_ids);
        }
        let next = result["nextCursor"]
            .as_str()
            .filter(|cursor| !cursor.is_empty());
        self.history_cursor = next
            .filter(|cursor| Some(*cursor) != self.history_cursor.as_deref())
            .map(str::to_owned);
        self.snapshot.history_more = self.history_cursor.is_some();
        self.settle_session_list();
    }

    fn lookup_indexed_threads(&mut self, listed: &HashSet<&str>) {
        let project = self.snapshot.project_workspace();
        let archived = self.snapshot.archive_view;
        let indexes = [
            (self.fork_index.records(project, archived), "分支"),
            (self.pin_index.records(project, archived), "置顶会话"),
        ];
        let mut requested = HashSet::new();
        for (result, label) in indexes {
            match result {
                Ok(records) => {
                    for entry in records {
                        if listed.contains(entry.id.as_str()) || !requested.insert(entry.id.clone())
                        {
                            continue;
                        }
                        let params = json!({"threadId": entry.id, "includeTurns": false});
                        self.request(
                            Request::IndexedThread {
                                workspace: self.snapshot.view.workspace.clone(),
                                thread_id: entry.id,
                                actual_workspace: entry.workspace,
                            },
                            "thread/read",
                            params,
                        );
                    }
                }
                Err(error) => {
                    self.snapshot.history_status = format!("读取{label}索引失败（{error}）")
                }
            }
        }
    }

    fn indexed_thread_read(&mut self, result: &Value, id: &str, workspace: &str) {
        let thread = &result["thread"];
        let query = self.snapshot.history_query.trim();
        let title = thread["name"]
            .as_str()
            .filter(|name| !name.trim().is_empty())
            .or_else(|| thread["preview"].as_str())
            .unwrap_or_default();
        if thread["id"] == id
            && (thread["cwd"] == workspace
                || self
                    .fork_index
                    .handoff(id)
                    .is_some_and(|location| location.workspace == workspace))
            && (query.is_empty() || title.to_lowercase().contains(&query.to_lowercase()))
            && !self.conversation_for_thread(id).is_some_and(|local| {
                self.snapshot
                    .conversations
                    .iter()
                    .any(|row| row.id == local && row.archived != self.snapshot.archive_view)
            })
        {
            self.merge_sessions(std::slice::from_ref(thread), Some(workspace));
        }
        self.settle_session_list();
    }

    fn settle_session_list(&mut self) {
        self.snapshot.history_loading = self.pending.values().any(|pending| {
            matches!(
                &pending.kind,
                Request::Sessions { .. } | Request::IndexedThread { .. }
            )
        });
        self.dirty = true;
    }

    fn merge_sessions(&mut self, threads: &[Value], linked_workspace: Option<&str>) {
        let project = self.snapshot.project_workspace().to_owned();
        let mut workspaces = BTreeMap::from([
            (self.snapshot.view.workspace.clone(), project.clone()),
            (project.clone(), project.clone()),
        ]);
        if let Some(workspace) = linked_workspace {
            workspaces.insert(workspace.into(), project);
        }
        self.merge_session_rows(
            threads,
            SessionListScope {
                workspaces: &workspaces,
                archived: self.snapshot.archive_view,
                searching: !self.snapshot.history_query.trim().is_empty(),
                refresh_existing: true,
                linked_read: linked_workspace.is_some(),
                restore_tab: false,
            },
        );
    }

    fn merge_session_rows(&mut self, threads: &[Value], scope: SessionListScope<'_>) -> Vec<i64> {
        let mut known: HashMap<_, _> = self
            .inactive
            .iter()
            .filter_map(|(&id, entry)| Some((entry.runtime.thread.as_ref()?.id.clone(), id)))
            .collect();
        if let Some(thread) = &self.runtime.thread {
            known.insert(thread.id.clone(), self.snapshot.view.conversation);
        }
        let mut seen = HashSet::new();
        let mut matches = Vec::new();
        for thread in threads {
            let Some(id) = thread["id"].as_str().filter(|id| !id.is_empty()) else {
                continue;
            };
            let Some(reported) = thread["cwd"].as_str() else {
                continue;
            };
            let handoff = self.fork_index.handoff(id);
            let workspace = handoff
                .as_ref()
                .map_or(reported, |location| location.workspace.as_str());
            let project = scope.workspaces.get(workspace).or_else(|| {
                if handoff.is_some() {
                    scope
                        .workspaces
                        .get(reported)
                        .filter(|project| self.fork_index.project(id).as_ref() == Some(*project))
                } else {
                    None
                }
            });
            let Some(project) = project else {
                continue;
            };
            if thread["ephemeral"].as_bool() == Some(true)
                || !scope.restore_tab && !thread["parentThreadId"].is_null()
                || !seen.insert(id)
            {
                continue;
            }
            let title = thread["name"]
                .as_str()
                .filter(|name| !name.trim().is_empty())
                .or_else(|| {
                    thread["preview"]
                        .as_str()
                        .filter(|preview| !preview.trim().is_empty())
                });
            let pinned = self.is_pinned(id);
            if pinned
                && scope.refresh_existing
                && !scope.linked_read
                && let Err(error) = self.pin_index.archive(id, scope.archived)
            {
                self.snapshot.pin_status = format!("无法更新置顶索引（{error}）");
            }
            if let Some(&conversation) = known.get(id) {
                matches.push(conversation);
                if !scope.refresh_existing {
                    continue;
                }
                self.update_archive_state(conversation, scope.archived);
                if let Some(row) = self
                    .snapshot
                    .conversations
                    .iter_mut()
                    .find(|row| row.id == conversation)
                {
                    row.pinned = pinned;
                }
                if let Some(title) = title {
                    self.set_title(conversation, conversation_title(title));
                }
                if scope.searching {
                    self.snapshot.history_matches.insert(conversation);
                }
                continue;
            }
            let options = TurnOptions::from_thread(thread);
            let project_workspace = self
                .fork_index
                .project(id)
                .or_else(|| self.pin_index.project(id))
                .or_else(|| (workspace != project).then(|| project.clone()))
                .filter(|project| project != workspace);
            let thread = id.to_owned();
            let title = conversation_title(title.unwrap_or("历史会话"));
            let id = self.next_conversation;
            self.next_conversation += 1;
            matches.push(id);
            if scope.searching {
                self.snapshot.history_matches.insert(id);
            }
            let mut conversation = Conversation::new(
                id,
                workspace.into(),
                project_workspace.as_deref(),
                Some(Thread {
                    id: thread,
                    resumed: false,
                    history_loaded: false,
                    history_cursor: None,
                    reported_options: options,
                }),
                false,
            );
            conversation.view.associated_worktree =
                handoff.as_ref().map(|location| location.worktree.clone());
            self.inactive.insert(id, conversation);
            self.snapshot.conversations.push(ConversationSummary {
                id,
                title,
                workspace: workspace.into(),
                project_workspace,
                archived: scope.archived,
                pinned,
                activity: ConversationActivity::Idle,
            });
        }
        matches
    }

    fn swap_conversation(&mut self, other: &mut Conversation) {
        std::mem::swap(&mut self.snapshot.view, &mut other.view);
        std::mem::swap(&mut self.runtime, &mut other.runtime);
    }

    fn navigation_target(&self, forward: bool) -> Option<(usize, i64)> {
        let mut index = self.visit;
        loop {
            index = if forward {
                index.checked_add(1)?
            } else {
                index.checked_sub(1)?
            };
            let id = *self.visits.get(index)?;
            if id != self.snapshot.view.conversation && self.inactive.contains_key(&id) {
                return Some((index, id));
            }
        }
    }

    fn show_conversation(&mut self, mut next: Conversation, visit: Option<usize>) {
        let previous = self.snapshot.view.conversation;
        if !self.snapshot.view.workspace.is_empty() && !self.snapshot.current_archived() {
            self.recent_project_conversations
                .insert(self.snapshot.view.project_workspace.clone(), previous);
        }
        self.swap_conversation(&mut next);
        if let Some(visit) = visit {
            self.visit = visit;
        } else {
            self.visits.truncate(self.visit + 1);
            self.visits.push(self.snapshot.view.conversation);
            self.visit = self.visits.len() - 1;
        }
        if !next.view.workspace.is_empty() {
            self.inactive.insert(previous, next);
        }
        self.snapshot.view.unread = false;
        if self.snapshot.view.status.is_empty() {
            self.idle_status();
        }
        self.dirty = true;
    }

    /// Process a scoped server event without changing the conversation shown in the window.
    fn with_conversation<R>(&mut self, id: i64, handle: impl FnOnce(&mut Self) -> R) -> Option<R> {
        if id == self.snapshot.view.conversation {
            return Some(handle(self));
        }
        let mut state = self.inactive.remove(&id)?;
        self.swap_conversation(&mut state);
        let was_busy = self.snapshot.view.busy;
        let result = handle(self);
        debug_assert_eq!(self.snapshot.view.conversation, id);
        self.snapshot.view.unread |= was_busy && !self.snapshot.view.busy;
        self.swap_conversation(&mut state);
        self.inactive.insert(id, state);
        Some(result)
    }

    fn conversation_for_thread(&self, thread_id: &str) -> Option<i64> {
        if self
            .runtime
            .thread
            .as_ref()
            .is_some_and(|thread| thread.id == thread_id)
        {
            return Some(self.snapshot.view.conversation);
        }
        self.inactive.iter().find_map(|(&id, state)| {
            state
                .runtime
                .thread
                .as_ref()
                .is_some_and(|thread| thread.id == thread_id)
                .then_some(id)
        })
    }

    fn activate_project(&mut self, path: String) {
        if path.is_empty() || self.snapshot.view.project_workspace == path {
            return;
        }
        if let Some(&id) = self.recent_project_conversations.get(&path)
            && self
                .snapshot
                .conversations
                .iter()
                .any(|row| row.id == id && !row.archived)
        {
            self.select_conversation(id, None);
        } else {
            self.new_conversation(path, None);
            self.snapshot.archive_view = false;
            self.clear_search();
            self.remember_project();
            self.list_sessions(false);
        }
    }

    fn new_conversation(&mut self, workspace: String, project_workspace: Option<String>) {
        self.snapshot.fork_error.clear();
        let id = self.next_conversation;
        self.next_conversation += 1;
        self.show_conversation(
            Conversation::new(
                id,
                workspace.clone(),
                project_workspace.as_deref(),
                None,
                false,
            ),
            None,
        );
        self.snapshot.conversations.insert(
            0,
            ConversationSummary {
                id,
                workspace,
                title: "新会话".into(),
                archived: false,
                pinned: false,
                project_workspace,
                activity: ConversationActivity::Idle,
            },
        );
        self.idle_status();
    }

    fn select_conversation(&mut self, id: i64, visit: Option<usize>) {
        if id == self.snapshot.view.conversation {
            if !self.snapshot.view.restoring
                && self
                    .runtime
                    .thread
                    .as_ref()
                    .is_some_and(|thread| !thread.history_loaded)
            {
                self.load_history(false);
            }
            return;
        }
        let Some(row) = self.snapshot.conversations.iter().find(|row| row.id == id) else {
            return;
        };
        let archived = row.archived;
        let Some(conversation) = self.inactive.remove(&id) else {
            return;
        };
        let changed_workspace = self.snapshot.view.workspace != conversation.view.workspace;
        let changed_project =
            self.snapshot.view.project_workspace != conversation.view.project_workspace;
        let changed_archive = self.snapshot.archive_view != archived;
        self.show_conversation(conversation, visit);
        if changed_project {
            self.remember_project();
        }
        if changed_workspace || changed_project || changed_archive {
            self.snapshot.archive_view = archived;
            self.clear_search();
            self.list_sessions(false);
        }
        if self
            .runtime
            .thread
            .as_ref()
            .is_some_and(|thread| !thread.history_loaded)
        {
            self.load_history(false);
        }
        self.read_queue();
    }

    fn load_history(&mut self, earlier: bool) {
        let Some(thread) = &self.runtime.thread else {
            return;
        };
        if !self.snapshot.connected
            || earlier && thread.history_cursor.is_none()
            || !earlier && !self.snapshot.view.entries.is_empty()
        {
            return;
        }
        let params = json!({
            "threadId": thread.id,
            "cursor": thread.history_cursor,
            "limit": RECENT_TURN_LIMIT,
            "sortDirection": "desc",
            "itemsView": "full",
        });
        self.snapshot.view.restoring = true;
        self.status(Status::LoadingHistory);
        self.request(
            Request::History {
                conversation: self.snapshot.view.conversation,
                earlier,
            },
            "thread/turns/list",
            params,
        );
    }

    fn history_loaded(&mut self, result: &Value, earlier: bool) {
        self.snapshot.view.restoring = false;
        let Some(turns) = result["data"].as_array() else {
            self.status("无法读取会话历史");
            return;
        };
        let Some(thread) = &mut self.runtime.thread else {
            return;
        };
        let mut entries = Vec::new();
        let mut points = Vec::new();
        for turn in turns.iter().rev() {
            let Some(items) = turn["items"].as_array() else {
                continue;
            };
            let start = entries.len();
            entries.extend(items.iter().filter_map(history_entry).map(Arc::new));
            if let Some(mut point) = history_fork_point(turn) {
                point.response = last_response(&entries, start);
                point.prompt = first_prompt(&entries, start);
                point.entries = start..entries.len();
                points.push(point);
            }
        }
        if earlier {
            let added = entries.len();
            self.snapshot.view.entries.splice(..0, entries);
            let existing = Arc::make_mut(&mut self.snapshot.view.fork_points);
            for point in existing.iter_mut() {
                point.entries.start += added;
                point.entries.end += added;
                if let Some(response) = &mut point.response {
                    *response += added;
                }
                if let Some(prompt) = &mut point.prompt {
                    *prompt += added;
                }
            }
            existing.splice(..0, points);
        } else {
            self.snapshot.view.entries = entries;
            self.snapshot.view.fork_points = Arc::new(points);
        }
        thread.history_loaded = true;
        thread.history_cursor = result["nextCursor"]
            .as_str()
            .filter(|cursor| {
                !cursor.is_empty() && thread.history_cursor.as_deref() != Some(*cursor)
            })
            .map(str::to_owned);
        self.snapshot.view.earlier_turns = thread.history_cursor.is_some();
        self.idle_status();
    }

    fn start_turn(&mut self, prompt: Prompt) {
        let (Some(thread), Some(turn)) = (&self.runtime.thread, &mut self.runtime.turn) else {
            return;
        };
        turn.sent = true;
        let request = Request::Turn(turn.attempt);
        let mut params = json!({
            "threadId":thread.id,
            "input":prompt.input(),
            "clientUserMessageId":turn.local_client_id,
            "cwd":self.snapshot.view.workspace,
        });
        if self.snapshot.view.associated_worktree.is_some() {
            params["runtimeWorkspaceRoots"] = json!([self.snapshot.view.workspace]);
        }
        if !turn.options.model.is_empty() {
            params["model"] = json!(turn.options.model);
        }
        if !turn.options.effort.is_empty() {
            params["effort"] = json!(turn.options.effort);
        }
        if let Some(plan) = turn.mode {
            let model = if turn.options.model.is_empty() {
                &thread.reported_options.model
            } else {
                &turn.options.model
            };
            if !model.is_empty() {
                let effort = if !turn.options.effort.is_empty() {
                    &turn.options.effort
                } else if plan && !self.plan_effort.is_empty() {
                    &self.plan_effort
                } else {
                    &thread.reported_options.effort
                };
                params["collaborationMode"] = json!({
                    "mode": if plan { "plan" } else { "default" },
                    "settings": {
                        "model": model,
                        "reasoning_effort": Some(effort).filter(|effort| !effort.is_empty()),
                        "developer_instructions": null,
                    },
                });
                turn.options = TurnOptions {
                    model: model.clone(),
                    effort: effort.clone(),
                };
            } else if plan {
                self.finish(
                    "请选择一个可用模型再发送计划请求",
                    ActivityState::Unfinished,
                );
                return;
            }
        }
        self.request(request, "turn/start", params);
    }

    fn interrupt_if_ready(&mut self) {
        let (Some(thread), Some(turn)) = (&self.runtime.thread, &mut self.runtime.turn) else {
            return;
        };
        let Some(id) = &turn.id else { return };
        if self.snapshot.view.stopping && !turn.interrupt_sent {
            turn.interrupt_sent = true;
            let request = Request::Interrupt(turn.attempt);
            let params = json!({"threadId":thread.id,"turnId":id});
            self.request(request, "turn/interrupt", params);
        }
    }

    pub(crate) fn server(&mut self, message: Value) -> Result<(), String> {
        if let Some(method) = message.get("method").and_then(Value::as_str) {
            let params = &message["params"];
            let owner = params["threadId"]
                .as_str()
                .and_then(|id| self.conversation_for_thread(id));
            if let Some(id) = message.get("id") {
                if method == "item/tool/requestUserInput" {
                    if let Some(owner) = owner {
                        self.with_conversation(owner, |session| {
                            session.request_user_input(id, params)
                        });
                    } else {
                        self.request_user_input(id, params);
                    }
                } else {
                    self.outbound.push(match method {
                        "item/commandExecution/requestApproval"
                        | "item/fileChange/requestApproval" => {
                            json!({"jsonrpc":"2.0","id":id,"result":{"decision":"decline"}})
                        }
                        _ => json!({
                            "jsonrpc":"2.0","id":id,
                            "error":{"code":-32601,"message":"Unsupported client request"},
                        }),
                    });
                }
            } else if method == "skills/changed" {
                self.invalidate_skills();
            } else if matches!(method, "thread/archived" | "thread/unarchived") {
                let archived = method == "thread/archived";
                if let Some(owner) = owner {
                    self.update_archive_state(owner, archived);
                } else if let Some(thread) = params["threadId"].as_str()
                    && let Err(error) = self.pin_index.archive(thread, archived)
                {
                    self.snapshot.pin_status = format!("无法更新置顶索引（{error}）");
                    self.dirty = true;
                }
                self.pending.retain(|_, pending| {
                    !matches!(&pending.kind, Request::IndexedThread { thread_id, .. }
                        if params["threadId"] == thread_id.as_str())
                });
                if self.snapshot.history_loading {
                    self.settle_session_list();
                }
            } else if let Some(owner) = owner {
                self.with_conversation(owner, |session| session.notification(method, params));
            }
            return Ok(());
        }

        let Some(id) = message.get("id").and_then(Value::as_u64) else {
            return Ok(());
        };
        let Some(request) = self.pending.remove(&id) else {
            return Ok(());
        };
        if let Some(owner) = request.owner {
            self.with_conversation(owner, |session| session.response(request.kind, &message))
                .unwrap_or(Ok(()))
        } else {
            self.response(request.kind, &message)
        }
    }

    fn response(&mut self, request: Request, message: &Value) -> Result<(), String> {
        if let Some(error) = message.get("error") {
            let reason = error["message"].as_str().unwrap_or("未知协议错误");
            match request {
                Request::Initialize => return Err(format!("连接失败：{reason}")),
                Request::RestoreTab(_) => self.finish_tab_restore(),
                Request::Handoff { .. } => {
                    self.handoff_failed(format!("工作目录更新失败（{reason}）"))
                }
                Request::Models { .. } => self.models_failed(format!("读取模型失败（{reason}）")),
                Request::Skills => self.set_skill_state(SkillCatalogState::Failed(reason.into())),
                Request::Followup { .. } => {
                    self.followup_failed(format!("追加失败（{reason}），草稿已保留"))
                }
                Request::Compact { conversation, .. }
                    if conversation == self.snapshot.view.conversation
                        && self.snapshot.view.compacting =>
                {
                    self.finish(format!("压缩失败（{reason}）"), ActivityState::Unfinished);
                }
                Request::Review { conversation, .. }
                    if conversation == self.snapshot.view.conversation
                        && self.snapshot.view.reviewing =>
                {
                    self.finish(format!("审阅失败（{reason}）"), ActivityState::Unfinished);
                }
                Request::Fork { worktree, .. } => self.fork_failed(fork_error_with_worktree(
                    format!("创建分支失败（{reason}）"),
                    worktree.as_deref(),
                )),
                Request::ForkTitle { .. } => {
                    self.fork_failed(format!("分支已创建，名称保存失败（{reason}）"))
                }
                Request::QueueList { conversation, .. }
                    if conversation == self.snapshot.view.conversation =>
                {
                    self.snapshot.view.queue_loading = false;
                    self.dirty = true;
                }
                Request::QueueAction {
                    conversation,
                    action,
                } if conversation == self.snapshot.view.conversation => {
                    self.queue_action_failed(&action, format!("队列操作失败（{reason}）"));
                }
                Request::Thread { attempt, .. }
                | Request::Resume { attempt, .. }
                | Request::Turn(attempt)
                    if self.is_current(attempt) =>
                {
                    self.finish(format!("发送失败：{reason}"), ActivityState::Unfinished);
                }
                Request::Sessions { workspace, more }
                    if workspace == self.snapshot.view.workspace =>
                {
                    self.snapshot.history_status = format!("会话列表加载失败：{reason}");
                    if !more {
                        self.lookup_indexed_threads(&HashSet::new());
                    }
                    self.settle_session_list();
                }
                Request::IndexedThread { .. } => self.settle_session_list(),
                Request::ProjectSessions { project, .. } => {
                    self.project_history_failed(&project, format!("读取会话失败（{reason}）"));
                }
                Request::SearchProjects => {
                    self.project_search_failed(format!("搜索会话失败（{reason}）"));
                }
                Request::Rename { conversation, .. } => {
                    self.rename_failed(conversation, format!("重命名失败（{reason}）"))
                }
                Request::SetArchived {
                    conversation,
                    archived,
                } => {
                    let action = if archived { "归档" } else { "恢复" };
                    self.archive_failed(conversation, format!("{action}失败（{reason}）"));
                }
                Request::History { conversation, .. }
                    if conversation == self.snapshot.view.conversation =>
                {
                    self.snapshot.view.restoring = false;
                    self.status(format!("会话历史加载失败：{reason}"));
                }
                Request::Interrupt(attempt) if self.is_current(attempt) => {
                    self.snapshot.view.stopping = false;
                    if let Some(turn) = &mut self.runtime.turn {
                        turn.interrupt_sent = false;
                    }
                    self.status(format!("停止失败：{reason}"));
                }
                _ => {}
            }
            return Ok(());
        }

        let result = &message["result"];
        match request {
            Request::Initialize => {
                self.outbound
                    .push(json!({"jsonrpc":"2.0","method":"initialized"}));
                self.snapshot.connected = true;
                self.idle_status();
                self.list_sessions(false);
                self.request_models(None);
                self.request(Request::Modes, "collaborationMode/list", json!({}));
                self.begin_tab_restore();
            }
            Request::RestoreTab(tab) => self.restore_native_tab(tab, result),
            Request::Handoff {
                thread,
                workspace,
                worktree,
                resume,
            } => {
                let confirmed = if resume {
                    result["thread"]["id"] == thread
                        && result
                            .get("cwd")
                            .and_then(Value::as_str)
                            .or_else(|| result["thread"]["cwd"].as_str())
                            == Some(workspace.as_str())
                } else {
                    result.is_object()
                };
                if confirmed {
                    self.handoff_confirmed(&thread, workspace, worktree);
                } else {
                    self.handoff_failed("Codex 返回的会话或运行目录与目标不一致".into());
                }
            }
            Request::Models { cursor } => self.models_listed(result, cursor),
            Request::Skills => {
                if let Some(catalog) = &self.snapshot.skills {
                    let state = match parse_catalog(result, &catalog.workspace) {
                        Ok(skills) => SkillCatalogState::Ready(skills),
                        Err(error) => SkillCatalogState::Failed(error),
                    };
                    self.set_skill_state(state);
                }
            }
            Request::Modes => self.modes_listed(result),
            Request::Review {
                conversation,
                prepare: Some(target),
            } if conversation == self.snapshot.view.conversation => {
                if let Some(thread) = &mut self.runtime.thread {
                    if result["thread"]["id"] != thread.id {
                        if self.snapshot.view.reviewing {
                            self.finish("Codex 返回了不同的会话 ID", ActivityState::Unfinished);
                        }
                        return Ok(());
                    }
                    thread.resumed = true;
                    self.remember_thread_defaults(result);
                } else {
                    if let Err(error) = self.install_thread(result) {
                        self.finish(error, ActivityState::Unfinished);
                        return Ok(());
                    }
                    self.set_title(
                        conversation,
                        conversation_title(&format!("审阅 · {}", target.title())),
                    );
                }
                if self.snapshot.view.reviewing {
                    if self.snapshot.view.stopping {
                        self.finish("已取消审阅", ActivityState::Interrupted);
                    } else {
                        self.send_review(target);
                    }
                }
            }
            Request::Review {
                conversation,
                prepare: None,
            } if conversation == self.snapshot.view.conversation
                && self.snapshot.view.reviewing =>
            {
                let expected = self
                    .runtime
                    .thread
                    .as_ref()
                    .map(|thread| thread.id.as_str());
                if expected.is_some()
                    && result["reviewThreadId"].as_str() == expected
                    && let Some(turn_id) = result["turn"]["id"].as_str()
                {
                    self.native_turn_started(turn_id);
                } else {
                    self.finish("Codex 未确认当前会话的审阅轮次", ActivityState::Unfinished);
                }
            }
            Request::Fork {
                source,
                title,
                worktree,
                initial_draft,
            } if source == self.snapshot.view.conversation => {
                self.fork_created(result, title, worktree, initial_draft)
            }
            Request::ForkTitle {
                conversation,
                title,
            } => {
                self.snapshot.fork_pending = false;
                self.set_title(conversation, title);
                if self.snapshot.history_loading {
                    self.list_sessions(false);
                }
            }
            Request::IndexedThread {
                workspace,
                thread_id,
                actual_workspace,
            } if workspace == self.snapshot.view.workspace => {
                self.indexed_thread_read(result, &thread_id, &actual_workspace)
            }
            Request::Compact {
                conversation,
                resume: true,
            } if conversation == self.snapshot.view.conversation => {
                let Some(thread) = &mut self.runtime.thread else {
                    return Ok(());
                };
                if result["thread"]["id"] != thread.id {
                    if self.snapshot.view.compacting {
                        self.finish("Codex 返回了不同的会话 ID", ActivityState::Unfinished);
                    }
                    return Ok(());
                }
                thread.resumed = true;
                self.remember_thread_defaults(result);
                if self.snapshot.view.compacting {
                    if self.snapshot.view.stopping {
                        self.finish("已取消压缩", ActivityState::Interrupted);
                    } else {
                        let params = json!({"threadId":result["thread"]["id"]});
                        self.request(
                            Request::Compact {
                                conversation,
                                resume: false,
                            },
                            "thread/compact/start",
                            params,
                        );
                    }
                }
            }
            Request::Followup { turn } => {
                let queued = turn.is_none();
                let acknowledged = match &turn {
                    Some(expected) => result["turnId"].as_str() == Some(expected.as_str()),
                    None => QueuedMessage::parse(&result["queuedSubmission"]).is_some(),
                };
                if !acknowledged {
                    self.followup_failed("发送回执无法确认，草稿已保留".into());
                    return Ok(());
                }
                self.snapshot.view.followup_pending = false;
                self.snapshot.view.followup_error.clear();
                if !self.snapshot.view.stopping {
                    self.status(if queued {
                        Status::Queued
                    } else {
                        Status::Steered
                    });
                } else {
                    self.dirty = true;
                }
                if queued {
                    self.read_queue();
                }
            }
            Request::QueueList {
                conversation,
                mut collected,
                mut cursors,
            } if conversation == self.snapshot.view.conversation => {
                if let Some(data) = result["data"].as_array() {
                    collected.extend(data.iter().filter_map(QueuedMessage::parse));
                    if let Some(cursor) = result["nextCursor"]
                        .as_str()
                        .filter(|cursor| !cursor.is_empty())
                    {
                        if cursors.insert(cursor.into())
                            && let Some(thread) = &self.runtime.thread
                        {
                            let thread = thread.id.clone();
                            self.read_queue_page(&thread, Some(cursor.into()), collected, cursors);
                            return Ok(());
                        }
                    } else {
                        let mut ids = HashSet::new();
                        if collected
                            .iter()
                            .all(|message| ids.insert(message.id.as_str()))
                            && *self.snapshot.view.queued != collected
                        {
                            self.snapshot.view.queued = Arc::new(collected);
                        }
                    }
                }
                self.snapshot.view.queue_loading = false;
                self.dirty = true;
            }
            Request::QueueAction {
                conversation,
                action,
            } if conversation == self.snapshot.view.conversation => {
                self.snapshot.view.queue_pending = false;
                match action {
                    QueueAction::Remove if result["deleted"] == false => {
                        self.status("该消息已离开待发队列")
                    }
                    QueueAction::Start => {
                        if let Some(turn_id) = result["turn"]["id"].as_str() {
                            self.native_turn_started(turn_id);
                        }
                    }
                    QueueAction::Resume => {
                        if let Some(thread) = &mut self.runtime.thread
                            && result["thread"]["id"] == thread.id
                        {
                            thread.resumed = true;
                            self.remember_thread_defaults(result);
                        }
                    }
                    QueueAction::Update(id) => {
                        if QueuedMessage::parse(&result["queuedSubmission"])
                            .is_some_and(|item| item.id == id)
                        {
                            self.snapshot.view.queue_edit_saved += 1;
                            self.dirty = true;
                        } else {
                            self.queue_action_failed(
                                &QueueAction::Update(id),
                                "无法确认待发消息已保存，编辑内容已保留".into(),
                            );
                            return Ok(());
                        }
                    }
                    _ => {}
                }
                self.read_queue();
            }
            Request::Thread { attempt, prompt } if self.is_current(attempt) => {
                if let Err(error) = self.install_thread(result) {
                    self.finish(error, ActivityState::Unfinished);
                } else {
                    self.start_turn(prompt);
                }
            }
            Request::Resume { attempt, prompt } if self.is_current(attempt) => {
                if let Some(thread) = &mut self.runtime.thread
                    && Some(thread.id.as_str()) == result["thread"]["id"].as_str()
                {
                    thread.resumed = true;
                    self.remember_thread_defaults(result);
                    self.start_turn(prompt);
                } else {
                    self.finish("Codex 返回了不同的会话 ID", ActivityState::Unfinished);
                }
            }
            Request::Sessions { workspace, more } if workspace == self.snapshot.view.workspace => {
                self.sessions_listed(result, more)
            }
            Request::ProjectSessions {
                project,
                workspaces,
            } => {
                self.project_sessions_listed(&project, &workspaces, result);
            }
            Request::SearchProjects => self.project_search_listed(result),
            Request::Rename { conversation, name } => {
                if let Some(view) = self.conversation_view_mut(conversation) {
                    view.rename_pending = false;
                    view.rename_error.clear();
                }
                self.set_title(conversation, name);
                if conversation == self.snapshot.view.conversation {
                    self.snapshot.fork_error.clear();
                    if self.snapshot.history_loading
                        || !self.snapshot.history_query.trim().is_empty()
                    {
                        self.list_sessions(false);
                    }
                } else {
                    self.manage_status(conversation, "会话已重命名".into());
                }
            }
            Request::SetArchived {
                conversation,
                archived,
            } => {
                if let Some(view) = self.conversation_view_mut(conversation) {
                    view.archive_pending = false;
                    if archived {
                        view.queued = Arc::default();
                        view.queue_loading = false;
                    }
                }
                self.update_archive_state(conversation, archived);
                if archived {
                    self.pending.retain(|_, pending| {
                        !matches!(&pending.kind, Request::QueueList { conversation: owner, .. } if *owner == conversation)
                    });
                }
                let index_update = self
                    .conversation_state(conversation)
                    .and_then(|(_, runtime)| runtime.thread.as_ref())
                    .map(|thread| self.fork_index.archive(&thread.id, archived))
                    .transpose();
                if self.snapshot.view.conversation == conversation {
                    self.snapshot.archive_view = archived;
                    if archived {
                        self.idle_status();
                    } else {
                        self.status("会话已恢复");
                        self.read_queue();
                    }
                    self.list_sessions(false);
                } else {
                    self.manage_status(
                        conversation,
                        if archived {
                            "会话已归档".into()
                        } else {
                            "会话已恢复".into()
                        },
                    );
                }
                if let Err(error) = index_update {
                    self.manage_status(
                        conversation,
                        format!("会话状态已保存，分支索引更新失败（{error}）"),
                    );
                }
            }
            Request::History {
                conversation,
                earlier,
            } if conversation == self.snapshot.view.conversation => {
                self.history_loaded(result, earlier)
            }
            Request::Turn(attempt) if self.is_current(attempt) => {
                let Some(id) = result["turn"]["id"].as_str().filter(|id| !id.is_empty()) else {
                    if self
                        .runtime
                        .turn
                        .as_ref()
                        .is_some_and(|turn| turn.id.is_none())
                    {
                        self.finish("Codex 没有返回消息轮次 ID", ActivityState::Unfinished);
                    }
                    return Ok(());
                };
                if let (Some(thread), Some(turn)) = (&mut self.runtime.thread, &self.runtime.turn) {
                    if !turn.options.model.is_empty() {
                        thread
                            .reported_options
                            .model
                            .clone_from(&turn.options.model);
                    }
                    if !turn.options.effort.is_empty() {
                        thread
                            .reported_options
                            .effort
                            .clone_from(&turn.options.effort);
                    }
                }
                self.set_turn_id(id);
            }
            _ => {}
        }
        Ok(())
    }

    fn is_current(&self, attempt: u64) -> bool {
        self.runtime
            .turn
            .as_ref()
            .is_some_and(|turn| turn.attempt == attempt)
    }

    fn set_turn_id(&mut self, id: &str) {
        if let Some(turn) = &mut self.runtime.turn
            && turn.sent
        {
            turn.id.get_or_insert_with(|| id.to_owned());
            if !id.is_empty() && std::mem::take(&mut self.snapshot.view.prompt_pending) {
                self.snapshot.view.sent_prompts = self.snapshot.view.sent_prompts.wrapping_add(1);
                self.dirty = true;
                self.remember_draft_thread();
            }
            self.interrupt_if_ready();
        }
    }

    fn notification(&mut self, method: &str, params: &Value) {
        if self.runtime.thread.is_none()
            || self
                .runtime
                .thread
                .as_ref()
                .map(|thread| thread.id.as_str())
                != params["threadId"].as_str()
        {
            return;
        }
        if method == "serverRequest/resolved" {
            let before = self.runtime.pending_input.len();
            self.runtime
                .pending_input
                .retain(|pending| pending.request != params["requestId"]);
            if self.runtime.pending_input.len() != before {
                self.show_question();
            }
            return;
        }
        if method == "thread/queue/changed" {
            self.read_queue();
            return;
        }
        if method == "thread/tokenUsage/updated" {
            let measured_turn = params["turnId"].as_str();
            let expected = match &self.runtime.turn {
                Some(turn) => turn.id.as_deref(),
                None => self.runtime.last_finished_turn.as_deref(),
            };
            if measured_turn.is_some()
                && measured_turn == expected
                && let Some(usage) = ContextUsage::parse(&params["tokenUsage"])
                && Some(&usage) != self.snapshot.view.context_usage.as_ref()
            {
                self.snapshot.view.context_usage = Some(usage);
                self.dirty = true;
            }
            return;
        }
        if method == "turn/started"
            && let Some(id) = params["turn"]["id"].as_str()
        {
            self.native_turn_started(id);
            return;
        }
        let turn_id = if method == "turn/completed" {
            params["turn"]["id"].as_str()
        } else {
            params["turnId"].as_str()
        };
        let Some(turn) = self.runtime.turn.as_mut() else {
            return;
        };
        if turn.id.is_none() || turn.id.as_deref() != turn_id {
            return;
        }

        match method {
            "item/completed" if params["item"]["type"] == "exitedReviewMode" => {
                let item = &params["item"];
                if let (Some(id), Some(report)) = (item["id"].as_str(), item["review"].as_str())
                    && !report.trim().is_empty()
                {
                    self.item(id, || Entry::ReviewReport(report.into()));
                }
            }
            "item/completed" if params["item"]["type"] == "contextCompaction" => {
                if let Some(id) = params["item"]["id"].as_str() {
                    self.item(id, || Entry::Compaction);
                }
            }
            "item/started" | "item/completed" if params["item"]["type"] == "userMessage" => {
                self.user_item(&params["item"]);
            }
            "item/agentMessage/delta" | "item/plan/delta" => {
                if let (Some(item), Some(delta)) =
                    (params["itemId"].as_str(), params["delta"].as_str())
                {
                    let kind = if method == "item/plan/delta" {
                        TextItem::Proposal
                    } else {
                        TextItem::Assistant
                    };
                    self.text_item(item, delta, kind, false);
                }
            }
            "item/completed"
                if matches!(
                    params["item"]["type"].as_str(),
                    Some("agentMessage" | "plan")
                ) =>
            {
                if let (Some(item), Some(text)) = (
                    params["item"]["id"].as_str(),
                    params["item"]["text"].as_str(),
                ) {
                    let kind = if params["item"]["type"] == "plan" {
                        TextItem::Proposal
                    } else {
                        TextItem::Assistant
                    };
                    self.text_item(item, text, kind, true);
                }
            }
            "item/started" | "item/completed" if params["item"]["type"] == "commandExecution" => {
                self.command_item(&params["item"], method == "item/completed");
            }
            "item/commandExecution/outputDelta" => {
                if let (Some(id), Some(delta)) =
                    (params["itemId"].as_str(), params["delta"].as_str())
                    && let Some(index) = turn.items.get(id)
                {
                    let entry = &mut self.snapshot.view.entries[*index];
                    if matches!(entry.as_ref(), Entry::Command(command) if command.state == ActivityState::Running && !command.output_truncated)
                        && let Entry::Command(command) = Arc::make_mut(entry)
                    {
                        command.append_output(delta);
                        self.dirty = true;
                    }
                }
            }
            "turn/plan/updated" => self.plan_updated(params),
            "turn/diff/updated" => {
                if let Some(diff) = params["diff"].as_str()
                    && (!diff.is_empty() || turn.diff_index.is_some())
                {
                    let entry = Arc::new(Entry::Changes(TurnDiff::parse(diff)));
                    if let Some(index) = turn.diff_index {
                        self.snapshot.view.entries[index] = entry;
                    } else {
                        turn.diff_index = Some(self.snapshot.view.entries.len());
                        self.snapshot.view.entries.push(entry);
                    }
                    self.dirty = true;
                }
            }
            "turn/completed" => {
                let turn = &params["turn"];
                if let Some(id) = turn["id"].as_str() {
                    self.record_fork_point(id);
                }
                match turn["status"].as_str() {
                    Some("completed") if self.snapshot.view.reviewing => {
                        self.finish(Status::Reviewed, ActivityState::Unfinished)
                    }
                    Some("completed") if self.snapshot.view.compacting => {
                        self.finish(Status::Compacted, ActivityState::Unfinished)
                    }
                    Some("completed") => self.finish(Status::Completed, ActivityState::Unfinished),
                    Some("interrupted") if self.snapshot.view.compacting => {
                        self.finish("已停止压缩", ActivityState::Interrupted)
                    }
                    Some("interrupted") if self.snapshot.view.reviewing => {
                        self.finish("已停止审阅", ActivityState::Interrupted)
                    }
                    Some("interrupted") => self.finish(Status::Stopped, ActivityState::Interrupted),
                    _ => self.finish(
                        format!(
                            "{}失败（{}）",
                            if self.snapshot.view.compacting {
                                "压缩"
                            } else if self.snapshot.view.reviewing {
                                "审阅"
                            } else {
                                "回复"
                            },
                            turn["error"]["message"].as_str().unwrap_or("未完成")
                        ),
                        ActivityState::Unfinished,
                    ),
                }
            }
            _ => {}
        }
    }

    fn item(&mut self, id: &str, create: impl FnOnce() -> Entry) -> Option<&mut Entry> {
        let turn = self.runtime.turn.as_mut()?;
        let index = *turn.items.entry(id.to_owned()).or_insert_with(|| {
            let index = self.snapshot.view.entries.len();
            self.snapshot.view.entries.push(Arc::new(create()));
            index
        });
        self.dirty = true;
        Some(Arc::make_mut(&mut self.snapshot.view.entries[index]))
    }

    fn text_item(&mut self, item: &str, value: &str, kind: TextItem, completed: bool) {
        let entry = self.item(item, || match kind {
            TextItem::Assistant => Entry::Assistant {
                text: String::new(),
                finalized: false,
            },
            TextItem::Proposal => Entry::Proposal {
                text: String::new(),
                finalized: false,
            },
        });
        let (text, finalized) = match (kind, entry) {
            (TextItem::Assistant, Some(Entry::Assistant { text, finalized }))
            | (TextItem::Proposal, Some(Entry::Proposal { text, finalized })) => (text, finalized),
            _ => return,
        };
        if completed {
            text.clear();
            text.push_str(value);
            *finalized = true;
        } else if !*finalized {
            text.push_str(value);
        }
    }

    fn command_item(&mut self, item: &Value, completed: bool) {
        let (Some(id), Some(command)) = (item["id"].as_str(), item["command"].as_str()) else {
            return;
        };
        let Some(Entry::Command(activity)) =
            self.item(id, || Entry::Command(CommandActivity::new(command)))
        else {
            return;
        };
        if completed {
            activity.complete(command, item);
        }
    }

    fn plan_updated(&mut self, params: &Value) {
        let (Some(turn), Some(plan)) = (self.runtime.turn.as_mut(), params["plan"].as_array())
        else {
            return;
        };
        let plan = Arc::new(Entry::Plan(PlanProgress {
            explanation: params["explanation"].as_str().map(str::to_owned),
            steps: plan
                .iter()
                .filter_map(|step| {
                    Some(PlanStep {
                        text: step["step"].as_str()?.to_owned(),
                        state: match step["status"].as_str()? {
                            "pending" => ActivityState::Pending,
                            "inProgress" => ActivityState::Running,
                            "completed" => ActivityState::Completed,
                            _ => return None,
                        },
                    })
                })
                .collect(),
        }));
        if let Some(index) = turn.plan_index {
            self.snapshot.view.entries[index] = plan;
        } else {
            turn.plan_index = Some(self.snapshot.view.entries.len());
            self.snapshot.view.entries.push(plan);
        }
        self.dirty = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hands_off_a_native_thread_and_restores_its_execution_directory_from_the_index()
    -> Result<(), Box<dyn std::error::Error>> {
        let scratch = tempfile::tempdir()?;
        let project = scratch.path().join("project");
        let worktree = scratch.path().join("worktree");
        std::fs::create_dir(&project)?;
        std::fs::create_dir(&worktree)?;
        let project = project.canonicalize()?.to_string_lossy().into_owned();
        let worktree = worktree.canonicalize()?.to_string_lossy().into_owned();
        let state = scratch.path().join("state");
        let index = state.join("forks");
        let mut session = initialized();
        session.fork_index = ThreadIndex::new(Some(index.clone()));
        session.drafts = DraftStore::new(Some(state.clone()));
        session.tabs = TabStore::new(Some(state.clone()));
        session.snapshot.view.workspace = worktree.clone();
        session.snapshot.view.project_workspace = project.clone();
        session.snapshot.conversations[0].workspace = worktree.clone();
        session.snapshot.conversations[0].project_workspace = Some(project.clone());
        session
            .install_thread(&json!({"thread":{"id":"native"}}))
            .unwrap();
        session.remember_draft_thread();
        let conversation = session.snapshot.view.conversation;
        let key = session.snapshot.view.draft_key.clone();
        session.drafts.save(crate::drafts::SavedDraft {
            key: key.clone(),
            workspace: worktree.clone(),
            project: project.clone(),
            content: crate::drafts::DraftContent {
                text: "保留的交接草稿".into(),
                ..Default::default()
            },
        })?;
        session.save_open_tabs(&[conversation], Some(conversation));

        session.input(Input::Handoff {
            conversation,
            original: worktree.clone(),
            workspace: project.clone(),
            worktree: worktree.clone(),
        });
        let request = next(&mut session);
        assert_eq!(request["method"], "thread/settings/update");
        assert_eq!(request["params"]["threadId"], "native");
        assert_eq!(request["params"]["cwd"], project);
        assert!(session.snapshot.view.handoff_pending);
        reply(&mut session, request, json!({}));
        assert_eq!(session.snapshot.view.workspace, project);
        assert_eq!(session.snapshot.view.draft_key, key);
        assert_eq!(
            session.snapshot.view.associated_worktree.as_deref(),
            Some(worktree.as_str())
        );
        assert_eq!(session.tabs.load()?.open[0].workspace, project);
        assert_eq!(session.drafts.records()?[0].workspace, project);

        let mut reopened = Protocol::new(project.clone(), false, Some(index));
        reopened.drain_outbound().for_each(drop);
        reopened.snapshot.connected = true;
        reopened.merge_sessions(
            &[json!({"id":"native","cwd":worktree,"name":"已交接会话"})],
            Some(&worktree),
        );
        let row = reopened
            .snapshot
            .conversations
            .iter()
            .find(|row| row.title == "已交接会话")
            .unwrap();
        assert_eq!(row.workspace, project);
        let restored = reopened.inactive.get(&row.id).unwrap();
        assert_eq!(
            restored.view.associated_worktree.as_deref(),
            Some(worktree.as_str())
        );
        let id = row.id;
        reopened.select_conversation(id, None);
        reopened.drain_outbound().for_each(drop);
        reopened.snapshot.view.restoring = false;
        reopened.snapshot.view.queue_loading = false;
        reopened.input(Input::Handoff {
            conversation: id,
            original: project.clone(),
            workspace: worktree.clone(),
            worktree: worktree.clone(),
        });
        let resume = next(&mut reopened);
        assert_eq!(resume["method"], "thread/resume");
        assert_eq!(resume["params"]["cwd"], worktree);
        reply(
            &mut reopened,
            resume,
            json!({"thread":{"id":"native","cwd":project},"cwd":worktree}),
        );
        reopened.input(Input::Send("从工作树继续".into()));
        let turn = next(&mut reopened);
        assert_eq!(turn["method"], "turn/start");
        assert_eq!(turn["params"]["threadId"], "native");
        assert_eq!(turn["params"]["cwd"], worktree);
        assert_eq!(turn["params"]["runtimeWorkspaceRoots"], json!([worktree]));
        Ok(())
    }

    #[test]
    fn starting_in_a_worktree_moves_the_draft_and_uses_its_directory_for_the_first_turn()
    -> Result<(), Box<dyn std::error::Error>> {
        let fixture = tempfile::tempdir()?;
        let source = fixture.path().join("local");
        let target = fixture.path().join("isolated");
        std::fs::create_dir(&source)?;
        std::fs::create_dir(&target)?;
        let source = source.canonicalize()?.to_string_lossy().into_owned();
        let target = target.canonicalize()?.to_string_lossy().into_owned();
        let state = fixture.path().join("profile");
        let mut session = initialized();
        session.snapshot.view.workspace = source.clone();
        session.snapshot.view.project_workspace = source.clone();
        session.snapshot.conversations[0].workspace = source.clone();
        session.drafts = DraftStore::new(Some(state.clone()));
        session.tabs = TabStore::new(Some(state));
        let id = session.snapshot.view.conversation;
        let draft = session.snapshot.view.draft_key.clone();
        let prompt = Prompt {
            text: "工作树中的第一条消息".into(),
            attachments: vec![crate::prompt::Attachment {
                path: Path::new(&source)
                    .join("note.md")
                    .to_string_lossy()
                    .into_owned(),
                content: crate::prompt::AttachmentContent::Text("选中的项目说明".into()),
            }],
        };
        session.drafts.save(crate::drafts::SavedDraft {
            key: draft.clone(),
            workspace: source.clone(),
            project: source.clone(),
            content: crate::drafts::DraftContent {
                text: prompt.text.clone(),
                attachments: prompt.attachments.clone(),
                ..crate::drafts::DraftContent::default()
            },
        })?;
        session.save_open_tabs(&[id], Some(id));

        session.input(Input::SendInWorktree {
            conversation: id,
            original: source.clone(),
            workspace: target.clone(),
            prompt: prompt.clone(),
        });
        let start = next(&mut session);
        assert_eq!(start["method"], "thread/start");
        assert_eq!(start["params"]["cwd"], target);
        assert_eq!(session.snapshot.view.conversation, id);
        assert_eq!(session.snapshot.project_workspace(), source);
        assert_eq!(session.snapshot.conversations[0].workspace, target);
        assert_eq!(session.drafts.records()?[0].workspace, target);
        assert_eq!(session.tabs.load()?.open[0].workspace, target);
        reply(
            &mut session,
            start,
            json!({"thread":{"id":"worktree-thread"}}),
        );
        let turn = next(&mut session);
        assert_eq!(turn["method"], "turn/start");
        assert_eq!(turn["params"]["input"], json!(prompt.input()));
        reply(&mut session, turn, json!({"turn":{"id":"first"}}));
        assert_eq!(
            session.snapshot.view.draft_key,
            DraftKey::Thread("worktree-thread".into())
        );
        assert_eq!(session.drafts.records()?[0].workspace, target);
        assert_eq!(
            session.tabs.load()?.open[0].key,
            DraftKey::Thread("worktree-thread".into())
        );
        Ok(())
    }

    #[test]
    fn restarting_restores_distinct_local_drafts_and_migrates_a_started_thread()
    -> std::io::Result<()> {
        let temporary = tempfile::tempdir()?;
        let workspace = temporary
            .path()
            .canonicalize()?
            .to_string_lossy()
            .into_owned();
        let directory = Some(temporary.path().join("profile"));
        let store = DraftStore::new(directory.clone());
        let first = DraftKey::local();
        let second = DraftKey::local();
        for (key, text) in [
            (first.clone(), "先写的草稿"),
            (second.clone(), "后写的草稿"),
        ] {
            store.save(crate::drafts::SavedDraft {
                key,
                workspace: workspace.clone(),
                project: workspace.clone(),
                content: crate::drafts::DraftContent {
                    text: text.into(),
                    attachments: Vec::new(),
                    ..crate::drafts::DraftContent::default()
                },
            })?;
        }
        let mut session = Protocol::new(workspace, false, None);
        session.restore_workspace_state(directory, false);
        assert_eq!(session.snapshot.view.draft_key, second);
        assert_eq!(session.snapshot.conversations[0].title, "后写的草稿");
        assert_eq!(session.snapshot.conversations[1].title, "先写的草稿");
        let id = session.snapshot.conversations[1].id;
        session.select_conversation(id, None);
        assert_eq!(session.snapshot.view.draft_key, first);
        session
            .install_thread(&json!({"thread": {"id": "existing-thread"}}))
            .unwrap();
        assert_eq!(session.snapshot.view.draft_key, first);
        session.drain_outbound().for_each(drop);
        session.snapshot.connected = true;
        session.input(Input::Send("先写的草稿".into()));
        let request = next(&mut session);
        assert_eq!(request["method"], "turn/start");
        assert_eq!(session.snapshot.view.draft_key, first);
        reply(&mut session, request, json!({"turn": {"id": "first-turn"}}));
        let thread = DraftKey::Thread("existing-thread".into());
        assert_eq!(session.snapshot.view.draft_key, thread);
        let records = store.records()?;
        assert_eq!(records[0].key, second);
        assert_eq!(records[1].key, thread);
        assert_eq!(records[1].content.text, "先写的草稿");
        Ok(())
    }

    fn next(session: &mut Protocol) -> Value {
        let mut frames: Vec<_> = session.drain_outbound().collect();
        // Legacy fixtures do not have queued messages. Resolve background reads
        // alongside the operation the fixture is testing.
        if frames.len() > 1
            && let Some(index) = frames
                .iter()
                .position(|frame| frame["method"] == "thread/queue/list")
        {
            reply(session, frames.remove(index), json!({"data":[]}));
        }
        assert_eq!(frames.len(), 1);
        frames.into_iter().next().unwrap()
    }

    fn reply(session: &mut Protocol, request: Value, value: Value) {
        session
            .server(json!({"id":request["id"],"result":value}))
            .unwrap();
    }

    fn initialized() -> Protocol {
        initialized_with_access(false)
    }

    #[test]
    fn skill_catalog_is_scoped_and_invalidation_drops_late_replies() {
        let mut session = initialized();
        session.input(Input::LoadSkills("/other".into()));
        assert!(session.drain_outbound().next().is_none());
        session.input(Input::LoadSkills("/workspace".into()));
        let obsolete = next(&mut session);
        assert_eq!(obsolete["method"], "skills/list");
        assert_eq!(obsolete["params"]["cwds"], json!(["/workspace"]));
        session.input(Input::LoadSkills("/workspace".into()));
        assert!(session.drain_outbound().next().is_none());

        session.server(json!({"method":"skills/changed"})).unwrap();
        assert!(session.snapshot.skills.is_none());
        session.input(Input::LoadSkills("/workspace".into()));
        let current = next(&mut session);
        reply(&mut session, obsolete, json!({"data": []}));
        assert!(matches!(
            session.snapshot.skills.as_ref().unwrap().state,
            SkillCatalogState::Loading
        ));
        let prefix = if cfg!(windows) { "C:" } else { "" };
        let project_skill = format!("{prefix}/workspace/ui/SKILL.md");
        let user_skill = format!("{prefix}/user/ui/SKILL.md");
        reply(
            &mut session,
            current,
            json!({"data":[
                {"cwd":"/different","skills":[]},
                {"cwd":"/workspace","skills":[
                    {"name":"check-ui","path":project_skill,"enabled":true,
                        "interface":{"shortDescription":"Verify the active UI"}},
                    {"name":"check-ui","path":user_skill,"enabled":true},
                    {"name":"hidden","path":"/user/hidden/SKILL.md","enabled":false},
                    {"name":"invalid","path":"../SKILL.md","enabled":true},
                    {"name":"duplicate","path":project_skill,"enabled":true}
                ]}
            ]}),
        );
        let SkillCatalogState::Ready(skills) = &session.snapshot.skills.as_ref().unwrap().state
        else {
            panic!("expected working-directory skills");
        };
        assert_eq!(skills.len(), 2);
        assert_eq!(skills[0].path, user_skill);
        assert_eq!(skills[1].path, project_skill);
        assert_eq!(skills[1].description, "Verify the active UI");

        session.input(Input::LoadSkills("/workspace".into()));
        assert!(session.drain_outbound().next().is_none());
        session.server(json!({"method":"skills/changed"})).unwrap();
        session.input(Input::LoadSkills("/workspace".into()));
        let malformed = next(&mut session);
        reply(&mut session, malformed, json!({"data":[]}));
        assert!(matches!(
            session.snapshot.skills.as_ref().unwrap().state,
            SkillCatalogState::Failed(_)
        ));
        session.input(Input::LoadSkills("/workspace".into()));
        assert_eq!(next(&mut session)["method"], "skills/list");
    }

    #[test]
    fn rejected_turn_does_not_acknowledge_the_composer() {
        let mut session = initialized();
        session.input(Input::Send("保留这条草稿".into()));
        assert!(session.snapshot.view.prompt_pending);
        assert_eq!(session.snapshot.view.sent_prompts, 0);
        let create = next(&mut session);
        reply(&mut session, create, json!({"thread": {"id": "thread"}}));
        let send = next(&mut session);
        assert_eq!(send["method"], "turn/start");
        assert!(session.snapshot.view.prompt_pending);
        session
            .server(json!({"id": send["id"], "error": {"message": "denied"}}))
            .unwrap();
        assert!(!session.snapshot.view.prompt_pending);
        assert_eq!(session.snapshot.view.sent_prompts, 0);
        assert!(matches!(
            session.snapshot.view.draft_key,
            DraftKey::Local(_)
        ));
        session.input(Input::Send("重试".into()));
        let send = next(&mut session);
        reply(&mut session, send, json!({"turn": {"id": "accepted"}}));
        assert!(!session.snapshot.view.prompt_pending);
        assert_eq!(session.snapshot.view.sent_prompts, 1);
    }

    fn initialized_with_access(workspace_write: bool) -> Protocol {
        let (mut session, list) = initial_listing(workspace_write);
        reply(&mut session, list, json!({"data":[]}));
        session
    }

    fn initial_listing(workspace_write: bool) -> (Protocol, Value) {
        let mut session = Protocol::new("/workspace".into(), workspace_write, None);
        let initialize = next(&mut session);
        assert_eq!(initialize["method"], "initialize");
        reply(&mut session, initialize, json!({}));
        let mut outbound = session.drain_outbound().collect::<Vec<_>>().into_iter();
        assert_eq!(outbound.next().unwrap()["method"], "initialized");
        let list = outbound.next().unwrap();
        assert_eq!(list["method"], "thread/list");
        assert_eq!(list["params"]["cwd"], "/workspace");
        let models = outbound.next().unwrap();
        assert_eq!(models["method"], "model/list");
        reply(&mut session, models, json!({"data":[]}));
        let modes = outbound.next().unwrap();
        assert_eq!(modes["method"], "collaborationMode/list");
        reply(&mut session, modes, json!({"data":[]}));
        assert_eq!(outbound.next(), None);
        (session, list)
    }

    #[test]
    fn background_project_pages_keep_the_active_search_and_load_worktree_threads() {
        let (mut session, active_listing) = initial_listing(false);
        let project = "/another-project";
        let worktree = "/worktrees/another-project-branch";
        let index = tempfile::tempdir().unwrap();
        session.fork_index = ThreadIndex::new(Some(index.path().to_owned()));
        session
            .fork_index
            .remember("linked", worktree, false, Some(project))
            .unwrap();
        session.snapshot.projects = Arc::new(vec!["/workspace".into(), project.into()]);
        session.snapshot.history_query = "keep this query".into();
        session.snapshot.history_matches.insert(0);

        session.input(Input::ProjectHistory {
            path: project.into(),
            more: false,
        });
        let first = next(&mut session);
        assert_eq!(first["method"], "thread/list");
        assert_eq!(first["params"]["cwd"], json!([project, worktree]));
        assert!(session.snapshot.project_histories[project].loading);
        session.input(Input::ProjectHistory {
            path: project.into(),
            more: false,
        });
        assert!(session.drain_outbound().next().is_none());
        reply(
            &mut session,
            first,
            json!({"data":[
            {"id":"linked","name":"Worktree implementation","cwd":worktree},
            {"id":"outside","name":"Wrong project","cwd":"/different"},
        ],"nextCursor":"second-project-page"}),
        );
        let row = session
            .snapshot
            .conversations
            .iter()
            .find(|row| row.title == "Worktree implementation")
            .unwrap();
        assert_eq!(row.project(), project);
        let selected = row.id;
        assert!(
            !session
                .snapshot
                .conversations
                .iter()
                .any(|row| row.title == "Wrong project")
        );
        assert_eq!(session.snapshot.view.workspace, "/workspace");
        assert_eq!(session.snapshot.history_query, "keep this query");
        assert_eq!(session.snapshot.history_matches, [0].into_iter().collect());
        assert!(
            session
                .pending
                .contains_key(&active_listing["id"].as_u64().unwrap())
        );
        assert!(session.snapshot.project_histories[project].more());

        session.input(Input::ProjectHistory {
            path: project.into(),
            more: true,
        });
        let second = next(&mut session);
        assert_eq!(second["params"]["cursor"], "second-project-page");
        assert_eq!(second["params"]["cwd"], json!([project, worktree]));
        reply(
            &mut session,
            second,
            json!({"data":[
            {"id":"ordinary","name":"Project history","cwd":project},
        ],"nextCursor":null}),
        );
        assert!(!session.snapshot.project_histories[project].more());
        assert_eq!(session.snapshot.view.workspace, "/workspace");
        session.input(Input::Select(selected));
        assert_eq!(session.snapshot.view.workspace, worktree);
        assert_eq!(session.snapshot.project_workspace(), project);
    }

    #[test]
    fn cross_project_search_ignores_stale_results_and_pages_archived_worktrees() {
        let (mut session, active_listing) = initial_listing(false);
        let project = "/other";
        let worktree = "/worktrees/branch";
        let index = tempfile::tempdir().unwrap();
        session.fork_index = ThreadIndex::new(Some(index.path().to_owned()));
        session
            .fork_index
            .remember("linked", worktree, true, Some(project))
            .unwrap();
        session.snapshot.projects = Arc::new(vec!["/workspace".into(), project.into()]);
        session.snapshot.history_query = "active query".into();

        session.input(Input::ProjectSearch {
            query: "old".into(),
            archived: false,
            more: false,
        });
        let old = next(&mut session);
        session.input(Input::ProjectSearch {
            query: " build ".into(),
            archived: true,
            more: false,
        });
        let new = next(&mut session);
        assert_eq!(new["params"]["searchTerm"], "build");
        assert_eq!(new["params"]["archived"], true);
        assert_eq!(
            new["params"]["cwd"],
            json!([project, "/workspace", worktree])
        );
        reply(
            &mut session,
            old,
            json!({"data":[{"id":"old","cwd":project,"name":"old result"}]}),
        );
        assert!(session.snapshot.project_search.loading);
        assert!(session.snapshot.project_search.matches.is_empty());
        assert!(
            !session
                .snapshot
                .conversations
                .iter()
                .any(|row| row.title == "old result")
        );
        reply(
            &mut session,
            new,
            json!({
                "data":[
                    {"id":"linked","cwd":worktree,"name":"build in worktree"},
                    {"id":"outside","cwd":"/unregistered","name":"outside"},
                ],
                "nextCursor":"page-2"
            }),
        );
        assert!(!session.snapshot.project_search.loading);
        let ids = session.snapshot.project_search.matches.clone();
        assert_eq!(ids.len(), 1);
        assert_eq!(session.snapshot.view.workspace, "/workspace");
        assert_eq!(session.snapshot.history_query, "active query");
        assert!(
            session
                .pending
                .contains_key(&active_listing["id"].as_u64().unwrap())
        );
        assert!(session.snapshot.project_search.more());
        session.input(Input::ProjectSearch {
            query: "build".into(),
            archived: true,
            more: true,
        });
        let page = next(&mut session);
        assert_eq!(page["params"]["cursor"], "page-2");
        assert_eq!(
            page["params"]["cwd"],
            json!([project, "/workspace", worktree])
        );
        reply(
            &mut session,
            page,
            json!({"data":[
                {"id":"linked","cwd":worktree,"name":"build in worktree"},
                {"id":"second","cwd":project,"name":"second build"},
            ]}),
        );
        assert_eq!(session.snapshot.project_search.matches.len(), 2);
        assert_eq!(session.snapshot.project_search.matches[0], ids[0]);
        assert!(!session.snapshot.project_search.more());
        session.input(Input::Select(ids[0]));
        assert_eq!(session.snapshot.view.workspace, worktree);
        assert_eq!(session.snapshot.project_workspace(), project);
        assert!(session.snapshot.current_archived());
    }

    #[test]
    fn managing_a_parked_thread_preserves_active_work_and_history_scope() {
        let (mut session, active_request) = initial_listing(false);
        let project = "/other";
        session.snapshot.projects = Arc::new(vec!["/workspace".into(), project.into()]);
        session.snapshot.history_query = "foreground query".into();
        session.snapshot.view.status = "foreground status".into();
        session.input(Input::ProjectHistory {
            path: project.into(),
            more: false,
        });
        let list = next(&mut session);
        reply(
            &mut session,
            list,
            json!({"data":[{"id":"parked","cwd":project,"name":"Old name"}]}),
        );
        let id = session.conversation_for_thread("parked").unwrap();
        session.input(Input::RenameConversation {
            conversation: id,
            name: "Changed name".into(),
        });
        let rename = next(&mut session);
        assert_eq!(rename["params"]["threadId"], "parked");
        assert!(session.inactive[&id].view.rename_pending);
        assert!(!session.snapshot.view.rename_pending);
        session
            .server(json!({"id":rename["id"],"error":{"message":"retry this"}}))
            .unwrap();
        assert!(
            session.inactive[&id]
                .view
                .rename_error
                .contains("retry this")
        );
        assert!(session.snapshot.view.rename_error.is_empty());
        session.input(Input::RenameConversation {
            conversation: id,
            name: "Changed name".into(),
        });
        let rename = next(&mut session);
        reply(&mut session, rename, json!({}));
        assert_eq!(
            session
                .snapshot
                .conversations
                .iter()
                .find(|row| row.id == id)
                .unwrap()
                .title,
            "Changed name"
        );
        assert!(session.inactive[&id].view.rename_error.is_empty());
        assert!(session.drain_outbound().next().is_none());

        session.input(Input::SetPinned {
            conversation: id,
            pinned: true,
        });
        assert!(session.is_pinned("parked"));
        session.input(Input::SetConversationArchived {
            conversation: id,
            archived: true,
        });
        let archive = next(&mut session);
        assert_eq!(archive["method"], "thread/archive");
        assert_eq!(archive["params"]["threadId"], "parked");
        assert!(session.inactive[&id].view.archive_pending);
        assert!(!session.snapshot.view.archive_pending);
        reply(&mut session, archive, json!({}));
        assert!(
            session
                .snapshot
                .conversations
                .iter()
                .find(|row| row.id == id)
                .unwrap()
                .archived
        );
        assert!(
            session
                .snapshot
                .conversations
                .iter()
                .find(|row| row.id == id)
                .unwrap()
                .pinned
        );
        assert!(!session.snapshot.archive_view);
        assert!(!session.inactive[&id].view.archive_pending);
        session.input(Input::SetConversationArchived {
            conversation: id,
            archived: false,
        });
        let restore = next(&mut session);
        assert_eq!(restore["method"], "thread/unarchive");
        reply(&mut session, restore, json!({}));
        assert!(
            !session
                .snapshot
                .conversations
                .iter()
                .find(|row| row.id == id)
                .unwrap()
                .archived
        );
        assert_eq!(session.snapshot.sidebar_status, "会话已恢复");
        assert_eq!(session.snapshot.view.conversation, 0);
        assert_eq!(session.snapshot.view.workspace, "/workspace");
        assert_eq!(
            session.snapshot.view.status,
            Status::from("foreground status")
        );
        assert_eq!(session.snapshot.history_query, "foreground query");
        assert!(
            session
                .pending
                .contains_key(&active_request["id"].as_u64().unwrap())
        );
        assert!(session.drain_outbound().next().is_none());
    }

    fn resolve_listing(session: &mut Protocol, cwd: &str) {
        let list = next(session);
        assert_eq!(list["method"], "thread/list");
        assert_eq!(list["params"]["cwd"], cwd);
        reply(session, list, json!({"data":[]}));
    }

    #[test]
    fn navigation_tracks_confirmed_visits_across_projects_and_discards_the_forward_branch() {
        let mut session = initialized();
        let original = session.snapshot.view.conversation;
        let first = session.take_snapshot().unwrap();
        assert!(!first.navigation_back && !first.navigation_forward);

        session.input(Input::Workspace("/second".into()));
        resolve_listing(&mut session, "/second");
        let project = session.snapshot.view.conversation;
        session.input(Input::Reset);
        let newer = session.snapshot.view.conversation;
        assert_ne!(project, newer);
        assert!(session.take_snapshot().unwrap().navigation_back);

        session.input(Input::Navigate { forward: false });
        assert_eq!(session.snapshot.view.conversation, project);
        let reached = session.take_snapshot().unwrap();
        assert!(reached.navigation_back && reached.navigation_forward);
        session.input(Input::Navigate { forward: false });
        resolve_listing(&mut session, "/workspace");
        assert_eq!(session.snapshot.view.conversation, original);
        let back = session.take_snapshot().unwrap();
        assert!(!back.navigation_back && back.navigation_forward);

        session.input(Input::Navigate { forward: true });
        resolve_listing(&mut session, "/second");
        assert_eq!(session.snapshot.view.conversation, project);
        session.input(Input::Reset);
        let another = session.take_snapshot().unwrap();
        assert!(another.navigation_back && !another.navigation_forward);
        assert_ne!(another.view.conversation, newer);
    }

    #[test]
    fn returning_to_an_archived_visit_switches_the_sidebar_in_the_same_project() {
        let (mut session, listing) = initial_listing(false);
        reply(
            &mut session,
            listing,
            json!({"data":[{"id":"saved","cwd":"/workspace"}]}),
        );
        let saved = session.snapshot.conversations[1].id;
        session.input(Input::Select(saved));
        let history = next(&mut session);
        reply(&mut session, history, json!({"data":[]}));
        session.input(Input::Reset);
        assert!(!session.snapshot.archive_view);
        session
            .server(json!({"method":"thread/archived","params":{"threadId":"saved"}}))
            .unwrap();

        session.input(Input::Navigate { forward: false });
        assert_eq!(session.snapshot.view.conversation, saved);
        assert!(session.snapshot.current_archived());
        assert!(session.snapshot.archive_view);
        let archived_list = next(&mut session);
        assert_eq!(archived_list["params"]["archived"], true);
        reply(
            &mut session,
            archived_list,
            json!({"data":[{"id":"saved","cwd":"/workspace"}]}),
        );
        assert!(session.take_snapshot().unwrap().navigation_forward);
        session.input(Input::Navigate { forward: true });
        assert!(!session.snapshot.archive_view);
        assert!(!session.snapshot.current_archived());
    }

    #[test]
    fn marking_activity_read_preserves_background_questions_and_running_turns() {
        let mut session = initialized();
        session.input(Input::Workspace("/waiting".into()));
        resolve_listing(&mut session, "/waiting");
        let waiting = session.snapshot.view.conversation;
        session.snapshot.view.busy = true;
        session.snapshot.view.question = Some(Arc::new(QuestionPage {
            token: 1,
            index: 0,
            total: 1,
            header: "确认".into(),
            text: "继续吗？".into(),
            options: Vec::new(),
            allows_text: true,
            secret: false,
        }));
        session.input(Input::Workspace("/front".into()));
        resolve_listing(&mut session, "/front");
        session.snapshot.view.busy = true;
        session.inactive.get_mut(&0).unwrap().view.unread = true;

        session.input(Input::MarkActivityRead);
        let snapshot = session.take_snapshot().unwrap();
        let activity = |id| {
            snapshot
                .conversations
                .iter()
                .find(|row| row.id == id)
                .unwrap()
                .activity
        };
        assert_eq!(activity(0), ConversationActivity::Idle);
        assert_eq!(activity(waiting), ConversationActivity::NeedsInput);
        assert_eq!(
            activity(snapshot.view.conversation),
            ConversationActivity::Running
        );
        assert_eq!(
            session.inactive[&waiting]
                .view
                .question
                .as_ref()
                .unwrap()
                .token,
            1
        );

        session.with_conversation(waiting, |session| {
            session.snapshot.view.question = None;
            session.snapshot.view.busy = false;
            session.dirty = true;
        });
        let snapshot = session.take_snapshot().unwrap();
        assert_eq!(
            snapshot
                .conversations
                .iter()
                .find(|row| row.id == waiting)
                .unwrap()
                .activity,
            ConversationActivity::Updated
        );
    }

    #[test]
    fn pins_a_saved_thread_and_loads_it_after_restart_outside_the_first_page()
    -> Result<(), Box<dyn std::error::Error>> {
        let state = tempfile::tempdir()?;
        let pinned = json!({"id":"old-thread","cwd":"/workspace","name":"老会话"});
        let (mut first, page) = initial_listing(false);
        first.restore_workspace_state(Some(state.path().to_owned()), false);
        reply(&mut first, page, json!({"data":[pinned]}));
        let original = first.snapshot.conversations[1].id;
        first.input(Input::Select(original));
        let history = next(&mut first);
        assert_eq!(history["params"]["threadId"], "old-thread");
        reply(&mut first, history, json!({"data":[]}));
        first.input(Input::SetPinned {
            conversation: original,
            pinned: true,
        });
        assert!(first.snapshot.conversations[1].pinned);
        assert!(first.drain_outbound().next().is_none());

        let (mut reopened, page) = initial_listing(false);
        reopened.restore_workspace_state(Some(state.path().to_owned()), false);
        reply(
            &mut reopened,
            page,
            json!({
                "data":[{"id":"recent","cwd":"/workspace","name":"新会话"}],
                "nextCursor":"second-page"
            }),
        );
        let lookup = next(&mut reopened);
        assert_eq!(lookup["method"], "thread/read");
        assert_eq!(
            lookup["params"],
            json!({"threadId":"old-thread","includeTurns":false})
        );
        reply(&mut reopened, lookup, json!({"thread":pinned}));
        let row = reopened.snapshot.conversations.last().unwrap();
        assert_ne!(row.id, original);
        assert!(row.pinned && reopened.snapshot.shows_conversation(row));
        assert_eq!(row.title, "老会话");
        let id = row.id;

        reopened.input(Input::History { more: true });
        let later = next(&mut reopened);
        reply(&mut reopened, later, json!({"data":[pinned]}));
        assert_eq!(reopened.snapshot.conversations.len(), 3);
        reopened.input(Input::Select(id));
        let history = next(&mut reopened);
        assert_eq!(history["params"]["threadId"], "old-thread");
        reply(&mut reopened, history, json!({"data":[]}));

        reopened.input(Input::SetPinned {
            conversation: original,
            pinned: false,
        });
        assert!(reopened.snapshot.conversations.last().unwrap().pinned);
        reopened.input(Input::SetPinned {
            conversation: id,
            pinned: false,
        });
        assert!(!reopened.snapshot.conversations.last().unwrap().pinned);
        assert!(reopened.pin_index.records("/workspace", false)?.is_empty());
        Ok(())
    }

    #[test]
    fn archive_events_move_pins_out_of_the_active_index_and_discard_stale_lookups()
    -> Result<(), Box<dyn std::error::Error>> {
        let state = tempfile::tempdir()?;
        let (mut session, page) = initial_listing(false);
        session.restore_workspace_state(Some(state.path().to_owned()), false);
        session
            .pin_index
            .remember("saved", "/worktree", false, Some("/workspace"))?;
        assert!(session.pin_index.records("/other", false)?.is_empty());
        reply(&mut session, page, json!({"data":[]}));
        let stale = next(&mut session);
        session.server(json!({"method":"thread/archived","params":{"threadId":"saved"}}))?;
        let thread = json!({"id":"saved","cwd":"/worktree","name":"Pinned worktree"});
        reply(&mut session, stale, json!({"thread":thread}));
        assert!(session.pin_index.records("/workspace", false)?.is_empty());
        assert!(session.conversation_for_thread("saved").is_none());

        session.input(Input::ArchiveView(true));
        let page = next(&mut session);
        reply(&mut session, page, json!({"data":[]}));
        let lookup = next(&mut session);
        reply(&mut session, lookup, json!({"thread":thread}));
        let row = session.snapshot.conversations.last().unwrap();
        assert!(row.archived && row.pinned);
        assert_eq!(row.project(), "/workspace");
        session.server(json!({"method":"thread/unarchived","params":{"threadId":"saved"}}))?;
        assert!(!session.snapshot.conversations.last().unwrap().archived);
        assert_eq!(session.pin_index.records("/workspace", false)?.len(), 1);
        Ok(())
    }

    #[test]
    fn streams_a_thread_and_interrupts_even_before_the_turn_response() {
        let mut session = initialized();
        session.input(Input::Send("检查布局".into()));
        let thread = next(&mut session);
        assert_eq!(thread["method"], "thread/start");
        assert_eq!(thread["params"]["cwd"], "/workspace");
        assert_eq!(thread["params"]["sandbox"], "read-only");
        assert_eq!(thread["params"]["approvalPolicy"], "never");
        reply(&mut session, thread, json!({"thread":{"id":"t1"}}));
        let start = next(&mut session);
        assert_eq!(start["params"]["input"][0]["text"], "检查布局");
        session.input(Input::Interrupt(session.snapshot.view.conversation));
        assert!(session.drain_outbound().next().is_none());
        session
            .server(json!({
                "method":"turn/started","params":{"threadId":"t1","turn":{"id":"a"}}
            }))
            .unwrap();
        let interrupt = next(&mut session);
        assert_eq!(interrupt["method"], "turn/interrupt");
        assert_eq!(interrupt["params"]["turnId"], "a");

        for (turn, delta) in [("old", "旧消息"), ("a", "流"), ("a", "式")] {
            session
                .server(json!({
                    "method":"item/agentMessage/delta",
                    "params":{"threadId":"t1","turnId":turn,"itemId":"i1","delta":delta},
                }))
                .unwrap();
        }
        session.server(json!({
            "method":"item/completed",
            "params":{"threadId":"t1","turnId":"a","item":{"id":"i1","type":"agentMessage","text":"流式"}},
        })).unwrap();
        session.server(json!({
            "method":"turn/completed","params":{"threadId":"t1","turn":{"id":"a","status":"interrupted"}},
        })).unwrap();
        reply(&mut session, start, json!({"turn":{"id":"a"}}));
        reply(&mut session, interrupt, json!({}));
        let state = session.take_snapshot().unwrap();
        assert_eq!(state.view.entries.len(), 2);
        assert!(
            matches!(state.view.entries[1].as_ref(), Entry::Assistant {text, ..} if text == "流式")
        );
        assert!(!state.view.busy);
        assert_eq!(state.view.status, Status::Stopped);
        assert_eq!(state.view.fork_points[0].response, Some(1));

        session.input(Input::Send("继续".into()));
        let second = next(&mut session);
        assert_eq!(second["method"], "turn/start");
        assert_eq!(second["params"]["threadId"], "t1");
    }

    #[test]
    fn cancels_a_pending_thread_and_declines_an_unsolicited_approval() {
        let mut session = initialized();
        session.input(Input::Send("first".into()));
        let first = next(&mut session);
        session.input(Input::Interrupt(session.snapshot.view.conversation));
        assert!(session.pending.is_empty());
        session.input(Input::Workspace("/replacement".into()));
        resolve_listing(&mut session, "/replacement");
        reply(&mut session, first, json!({"thread":{"id":"stale"}}));
        assert!(session.drain_outbound().next().is_none());
        session.input(Input::Send("second".into()));
        assert_eq!(next(&mut session)["params"]["cwd"], "/replacement");

        session
            .server(json!({"id":"approval-1","method":"item/fileChange/requestApproval"}))
            .unwrap();
        assert_eq!(
            next(&mut session),
            json!({
                "jsonrpc":"2.0","id":"approval-1","result":{"decision":"decline"}
            })
        );
    }
    #[test]
    fn switching_conversations_preserves_workspace_messages_and_thread() {
        let mut session = initialized();
        session.input(Input::Send("Inspect  layout".into()));
        let thread = next(&mut session);
        reply(&mut session, thread, json!({"thread":{"id":"original"}}));
        let turn = next(&mut session);
        reply(&mut session, turn, json!({"turn":{"id":"done"}}));
        session.server(json!({
            "method":"turn/completed","params":{"threadId":"original","turn":{"id":"done","status":"completed"}}
        })).unwrap();
        assert_eq!(session.snapshot.conversations[0].title, "Inspect layout");

        session.input(Input::Workspace("/another".into()));
        resolve_listing(&mut session, "/another");
        let alternate = session.snapshot.view.conversation;
        assert_ne!(alternate, 0);
        assert!(session.snapshot.view.entries.is_empty());
        assert_eq!(
            session.snapshot.projects.as_ref(),
            &["/another", "/workspace"]
        );
        assert!(
            !session
                .snapshot
                .shows_conversation(&session.snapshot.conversations[1])
        );
        session.input(Input::ArchiveView(true));
        resolve_listing(&mut session, "/another");
        session.input(Input::Search("missing".into()));
        let stale_search = next(&mut session);
        assert_eq!(stale_search["params"]["cwd"], "/another");
        session.input(Input::Workspace("/workspace".into()));
        resolve_listing(&mut session, "/workspace");
        reply(
            &mut session,
            stale_search,
            json!({"data":[{"id":"stale","cwd":"/another"}]}),
        );
        assert_eq!(session.snapshot.view.workspace, "/workspace");
        assert_eq!(session.snapshot.view.conversation, 0);
        assert!(!session.snapshot.archive_view);
        assert!(session.snapshot.history_query.is_empty());
        assert_eq!(session.snapshot.conversations.len(), 2);
        assert_eq!(
            session.snapshot.projects.as_ref(),
            &["/workspace", "/another"]
        );
        assert!(
            !session
                .snapshot
                .shows_conversation(&session.snapshot.conversations[0])
        );
        assert!(
            matches!(session.snapshot.view.entries[0].as_ref(), Entry::User(message) if message.prompt_text() == "Inspect  layout")
        );
        session.input(Input::Send("Continue".into()));
        let continued = next(&mut session);
        assert_eq!(continued["method"], "turn/start");
        assert_eq!(continued["params"]["threadId"], "original");
        session.input(Input::Select(alternate));
        assert_eq!(session.snapshot.view.conversation, alternate);
        assert!(session.inactive[&0].view.busy);
    }

    #[test]
    fn updates_activity_in_place_and_keeps_the_final_command_output() {
        let mut session = initialized();
        session.input(Input::Send("检查例子".into()));
        let thread = next(&mut session);
        reply(&mut session, thread, json!({"thread":{"id":"t"}}));
        let turn = next(&mut session);
        reply(&mut session, turn, json!({"turn":{"id":"a"}}));

        for (method, payload) in [
            (
                "turn/plan/updated",
                json!({
                    "explanation":"先看构建入口",
                    "plan":[{"step":"查看示例","status":"inProgress"}]
                }),
            ),
            (
                "item/started",
                json!({
                    "item":{"type":"commandExecution","id":"cmd","command":"ls examples","status":"inProgress"}
                }),
            ),
            (
                "item/commandExecution/outputDelta",
                json!({"itemId":"cmd","delta":"临时结果\n"}),
            ),
        ] {
            event(&mut session, "a", method, payload);
        }
        let running = session.take_snapshot().unwrap();
        assert_eq!(running.view.entries.len(), 3);
        assert!(
            matches!(running.view.entries[2].as_ref(), Entry::Command(command)
            if command.state == ActivityState::Running && command.output == "临时结果\n")
        );

        for (turn, method, payload) in [
            (
                "old",
                "item/commandExecution/outputDelta",
                json!({"itemId":"cmd","delta":"过期输出"}),
            ),
            (
                "a",
                "item/completed",
                json!({"item":{
                    "type":"commandExecution","id":"cmd","command":"ls examples",
                    "status":"completed","exitCode":2,"aggregatedOutput":"最终结果\n"
                }}),
            ),
            (
                "a",
                "item/commandExecution/outputDelta",
                json!({"itemId":"cmd","delta":"迟到输出"}),
            ),
            (
                "a",
                "turn/plan/updated",
                json!({"plan":[
                    {"step":"查看示例","status":"completed"},{"step":"整理结论","status":"inProgress"},
                ]}),
            ),
            (
                "a",
                "item/started",
                json!({"item":{
                    "type":"commandExecution","id":"next","command":"ls docs","status":"inProgress"
                }}),
            ),
        ] {
            event(&mut session, turn, method, payload);
        }
        event(
            &mut session,
            "a",
            "turn/completed",
            json!({"turn":{"id":"a","status":"interrupted"}}),
        );
        let stopped = session.take_snapshot().unwrap();
        assert_eq!(stopped.view.entries.len(), 4);
        assert!(Arc::ptr_eq(
            &running.view.entries[0],
            &stopped.view.entries[0]
        ));
        assert!(matches!(stopped.view.entries[1].as_ref(), Entry::Plan(plan)
            if plan.steps.len() == 2 && plan.steps[0].state == ActivityState::Completed
                && plan.steps[1].state == ActivityState::Interrupted));
        assert!(
            matches!(stopped.view.entries[2].as_ref(), Entry::Command(command)
            if command.state == ActivityState::Failed && command.exit_code == Some(2)
                && command.output == "最终结果\n")
        );
        assert!(
            matches!(stopped.view.entries[3].as_ref(), Entry::Command(command)
            if command.state == ActivityState::Interrupted)
        );
    }

    fn event(session: &mut Protocol, turn: &str, method: &str, mut params: Value) {
        params["threadId"] = json!("t");
        params["turnId"] = json!(turn);
        session
            .server(json!({"method":method,"params":params}))
            .unwrap();
    }

    #[test]
    fn overlapping_turns_preserve_thread_start_replies_questions_and_stops_across_projects() {
        let mut session = initialized();
        session.input(Input::Send("first project".into()));
        let start_a = next(&mut session);
        session.input(Input::Workspace("/second".into()));
        resolve_listing(&mut session, "/second");
        let second = session.snapshot.view.conversation;
        session.input(Input::Send("second project".into()));
        let start_b = next(&mut session);

        reply(&mut session, start_a, json!({"thread":{"id":"t"}}));
        let turn_a = next(&mut session);
        assert_eq!(turn_a["params"]["threadId"], "t");
        assert_eq!(turn_a["params"]["input"][0]["text"], "first project");
        reply(&mut session, start_b, json!({"thread":{"id":"second"}}));
        let turn_b = next(&mut session);
        assert_eq!(turn_b["params"]["threadId"], "second");
        assert_eq!(turn_b["params"]["input"][0]["text"], "second project");
        // Responses and item IDs can arrive interleaved on the same connection.
        reply(&mut session, turn_b, json!({"turn":{"id":"turn-b"}}));
        reply(&mut session, turn_a, json!({"turn":{"id":"turn-a"}}));
        event(
            &mut session,
            "turn-a",
            "item/agentMessage/delta",
            json!({"itemId":"message","delta":"First reply"}),
        );
        session.server(json!({
            "method":"item/agentMessage/delta",
            "params":{"threadId":"second","turnId":"turn-b","itemId":"message","delta":"Second reply"},
        })).unwrap();
        assert!(matches!(session.snapshot.view.entries[1].as_ref(),
            Entry::Assistant{text,..} if text == "Second reply"));

        session
            .server(json!({
                "method":"item/tool/requestUserInput","id":"question-a",
                "params":{"threadId":"t","turnId":"turn-a","questions":[
                    {"id":"check","header":"范围","question":"继续吗？","options":[]}
                ]},
            }))
            .unwrap();
        let status = session.take_snapshot().unwrap();
        assert_eq!(status.view.conversation, second);
        assert!(status.view.question.is_none());
        assert_eq!(
            status
                .conversations
                .iter()
                .find(|row| row.id == 0)
                .unwrap()
                .activity,
            ConversationActivity::NeedsInput
        );

        session.input(Input::Interrupt(0));
        assert!(session.drain_outbound().next().is_none());
        session.input(Input::Interrupt(session.snapshot.view.conversation));
        let stop = next(&mut session);
        assert_eq!(
            stop["params"],
            json!({"threadId":"second","turnId":"turn-b"})
        );
        session
            .server(json!({
                "method":"turn/completed",
                "params":{"threadId":"second","turn":{"id":"turn-b","status":"interrupted"}},
            }))
            .unwrap();
        assert!(!session.snapshot.view.busy);

        session.input(Input::Workspace("/workspace".into()));
        resolve_listing(&mut session, "/workspace");
        assert!(session.snapshot.view.busy);
        assert!(matches!(session.snapshot.view.entries[1].as_ref(),
            Entry::Assistant{text,..} if text == "First reply"));
        let question = session.snapshot.view.question.clone().unwrap();
        session.input(Input::AnswerQuestion {
            token: question.token,
            index: 0,
            value: Some("继续".into()),
        });
        assert_eq!(next(&mut session)["id"], "question-a");

        session.input(Input::Workspace("/second".into()));
        resolve_listing(&mut session, "/second");
        event(
            &mut session,
            "turn-a",
            "turn/completed",
            json!({"turn":{"id":"turn-a","status":"completed"}}),
        );
        let status = session.take_snapshot().unwrap();
        assert_eq!(status.view.conversation, second);
        assert_eq!(status.view.status, Status::Stopped);
        assert_eq!(
            status
                .conversations
                .iter()
                .find(|row| row.id == 0)
                .unwrap()
                .activity,
            ConversationActivity::Updated
        );
        session.input(Input::Workspace("/workspace".into()));
        resolve_listing(&mut session, "/workspace");
        let status = session.take_snapshot().unwrap();
        assert_eq!(status.view.status, Status::Completed);
        assert_eq!(
            status
                .conversations
                .iter()
                .find(|row| row.id == 0)
                .unwrap()
                .activity,
            ConversationActivity::Idle
        );
    }

    #[test]
    fn a_background_start_timeout_does_not_disconnect_the_foreground_turn() {
        let mut session = initialized();
        session.input(Input::Send("background".into()));
        let start = next(&mut session);
        session.input(Input::Reset);
        session.input(Input::Send("foreground".into()));
        let front = next(&mut session);
        reply(&mut session, front, json!({"thread":{"id":"foreground"}}));
        let turn = next(&mut session);
        reply(&mut session, turn, json!({"turn":{"id":"running"}}));
        session
            .pending
            .get_mut(&start["id"].as_u64().unwrap())
            .unwrap()
            .sent = Instant::now() - RESPONSE_TIMEOUT;
        assert!(!session.expired());
        assert!(session.snapshot.connected && session.snapshot.view.busy);
        assert_eq!(
            session.inactive[&0].view.status,
            Status::from("等待轮次响应超时")
        );
        assert_eq!(
            session.inactive[&0].view.activity(),
            ConversationActivity::Updated
        );
        assert_eq!(
            session.runtime.turn.as_ref().unwrap().id.as_deref(),
            Some("running")
        );
    }

    #[test]
    fn native_review_restores_its_report_without_sending_a_prompt() {
        let mut session = initialized();
        session.input(Input::Review(ReviewTarget::WorkingTree));
        assert!(session.snapshot.view.reviewing && session.snapshot.view.busy);
        let thread = next(&mut session);
        assert_eq!(thread["method"], "thread/start");
        assert_eq!(thread["params"]["sandbox"], "read-only");
        reply(&mut session, thread, json!({"thread":{"id":"t"}}));

        let request = next(&mut session);
        assert_eq!(request["method"], "review/start");
        assert_eq!(
            request["params"],
            json!({
                "threadId":"t","delivery":"inline","target":{"type":"uncommittedChanges"}
            })
        );
        event(
            &mut session,
            "a",
            "turn/started",
            json!({"turn":{"id":"a"}}),
        );
        let item = json!({
            "id":"report","type":"exitedReviewMode","review":"The counter label needs updating."
        });
        event(
            &mut session,
            "previous",
            "item/completed",
            json!({"item":item}),
        );
        assert!(session.snapshot.view.entries.is_empty());
        for _ in 0..2 {
            event(&mut session, "a", "item/completed", json!({"item":item}));
        }
        event(
            &mut session,
            "a",
            "turn/completed",
            json!({"turn":{"id":"a","status":"completed"}}),
        );
        reply(
            &mut session,
            request,
            json!({"reviewThreadId":"t","turn":{"id":"a"}}),
        );
        assert_eq!(session.snapshot.view.entries.len(), 1);
        assert!(matches!(session.snapshot.view.entries[0].as_ref(),
            Entry::ReviewReport(text) if text == "The counter label needs updating."));
        assert!(matches!(history_entry(&item),
            Some(Entry::ReviewReport(text)) if text == "The counter label needs updating."));
        assert_eq!(session.snapshot.view.sent_prompts, 0);
        assert_eq!(session.snapshot.view.status, Status::Reviewed);
        assert!(!session.snapshot.view.reviewing && !session.snapshot.view.busy);
    }

    #[test]
    fn stopping_a_branch_review_waits_for_the_native_turn_id() {
        let mut session = initialized();
        session.input(Input::Review(ReviewTarget::Branch("main".into())));
        let setup = next(&mut session);
        reply(&mut session, setup, json!({"thread":{"id":"t"}}));
        let request = next(&mut session);
        assert_eq!(
            request["params"]["target"],
            json!({"type":"baseBranch","branch":"main"})
        );
        session.input(Input::Interrupt(session.snapshot.view.conversation));
        assert!(session.snapshot.view.stopping);
        assert!(session.drain_outbound().next().is_none());
        reply(
            &mut session,
            request,
            json!({"reviewThreadId":"t","turn":{"id":"a"}}),
        );
        let interrupt = next(&mut session);
        assert_eq!(interrupt["method"], "turn/interrupt");
        assert_eq!(interrupt["params"], json!({"threadId":"t","turnId":"a"}));
        event(
            &mut session,
            "a",
            "turn/completed",
            json!({"turn":{"id":"a","status":"interrupted"}}),
        );
        assert!(!session.snapshot.view.reviewing && !session.snapshot.view.busy);
    }

    #[test]
    fn command_output_cap_preserves_utf8_and_ignores_the_rest() {
        let mut activity = CommandActivity {
            command: String::new(),
            state: ActivityState::Running,
            exit_code: None,
            output: "x".repeat(COMMAND_OUTPUT_LIMIT - 1),
            output_truncated: false,
        };
        activity.append_output("中");
        activity.append_output("z");
        assert!(activity.output_truncated);
        assert_eq!(activity.output.len(), COMMAND_OUTPUT_LIMIT - 1);
        assert!(activity.output.ends_with('x'));
    }

    #[test]
    fn turn_diff_snapshots_replace_only_the_matching_turn_and_survive_switching() {
        let mut session = initialized_with_access(true);
        session.input(Input::Send("first".into()));
        let thread = next(&mut session);
        assert_eq!(thread["params"]["sandbox"], "workspace-write");
        assert_eq!(thread["params"]["approvalPolicy"], "never");
        reply(&mut session, thread, json!({"thread":{"id":"t"}}));
        let turn = next(&mut session);
        reply(&mut session, turn, json!({"turn":{"id":"a"}}));

        let emit_diff = |session: &mut Protocol, turn, text| {
            event(session, turn, "turn/diff/updated", json!({"diff":text}));
        };
        emit_diff(
            &mut session,
            "a",
            "diff --git a/first b/first\nnew file mode 100644\n",
        );
        emit_diff(&mut session, "a", "");
        assert_eq!(session.snapshot.view.entries.len(), 2);
        assert!(
            matches!(session.snapshot.view.entries[1].as_ref(), Entry::Changes(diff) if diff.files.is_empty())
        );
        emit_diff(
            &mut session,
            "a",
            "diff --git a/first b/first\nnew file mode 100644\n",
        );
        event(
            &mut session,
            "a",
            "turn/completed",
            json!({"turn":{"id":"a","status":"completed"}}),
        );
        let first = session.snapshot.view.entries[1].clone();

        session.input(Input::Send("second".into()));
        let turn = next(&mut session);
        reply(&mut session, turn, json!({"turn":{"id":"b"}}));
        emit_diff(&mut session, "a", "diff --git a/stale b/stale\n");
        emit_diff(
            &mut session,
            "b",
            "diff --git a/second b/second\nnew file mode 100644\n",
        );
        event(
            &mut session,
            "b",
            "turn/completed",
            json!({"turn":{"id":"b","status":"completed"}}),
        );
        assert_eq!(session.snapshot.view.entries.len(), 4);
        assert!(Arc::ptr_eq(&first, &session.snapshot.view.entries[1]));
        assert!(
            matches!(session.snapshot.view.entries[3].as_ref(), Entry::Changes(diff) if diff.files[0].path == "second")
        );

        session.input(Input::Workspace("/another".into()));
        resolve_listing(&mut session, "/another");
        assert!(session.snapshot.view.entries.is_empty());
        session.input(Input::Select(0));
        resolve_listing(&mut session, "/workspace");
        assert!(Arc::ptr_eq(&first, &session.snapshot.view.entries[1]));
        assert_eq!(session.snapshot.view.entries.len(), 4);
    }

    #[test]
    fn lists_persisted_sessions_then_loads_and_resumes_the_selected_thread() {
        let (mut session, list) = initial_listing(false);
        reply(
            &mut session,
            list,
            json!({
                "data":[
                    {"id":"saved","cwd":"/workspace","name":"Saved conversation"},
                    {"id":"saved","cwd":"/workspace","name":"Duplicate"},
                    {"id":"elsewhere","cwd":"/another","name":"Outside workspace"},
                    {"id":"child","cwd":"/workspace","parentThreadId":"saved"},
                ],
                "nextCursor":"next",
            }),
        );
        assert_eq!(session.snapshot.conversations.len(), 2);
        assert_eq!(
            session.snapshot.conversations[1].title,
            "Saved conversation"
        );
        assert!(session.snapshot.history_more);

        session.input(Input::History { more: true });
        let page = next(&mut session);
        assert_eq!(page["params"]["cursor"], "next");
        reply(
            &mut session,
            page,
            json!({"data":[{"id":"older","cwd":"/workspace","preview":"older"}]}),
        );
        assert!(!session.snapshot.history_more);
        assert_eq!(session.snapshot.conversations[2].title, "older");

        session.input(Input::Select(1));
        let history = next(&mut session);
        assert_eq!(history["method"], "thread/turns/list");
        assert_eq!(history["params"]["threadId"], "saved");
        assert!(session.snapshot.view.restoring);
        session.input(Input::Send("too early".into()));
        assert!(session.drain_outbound().next().is_none());
        reply(
            &mut session,
            history,
            json!({
                "data":[
                    {"id":"latest","status":"completed","items":[{"type":"agentMessage","text":"latest response"}]},
                    {"id":"prior","status":"completed","items":[
                        {"type":"userMessage","content":[{"type":"text","text":"first prompt"}]},
                        {"type":"commandExecution","command":"ls","status":"completed","exitCode":0,"aggregatedOutput":"docs\n"}
                    ]},
                ],
            "nextCursor":"earlier",
            }),
        );
        assert!(!session.snapshot.view.restoring);
        assert!(
            matches!(session.snapshot.view.entries[0].as_ref(), Entry::User(message) if message.prompt_text() == "first prompt")
        );
        assert!(
            matches!(session.snapshot.view.entries[1].as_ref(), Entry::Command(cmd) if cmd.output == "docs\n")
        );
        assert!(
            matches!(session.snapshot.view.entries[2].as_ref(), Entry::Assistant { text, .. } if text == "latest response")
        );
        assert!(session.snapshot.view.earlier_turns);
        assert_eq!(session.snapshot.view.fork_points[0].prompt, Some(0));
        assert_eq!(session.snapshot.view.fork_points[1].response, Some(2));
        let original = session.snapshot.view.entries[0].clone();
        session.input(Input::EarlierTurns);
        let older = next(&mut session);
        assert_eq!(older["method"], "thread/turns/list");
        assert_eq!(older["params"]["cursor"], "earlier");
        reply(
            &mut session,
            older,
            json!({
                "data":[{"id":"earliest","status":"completed","items":[{"type":"agentMessage","text":"earlier response"}]}],
            }),
        );
        assert!(!session.snapshot.view.earlier_turns);
        assert!(
            matches!(session.snapshot.view.entries[0].as_ref(), Entry::Assistant { text, .. } if text == "earlier response")
        );
        assert!(Arc::ptr_eq(&original, &session.snapshot.view.entries[1]));
        let points = &session.snapshot.view.fork_points;
        assert_eq!(points.len(), 3);
        assert_eq!((&*points[0].id, points[0].response), ("earliest", Some(0)));
        assert_eq!((&*points[1].id, points[1].prompt), ("prior", Some(1)));
        assert_eq!((&*points[2].id, points[2].response), ("latest", Some(3)));
        assert_eq!(points[0].entries, 0..1);
        assert_eq!(points[1].entries, 1..3);
        assert_eq!(points[2].entries, 3..4);

        session.input(Input::Send("continue".into()));
        let resume = next(&mut session);
        assert_eq!(resume["method"], "thread/resume");
        assert_eq!(resume["params"]["threadId"], "saved");
        assert_eq!(resume["params"]["excludeTurns"], true);
        assert_eq!(resume["params"]["sandbox"], "read-only");
        reply(&mut session, resume, json!({"thread":{"id":"saved"}}));
        let turn = next(&mut session);
        assert_eq!(turn["method"], "turn/start");
        assert_eq!(turn["params"]["threadId"], "saved");
        assert_eq!(turn["params"]["input"][0]["text"], "continue");
    }

    #[test]
    fn a_slow_history_request_can_be_retried_without_disconnecting() {
        let (mut session, timed_out) = initial_listing(false);
        session
            .pending
            .get_mut(&timed_out["id"].as_u64().unwrap())
            .unwrap()
            .sent = Instant::now() - RESPONSE_TIMEOUT;
        assert!(!session.expired());
        assert!(session.snapshot.connected);
        assert!(!session.snapshot.history_loading);
        session.input(Input::History { more: false });
        let retry = next(&mut session);
        reply(
            &mut session,
            timed_out,
            json!({"data":[{"id":"late","cwd":"/workspace"}]}),
        );
        assert!(session.snapshot.history_loading);
        assert_eq!(session.snapshot.conversations.len(), 1);
        reply(
            &mut session,
            retry,
            json!({"data":[{"id":"saved","cwd":"/workspace"}]}),
        );
        assert!(!session.snapshot.history_loading);
        assert_eq!(session.snapshot.conversations.len(), 2);
    }

    #[test]
    fn workspace_search_uses_the_latest_query_and_preserves_known_rows() {
        let (mut session, list) = initial_listing(false);
        reply(
            &mut session,
            list,
            json!({"data":[{"id":"known","cwd":"/workspace","name":"Bazel cache"}]}),
        );
        session.input(Input::Search("old query".into()));
        let stale = next(&mut session);
        session.input(Input::Search(" cache ".into()));
        let query = next(&mut session);
        assert_eq!(query["params"]["searchTerm"], "cache");
        assert_eq!(query["params"]["cwd"], "/workspace");
        assert_eq!(
            query["params"]["sourceKinds"],
            json!(["cli", "vscode", "appServer"])
        );
        reply(
            &mut session,
            stale,
            json!({"data":[{"id":"stale","cwd":"/workspace","name":"Old hit"}]}),
        );
        assert!(session.snapshot.history_loading);
        reply(
            &mut session,
            query,
            json!({
                "data":[
                    {"id":"known","cwd":"/workspace","name":"Bazel cache"},
                    {"id":"new","cwd":"/workspace","name":"Imported result"},
                    {"id":"outside","cwd":"/other","name":"Foreign cache"},
                ],
                "nextCursor":"cache-more",
            }),
        );
        let rows = &session.snapshot.conversations;
        assert_eq!(rows.len(), 3);
        assert!(!session.snapshot.shows_conversation(&rows[0]));
        assert!(session.snapshot.shows_conversation(&rows[1]));
        assert!(session.snapshot.shows_conversation(&rows[2]));
        session.input(Input::History { more: true });
        let page = next(&mut session);
        assert_eq!(page["params"]["cursor"], "cache-more");
        assert_eq!(page["params"]["searchTerm"], "cache");
        session.input(Input::Search(String::new()));
        let cleared = next(&mut session);
        assert_eq!(cleared["params"]["searchTerm"], Value::Null);
        assert_eq!(cleared["params"]["cursor"], Value::Null);
        assert!(
            session
                .snapshot
                .conversations
                .iter()
                .all(|row| session.snapshot.shows_conversation(row))
        );
        reply(
            &mut session,
            page,
            json!({"data":[{"id":"stale-page","cwd":"/workspace"}]}),
        );
        assert_eq!(session.snapshot.conversations.len(), 3);
    }

    #[test]
    fn renaming_a_stored_thread_waits_for_acknowledgment() {
        let (mut session, list) = initial_listing(false);
        reply(
            &mut session,
            list,
            json!({"data":[{"id":"stored","cwd":"/workspace","name":"Original"}]}),
        );
        session.input(Input::Select(1));
        let history = next(&mut session);
        reply(&mut session, history, json!({"data":[]}));
        assert!(session.take_snapshot().unwrap().view.renamable);

        session.input(Input::Rename("    ".into()));
        assert!(session.drain_outbound().next().is_none());
        assert!(!session.snapshot.view.rename_error.is_empty());
        session.input(Input::Rename("  新名称  ".into()));
        let first = next(&mut session);
        assert_eq!(first["method"], "thread/name/set");
        assert_eq!(
            first["params"],
            json!({"threadId":"stored","name":"新名称"})
        );
        assert!(session.snapshot.view.rename_pending);
        assert_eq!(session.snapshot.conversations[1].title, "Original");
        session
            .server(
                json!({"id":first["id"],"error":{"code":-32603,"message":"fixture write failed"}}),
            )
            .unwrap();
        assert!(!session.snapshot.view.rename_pending);
        assert!(
            session
                .snapshot
                .view
                .rename_error
                .contains("fixture write failed")
        );
        assert_eq!(session.snapshot.conversations[1].title, "Original");
        session.input(Input::Rename("新名称".into()));
        let retry = next(&mut session);
        session.input(Input::History { more: false });
        let stale_listing = next(&mut session);
        reply(&mut session, retry, json!({}));
        let new_listing = next(&mut session);
        reply(
            &mut session,
            stale_listing,
            json!({"data":[{"id":"stored","cwd":"/workspace","name":"Original"}]}),
        );
        assert!(!session.snapshot.view.rename_pending);
        assert!(session.snapshot.view.rename_error.is_empty());
        assert_eq!(session.snapshot.conversations[1].title, "新名称");
        reply(
            &mut session,
            new_listing,
            json!({"data":[{"id":"stored","cwd":"/workspace","name":"新名称"}]}),
        );
        assert_eq!(session.snapshot.conversations.len(), 2);
    }

    #[test]
    fn archives_and_restores_a_loaded_session_without_losing_its_thread() {
        let mut session = initialized();
        session.input(Input::Send("first prompt".into()));
        let thread = next(&mut session);
        reply(&mut session, thread, json!({"thread":{"id":"t"}}));
        let turn = next(&mut session);
        reply(&mut session, turn, json!({"turn":{"id":"a"}}));
        event(
            &mut session,
            "a",
            "turn/completed",
            json!({"turn":{"id":"a","status":"completed"}}),
        );
        let original = session.snapshot.view.entries[0].clone();
        session.input(Input::History { more: false });
        let stale_listing = next(&mut session);

        session.input(Input::SetArchived(true));
        let archive = next(&mut session);
        assert_eq!(archive["method"], "thread/archive");
        assert_eq!(archive["params"]["threadId"], "t");
        assert!(!session.snapshot.current_archived());
        session.input(Input::Send("blocked while pending".into()));
        assert!(session.drain_outbound().next().is_none());
        reply(&mut session, archive, json!({}));
        let archived_list = next(&mut session);
        assert_eq!(archived_list["params"]["archived"], true);
        assert!(session.snapshot.current_archived());
        assert!(session.snapshot.archive_view);
        reply(
            &mut session,
            stale_listing,
            json!({"data":[{"id":"t","cwd":"/workspace"}]}),
        );
        reply(
            &mut session,
            archived_list,
            json!({"data":[{"id":"t","cwd":"/workspace"}]}),
        );
        assert_eq!(session.snapshot.conversations.len(), 1);
        assert!(session.snapshot.current_archived());
        assert!(Arc::ptr_eq(&session.snapshot.view.entries[0], &original));
        session.input(Input::Send("blocked while archived".into()));
        assert!(session.drain_outbound().next().is_none());

        session.input(Input::SetArchived(false));
        let restore = next(&mut session);
        assert_eq!(restore["method"], "thread/unarchive");
        assert_eq!(restore["params"]["threadId"], "t");
        reply(&mut session, restore, json!({"thread":{"id":"t"}}));
        let active_list = next(&mut session);
        assert_eq!(active_list["params"]["archived"], false);
        assert!(!session.snapshot.archive_view);
        assert!(!session.snapshot.current_archived());
        session.input(Input::Send("next prompt".into()));
        let resume = next(&mut session);
        assert_eq!(resume["method"], "thread/resume");
        assert_eq!(resume["params"]["threadId"], "t");
        reply(&mut session, resume, json!({"thread":{"id":"t"}}));
        assert_eq!(next(&mut session)["params"]["threadId"], "t");
        assert!(Arc::ptr_eq(&session.snapshot.view.entries[0], &original));
    }

    #[test]
    fn archived_results_are_scoped_and_can_be_read_before_restore() {
        let (mut session, stale) = initial_listing(false);
        session.input(Input::ArchiveView(true));
        let archive_view = next(&mut session);
        assert_eq!(archive_view["params"]["archived"], true);
        assert_eq!(archive_view["params"]["cwd"], "/workspace");
        reply(
            &mut session,
            stale,
            json!({"data":[{"id":"active","cwd":"/workspace"}]}),
        );
        reply(
            &mut session,
            archive_view,
            json!({"data":[
                {"id":"stored","cwd":"/workspace","name":"Stored"},
                {"id":"other","cwd":"/outside","name":"Outside workdir"},
            ]}),
        );
        assert_eq!(session.snapshot.conversations.len(), 2);
        assert!(
            !session
                .snapshot
                .shows_conversation(&session.snapshot.conversations[0])
        );
        assert!(
            session
                .snapshot
                .shows_conversation(&session.snapshot.conversations[1])
        );
        session.input(Input::Select(1));
        let history = next(&mut session);
        assert_eq!(history["method"], "thread/turns/list");
        assert_eq!(history["params"]["threadId"], "stored");
        reply(
            &mut session,
            history,
            json!({"data":[{
                "items":[{"type":"agentMessage","text":"archived transcript"}],
            }]}),
        );
        assert!(session.snapshot.current_archived());
        assert!(
            matches!(session.snapshot.view.entries[0].as_ref(), Entry::Assistant {text,..} if text == "archived transcript")
        );
        session.input(Input::SetArchived(false));
        let restore = next(&mut session);
        session
            .server(json!({"id":restore["id"],"error":{"code":-32603,"message":"fixture error"}}))
            .unwrap();
        assert!(session.snapshot.current_archived());
        assert!(!session.snapshot.view.archive_pending);
        assert!(
            session
                .snapshot
                .view
                .status
                .presentation()
                .1
                .contains("fixture error")
        );
    }

    #[test]
    fn model_catalog_filters_options_and_sends_the_selected_effort() {
        let mut session = initialized();
        session.input(Input::LoadModels);
        let listing = next(&mut session);
        assert_eq!(listing["params"]["includeHidden"], false);
        reply(
            &mut session,
            listing,
            json!({
                "data":[
                    {"model":"visible","displayName":"Visible","defaultReasoningEffort":"medium",
                     "supportedReasoningEfforts":[{"reasoningEffort":"medium"},{"reasoningEffort":"high"},{"reasoningEffort":"high"}]},
                    {"model":"hidden","hidden":true},
                ],
                "nextCursor":"next",
            }),
        );
        let page = next(&mut session);
        assert_eq!(page["params"]["cursor"], "next");
        reply(
            &mut session,
            page,
            json!({
                "data":[{"model":"lighter","defaultReasoningEffort":"low","supportedReasoningEfforts":[{"reasoningEffort":"low"}]}],
            }),
        );
        assert!(!session.snapshot.models_loading);
        assert_eq!(session.snapshot.models.len(), 2);
        assert_eq!(session.snapshot.models[0].efforts, ["medium", "high"]);
        session.input(Input::ChooseModel("visible".into()));
        assert_eq!(session.snapshot.view.turn_options.effort, "medium");
        session.input(Input::ChooseEffort("high".into()));
        session.input(Input::ChooseEffort("unsupported".into()));
        session.input(Input::ChooseModel("hidden".into()));
        assert_eq!(session.snapshot.view.turn_options.model, "visible");
        assert_eq!(session.snapshot.view.turn_options.effort, "high");
        session.input(Input::Send("first".into()));
        let create = next(&mut session);
        assert_eq!(create["params"]["model"], "visible");
        session.input(Input::ChooseModel("lighter".into()));
        reply(&mut session, create, json!({"thread":{"id":"t"}}));
        let turn = next(&mut session);
        assert_eq!(turn["params"]["model"], "visible");
        assert_eq!(turn["params"]["effort"], "high");
        reply(&mut session, turn, json!({"turn":{"id":"a"}}));
        event(
            &mut session,
            "a",
            "turn/completed",
            json!({"turn":{"id":"a","status":"completed"}}),
        );

        session.input(Input::Reset);
        assert!(session.snapshot.view.turn_options.model.is_empty());
        session.input(Input::ChooseModel("lighter".into()));
        assert_eq!(session.snapshot.view.turn_options.effort, "low");
        session.input(Input::Select(0));
        assert_eq!(session.snapshot.view.turn_options.effort, "high");
        session.input(Input::ChooseModel("lighter".into()));
        session.input(Input::Send("second".into()));
        let turn = next(&mut session);
        assert_eq!(turn["params"]["threadId"], "t");
        assert_eq!(turn["params"]["model"], "lighter");
        assert_eq!(turn["params"]["effort"], "low");
    }

    #[test]
    fn a_failed_catalog_page_keeps_the_last_complete_model_list() {
        let mut session = initialized();
        session.input(Input::LoadModels);
        let request = next(&mut session);
        reply(&mut session, request, json!({"data":[{"model":"last"}]}));
        let complete = session.snapshot.models.clone();

        session.input(Input::LoadModels);
        let request = next(&mut session);
        reply(
            &mut session,
            request,
            json!({"data":[{"model":"incomplete"}],"nextCursor":"more"}),
        );
        let next_page = next(&mut session);
        assert!(Arc::ptr_eq(&complete, &session.snapshot.models));
        session
            .server(json!({"id":next_page["id"],"error":{"code":-32603,"message":"fixture error"}}))
            .unwrap();
        assert!(!session.snapshot.models_loading);
        assert!(session.snapshot.models_status.contains("fixture error"));
        assert!(Arc::ptr_eq(&complete, &session.snapshot.models));

        session.input(Input::LoadModels);
        let request = next(&mut session);
        reply(&mut session, request, json!({"data":[{"model":"updated"}]}));
        assert_eq!(session.snapshot.models.len(), 1);
        assert_eq!(session.snapshot.models[0].id, "updated");
    }

    #[test]
    fn plan_mode_uses_the_native_preset_and_finalizes_one_proposal() {
        let mut session = initialized();
        session.modes_listed(&json!({"data":[
            {"mode":"plan","reasoning_effort":"medium"},{"mode":"default","reasoning_effort":null}
        ]}));
        assert!(session.snapshot.plan_available);
        session.input(Input::ChoosePlanMode(true));
        session.input(Input::Send("先讨论".into()));
        let thread = next(&mut session);
        reply(
            &mut session,
            thread,
            json!({"thread":{"id":"t"},"model":"fixture","reasoningEffort":"low"}),
        );
        let turn = next(&mut session);
        assert_eq!(
            turn["params"]["collaborationMode"],
            json!({
                "mode":"plan","settings":{"model":"fixture","reasoning_effort":"medium","developer_instructions":null}
            })
        );
        reply(&mut session, turn, json!({"turn":{"id":"a"}}));
        event(
            &mut session,
            "a",
            "item/plan/delta",
            json!({"itemId":"p","delta":"草稿"}),
        );
        assert!(
            matches!(session.snapshot.view.entries[1].as_ref(), Entry::Proposal{text,finalized:false} if text == "草稿")
        );
        event(
            &mut session,
            "a",
            "item/completed",
            json!({"item":{"type":"plan","id":"p","text":"定稿"}}),
        );
        event(
            &mut session,
            "a",
            "item/plan/delta",
            json!({"itemId":"p","delta":"迟到"}),
        );
        event(
            &mut session,
            "a",
            "turn/completed",
            json!({"turn":{"id":"a","status":"completed"}}),
        );
        assert_eq!(session.snapshot.view.entries.len(), 2);
        assert!(
            matches!(session.snapshot.view.entries[1].as_ref(), Entry::Proposal{text,finalized:true} if text == "定稿")
        );

        session.input(Input::ChoosePlanMode(false));
        session.input(Input::Send("按方案执行".into()));
        let execute = next(&mut session);
        assert_eq!(execute["params"]["collaborationMode"]["mode"], "default");
        assert_eq!(execute["params"]["threadId"], "t");
    }

    #[test]
    fn question_pages_validate_answers_and_keep_request_ids_separate() {
        let mut session = initialized();
        session.modes_listed(&json!({"data":[{"mode":"plan"},{"mode":"default"}]}));
        session.input(Input::ChoosePlanMode(true));
        session.input(Input::Send("先讨论".into()));
        let start = next(&mut session);
        reply(
            &mut session,
            start,
            json!({"thread":{"id":"t"},"model":"fixture"}),
        );
        let turn = next(&mut session);
        reply(&mut session, turn, json!({"turn":{"id":"a"}}));
        let question = |id: &str, turn: &str, questions: Value| {
            json!({
                "jsonrpc":"2.0","id":id,"method":"item/tool/requestUserInput",
                "params":{"threadId":"t","turnId":turn,"questions":questions}
            })
        };
        let options = json!([{"id":"example","header":"范围","question":"选哪个示例？",
             "options":[{"label":"计数器","description":"桌面计数器"}]}]);
        session
            .server(question("stale", "old", options.clone()))
            .unwrap();
        assert_eq!(next(&mut session)["error"]["code"], -32602);
        assert!(session.snapshot.view.question.is_none());
        let mut two = options.as_array().unwrap().clone();
        two.push(json!({"id":"detail","header":"细节","question":"补充说明？","isSecret":true}));
        session.server(question("first", "a", two.into())).unwrap();
        session.server(question("second", "a", options)).unwrap();
        let token = session.snapshot.view.question.as_ref().unwrap().token;
        session.input(Input::AnswerQuestion {
            token,
            index: 0,
            value: Some("不在选项内".into()),
        });
        assert_eq!(session.snapshot.view.question.as_ref().unwrap().index, 0);
        session.input(Input::AnswerQuestion {
            token,
            index: 0,
            value: Some("计数器".into()),
        });
        let page = session.snapshot.view.question.as_ref().unwrap();
        assert_eq!(page.index, 1);
        assert!(page.secret && page.allows_text);
        session.input(Input::AnswerQuestion {
            token,
            index: 0,
            value: None,
        });
        assert!(session.drain_outbound().next().is_none());
        session.input(Input::AnswerQuestion {
            token,
            index: 1,
            value: None,
        });
        assert_eq!(
            next(&mut session),
            json!({
                "jsonrpc":"2.0","id":"first","result":{"answers":{"example":{"answers":["计数器"]}}}
            })
        );
        assert_ne!(
            session.snapshot.view.question.as_ref().unwrap().token,
            token
        );
        event(
            &mut session,
            "a",
            "serverRequest/resolved",
            json!({"requestId":"second"}),
        );
        assert!(session.snapshot.view.question.is_none());
    }

    #[test]
    fn queue_reorder_submits_the_entire_paged_order_using_stable_ids() {
        let mut session = initialized();
        session
            .install_thread(&json!({"thread":{"id":"t"}}))
            .unwrap();
        session.read_queue();
        let first = next(&mut session);
        let a = json!({"id":"a","input":[{"type":"text","text":"第一项"}]});
        let b = json!({"id":"b","input":[{"type":"text","text":"第二项"}]});
        reply(
            &mut session,
            first,
            json!({"data":[a],"nextCursor":"page-2"}),
        );
        assert!(session.snapshot.view.queue_loading);
        assert!(session.snapshot.view.queued.is_empty());
        let second = next(&mut session);
        assert_eq!(second["params"]["cursor"], "page-2");
        reply(&mut session, second, json!({"data":[b],"nextCursor":null}));

        session.input(Input::MoveQueued {
            conversation: 0,
            id: "a".into(),
            step: -1,
        });
        session.input(Input::MoveQueued {
            conversation: 1,
            id: "b".into(),
            step: -1,
        });
        assert!(session.drain_outbound().next().is_none());
        session.input(Input::MoveQueued {
            conversation: 0,
            id: "b".into(),
            step: -1,
        });
        let move_row = next(&mut session);
        assert_eq!(move_row["method"], "thread/queue/reorder");
        assert_eq!(move_row["params"]["queuedSubmissionIds"], json!(["b", "a"]));
        assert_eq!(session.snapshot.view.queued[0].id, "a");
        reply(&mut session, move_row, json!({}));
        let reload = next(&mut session);
        reply(&mut session, reload, json!({"data":[b,a]}));
        assert_eq!(session.snapshot.view.queued[0].id, "b");
    }

    #[test]
    fn queue_edit_preserves_wire_attachments_and_rejects_stale_content() {
        let mut session = initialized();
        session
            .install_thread(&json!({"thread":{"id":"t"}}))
            .unwrap();
        let encoded = json!({
            "id":"a","input":[
                {"type":"text","text":"原正文","text_elements":[]},
                {"type":"text","text":"XRack attached file: {\"path\":\"/workspace/readme.md\",\"contents\":\"instructions\"}",
                    "text_elements":[{"byteRange":{"start":0,"end":1},"placeholder":"附件"}]},
                {"type":"skill","name":"lint","path":"/workspace/SKILL.md"},
                {"type":"localImage","path":"/workspace/picture.png"}
            ]
        });
        let original = QueuedMessage::parse(&encoded).unwrap();
        session.snapshot.view.queued = Arc::new(vec![original.clone()]);
        session.input(Input::UpdateQueued {
            conversation: 0,
            original: original.clone(),
            text: "  新正文\n下一行".into(),
        });
        let request = next(&mut session);
        assert_eq!(request["method"], "thread/queue/update");
        assert_eq!(request["params"]["queuedSubmissionId"], "a");
        let input = &request["params"]["input"];
        assert_eq!(input[0]["text"], "  新正文\n下一行");
        assert_eq!(
            input.as_array().unwrap()[1..],
            encoded["input"].as_array().unwrap()[1..]
        );
        let updated = json!({"id":"a","input":input});
        reply(&mut session, request, json!({"queuedSubmission":updated}));
        assert_eq!(session.snapshot.view.queue_edit_saved, 1);
        let reload = next(&mut session);
        reply(&mut session, reload, json!({"data":[updated]}));
        session.input(Input::UpdateQueued {
            conversation: 0,
            original,
            text: "另一版".into(),
        });
        assert!(session.drain_outbound().next().is_none());
        assert!(!session.snapshot.view.queue_edit_error.is_empty());

        let current = session.snapshot.view.queued[0].clone();
        session.input(Input::UpdateQueued {
            conversation: 0,
            original: current,
            text: "修改".into(),
        });
        let request = next(&mut session);
        session
            .server(json!({"id":request["id"],"error":{"message":"submission no longer queued"}}))
            .unwrap();
        assert_eq!(session.snapshot.view.queue_edit_saved, 1);
        assert!(!session.snapshot.view.queue_pending);
        assert!(
            session
                .snapshot
                .view
                .queue_edit_error
                .contains("submission no longer queued")
        );
        assert_eq!(next(&mut session)["method"], "thread/queue/list");
    }

    #[test]
    fn steer_uses_the_active_turn_id_and_native_queue_turns_remain_distinct() {
        let mut session = initialized();
        session.input(Input::Send("开始检查".into()));
        let create = next(&mut session);
        reply(&mut session, create, json!({"thread":{"id":"t"}}));
        let start = next(&mut session);
        let client_id = start["params"]["clientUserMessageId"]
            .as_str()
            .unwrap()
            .to_owned();
        reply(&mut session, start, json!({"turn":{"id":"a"}}));
        let user_item = |id, client, text| {
            json!({"item":{
                "type":"userMessage","id":id,"clientId":client,"content":[{"type":"text","text":text}]
            }})
        };
        event(
            &mut session,
            "a",
            "item/started",
            user_item("original", client_id.as_str(), "开始检查"),
        );
        assert_eq!(session.snapshot.view.entries.len(), 1);
        session.input(Input::Followup {
            conversation: 0,
            turn: Some("stale".into()),
            prompt: "保留草稿".into(),
        });
        assert!(session.drain_outbound().next().is_none());
        assert!(!session.snapshot.view.followup_error.is_empty());

        session.input(Input::Followup {
            conversation: 0,
            turn: Some("a".into()),
            prompt: "关注焦点".into(),
        });
        let steer = next(&mut session);
        assert_eq!(steer["method"], "turn/steer");
        assert_eq!(steer["params"]["expectedTurnId"], "a");
        let steer_id = steer["params"]["clientUserMessageId"]
            .as_str()
            .unwrap()
            .to_owned();
        reply(&mut session, steer, json!({"turnId":"a"}));
        event(
            &mut session,
            "a",
            "item/started",
            user_item("s", steer_id.as_str(), "关注焦点"),
        );
        event(
            &mut session,
            "a",
            "item/completed",
            user_item("s", steer_id.as_str(), "关注焦点"),
        );
        assert_eq!(session.snapshot.view.entries.len(), 2);

        session.input(Input::Followup {
            conversation: 0,
            turn: None,
            prompt: "下一轮见".into(),
        });
        let add = next(&mut session);
        assert_eq!(add["method"], "thread/queue/add");
        let queued = json!({"id":"q","clientUserMessageId":add["params"]["clientUserMessageId"],
            "input":[{"type":"text","text":"下一轮见"}]});
        reply(&mut session, add, json!({"queuedSubmission":queued}));
        let list = next(&mut session);
        assert_eq!(list["method"], "thread/queue/list");
        reply(&mut session, list, json!({"data":[queued]}));
        assert_eq!(session.snapshot.view.queued[0].text, "下一轮见");
        assert_eq!(session.snapshot.view.entries.len(), 2);

        event(
            &mut session,
            "a",
            "turn/completed",
            json!({"turn":{"id":"a","status":"completed"}}),
        );
        event(
            &mut session,
            "a",
            "turn/started",
            json!({"turn":{"id":"a"}}),
        );
        assert!(!session.snapshot.view.busy);
        event(
            &mut session,
            "b",
            "turn/started",
            json!({"turn":{"id":"b"}}),
        );
        event(
            &mut session,
            "b",
            "item/started",
            user_item("next", "queued-client", "下一轮见"),
        );
        event(&mut session, "b", "thread/queue/changed", json!({}));
        let list = next(&mut session);
        reply(&mut session, list, json!({"data":[]}));
        let snapshot = session.take_snapshot().unwrap();
        assert_eq!(snapshot.view.active_turn_id, "b");
        assert!(snapshot.view.queued.is_empty());
        assert!(
            matches!(snapshot.view.entries[2].as_ref(), Entry::User(message) if message.prompt_text() == "下一轮见")
        );
        event(
            &mut session,
            "b",
            "turn/completed",
            json!({"turn":{"id":"b","status":"completed"}}),
        );
        assert!(!session.snapshot.view.busy);
    }

    #[test]
    fn a_saved_queue_resumes_with_current_sandbox_and_tracks_the_autostart() {
        let (mut session, list) = initial_listing(false);
        reply(
            &mut session,
            list,
            json!({"data":[{"id":"t","cwd":"/workspace"}]}),
        );
        session.input(Input::Select(1));
        let frames = session.drain_outbound().collect::<Vec<_>>();
        assert_eq!(frames.len(), 2);
        reply(&mut session, frames[0].clone(), json!({"data":[]}));
        reply(
            &mut session,
            frames[1].clone(),
            json!({"data":[
                {"id":"drop","input":[{"type":"text","text":"删除我"}]},
                {"id":"resume","input":[{"type":"text","text":"继续我"}]}
            ]}),
        );
        session.input(Input::RemoveQueued("drop".into()));
        let remove = next(&mut session);
        assert_eq!(remove["params"]["queuedSubmissionId"], "drop");
        reply(&mut session, remove, json!({"deleted":true}));
        let list = next(&mut session);
        reply(
            &mut session,
            list,
            json!({"data":[{"id":"resume","input":[{"type":"text","text":"继续我"}]}]}),
        );
        session.input(Input::ResumeQueue);
        let resume = next(&mut session);
        assert_eq!(resume["method"], "thread/resume");
        assert_eq!(resume["params"]["sandbox"], "read-only");
        event(
            &mut session,
            "q",
            "turn/started",
            json!({"turn":{"id":"q"}}),
        );
        assert!(session.snapshot.view.busy);
        reply(&mut session, resume, json!({"thread":{"id":"t"}}));
        let refreshed = next(&mut session);
        reply(&mut session, refreshed, json!({"data":[]}));
        assert!(!session.snapshot.view.queue_pending);
        assert!(session.take_snapshot().unwrap().view.queue_resumed);
    }

    #[test]
    fn attachments_use_one_payload_format_for_start_steer_and_queue() {
        use crate::prompt::{Attachment, AttachmentContent, ExcerptOrigin, FileExcerpt};
        let mut session = initialized();
        let prompt = Prompt {
            text: String::new(),
            attachments: vec![
                Attachment {
                    path: "/workspace/image.png".into(),
                    content: AttachmentContent::Image,
                },
                Attachment {
                    path: "/workspace/README.md".into(),
                    content: AttachmentContent::Text("Run Bazel".into()),
                },
                Attachment {
                    path: "/workspace/src/main.rs".into(),
                    content: AttachmentContent::Excerpt(FileExcerpt {
                        contents: "let count = 2;".into(),
                        lines: Some((14, 14)),
                        origin: ExcerptOrigin::Draft,
                    }),
                },
            ],
        };
        session.input(Input::Send(prompt.clone()));
        let create = next(&mut session);
        reply(&mut session, create, json!({"thread":{"id":"t"}}));
        let start = next(&mut session);
        assert_eq!(start["params"]["input"], Value::Array(prompt.input()));
        assert_eq!(start["params"]["input"][0]["type"], "localImage");
        assert_eq!(session.snapshot.conversations[0].title, "image.png");
        assert!(
            matches!(session.snapshot.view.entries[0].as_ref(), Entry::User(message) if message.display.contains("附件 · README.md"))
        );
        assert!(
            matches!(session.snapshot.view.entries[0].as_ref(), Entry::User(message) if message.display.contains("草稿选段 · main.rs:14"))
        );
        reply(&mut session, start, json!({"turn":{"id":"a"}}));
        session.input(Input::Followup {
            conversation: 0,
            turn: Some("a".into()),
            prompt: prompt.clone(),
        });
        let steer = next(&mut session);
        assert_eq!(steer["params"]["input"], Value::Array(prompt.input()));
        reply(&mut session, steer, json!({"turnId":"a"}));
        session.input(Input::Followup {
            conversation: 0,
            turn: None,
            prompt: prompt.clone(),
        });
        let queued = next(&mut session);
        assert_eq!(queued["params"]["input"], Value::Array(prompt.input()));
        let recovered = history_entry(&json!({"type":"userMessage","content":prompt.input()}));
        assert!(
            matches!(recovered, Some(Entry::User(message)) if message.display == prompt.display() && message.prompt_text() == prompt.text)
        );
    }

    #[test]
    fn a_known_text_only_model_leaves_an_image_prompt_unaccepted() {
        use crate::prompt::{Attachment, AttachmentContent};
        let mut session = initialized();
        session.input(Input::LoadModels);
        let models = next(&mut session);
        reply(
            &mut session,
            models,
            json!({"data":[{"model":"text-model","inputModalities":["text"]}]}),
        );
        session.input(Input::ChooseModel("text-model".into()));
        session.input(Input::Send(Prompt {
            text: "看图".into(),
            attachments: vec![Attachment {
                path: "/workspace/image.png".into(),
                content: AttachmentContent::Image,
            }],
        }));
        assert_eq!(session.snapshot.view.sent_prompts, 0);
        assert!(session.drain_outbound().next().is_none());
        assert!(
            session
                .snapshot
                .view
                .status
                .presentation()
                .1
                .contains("支持图片")
        );
    }

    #[test]
    fn compaction_updates_the_context_measurement_without_starting_a_user_message() {
        let mut session = initialized();
        session.input(Input::Send("开始".into()));
        let create = next(&mut session);
        reply(&mut session, create, json!({"thread":{"id":"t"}}));
        let turn = next(&mut session);
        reply(&mut session, turn, json!({"turn":{"id":"a"}}));
        let usage = |last, total| {
            json!({"tokenUsage": {
                "last":{"totalTokens":last,"inputTokens":last},
                "total":{"totalTokens":total,"inputTokens":total},
                "modelContextWindow":100000
            }})
        };
        event(
            &mut session,
            "a",
            "thread/tokenUsage/updated",
            usage(20000, 20000),
        );
        event(
            &mut session,
            "stale",
            "thread/tokenUsage/updated",
            usage(90000, 90000),
        );
        assert_eq!(
            session
                .snapshot
                .view
                .context_usage
                .as_ref()
                .unwrap()
                .fraction(),
            Some(0.2)
        );
        event(
            &mut session,
            "a",
            "turn/completed",
            json!({"turn":{"id":"a","status":"completed"}}),
        );

        session.input(Input::Compact);
        let compact = next(&mut session);
        assert_eq!(compact["method"], "thread/compact/start");
        assert_eq!(compact["params"]["threadId"], "t");
        assert!(session.snapshot.view.compacting);
        assert!(session.snapshot.view.busy);
        session.input(Input::Send("等待压缩完成".into()));
        assert!(session.drain_outbound().next().is_none());
        reply(&mut session, compact, json!({}));
        event(
            &mut session,
            "compact",
            "turn/started",
            json!({"turn":{"id":"compact"}}),
        );
        event(
            &mut session,
            "compact",
            "thread/tokenUsage/updated",
            usage(4000, 40000),
        );
        for _ in 0..2 {
            event(
                &mut session,
                "compact",
                "item/completed",
                json!({"item":{"type":"contextCompaction","id":"cc"}}),
            );
        }
        event(
            &mut session,
            "compact",
            "turn/completed",
            json!({"turn":{"id":"compact","status":"completed"}}),
        );
        let context = session.snapshot.view.context_usage.as_ref().unwrap();
        assert_eq!(context.last.total, 4000);
        assert_eq!(context.total.total, 40000);
        assert_eq!(context.fraction(), Some(0.04));
        assert_eq!(session.snapshot.view.status, Status::Compacted);
        assert!(!session.snapshot.view.compacting);
        assert!(!session.snapshot.view.busy);
        assert_eq!(session.snapshot.view.entries.len(), 2);
        assert!(matches!(
            session.snapshot.view.entries[1].as_ref(),
            Entry::Compaction
        ));
        assert_eq!(session.snapshot.view.sent_prompts, 1);

        session.input(Input::Reset);
        assert!(session.snapshot.view.context_usage.is_none());
        session.input(Input::Select(0));
        assert_eq!(
            session
                .snapshot
                .view
                .context_usage
                .as_ref()
                .unwrap()
                .last
                .total,
            4000
        );
    }

    #[test]
    fn compaction_on_a_cold_thread_resumes_with_current_permissions() {
        let (mut session, list) = initial_listing(false);
        reply(
            &mut session,
            list,
            json!({"data":[{"id":"t","cwd":"/workspace"}]}),
        );
        session.input(Input::Select(1));
        let history = next(&mut session);
        reply(
            &mut session,
            history,
            json!({"data":[{"items":[{"type":"contextCompaction","id":"old"}]}]}),
        );
        assert!(matches!(
            session.snapshot.view.entries[0].as_ref(),
            Entry::Compaction
        ));
        session.input(Input::Compact);
        let resume = next(&mut session);
        assert_eq!(resume["method"], "thread/resume");
        assert_eq!(resume["params"]["sandbox"], "read-only");
        reply(&mut session, resume, json!({"thread":{"id":"t"}}));
        let compact = next(&mut session);
        assert_eq!(compact["method"], "thread/compact/start");
        session
            .server(json!({"id":compact["id"],"error":{"code":-32603,"message":"fixture error"}}))
            .unwrap();
        assert!(!session.snapshot.view.compacting);
        assert!(!session.snapshot.view.busy);
        assert!(
            session
                .snapshot
                .view
                .status
                .presentation()
                .1
                .contains("fixture error")
        );
        assert_eq!(session.snapshot.view.entries.len(), 1);
    }

    #[test]
    fn startup_tabs_restore_in_saved_order_and_follow_the_requested_workspace_rule()
    -> std::io::Result<()> {
        let directory = tempfile::tempdir()?;
        let root = directory.path().canonicalize()?;
        let alpha = root.join("alpha");
        let beta = root.join("beta");
        std::fs::create_dir_all(&alpha)?;
        std::fs::create_dir_all(&beta)?;
        let alpha = alpha.to_string_lossy().into_owned();
        let beta = beta.to_string_lossy().into_owned();
        let state = root.join("state");
        let store = TabStore::new(Some(state.clone()));
        let draft = DraftKey::local();
        let selected = DraftKey::Thread("other-project".into());
        DraftStore::new(Some(state.clone())).save(crate::drafts::SavedDraft {
            key: draft.clone(),
            workspace: alpha.clone(),
            project: alpha.clone(),
            content: crate::drafts::DraftContent {
                text: "原有本地草稿".into(),
                attachments: Vec::new(),
                ..crate::drafts::DraftContent::default()
            },
        })?;
        let tab = |key, workspace: &str| SavedTab {
            key,
            workspace: workspace.into(),
            project: workspace.into(),
            archived: false,
        };
        let saved = SavedTabs {
            open: vec![
                tab(selected.clone(), &beta),
                tab(draft.clone(), &alpha),
                tab(
                    DraftKey::Thread("older-than-the-sidebar-page".into()),
                    &alpha,
                ),
                tab(DraftKey::Thread("unavailable".into()), &alpha),
            ],
            active: Some(selected.clone()),
        };
        store.save(&saved)?;
        for keep_launch_workspace in [false, true] {
            let mut session = Protocol::new(alpha.clone(), false, None);
            session.restore_workspace_state(Some(state.clone()), keep_launch_workspace);
            assert!(session.snapshot.tabs_restoring);
            let initialize = next(&mut session);
            reply(&mut session, initialize, json!({}));
            let requests: Vec<_> = session.drain_outbound().collect();
            let request = |id| {
                requests
                    .iter()
                    .find(|request| {
                        request["method"] == "thread/read" && request["params"]["threadId"] == id
                    })
                    .unwrap()
                    .clone()
            };
            reply(
                &mut session,
                request("older-than-the-sidebar-page"),
                json!({"thread":{"id":"older-than-the-sidebar-page","cwd":alpha,"name":"以前的任务"}}),
            );
            let missing = request("unavailable");
            session
                .server(json!({"id":missing["id"],"error":{"code":-32000,"message":"not found"}}))
                .unwrap();
            assert!(session.snapshot.tabs_restoring);
            assert!(session.snapshot.restored_tabs.is_none());
            reply(
                &mut session,
                request("other-project"),
                json!({"thread":{"id":"other-project","cwd":beta,"name":"另一个项目","parentThreadId":"source"}}),
            );
            assert!(!session.snapshot.tabs_restoring);
            let first = session.conversation_for_thread("other-project").unwrap();
            let last = session
                .conversation_for_thread("older-than-the-sidebar-page")
                .unwrap();
            let order = session.snapshot.restored_tabs.as_deref().unwrap();
            assert_eq!(order.as_slice(), [first, 0, last]);
            let active = if keep_launch_workspace { 0 } else { first };
            assert_eq!(session.snapshot.view.conversation, active);
            let requested_history: Vec<_> = session
                .drain_outbound()
                .filter(|request| request["method"] == "thread/turns/list")
                .collect();
            assert_eq!(requested_history.len(), usize::from(!keep_launch_workspace));
            if let Some(history) = requested_history.first() {
                assert_eq!(history["params"]["threadId"], "other-project");
            }
            assert_eq!(store.load()?.open, saved.open);
            assert_eq!(session.drafts.records()?[0].content.text, "原有本地草稿");
            if keep_launch_workspace {
                session.input(Input::SaveOpenTabs {
                    open: vec![last, first, 0],
                    active: Some(active),
                });
                let saved_again = store.load()?;
                assert_eq!(saved_again.active, Some(draft.clone()));
                assert_eq!(
                    saved_again.open[0].key,
                    DraftKey::Thread("older-than-the-sidebar-page".into())
                );
                assert_eq!(saved_again.open[1].key, selected);
                assert_eq!(saved_again.open[2].key, draft);
            }
        }
        Ok(())
    }

    #[test]
    fn edited_first_prompt_forks_before_its_turn_and_seeds_only_the_new_conversation() {
        let mut session = initialized();
        session.input(Input::Send("原始提示".into()));
        let create = next(&mut session);
        reply(&mut session, create, json!({"thread":{"id":"t"}}));
        let start = next(&mut session);
        reply(&mut session, start, json!({"turn":{"id":"first"}}));
        event(
            &mut session,
            "first",
            "turn/completed",
            json!({"turn":{"id":"first","status":"completed"}}),
        );
        assert_eq!(session.snapshot.view.fork_points[0].prompt, Some(0));
        assert_eq!(session.snapshot.view.fork_points[0].entries, 0..1);
        let source = session.snapshot.view.conversation;
        let original = session.snapshot.view.entries[0].clone();

        session.input(Input::ForkBeforePrompt {
            conversation: source,
            turn: "unknown".into(),
            draft: "修改后的提示".into(),
        });
        assert!(session.drain_outbound().next().is_none());
        session.input(Input::ForkBeforePrompt {
            conversation: source,
            turn: "first".into(),
            draft: "修改后的提示".into(),
        });
        let fork = next(&mut session);
        assert_eq!(fork["method"], "thread/fork");
        assert_eq!(fork["params"]["beforeTurnId"], "first");
        assert!(fork["params"]["lastTurnId"].is_null());
        assert_eq!(fork["params"]["deferGoalContinuation"], true);
        assert!(session.snapshot.view.initial_draft.is_none());
        reply(&mut session, fork, json!({"thread":{"id":"edited"}}));

        assert_ne!(session.snapshot.view.conversation, source);
        assert_eq!(
            session.snapshot.view.initial_draft.as_deref(),
            Some("修改后的提示")
        );
        assert!(!session.snapshot.view.busy);
        let previous = &session.inactive[&source].view;
        assert!(Arc::ptr_eq(&previous.entries[0], &original));
        assert!(previous.initial_draft.is_none());
        let requests = session.drain_outbound().collect::<Vec<_>>();
        assert!(
            !requests
                .iter()
                .any(|request| request["method"] == "turn/start")
        );
        let history = requests
            .into_iter()
            .find(|request| request["method"] == "thread/turns/list")
            .unwrap();
        assert_eq!(history["params"]["threadId"], "edited");
        reply(&mut session, history, json!({"data":[]}));
        assert!(session.snapshot.view.entries.is_empty());
        assert_eq!(
            session.snapshot.view.initial_draft.as_deref(),
            Some("修改后的提示")
        );
    }

    #[test]
    fn a_cutoff_fork_reads_the_native_branch_history_and_preserves_the_source() {
        let mut session = initialized();
        session.input(Input::Send("第一个问题".into()));
        let create = next(&mut session);
        reply(&mut session, create, json!({"thread":{"id":"t"}}));
        let turn = next(&mut session);
        reply(&mut session, turn, json!({"turn":{"id":"a"}}));
        event(
            &mut session,
            "a",
            "turn/completed",
            json!({"turn":{"id":"a","status":"completed"}}),
        );
        session.input(Input::Send("只留在原会话".into()));
        let turn = next(&mut session);
        reply(&mut session, turn, json!({"turn":{"id":"b"}}));
        event(
            &mut session,
            "b",
            "turn/completed",
            json!({"turn":{"id":"b","status":"completed"}}),
        );
        assert_eq!(session.snapshot.view.fork_points[0].id, "a");
        assert_eq!(session.snapshot.view.fork_points[1].label, "只留在原会话");
        let parent_entries = session.snapshot.view.entries.clone();

        session.input(Input::Fork {
            title: "新分支".into(),
            last_turn: "unknown".into(),
            worktree: None,
        });
        assert!(!session.snapshot.fork_error.is_empty());
        assert!(session.drain_outbound().next().is_none());
        session.input(Input::Fork {
            title: " 参数对照 ".into(),
            last_turn: "a".into(),
            worktree: None,
        });
        let fork = next(&mut session);
        assert_eq!(fork["method"], "thread/fork");
        assert_eq!(fork["params"]["threadId"], "t");
        assert_eq!(fork["params"]["lastTurnId"], "a");
        assert_eq!(fork["params"]["sandbox"], "read-only");
        assert_eq!(fork["params"]["deferGoalContinuation"], true);
        reply(
            &mut session,
            fork,
            json!({"thread":{"id":"branch","preview":"第一个问题"},"model":"fixture"}),
        );
        assert_eq!(session.snapshot.view.conversation, 1);
        assert!(session.snapshot.view.entries.is_empty());
        assert_eq!(session.snapshot.forks_created, 1);
        let requests = session.drain_outbound().collect::<Vec<_>>();
        assert_eq!(requests.len(), 4);
        for (method, result) in [
            (
                "thread/turns/list",
                json!({"data":[{"id":"a","status":"completed","items":[
                    {"type":"userMessage","content":[{"type":"text","text":"第一个问题"}]}
                ]}]}),
            ),
            ("thread/queue/list", json!({"data":[]})),
            ("thread/list", json!({"data":[]})),
            ("thread/name/set", json!({})),
        ] {
            let request = requests
                .iter()
                .find(|request| request["method"] == method)
                .unwrap()
                .clone();
            if method == "thread/name/set" {
                assert_eq!(request["params"]["name"], "参数对照");
            }
            if method != "thread/list" {
                assert_eq!(request["params"]["threadId"], "branch");
            }
            reply(&mut session, request, result);
        }
        assert!(!session.snapshot.view.restoring && !session.snapshot.fork_pending);
        assert_eq!(session.snapshot.conversations[0].title, "参数对照");
        assert_eq!(session.snapshot.view.entries.len(), 1);
        assert_eq!(session.snapshot.view.fork_points.len(), 1);
        session.input(Input::Send("分支继续".into()));
        let continue_branch = next(&mut session);
        assert_eq!(continue_branch["params"]["threadId"], "branch");
        let source = session.inactive.get(&0).unwrap();
        assert_eq!(source.view.entries.len(), 2);
        assert!(Arc::ptr_eq(&source.view.entries[1], &parent_entries[1]));
        assert_eq!(source.view.fork_points.len(), 2);
    }

    #[test]
    fn a_source_turn_starting_during_fork_does_not_move_its_live_entries() {
        let mut session = initialized();
        session.input(Input::Send("原会话".into()));
        let create = next(&mut session);
        reply(&mut session, create, json!({"thread":{"id":"t"}}));
        let turn = next(&mut session);
        reply(&mut session, turn, json!({"turn":{"id":"a"}}));
        event(
            &mut session,
            "a",
            "turn/completed",
            json!({"turn":{"id":"a","status":"completed"}}),
        );
        session.input(Input::Fork {
            title: String::new(),
            last_turn: String::new(),
            worktree: None,
        });
        let fork = next(&mut session);
        assert!(fork["params"]["lastTurnId"].is_null());
        event(
            &mut session,
            "queued",
            "turn/started",
            json!({"turn":{"id":"queued"}}),
        );
        reply(&mut session, fork, json!({"thread":{"id":"branch"}}));
        assert_eq!(session.snapshot.view.conversation, 0);
        assert!(session.snapshot.view.busy);
        assert!(session.inactive.get(&1).unwrap().view.entries.is_empty());
        event(
            &mut session,
            "queued",
            "item/agentMessage/delta",
            json!({"itemId":"answer","delta":"留在原会话"}),
        );
        event(
            &mut session,
            "queued",
            "turn/completed",
            json!({"turn":{"id":"queued","status":"completed"}}),
        );
        assert!(
            matches!(session.snapshot.view.entries[1].as_ref(), Entry::Assistant {text,..} if text == "留在原会话")
        );
    }

    #[test]
    fn an_unused_fork_reappears_after_restart_via_its_native_id()
    -> Result<(), Box<dyn std::error::Error>> {
        let tmp = tempfile::tempdir()?;
        let path = tmp.path().join("forks");
        let mut before = initialized();
        before.fork_index = ThreadIndex::new(Some(path.clone()));
        before.input(Input::Send("原会话".into()));
        let create = next(&mut before);
        reply(&mut before, create, json!({"thread":{"id":"t"}}));
        let turn = next(&mut before);
        reply(&mut before, turn, json!({"turn":{"id":"a"}}));
        event(
            &mut before,
            "a",
            "turn/completed",
            json!({"turn":{"id":"a","status":"completed"}}),
        );
        before.input(Input::Fork {
            title: "构建分支".into(),
            last_turn: String::new(),
            worktree: None,
        });
        let fork = next(&mut before);
        reply(&mut before, fork, json!({"thread":{"id":"branch"}}));
        assert_eq!(before.fork_index.ids("/workspace", false)?, ["branch"]);
        drop(before);

        let mut after = initialized();
        after.fork_index = ThreadIndex::new(Some(path));
        after.input(Input::History { more: false });
        let list = next(&mut after);
        reply(&mut after, list, json!({"data":[]}));
        let stale_lookup = next(&mut after);
        assert_eq!(stale_lookup["method"], "thread/read");
        assert_eq!(stale_lookup["params"]["threadId"], "branch");

        after.input(Input::Search("构建".into()));
        let list = next(&mut after);
        reply(&mut after, list, json!({"data":[]}));
        let fresh_lookup = next(&mut after);
        reply(
            &mut after,
            stale_lookup,
            json!({"thread":{"id":"branch","cwd":"/workspace","name":"旧标题"}}),
        );
        assert_eq!(after.snapshot.conversations.len(), 1);
        assert!(after.snapshot.history_loading);
        let branch = json!({"id":"branch","cwd":"/workspace","name":"构建分支"});
        reply(&mut after, fresh_lookup, json!({"thread":branch}));
        assert_eq!(after.snapshot.conversations.len(), 2);
        assert_eq!(after.snapshot.conversations[1].title, "构建分支");
        assert!(
            after
                .snapshot
                .shows_conversation(&after.snapshot.conversations[1])
        );
        assert!(!after.snapshot.history_loading);

        after.input(Input::Search(String::new()));
        let list = next(&mut after);
        reply(&mut after, list, json!({"data":[branch]}));
        assert!(after.drain_outbound().next().is_none());
        assert!(after.fork_index.ids("/workspace", false)?.is_empty());
        Ok(())
    }

    #[test]
    fn a_worktree_fork_uses_the_new_directory_and_reappears_under_its_project()
    -> Result<(), Box<dyn std::error::Error>> {
        let tmp = tempfile::tempdir()?;
        let index = tmp.path().join("index");
        let mut source = initialized();
        source.fork_index = ThreadIndex::new(Some(index.clone()));
        source.input(Input::Send("原项目".into()));
        let create = next(&mut source);
        reply(&mut source, create, json!({"thread":{"id":"t"}}));
        let turn = next(&mut source);
        reply(&mut source, turn, json!({"turn":{"id":"a"}}));
        event(
            &mut source,
            "a",
            "turn/completed",
            json!({"turn":{"id":"a","status":"completed"}}),
        );
        source.input(Input::Fork {
            title: "隔离验证".into(),
            last_turn: "a".into(),
            worktree: Some("/trees/linux/src".into()),
        });
        let fork = next(&mut source);
        assert_eq!(fork["params"]["cwd"], "/trees/linux/src");
        assert_eq!(fork["params"]["sandbox"], "read-only");
        reply(
            &mut source,
            fork,
            json!({"thread":{"id":"branch","cwd":"/trees/linux/src"}}),
        );
        assert_eq!(source.snapshot.view.workspace, "/trees/linux/src");
        assert_eq!(source.snapshot.project_workspace(), "/workspace");
        assert_eq!(source.snapshot.projects.as_ref(), &["/workspace"]);
        assert!(
            source
                .snapshot
                .shows_conversation(&source.snapshot.conversations[0])
        );
        let outbound: Vec<_> = source.drain_outbound().collect();
        let listing = outbound
            .iter()
            .find(|frame| frame["method"] == "thread/list")
            .unwrap();
        assert_eq!(
            listing["params"]["cwd"],
            json!(["/workspace", "/trees/linux/src"])
        );
        assert_eq!(
            source.fork_index.project("branch").as_deref(),
            Some("/workspace")
        );
        reply(
            &mut source,
            listing.clone(),
            json!({"data":[
                {"id":"branch","cwd":"/trees/linux/src","name":"隔离验证"}
            ]}),
        );
        assert_eq!(source.fork_index.ids("/workspace", false)?, ["branch"]);
        drop(source);

        let mut opened = initialized();
        opened.fork_index = ThreadIndex::new(Some(index));
        opened.input(Input::Search("验证".into()));
        let list = next(&mut opened);
        reply(&mut opened, list, json!({"data":[]}));
        let lookup = next(&mut opened);
        assert_eq!(lookup["method"], "thread/read");
        reply(
            &mut opened,
            lookup,
            json!({"thread":{
                "id":"branch","cwd":"/trees/linux/src","name":"隔离验证"
            }}),
        );
        let branch = &opened.snapshot.conversations[1];
        assert!(opened.snapshot.shows_conversation(branch));
        assert_eq!(branch.workspace, "/trees/linux/src");
        assert_eq!(branch.project_workspace.as_deref(), Some("/workspace"));
        Ok(())
    }
}
