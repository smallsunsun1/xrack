use crate::prompt::{attachment_label, user_input_text};
use serde_json::{Value, json};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QueuedMessage {
    pub id: String,
    pub text: String,
    input: Vec<Value>,
}

impl QueuedMessage {
    pub(crate) fn parse(message: &Value) -> Option<Self> {
        let id = message["id"].as_str().filter(|id| !id.is_empty())?;
        let input = message["input"].as_array()?.clone();
        Some(Self {
            id: id.into(),
            text: user_input_text(&message["input"]).unwrap_or_else(|| "附件消息".into()),
            input,
        })
    }

    // An XRack prompt has one plain text item and any number of attachments.
    // Text spans created by other clients may carry offsets or bindings.
    fn plain_text_slot(&self) -> Result<Option<(usize, &str)>, ()> {
        let mut slot = None;
        for (index, item) in self.input.iter().enumerate() {
            if item["type"] != "text" {
                continue;
            }
            let text = item["text"].as_str().ok_or(())?;
            if attachment_label(text).is_some() {
                continue;
            }
            if slot.is_some()
                || item
                    .get("text_elements")
                    .is_some_and(|elements| !elements.as_array().is_some_and(Vec::is_empty))
            {
                return Err(());
            }
            slot = Some((index, text));
        }
        Ok(slot)
    }

    pub fn editable_text(&self) -> Option<&str> {
        self.plain_text_slot()
            .ok()
            .map(|slot| slot.map_or("", |(_, text)| text))
    }

    pub fn has_attachments(&self) -> bool {
        self.plain_text_slot()
            .is_ok_and(|slot| self.input.len() > usize::from(slot.is_some()))
    }

    pub(crate) fn with_text(&self, text: String) -> Option<Vec<Value>> {
        let slot = self.plain_text_slot().ok()?;
        let mut input = self.input.clone();
        if text.trim().is_empty() {
            if let Some((index, _)) = slot {
                input.remove(index);
            }
        } else if let Some((index, _)) = slot {
            input[index]["text"] = json!(text);
        } else {
            input.insert(0, json!({"type":"text","text":text,"text_elements":[]}));
        }
        (!input.is_empty()).then_some(input)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_attachment_only_input_and_leaves_structured_text_spans_intact() {
        let file = json!({"type":"localImage","path":"/work/image.png","detail":"high"});
        let message = QueuedMessage::parse(&json!({"id":"file","input":[file]})).unwrap();
        assert_eq!(message.editable_text(), Some(""));
        assert!(message.has_attachments());
        let replaced = message
            .with_text("  unchanged indent\ncontinued".into())
            .unwrap();
        assert_eq!(replaced[0]["text"], "  unchanged indent\ncontinued");
        assert_eq!(replaced[1], file);

        let structured = QueuedMessage::parse(&json!({
            "id":"mention",
            "input":[{"type":"text","text":"😀 inspect","text_elements":[
                {"byteRange":{"start":0,"end":4},"placeholder":"mention"}
            ]}]
        }))
        .unwrap();
        assert_eq!(structured.editable_text(), None);
        assert!(structured.with_text("new text".into()).is_none());
        let plain = QueuedMessage::parse(&json!({
            "id":"text","input":[{"type":"text","text":"original"}]
        }))
        .unwrap();
        assert!(plain.with_text(" \n".into()).is_none());
    }
}
