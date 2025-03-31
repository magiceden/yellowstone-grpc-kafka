use {
    chrono::{DateTime, Utc},
    serde::Serialize,
    serde_json::Value,
    std::collections::HashMap,
};

#[derive(Debug, Serialize)]
pub struct WssEvent<'a> {
    event: Value,
    created_at: DateTime<Utc>,
    tags: &'a HashMap<String, String>,
}

impl<'a> WssEvent<'a> {
    pub fn new(message: String, tags: &HashMap<String, String>) -> WssEvent {
        WssEvent {
            event: json5::from_str(&message).expect("Message count not be deserialized"),
            created_at: Utc::now(),
            tags,
        }
    }

    pub fn to_string(&self) -> String {
        json5::to_string(self).expect("WssEvent count not be serialized")
    }
}
