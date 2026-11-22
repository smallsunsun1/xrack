use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelInfo {
    pub id: String,
    pub name: String,
    pub efforts: Vec<String>,
    pub default_effort: String,
    pub allows_images: Option<bool>,
}

impl ModelInfo {
    pub(crate) fn parse(value: &Value) -> Option<Self> {
        let id = value["model"].as_str().filter(|id| !id.is_empty())?;
        if value["hidden"].as_bool() == Some(true) {
            return None;
        }
        let mut efforts = Vec::new();
        if let Some(options) = value["supportedReasoningEfforts"].as_array() {
            for effort in options
                .iter()
                .filter_map(|option| option["reasoningEffort"].as_str())
                .filter(|name| !name.is_empty())
            {
                if !efforts.iter().any(|known| known == effort) {
                    efforts.push(effort.to_owned());
                }
            }
        }
        let default_effort = value["defaultReasoningEffort"]
            .as_str()
            .filter(|effort| efforts.iter().any(|supported| supported == effort))
            .unwrap_or_default();
        Some(Self {
            id: id.into(),
            name: value["displayName"]
                .as_str()
                .filter(|name| !name.trim().is_empty())
                .unwrap_or(id)
                .into(),
            efforts,
            default_effort: default_effort.into(),
            allows_images: value["inputModalities"]
                .as_array()
                .map(|modes| modes.iter().any(|mode| mode == "image")),
        })
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TurnOptions {
    pub model: String,
    pub effort: String,
}

impl TurnOptions {
    pub(crate) fn from_thread(thread: &Value) -> Self {
        Self {
            model: thread["model"].as_str().unwrap_or_default().into(),
            effort: thread["reasoningEffort"]
                .as_str()
                .unwrap_or_default()
                .into(),
        }
    }

    pub fn supported(&self, models: &[ModelInfo]) -> Self {
        let Some(model) = models.iter().find(|model| model.id == self.model) else {
            return Self::default();
        };
        Self {
            model: model.id.clone(),
            effort: if model.efforts.contains(&self.effort) {
                self.effort.clone()
            } else {
                model.default_effort.clone()
            },
        }
    }
}
