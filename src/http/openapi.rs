use std::time::Duration;

use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use serde::{Deserialize, Serialize};

const BODY_LIMIT_BYTES: usize = 5 * 1024 * 1024;

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenApiHeader {
    pub key: String,
    pub value: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenApiImportParams {
    pub url: String,
    #[serde(default)]
    pub headers: Vec<OpenApiHeader>,
}

pub async fn fetch_openapi(params: OpenApiImportParams) -> Result<String, String> {
    let parsed =
        reqwest::Url::parse(&params.url).map_err(|error| format!("URL 无法解析：{error}"))?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err(format!(
            "不支持的 scheme：{}（只允许 http/https）",
            parsed.scheme()
        ));
    }
    let mut headers = HeaderMap::new();
    for header in params.headers {
        if header.key.trim().is_empty() {
            continue;
        }
        let name = HeaderName::from_bytes(header.key.as_bytes())
            .map_err(|error| format!("非法 header key `{}`：{error}", header.key))?;
        let value = HeaderValue::from_str(&header.value)
            .map_err(|error| format!("非法 header value（key=`{}`）：{error}", header.key))?;
        headers.insert(name, value);
    }
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .redirect(reqwest::redirect::Policy::limited(3))
        .build()
        .map_err(|error| format!("创建 HTTP 客户端失败：{error}"))?;
    let mut response = client
        .get(parsed)
        .headers(headers)
        .send()
        .await
        .map_err(|error| format!("请求失败：{error}"))?;
    if let Some(length) = response.content_length() {
        if length > BODY_LIMIT_BYTES as u64 {
            return Err("响应体超过 5 MB 上限".into());
        }
    }
    let status = response.status();
    let mut body = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|error| format!("读取响应流失败：{error}"))?
    {
        if body.len() + chunk.len() > BODY_LIMIT_BYTES {
            return Err("响应体超过 5 MB 上限".into());
        }
        body.extend_from_slice(&chunk);
    }
    if !status.is_success() {
        let snippet: String = String::from_utf8_lossy(&body).chars().take(500).collect();
        return Err(format!("HTTP {}: {snippet}", status.as_u16()));
    }
    String::from_utf8(body).map_err(|error| format!("响应体不是合法 UTF-8：{error}"))
}
