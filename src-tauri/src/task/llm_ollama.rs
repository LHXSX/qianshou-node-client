use async_trait::async_trait;
use serde_json::{json, Value};

use super::llm_runtime::{LlmInferRequest, LlmInferResponse, LLMRuntime};

const DEFAULT_OLLAMA_BASE: &str = "http://127.0.0.1:11434";
const DEFAULT_MODEL: &str = "qwen2.5:1.5b";
const DEFAULT_TEMPERATURE: f32 = 0.2;
const DEFAULT_MAX_TOKENS: u32 = 2048;

pub struct OllamaRuntime {
    base_url: String,
    model: String,
    client: reqwest::Client,
}

impl OllamaRuntime {
    pub fn new(base_url: Option<&str>, model: Option<&str>) -> Self {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(120))
            .build()
            .expect("OllamaRuntime reqwest client build");

        Self {
            base_url: base_url.unwrap_or(DEFAULT_OLLAMA_BASE).trim_end_matches('/').to_string(),
            model: model.unwrap_or(DEFAULT_MODEL).to_string(),
            client,
        }
    }

    pub fn with_env() -> Self {
        let base_url = std::env::var("OLLAMA_BASE_URL").ok();
        let model = std::env::var("OLLAMA_MODEL").ok();
        Self::new(base_url.as_deref(), model.as_deref())
    }

    fn chat_url(&self) -> String {
        format!("{}/v1/chat/completions", self.base_url)
    }

    fn health_url(&self) -> String {
        format!("{}/api/tags", self.base_url)
    }
}

#[async_trait]
impl LLMRuntime for OllamaRuntime {
    async fn infer(&self, req: &LlmInferRequest) -> Result<LlmInferResponse, String> {
        let temperature = req.temperature.unwrap_or(DEFAULT_TEMPERATURE);
        let max_tokens = req.max_tokens.unwrap_or(DEFAULT_MAX_TOKENS);

        let mut body = json!({
            "model": self.model,
            "messages": [
                {"role": "user", "content": req.prompt}
            ],
            "temperature": temperature,
            "max_tokens": max_tokens,
            "stream": false,
        });

        if let Some(ref tools) = req.tools {
            body["tools"] = tools.clone();
        }

        if let Some(ref schema) = req.output_schema {
            body["response_format"] = json!({
                "type": "json_schema",
                "json_schema": {
                    "name": "output",
                    "schema": schema,
                }
            });
        }

        let resp = self
            .client
            .post(&self.chat_url())
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("ollama 请求失败: {}", e))?;

        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let text = resp.text().await.unwrap_or_default();
            return Err(format!("ollama HTTP {}: {}", status, text));
        }

        let data: Value = resp
            .json()
            .await
            .map_err(|e| format!("ollama 响应解析失败: {}", e))?;

        let choice = data["choices"]
            .get(0)
            .ok_or_else(|| format!("ollama 返回空 choices: {}", data))?;

        let message = &choice["message"];
        let content = message["content"]
            .as_str()
            .unwrap_or("")
            .to_string();

        let tool_calls = message.get("tool_calls").cloned();

        let tokens_used = data["usage"]["total_tokens"]
            .as_u64()
            .unwrap_or(0) as u32;

        Ok(LlmInferResponse {
            content,
            tool_calls,
            model: self.model.clone(),
            tokens_used,
        })
    }

    async fn health_check(&self) -> Result<bool, String> {
        let resp = self
            .client
            .get(&self.health_url())
            .send()
            .await
            .map_err(|e| format!("ollama 健康检查失败: {}", e))?;

        Ok(resp.status().is_success())
    }

    fn runtime_name(&self) -> &'static str {
        "ollama"
    }
}

/// 启动时探测本机是否运行 Ollama 并取出已加载模型 tag。
///
/// 返回 `Some(models)` 表示 Ollama 在线（即使模型列表为空，也算装了 runtime）；
/// 返回 `None` 表示本机没装/没启动 Ollama（连接被拒、404、超时等）。
///
/// 用于客户端 hello 帧的 `ai_runtime_ready` + `ollama_models` 上报。
pub async fn detect_ollama() -> Option<Vec<String>> {
    let base = std::env::var("OLLAMA_BASE_URL")
        .unwrap_or_else(|_| DEFAULT_OLLAMA_BASE.to_string());
    let url = format!("{}/api/tags", base.trim_end_matches('/'));
    let client = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(2))
        .build()
    {
        Ok(c) => c,
        Err(_) => return None,
    };
    let resp = match client.get(&url).send().await {
        Ok(r) => r,
        Err(_) => return None,
    };
    if !resp.status().is_success() {
        return None;
    }
    let body: Value = match resp.json().await {
        Ok(v) => v,
        Err(_) => return Some(Vec::new()),  // 在线但没拿到 JSON
    };
    let models = body
        .get("models")
        .and_then(|m| m.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|m| {
                    m.get("name")
                        .and_then(|n| n.as_str())
                        .map(|s| s.to_string())
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    Some(models)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    #[ignore = "需要本地 ollama 运行"]
    async fn test_ollama_health() {
        let rt = OllamaRuntime::with_env();
        let ok = rt.health_check().await;
        assert!(ok.is_ok());
    }
}