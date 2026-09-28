//! Structured logs forwarded to Kotlin.
//!
//! The core logs with `tracing`, as the rest of the workspace does. On
//! Android nothing reads stderr, so Kotlin installs a [`LogSink`] once at
//! startup and forwards each record to `android.util.Log` (or wherever). Each
//! record keeps its key-value fields separate from the message, so nothing
//! is flattened into a string before Kotlin sees it.

use std::sync::Arc;

use tracing::{
    Event, Subscriber,
    field::{Field, Visit},
};
use tracing_subscriber::{
    Layer, filter::LevelFilter, layer::Context, layer::SubscriberExt, util::SubscriberInitExt,
};

/// Log severity, most severe first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum LogLevel {
    Error,
    Warn,
    Info,
    Debug,
    Trace,
}

/// One log event.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct LogRecord {
    pub level: LogLevel,
    /// The Rust module that logged, e.g. `rostrum_ffi::feed`.
    pub target: String,
    pub message: String,
    pub fields: Vec<LogField>,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct LogField {
    pub key: String,
    pub value: String,
}

/// Where the core's logs go. Called on whichever thread logged; keep it
/// cheap and never call back into the core from it.
#[uniffi::export(with_foreign)]
pub trait LogSink: Send + Sync {
    fn log(&self, record: LogRecord);
}

/// Route the core's logs at `level` and above to `sink`. Only the first call
/// in a process takes effect; later calls return `false`.
#[uniffi::export]
pub fn install_log_sink(sink: Arc<dyn LogSink>, level: LogLevel) -> bool {
    tracing_subscriber::registry()
        .with(SinkLayer { sink }.with_filter(LevelFilter::from(level)))
        .try_init()
        .is_ok()
}

impl From<LogLevel> for LevelFilter {
    fn from(level: LogLevel) -> Self {
        match level {
            LogLevel::Error => Self::ERROR,
            LogLevel::Warn => Self::WARN,
            LogLevel::Info => Self::INFO,
            LogLevel::Debug => Self::DEBUG,
            LogLevel::Trace => Self::TRACE,
        }
    }
}

impl From<&tracing::Level> for LogLevel {
    fn from(level: &tracing::Level) -> Self {
        match *level {
            tracing::Level::ERROR => Self::Error,
            tracing::Level::WARN => Self::Warn,
            tracing::Level::INFO => Self::Info,
            tracing::Level::DEBUG => Self::Debug,
            tracing::Level::TRACE => Self::Trace,
        }
    }
}

struct SinkLayer {
    sink: Arc<dyn LogSink>,
}

impl<S: Subscriber> Layer<S> for SinkLayer {
    fn on_event(&self, event: &Event<'_>, _: Context<'_, S>) {
        self.sink.log(record_of(event));
    }
}

fn record_of(event: &Event<'_>) -> LogRecord {
    let mut fields = FieldCollector::default();
    event.record(&mut fields);
    let metadata = event.metadata();
    LogRecord {
        level: LogLevel::from(metadata.level()),
        target: metadata.target().to_string(),
        message: fields.message,
        fields: fields.fields,
    }
}

/// Splits an event into its `message` and the remaining key-value pairs.
#[derive(Default)]
struct FieldCollector {
    message: String,
    fields: Vec<LogField>,
}

impl FieldCollector {
    fn push(&mut self, field: &Field, value: String) {
        if field.name() == "message" {
            self.message = value;
        } else {
            self.fields.push(LogField {
                key: field.name().to_string(),
                value,
            });
        }
    }
}

impl Visit for FieldCollector {
    fn record_str(&mut self, field: &Field, value: &str) {
        self.push(field, value.to_string());
    }

    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        self.push(field, format!("{value:?}"));
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;

    #[derive(Default)]
    struct Collect(Mutex<Vec<LogRecord>>);

    impl LogSink for Collect {
        fn log(&self, record: LogRecord) {
            if let Ok(mut records) = self.0.lock() {
                records.push(record);
            }
        }
    }

    #[test]
    fn events_arrive_with_their_fields_kept_apart_from_the_message() {
        let sink = Arc::new(Collect::default());
        let subscriber = tracing_subscriber::registry()
            .with(SinkLayer { sink: sink.clone() }.with_filter(LevelFilter::from(LogLevel::Info)));
        tracing::subscriber::with_default(subscriber, || {
            tracing::info!(repo = "a/b", count = 3, "refreshed");
            tracing::debug!("filtered out");
        });

        let records = sink.0.lock().expect("lock").clone();
        assert_eq!(records.len(), 1);
        let record = &records[0];
        assert_eq!(record.level, LogLevel::Info);
        assert_eq!(record.message, "refreshed");
        assert_eq!(
            record.fields,
            vec![
                LogField {
                    key: "repo".into(),
                    value: "a/b".into()
                },
                LogField {
                    key: "count".into(),
                    value: "3".into()
                },
            ]
        );
    }
}
