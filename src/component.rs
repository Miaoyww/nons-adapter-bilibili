use crate::model::Result as AdapterResult;
use crate::{Adapter, Host, Response};
use std::cell::RefCell;

wit_bindgen::generate!({
    path: ["generated-wit/plugin.wit", "generated-wit/host.wit", "interfaces/wasi-clocks", "interfaces/wasi-random"],
    inline: "package nons:bilibili; world adapter { import nons:host/transport@1.1.0; import nons:plugin/host@1.0.0; import wasi:clocks/wall-clock@0.2.0; import wasi:random/random@0.2.0; export initialize: func() -> result<_, string>; export shutdown: func() -> result<_, string>; export call: func(method: string, args: string) -> result<string, string>; }",
    world: "nons:bilibili/adapter",
    generate_all,
});

struct Component;
struct Transport;
thread_local! { static ADAPTER: RefCell<Adapter> = RefCell::new(Adapter::default()); }
use nons::host::transport;

impl Host for Transport {
    fn http(
        &self,
        method: &str,
        url: &str,
        headers: Vec<(String, String)>,
        body: Vec<u8>,
        follow_redirects: bool,
        timeout_ms: u32,
    ) -> AdapterResult<Response> {
        let response = transport::http_request(&transport::Request {
            url: url.into(),
            method: method.into(),
            headers: headers
                .into_iter()
                .map(|(name, value)| transport::Header { name, value })
                .collect(),
            body,
            follow_redirects,
            timeout_ms,
        })
        .map_err(|error| crate::model::Error::host(&error, "host.http"))?;
        Ok(Response {
            status: response.status,
            headers: response
                .headers
                .into_iter()
                .map(|header| (header.name, header.value))
                .collect(),
            body: response.body,
            url: response.url,
        })
    }
    fn now_ms(&self) -> u64 {
        let now = wasi::clocks::wall_clock::now();
        now.seconds
            .saturating_mul(1000)
            .saturating_add((now.nanoseconds / 1_000_000) as u64)
    }
    fn state_get(&self, key: &str) -> AdapterResult<Option<String>> {
        transport::state_get(key)
            .map_err(|error| crate::model::Error::host(&error, "host.state-get"))
    }
    fn state_set(&self, key: &str, value: &str) -> AdapterResult<()> {
        transport::state_set(key, value)
            .map_err(|error| crate::model::Error::host(&error, "host.state-set"))
    }
    fn credential_read(&self) -> AdapterResult<Vec<u8>> {
        transport::credential_read()
            .map_err(|error| crate::model::Error::host(&error, "host.credential-read"))
    }
    fn credential_publish(&self, metadata: &str, data: &[u8]) -> AdapterResult<()> {
        transport::credential_publish(metadata, data)
            .map_err(|error| crate::model::Error::host(&error, "host.credential-publish"))
    }
    fn credential_replace(&self, data: &[u8]) -> AdapterResult<()> {
        transport::credential_replace(data)
            .map_err(|error| crate::model::Error::host(&error, "host.credential-replace"))
    }
    fn credential_invalidate(&self) -> AdapterResult<()> {
        transport::credential_invalidate()
            .map_err(|error| crate::model::Error::host(&error, "host.credential-invalidate"))
    }
    fn diagnostic(&self, stage: &str, class: &str, code: &str) {
        let _ = nons::plugin::host::call(
            "diagnostic.report",
            &serde_json::json!({"stage":stage,"class":class,"code":code}).to_string(),
        );
    }
}

impl Guest for Component {
    fn initialize() -> std::result::Result<(), String> {
        Ok(())
    }
    fn shutdown() -> std::result::Result<(), String> {
        Ok(())
    }
    fn call(method: String, args: String) -> std::result::Result<String, String> {
        Ok(ADAPTER.with(|adapter| adapter.borrow_mut().call(&Transport, &method, &args)))
    }
}
export!(Component);
