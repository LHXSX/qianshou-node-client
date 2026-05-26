use async_trait::async_trait;
use serde_json::Value;

#[derive(Debug, Clone)]
pub struct LlmInferRequest {
    pub prompt: String,
    pub tools: Option<Value>,
    pub output_schema: Option<Value>,
    pub temperature: Option<f32>,
    pub max_tokens: Option<u32>,
}

#[derive(Debug, Clone)]
pub struct LlmInferResponse {
    pub content: String,
    pub tool_calls: Option<Value>,
    pub model: String,
    pub tokens_used: u32,
}

#[async_trait]
pub trait LLMRuntime: Send + Sync {
    async fn infer(&self, req: &LlmInferRequest) -> Result<LlmInferResponse, String>;

    async fn health_check(&self) -> Result<bool, String>;

    fn runtime_name(&self) -> &'static str;
}