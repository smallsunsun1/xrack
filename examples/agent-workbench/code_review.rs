use serde_json::{Value, json};

#[derive(Clone, Debug)]
pub enum ReviewTarget {
    WorkingTree,
    Branch(String),
    Commit(String),
}

impl ReviewTarget {
    pub(crate) fn params(&self) -> Value {
        match self {
            Self::WorkingTree => json!({"type":"uncommittedChanges"}),
            Self::Branch(branch) => json!({"type":"baseBranch","branch":branch}),
            Self::Commit(sha) => json!({"type":"commit","sha":sha}),
        }
    }

    pub(crate) fn title(&self) -> String {
        match self {
            Self::WorkingTree => "未提交改动".into(),
            Self::Branch(branch) => format!("对比 {branch}"),
            Self::Commit(sha) => format!("提交 {}", sha.chars().take(10).collect::<String>()),
        }
    }
}
