use serde_json::{json, Map, Value};

pub const MAX_DURATION_MS: u64 = 24 * 60 * 60 * 1000;

pub fn text(value: &Value) -> String {
    match value {
        Value::String(value) => value.clone(),
        Value::Number(value) => value.to_string(),
        _ => String::new(),
    }
}
pub fn first<'a>(value: &'a Value, keys: &[&str]) -> &'a Value {
    keys.iter()
        .map(|key| &value[*key])
        .find(|value| !value.is_null())
        .unwrap_or(&Value::Null)
}
pub fn integer(value: &Value) -> Option<u64> {
    let number = match value {
        Value::Number(value) => value.as_u64(),
        Value::String(value) => value.trim().parse().ok(),
        _ => None,
    }?;
    (number <= 9_007_199_254_740_991).then_some(number)
}
pub fn valid_bvid(value: &str) -> bool {
    value.len() >= 10
        && value.len() <= 16
        && value.starts_with("BV")
        && value.bytes().all(|byte| byte.is_ascii_alphanumeric())
}
pub fn parse_video_id(value: &str) -> Option<(&str, Option<u64>)> {
    let (bvid, cid) = value
        .split_once(':')
        .map_or((value, None), |(bvid, cid)| (bvid, cid.parse().ok()));
    valid_bvid(bvid).then_some((bvid, cid))
}
pub fn reference(kind: &str, id: &str) -> Value {
    json!({"source":"bilibili","kind":kind,"id":id})
}
pub fn check_reference<'a>(value: &'a Value, kind: &str) -> Result<&'a str> {
    let id = value["id"].as_str().unwrap_or("");
    if value["source"] != "bilibili"
        || !(value["kind"] == kind || kind == "mv" && value["kind"] == "track")
        || !matches!(kind, "track" | "mv")
        || parse_video_id(id).is_none()
    {
        return Err(Error::new("invalidData", "reference.source-kind-id"));
    }
    Ok(id)
}
pub fn secure_image(value: &Value) -> String {
    let url = text(value);
    let url = if url.starts_with("//") {
        format!("https:{url}")
    } else {
        url.replacen("http:", "https:", 1)
    };
    if url.contains("hdslb.com/bfs/") && !url.contains('@') {
        format!("{url}@800w_800h")
    } else {
        url
    }
}
pub fn cover(value: &Value) -> Value {
    let url = secure_image(value);
    json!({"original":url,"small":url,"normal":url,"large":url,"exlarge":url})
}
pub fn duration_text(value: &Value) -> u64 {
    let value = text(value);
    let mut seconds = 0_u64;
    for part in value.split(':') {
        let Ok(part) = part.trim().parse::<u64>() else {
            return 0;
        };
        seconds = seconds.saturating_mul(60).saturating_add(part);
    }
    seconds.saturating_mul(1000).min(MAX_DURATION_MS)
}
pub fn title(value: &Value) -> String {
    let source = text(first(value, &["title", "part"]));
    let mut text = String::with_capacity(source.len());
    let mut in_tag = false;
    for character in source.chars() {
        match character {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => text.push(character),
            _ => {}
        }
    }
    html_escape::decode_html_entities(text.trim()).into_owned()
}
pub fn track(value: &Value, requested: &Value, cid_override: Option<u64>) -> Value {
    let bvid = text(&value["bvid"]);
    let pages = value["pages"].as_array();
    let page = cid_override.and_then(|cid| {
        pages?
            .iter()
            .find(|page| integer(&page["cid"]) == Some(cid))
    });
    let cid = cid_override
        .or_else(|| integer(&value["cid"]))
        .or_else(|| pages?.first().and_then(|page| integer(&page["cid"])))
        .unwrap_or(0);
    let id = page
        .and_then(|page| integer(&page["cid"]))
        .filter(|_| cid_override.is_some())
        .map_or_else(|| bvid.clone(), |cid| format!("{bvid}:{cid}"));
    let owner = &value["owner"];
    let artist = text(first(owner, &["name", "uname"]));
    let mid = integer(first(owner, &["mid", "uid"]));
    let artist_reference = mid.map(|mid| reference("artist", &mid.to_string()));
    json!({
        "reference": if requested.is_null() { reference("track", &id) } else { requested.clone() },
        "title": page.map_or_else(|| title(value), title),
        "aliases": [],
        "artist": artist,
        "artists": if artist.is_empty() { vec![] } else { vec![json!({"name":artist,"reference":artist_reference})] },
        "album": "",
        "albumReference": Value::Null,
        "durationMs": page.map(|page| duration_text(&page["duration"])).unwrap_or_else(|| duration_text(&value["duration"])),
        "cover": cover(&value["pic"]),
        "associations": [],
        "lyricIdentifiers": if cid > 0 { vec![json!({"kind":"bilibili-cid","id":cid.to_string()})] } else { vec![] }
    })
}
pub fn mv(value: &Value) -> Option<Value> {
    let bvid = text(&value["bvid"]);
    if !valid_bvid(&bvid) {
        return None;
    }
    Some(json!({
        "reference": reference("track", &bvid),
        "title": title(value),
        "uploader": text(&value["author"]),
        "durationMs": duration_text(&value["duration"]),
        "cover": cover(&value["pic"])
    }))
}
pub struct Page {
    pub page: u64,
    pub limit: usize,
    pub scope: String,
}
pub fn page(value: &Value, scope: String) -> Result<Page> {
    let limit = integer(&value["limit"]).unwrap_or(20).clamp(1, 100) as usize;
    let mut page = 1;
    if let Some(cursor) = value["cursor"].as_str().filter(|value| !value.is_empty()) {
        let cursor: Value = serde_json::from_str(cursor)
            .map_err(|_| Error::new("invalidData", "page.cursor-json"))?;
        if cursor["scope"] != scope
            || cursor["limit"].as_u64() != Some(limit as u64)
            || !cursor["page"]
                .as_u64()
                .is_some_and(|value| (2..=100).contains(&value))
        {
            return Err(Error::new("invalidData", "page.cursor-scope"));
        }
        page = cursor["page"].as_u64().unwrap();
    } else if !value["cursor"].is_null() {
        return Err(Error::new("invalidData", "page.cursor-json"));
    }
    Ok(Page { page, limit, scope })
}
pub fn page_result(items: Vec<Value>, state: &Page, pages: u64) -> Value {
    let cursor = (state.page < pages && !items.is_empty())
        .then(|| json!({"page":state.page+1,"limit":state.limit,"scope":state.scope}).to_string());
    json!({"items":items,"nextCursor":cursor})
}
pub fn response(status: u16, body: &[u8]) -> Result<Value> {
    if !(200..300).contains(&status) {
        return Err(Error::new(
            if matches!(status, 412 | 429) {
                "rateLimited"
            } else if matches!(status, 401 | 403) {
                "permissionDenied"
            } else if status == 404 {
                "notFound"
            } else {
                "network"
            },
            &format!("http.status-{status}"),
        ));
    }
    serde_json::from_slice(body).map_err(|_| Error::new("invalidData", "response.json"))
}
pub fn envelope(value: Value, nav: bool) -> Result<Value> {
    if nav {
        return Ok(value["data"].clone());
    }
    let code = value["code"].as_i64().unwrap_or(-1);
    if code != 0 {
        let code_name = match code {
            -101 => "unauthenticated",
            -400 => "invalidData",
            -403 => "permissionDenied",
            -404 => "notFound",
            -412 | -509 => "rateLimited",
            62002 => "permissionDenied",
            _ => "sourceUnavailable",
        };
        return Err(Error::new(code_name, &format!("api.code-{code}")));
    }
    Ok(value["data"].clone())
}
pub fn map_object(value: &Value) -> Result<&Map<String, Value>> {
    value
        .as_object()
        .ok_or_else(|| Error::new("invalidData", "response.object"))
}

