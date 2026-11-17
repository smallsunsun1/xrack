use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    fs,
    io::{self, BufReader},
    path::Path,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Pending,
    Active,
    Completed,
}

impl Status {
    pub(crate) const ALL: [Self; 3] = [Self::Pending, Self::Active, Self::Completed];

    pub(crate) fn index(self) -> i64 {
        match self {
            Self::Pending => 0,
            Self::Active => 1,
            Self::Completed => 2,
        }
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Pending => "待处理",
            Self::Active => "处理中",
            Self::Completed => "已完成",
        }
    }

    pub(crate) fn from_index(index: i64) -> Option<Self> {
        usize::try_from(index)
            .ok()
            .and_then(|index| Self::ALL.get(index).copied())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Record {
    pub id: i64,
    pub asset: String,
    pub site: String,
    pub owner: String,
    pub status: Status,
    pub note: String,
}

impl Record {
    pub(crate) fn code(&self) -> String {
        format!("INSP-{:05}", self.id)
    }

    pub(crate) fn validate(&self) -> io::Result<()> {
        if self.id <= 0 || self.asset.trim().is_empty() || self.site.trim().is_empty() {
            return Err(invalid(format!(
                "记录 {} 缺少有效编号、设备或站点",
                self.id
            )));
        }
        if self.owner.trim().is_empty() {
            return Err(invalid("请填写负责人"));
        }
        if self.status == Status::Completed && self.note.trim().is_empty() {
            return Err(invalid("标记完成前请填写复检备注"));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEntry {
    pub record: i64,
    pub previous: Status,
    pub current: Status,
    pub owner: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    schema: u32,
    pub name: String,
    pub records: Vec<Record>,
    #[serde(default)]
    pub history: Vec<AuditEntry>,
}

impl Project {
    pub fn sample(count: usize) -> Self {
        let assets = ["配电柜", "循环泵", "温度探头", "排烟风机", "控制器"];
        let sites = ["北区机房", "东侧车间", "西区仓库", "中央监控室"];
        let teams = ["张晨", "李珂", "周予", "陈洁"];
        let records = (0..count)
            .map(|index| {
                let status = match index % 5 {
                    0 | 1 => Status::Pending,
                    2 => Status::Active,
                    _ => Status::Completed,
                };
                Record {
                    id: index as i64 + 1,
                    asset: format!(
                        "{} {}",
                        assets[index % assets.len()],
                        index / assets.len() + 1
                    ),
                    site: sites[index % sites.len()].into(),
                    owner: teams[index % teams.len()].into(),
                    status,
                    note: if status == Status::Completed {
                        "复检通过".into()
                    } else {
                        String::new()
                    },
                }
            })
            .collect();
        Self {
            schema: 1,
            name: "北区设施巡检".into(),
            records,
            history: Vec::new(),
        }
    }

    pub fn read(path: &Path) -> io::Result<Self> {
        let project: Self = serde_json::from_reader(BufReader::new(fs::File::open(path)?))?;
        project.validate()?;
        Ok(project)
    }

    pub fn write(&self, path: &Path) -> io::Result<()> {
        self.validate()?;
        let parent = path
            .parent()
            .filter(|path| !path.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        let mut output = tempfile::NamedTempFile::new_in(parent)?;
        serde_json::to_writer_pretty(output.as_file_mut(), self)?;
        if let Ok(metadata) = fs::metadata(path) {
            output.as_file().set_permissions(metadata.permissions())?;
        }
        output.as_file().sync_all()?;
        output.persist(path)?;
        Ok(())
    }

    fn validate(&self) -> io::Result<()> {
        if self.schema != 1 || self.name.trim().is_empty() {
            return Err(invalid("项目名称或文件版本无效"));
        }
        let mut keys = HashSet::with_capacity(self.records.len());
        for record in &self.records {
            record.validate()?;
            if !keys.insert(record.id) {
                return Err(invalid(format!("项目中有重复记录编号 {}", record.id)));
            }
        }
        Ok(())
    }
}

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}
