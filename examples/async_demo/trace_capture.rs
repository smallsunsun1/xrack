use std::fmt;
use tracing::{
    Event, Id, Subscriber,
    field::{Field, Visit},
    span::Attributes,
};
use tracing_subscriber::{Layer, layer::Context, registry::LookupSpan};
use xrack_runtime::ui_trace::{Buffer, Field as Value, Fields, TARGET};

pub struct Capture(pub Buffer);

#[derive(Default)]
struct SpanFields(Fields);

impl Visit for SpanFields {
    fn record_u64(&mut self, field: &Field, value: u64) {
        self.0.insert(field.name(), Value::Number(value));
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        self.0.insert(field.name(), Value::Text(value.into()));
    }

    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        if field.name() == "requests" {
            self.0
                .insert(field.name(), Value::Text(format!("{value:?}")));
        }
    }
}

impl<S> Layer<S> for Capture
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    fn on_new_span(&self, attributes: &Attributes<'_>, id: &Id, context: Context<'_, S>) {
        if attributes.metadata().target() == TARGET
            && let Some(span) = context.span(id)
        {
            let mut fields = SpanFields::default();
            attributes.record(&mut fields);
            span.extensions_mut().insert(fields);
        }
    }

    fn on_event(&self, event: &Event<'_>, context: Context<'_, S>) {
        if event.metadata().target() != TARGET {
            return;
        }
        let mut fields = SpanFields::default();
        if let Some(scope) = context.event_scope(event) {
            for span in scope.from_root() {
                if let Some(inherited) = span.extensions().get::<SpanFields>() {
                    fields.0.extend(inherited.0.clone());
                }
            }
        }
        event.record(&mut fields);
        self.0.push(fields.0);
    }
}

pub fn events_json(events: Vec<Fields>) -> serde_json::Value {
    serde_json::Value::Array(
        events
            .into_iter()
            .map(|fields| {
                serde_json::Value::Object(
                    fields
                        .into_iter()
                        .map(|(key, value)| {
                            let value = match value {
                                Value::Number(value) => value.into(),
                                Value::Text(value) => value.into(),
                            };
                            (key.into(), value)
                        })
                        .collect(),
                )
            })
            .collect(),
    )
}
