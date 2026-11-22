use serde_json::{Map, Value, json};
use std::collections::HashSet;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuestionOption {
    pub label: String,
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuestionPage {
    pub token: i64,
    pub index: usize,
    pub total: usize,
    pub header: String,
    pub text: String,
    pub options: Vec<QuestionOption>,
    pub allows_text: bool,
    pub secret: bool,
}

struct Question {
    id: String,
    page: QuestionPage,
}

pub(crate) struct PendingInput {
    pub request: Value,
    token: i64,
    index: usize,
    questions: Vec<Question>,
    answers: Map<String, Value>,
}

impl PendingInput {
    pub fn parse(request: Value, token: i64, payload: &Value) -> Option<Self> {
        let questions = payload["questions"].as_array()?;
        if !(1..=3).contains(&questions.len()) {
            return None;
        }
        let mut seen = HashSet::new();
        let mut parsed = Vec::with_capacity(questions.len());
        for (index, question) in questions.iter().enumerate() {
            let id = question["id"].as_str().filter(|id| !id.is_empty())?;
            if !seen.insert(id) {
                return None;
            }
            let options = question["options"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|option| {
                    Some(QuestionOption {
                        label: option["label"]
                            .as_str()
                            .filter(|label| !label.is_empty())?
                            .into(),
                        description: option["description"].as_str().unwrap_or_default().into(),
                    })
                })
                .collect::<Vec<_>>();
            let secret = question["isSecret"].as_bool() == Some(true);
            let allows_text = question["isOther"].as_bool() == Some(true) || options.is_empty();
            parsed.push(Question {
                id: id.into(),
                page: QuestionPage {
                    token,
                    index,
                    total: questions.len(),
                    options,
                    allows_text,
                    secret,
                    header: question["header"].as_str().unwrap_or_default().into(),
                    text: question["question"].as_str()?.into(),
                },
            });
        }
        Some(Self {
            request,
            token,
            index: 0,
            questions: parsed,
            answers: Map::new(),
        })
    }

    pub fn current(&self) -> Option<&QuestionPage> {
        Some(&self.questions.get(self.index)?.page)
    }

    pub fn answer(&mut self, token: i64, index: i64, value: Option<String>) -> bool {
        let Some(question) = self.questions.get(self.index) else {
            return false;
        };
        if token != self.token || usize::try_from(index).ok() != Some(self.index) {
            return false;
        }
        if let Some(answer) = value {
            if answer.trim().is_empty()
                || !question.page.allows_text
                    && !question
                        .page
                        .options
                        .iter()
                        .any(|option| option.label == answer)
            {
                return false;
            }
            self.answers
                .insert(question.id.clone(), json!({"answers":[answer]}));
        }
        self.index += 1;
        true
    }

    pub fn response(self) -> Value {
        json!({"jsonrpc":"2.0","id":self.request,"result":{"answers":self.answers}})
    }
}
