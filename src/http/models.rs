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
