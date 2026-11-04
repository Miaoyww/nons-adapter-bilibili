//! Bilibili protocol guest. Network and state access are scoped by the Host.
pub mod api;
pub mod model;
pub mod wbi;

#[cfg(target_arch = "wasm32")]
mod component;

use model::Result;
use serde_json::{json, Value};

pub const MAX_JSON: usize = 64 * 1024;

pub struct Response {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

pub trait Host {
    fn http(
        &self,
        method: &str,
        url: &str,
        headers: Vec<(String, String)>,
        body: Vec<u8>,
        timeout_ms: u32,
    ) -> Result<Response>;
    fn now_ms(&self) -> u64;
    fn state_get(&self, key: &str) -> Result<Option<String>>;
    fn state_set(&self, key: &str, value: &str) -> Result<()>;
    fn credential_read(&self) -> Result<Vec<u8>>;
    fn credential_publish(&self, metadata: &str, data: &[u8]) -> Result<()>;
    fn credential_replace(&self, data: &[u8]) -> Result<()>;
    fn credential_invalidate(&self) -> Result<()>;
    fn diagnostic(&self, stage: &str, class: &str, code: &str);
}

#[derive(Default)]
pub struct Adapter {
    login: Option<String>,
}
impl Adapter {
    pub fn call(&mut self, host: &impl Host, method: &str, args: &str) -> String {
        let result = self.dispatch(host, method, args).and_then(|value| {
            let encoded = json!({"status":"ok","data":value}).to_string();
            if encoded.len() > MAX_JSON {
                return Err(model::Error::new("invalidData", "transport.reply-size"));
            }
            Ok(encoded)
        });
        match result {
            Ok(value) => value,
            Err(error) => {
                host.diagnostic(&error.stage, "protocol", &error.code);
                json!({"status":"error","data":{"code":error.code,"retryAfterMs":error.retry_after_ms}})
                    .to_string()
            }
        }
    }
    fn dispatch(&mut self, host: &impl Host, method: &str, args: &str) -> Result<Value> {
        if args.len() > MAX_JSON {
            return Err(model::Error::new("invalidData", "transport.args-size"));
        }
        if method == "music.descriptor" {
            let manifest: Value = serde_json::from_str(include_str!("../manifest.json"))
                .map_err(|_| model::Error::new("invalidData", "descriptor.json"))?;
            return Ok(manifest["descriptor"].clone());
        }
        let input: Value = serde_json::from_str(args)
            .map_err(|_| model::Error::new("invalidData", "transport.args-json"))?;
        // 登录靠 HTTP 轮询，运行时事件无 session 包装，只需维持会话存活。
        if method == "runtime:event" {
            return Ok(Value::Null);
        }
        if input["session"]["source"] != "bilibili" {
            return Err(model::Error::new("permissionDenied", "transport.source"));
        }
        let request = &input["request"];
        match method {
            "music.prepare-login" => {
                api::prepare_login(request)?;
                Ok(Value::Null)
            }
            "music.account" => api::account(host, &mut self.login, request),
            "music.read-track" => api::read_track(host, request),
            "music.business" => {
                if request["operation"] != "search" || request["kind"] != "track" {
                    return Err(model::Error::new("unsupported", "business.operation"));
                }
                api::search_tracks(host, request)
            }
            "music.resolve-playback" => api::resolve_playback(host, request),
            "music.search-mvs" => api::search_mvs(host, request),
            "music.resolve-mv" => api::resolve_mv(host, request),
            _ => Err(model::Error::new("unsupported", "transport.method")),
        }
    }
}