#[derive(Clone, Debug)]
pub struct Error {
    pub code: String,
    pub stage: String,
    pub retry_after_ms: Option<u64>,
}
impl Error {
    pub fn new(code: &str, stage: &str) -> Self {
        Self {
            code: code.into(),
            stage: stage.into(),
            retry_after_ms: None,
        }
    }
    pub fn host(code: &str, stage: &str) -> Self {
        Self::new(
            match code {
                "cancelled" => "cancelled",
                "timeout" | "deadlineExceeded" => "deadlineExceeded",
                "staleContext" => "staleContext",
                "permissionDenied" => "permissionDenied",
                "unauthenticated" => "unauthenticated",
                "rateLimited" => "rateLimited",
                "invalidData" => "invalidData",
                "network" => "network",
                _ => "sourceUnavailable",
            },
            stage,
        )
    }
}
impl std::fmt::Display for Error {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{} ({})", self.code, self.stage)
    }
}
impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_bvids_and_optional_cids() {
        assert!(valid_bvid("BV1xx411c7mD"));
        assert!(!valid_bvid("BV1xx411c7mD:123"));
        assert!(!valid_bvid("av123"));
        assert!(!valid_bvid("BV1xx411c7mD:"));
        assert_eq!(
            parse_video_id("BV1xx411c7mD:123"),
            Some(("BV1xx411c7mD", Some(123)))
        );
    }

    #[test]
    fn decodes_search_markup_and_duration() {
        assert_eq!(
            title(&json!({"title":"<em class=\"keyword\">晴天</em> &amp; 后弦"})),
            "晴天 & 后弦"
        );
        assert_eq!(duration_text(&json!("3:15")), 195_000);
        assert_eq!(duration_text(&json!("61:01")), 3_661_000);
    }

    #[test]
    fn page_cursor_is_bound_to_query_and_limit() {
        let cursor = json!({"page":2,"limit":20,"scope":"search:q"}).to_string();
        let value = json!({"cursor":cursor,"limit":20});
        assert_eq!(page(&value, "search:q".into()).unwrap().page, 2);
        assert!(page(&value, "search:other".into()).is_err());
    }
}
