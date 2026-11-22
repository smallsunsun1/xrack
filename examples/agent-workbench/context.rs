use serde_json::Value;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TokenCounts {
    pub total: u64,
    pub input: u64,
    pub cached: u64,
    pub output: u64,
    pub reasoning: u64,
}

impl TokenCounts {
    fn parse(value: &Value) -> Option<Self> {
        Some(Self {
            total: value["totalTokens"].as_u64()?,
            input: value["inputTokens"].as_u64().unwrap_or_default(),
            cached: value["cachedInputTokens"].as_u64().unwrap_or_default(),
            output: value["outputTokens"].as_u64().unwrap_or_default(),
            reasoning: value["reasoningOutputTokens"].as_u64().unwrap_or_default(),
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContextUsage {
    pub last: TokenCounts,
    pub total: TokenCounts,
    pub window: Option<u64>,
}

impl ContextUsage {
    pub(crate) fn parse(value: &Value) -> Option<Self> {
        Some(Self {
            last: TokenCounts::parse(&value["last"])?,
            total: TokenCounts::parse(&value["total"])?,
            window: value["modelContextWindow"]
                .as_u64()
                .filter(|window| *window > 0),
        })
    }

    pub fn fraction(&self) -> Option<f64> {
        Some((self.last.total as f64 / self.window? as f64).clamp(0.0, 1.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn reports_counts_without_inventing_an_unknown_context_window() {
        let usage = ContextUsage::parse(&json!({
            "last":{"totalTokens":120},
            "total":{"totalTokens":800},
            "modelContextWindow":null,
        }))
        .unwrap();
        assert_eq!(usage.last.total, 120);
        assert_eq!(usage.total.total, 800);
        assert_eq!(usage.fraction(), None);
        let small = ContextUsage {
            window: Some(100),
            ..usage
        };
        assert_eq!(small.fraction(), Some(1.0));
    }
}
