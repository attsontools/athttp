use serde::{Deserialize, Serialize};

fn default_true() -> bool {
    true
}

fn default_timeout() -> Option<u32> {
    Some(30)
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KeyValue {
    pub key: String,
    pub value: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub description: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct HttpAuth {
    #[serde(default = "default_auth_type")]
    pub r#type: String,
    #[serde(default)]
    pub bearer_token: String,
    #[serde(default)]
    pub basic_user: String,
    #[serde(default)]
    pub basic_pass: String,
}

fn default_auth_type() -> String {
    "none".to_string()
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MultipartField {
    pub key: String,
    pub kind: String,
    pub value: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HttpRequestSpec {
    pub method: String,
    pub url: String,
    #[serde(default)]
    pub headers: Vec<KeyValue>,
    #[serde(default)]
    pub query_params: Vec<KeyValue>,
    #[serde(default)]
    pub auth: HttpAuth,
    #[serde(default)]
    pub body_type: String,
    #[serde(default)]
    pub body: String,
    #[serde(default)]
    pub multipart_fields: Vec<MultipartField>,
    #[serde(default = "default_timeout")]
    pub timeout_seconds: Option<u32>,
    #[serde(default = "default_true")]
    pub follow_redirects: bool,
    #[serde(default = "default_true")]
    pub verify_ssl: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HttpSendParams {
    pub request: HttpRequestSpec,
    #[serde(default)]
    pub cancel_token_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HttpResponseInfo {
    pub status: u16,
    pub status_text: String,
    pub headers: Vec<(String, String)>,
    pub body: String,
    pub body_bytes: u64,
    pub elapsed_ms: u128,
    pub final_url: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HttpTabRow {
    pub id: String,
    pub title: String,
    pub order_index: i64,
    pub is_active: bool,
    pub spec_json: String,
    pub updated_at: i64,
    #[serde(default = "default_kind")]
    pub kind: String,
}

fn default_kind() -> String {
    "http".to_string()
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HttpHistoryRow {
    pub id: String,
    pub method: String,
    pub url: String,
    pub status: Option<i64>,
    pub elapsed_ms: Option<i64>,
    pub body_bytes: Option<i64>,
    pub spec_json: String,
    pub resp_summary: Option<String>,
    pub created_at: i64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HttpEnvRow {
    pub id: String,
    pub name: String,
    pub is_active: bool,
    pub order_index: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HttpEnvVarRow {
    pub id: String,
    pub env_id: String,
    pub key: String,
    pub value: String,
    pub enabled: bool,
    pub order_index: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HttpCollectionRow {
    pub id: String,
    pub name: String,
    pub order_index: i64,
    pub updated_at: i64,
    #[serde(default)]
    pub source_url: Option<String>,
    #[serde(default)]
    pub source_headers_json: Option<String>,
    #[serde(default)]
    pub sync_interval_secs: Option<i64>,
    #[serde(default)]
    pub last_synced_at: Option<i64>,
    #[serde(default)]
    pub last_sync_error: Option<String>,
    #[serde(default)]
    pub base_url: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HttpCollectionFolderRow {
    pub id: String,
    pub collection_id: String,
    pub parent_id: Option<String>,
    pub name: String,
    pub order_index: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HttpCollectionRequestRow {
    pub id: String,
    pub collection_id: String,
    pub folder_id: Option<String>,
    pub name: String,
    pub method: String,
    pub spec_json: String,
    pub order_index: i64,
    pub updated_at: i64,
    #[serde(default)]
    pub source_key: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SseSpec {
    pub url: String,
    #[serde(default)]
    pub headers: Vec<KeyValue>,
    #[serde(default)]
    pub query_params: Vec<KeyValue>,
    #[serde(default)]
    pub auth: HttpAuth,
    #[serde(default)]
    pub timeout_seconds: Option<u32>,
    #[serde(default = "default_true")]
    pub verify_ssl: bool,
    #[serde(default)]
    pub last_event_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum StreamMessage {
    Open {
        at_ms: u64,
        status: Option<u16>,
        headers: Vec<(String, String)>,
    },
    SseEvent {
        at_ms: u64,
        event: String,
        data: String,
        id: Option<String>,
        retry_ms: Option<u64>,
        truncated: bool,
    },
    Closed {
        at_ms: u64,
        code: Option<u16>,
        reason: String,
    },
    Error {
        at_ms: u64,
        message: String,
    },
    BufferTruncated {
        at_ms: u64,
        dropped: usize,
    },
}

impl StreamMessage {
    pub fn approx_size(&self) -> usize {
        match self {
            Self::Open { headers, .. } => {
                64 + headers
                    .iter()
                    .map(|(k, v)| k.len() + v.len())
                    .sum::<usize>()
            }
            Self::SseEvent {
                event, data, id, ..
            } => 32 + event.len() + data.len() + id.as_deref().map(str::len).unwrap_or(0),
            Self::Closed { reason, .. } => 24 + reason.len(),
            Self::Error { message, .. } => 24 + message.len(),
            Self::BufferTruncated { .. } => 32,
        }
    }
}
