use std::path::Path;
use std::str::FromStr;
use std::time::{Duration, Instant};

use base64::Engine;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use reqwest::multipart;
use reqwest::Method;

use super::cancel::HttpCancelState;
use super::models::{HttpRequestSpec, HttpResponseInfo};

pub async fn send(
    spec: HttpRequestSpec,
    cancel_token_id: Option<String>,
    cancel_state: Option<&HttpCancelState>,
) -> Result<HttpResponseInfo, String> {
    let method = Method::from_bytes(spec.method.trim().to_uppercase().as_bytes())
        .map_err(|error| format!("非法 HTTP 方法：{error}"))?;
    let timeout = Duration::from_secs(spec.timeout_seconds.unwrap_or(30).clamp(1, 300) as u64);
    let redirect = if spec.follow_redirects {
        reqwest::redirect::Policy::limited(10)
    } else {
        reqwest::redirect::Policy::none()
    };
    let client = reqwest::Client::builder()
        .timeout(timeout)
        .redirect(redirect)
        .danger_accept_invalid_certs(!spec.verify_ssl)
        .build()
        .map_err(|error| format!("构造 HTTP 客户端失败：{error}"))?;

    let mut url =
        reqwest::Url::parse(spec.url.trim()).map_err(|error| format!("URL 非法：{error}"))?;
    for parameter in &spec.query_params {
        if parameter.enabled && !parameter.key.trim().is_empty() {
            url.query_pairs_mut()
                .append_pair(&parameter.key, &parameter.value);
        }
    }

    let mut headers = HeaderMap::new();
    for header in &spec.headers {
        if !header.enabled || header.key.trim().is_empty() {
            continue;
        }
        let name = HeaderName::from_str(&header.key)
            .map_err(|error| format!("非法 header 名 {}：{error}", header.key))?;
        let value = HeaderValue::from_str(&header.value)
            .map_err(|error| format!("非法 header 值 {}：{error}", header.key))?;
        headers.insert(name, value);
    }
    match spec.auth.r#type.as_str() {
        "bearer" if !spec.auth.bearer_token.is_empty() => {
            let value = HeaderValue::from_str(&format!("Bearer {}", spec.auth.bearer_token))
                .map_err(|error| format!("非法 bearer token：{error}"))?;
            headers.insert(reqwest::header::AUTHORIZATION, value);
        }
        "basic" => {
            let raw = format!("{}:{}", spec.auth.basic_user, spec.auth.basic_pass);
            let encoded = base64::engine::general_purpose::STANDARD.encode(raw.as_bytes());
            headers.insert(
                reqwest::header::AUTHORIZATION,
                HeaderValue::from_str(&format!("Basic {encoded}")).unwrap(),
            );
        }
        _ => {}
    }

    let mut request = client.request(method, url).headers(headers);
    match spec.body_type.trim() {
        "" | "none" => {}
        "json" => {
            request = request
                .header(reqwest::header::CONTENT_TYPE, "application/json")
                .body(spec.body.clone());
        }
        "form" => {
            let mut serializer = url::form_urlencoded::Serializer::new(String::new());
            for (key, value) in url::form_urlencoded::parse(spec.body.as_bytes()).into_owned() {
                serializer.append_pair(&key, &value);
            }
            request = request
                .header(
                    reqwest::header::CONTENT_TYPE,
                    "application/x-www-form-urlencoded",
                )
                .body(serializer.finish());
        }
        "multipart" => {
            let mut form = multipart::Form::new();
            for field in &spec.multipart_fields {
                if !field.enabled || field.key.trim().is_empty() {
                    continue;
                }
                if field.kind == "file" {
                    let path = Path::new(&field.value);
                    let filename = path
                        .file_name()
                        .map(|value| value.to_string_lossy().into_owned())
                        .unwrap_or_else(|| field.key.clone());
                    let bytes = tokio::fs::read(path)
                        .await
                        .map_err(|error| format!("读取文件 {} 失败：{error}", field.value))?;
                    form = form.part(
                        field.key.clone(),
                        multipart::Part::bytes(bytes).file_name(filename),
                    );
                } else {
                    form = form.text(field.key.clone(), field.value.clone());
                }
            }
            request = request.multipart(form);
        }
        _ => request = request.body(spec.body.clone()),
    }

    let started = Instant::now();
    let cancel_rx = cancel_token_id
        .as_ref()
        .and_then(|id| cancel_state.map(|state| state.register(id.clone())));
    let response_result = if let Some(receiver) = cancel_rx {
        tokio::select! {
            result = request.send() => result,
            _ = receiver => return Err("请求已取消".to_string()),
        }
    } else {
        request.send().await
    };
    if let (Some(id), Some(state)) = (cancel_token_id.as_deref(), cancel_state) {
        state.unregister(id);
    }
    let response = response_result.map_err(|error| format!("请求失败：{error}"))?;

    let final_url = response.url().to_string();
    let status = response.status();
    let status_text = status.canonical_reason().unwrap_or_default().to_string();
    let response_headers = response
        .headers()
        .iter()
        .map(|(key, value)| {
            (
                key.to_string(),
                value.to_str().unwrap_or("<binary>").to_string(),
            )
        })
        .collect();
    let bytes = response
        .bytes()
        .await
        .map_err(|error| format!("读取响应体失败：{error}"))?;
    Ok(HttpResponseInfo {
        status: status.as_u16(),
        status_text,
        headers: response_headers,
        body: String::from_utf8_lossy(&bytes).into_owned(),
        body_bytes: bytes.len() as u64,
        elapsed_ms: started.elapsed().as_millis(),
        final_url,
    })
}
