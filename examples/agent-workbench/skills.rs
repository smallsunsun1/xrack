use serde_json::Value;
use std::{collections::HashSet, path::Path};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Skill {
    pub name: String,
    pub path: String,
    pub description: String,
}

#[derive(Clone)]
pub struct SkillCatalog {
    pub workspace: String,
    pub state: SkillCatalogState,
}

#[derive(Clone)]
pub enum SkillCatalogState {
    Loading,
    Ready(Vec<Skill>),
    Failed(String),
}

pub(crate) fn parse_catalog(result: &Value, workspace: &str) -> Result<Vec<Skill>, String> {
    let entry = result["data"]
        .as_array()
        .and_then(|entries| entries.iter().find(|entry| entry["cwd"] == workspace))
        .ok_or("Codex returned no catalog for the requested working directory")?;
    let skills = entry["skills"].as_array().ok_or("Missing skills array")?;
    let mut paths = HashSet::new();
    let mut skills: Vec<_> = skills
        .iter()
        .filter_map(|skill| {
            let name = skill["name"].as_str()?.trim();
            let path = skill["path"].as_str()?;
            let file = Path::new(path);
            if skill["enabled"] != true
                || name.is_empty()
                || !file.is_absolute()
                || file.file_name()? != "SKILL.md"
                || !paths.insert(path)
            {
                return None;
            }
            let description = [
                &skill["interface"]["shortDescription"],
                &skill["shortDescription"],
                &skill["description"],
            ]
            .into_iter()
            .filter_map(Value::as_str)
            .find(|description| !description.trim().is_empty())
            .unwrap_or_default();
            Some(Skill {
                name: name.into(),
                path: path.into(),
                description: description.split_whitespace().collect::<Vec<_>>().join(" "),
            })
        })
        .collect();
    if skills.is_empty()
        && let Some(error) = entry["errors"]
            .as_array()
            .and_then(|errors| errors.iter().find_map(|error| error["message"].as_str()))
    {
        return Err(error.into());
    }
    skills.sort_by(|a, b| (&a.name, &a.path).cmp(&(&b.name, &b.path)));
    Ok(skills)
}
