use crate::{model::*, wbi, Host};
use base64::Engine as _;
use serde_json::{json, Value};
use std::collections::BTreeMap;

const MOBILE_UA: &str =
    "Mozilla/5.0 (iPhone; CPU iPhone OS 14_0_1 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Mobile/15E148 BiliApp/6.66.0";
const WEB_UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/155.0.0.0 Safari/537.36 Edg/155.0.0.0";
const REFERER: &str = "https://www.bilibili.com/";
const ORIGIN: &str = "https://www.bilibili.com";
const PASSPORT: &str = "https://passport.bilibili.com";
const API: &str = "https://api.bilibili.com";

fn cookie(host: &impl Host) -> String {
    host.credential_read()
        .ok()
        .filter(|value| !value.is_empty())
        .and_then(|value| String::from_utf8(value).ok())
        .unwrap_or_default()
}

fn headers(host: &impl Host, overrides: &[(&str, &str)]) -> Vec<(String, String)> {
    let mut headers = BTreeMap::from([
        ("Cookie".to_owned(), cookie(host)),
        ("Origin".to_owned(), ORIGIN.to_owned()),
        ("Referer".to_owned(), REFERER.to_owned()),
        ("User-Agent".to_owned(), MOBILE_UA.to_owned()),
    ]);
    for (name, value) in overrides {
        headers.insert((*name).to_owned(), (*value).to_owned());
    }
    headers.into_iter().collect()
}

fn get(
    host: &impl Host,
    endpoint: &str,
    query: &str,
    headers: Vec<(String, String)>,
) -> Result<Value> {
    let url = if query.is_empty() {
        format!("https://api.bilibili.com{endpoint}")
    } else {
        format!("https://api.bilibili.com{endpoint}?{query}")
    };
    let reply = host.http("GET", &url, headers, vec![], 10_000)?;
    response(reply.status, &reply.body)
}

fn raw_get(host: &impl Host, url: &str, headers: Vec<(String, String)>) -> Result<crate::Response> {
    host.http("GET", url, headers, vec![], 10_000)
}

pub fn prepare_login(request: &Value) -> Result<()> {
    match request["method"].as_str().unwrap_or("qr") {
        "qr" => Ok(()),
        _ => Err(Error::new("unsupported", "account.login-method")),
    }
}

fn login_cookie(headers: &[(String, String)]) -> Result<String> {
    let mut values = BTreeMap::new();
    for (name, value) in headers {
        if !name.eq_ignore_ascii_case("set-cookie") {
            continue;
        }
        let Some((name, value)) = value
            .split_once(';')
            .unwrap_or((value, ""))
            .0
            .split_once('=')
        else {
            continue;
        };
        let value = value.trim();
        if name.is_empty()
            || value.is_empty()
            || !name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
            || value
                .bytes()
                .any(|byte| byte.is_ascii_control() || byte == b';')
        {
            continue;
        }
        if matches!(
            name,
            "SESSDATA"
                | "bili_jct"
                | "DedeUserID"
                | "DedeUserID__ckMd5"
                | "sid"
                | "buvid3"
                | "buvid4"
        ) {
            values.insert(name, value);
        }
    }
    if !values.contains_key("SESSDATA") || !values.contains_key("bili_jct") {
        return Err(Error::new("unauthenticated", "account.login-cookie"));
    }
    Ok(values
        .into_iter()
        .map(|(name, value)| format!("{name}={value}"))
        .collect::<Vec<_>>()
        .join("; "))
}

fn profile(host: &impl Host, cookie_override: Option<&str>) -> Result<Option<Value>> {
    let cookie = cookie_override
        .map(str::to_owned)
        .unwrap_or_else(|| cookie(host));
    if cookie.is_empty() {
        return Ok(None);
    }
    let reply = raw_get(
        host,
        &format!("{API}/x/space/myinfo"),
        headers(host, &[("Cookie", &cookie)]),
    )?;
    let value = response(reply.status, &reply.body)?;
    let code = value["code"].as_i64().unwrap_or(-1);
    if code == -101 {
        let _ = host.credential_invalidate();
        return Ok(None);
    }
    let data = envelope(value, false)?;
    if !integer(&data["mid"]).is_some_and(|mid| mid > 0) {
        return Ok(None);
    }
    Ok(Some(data))
}

fn account_record(data: &Value) -> Result<Value> {
    let mid = integer(&data["mid"])
        .filter(|mid| *mid > 0)
        .ok_or_else(|| Error::new("invalidData", "account.mid"))?;
    let name = text(&data["name"]);
    Ok(json!({
        "reference": {"source":"bilibili","id":mid.to_string()},
        "displayName": if name.is_empty() { mid.to_string() } else { name },
        "avatar": cover(&data["face"])
    }))
}

pub fn account(host: &impl Host, login: &mut Option<String>, request: &Value) -> Result<Value> {
    match request["operation"].as_str().unwrap_or("") {
        "beginLogin" => {
            prepare_login(request)?;
            let reply = raw_get(
                host,
                &format!("{PASSPORT}/x/passport-login/web/qrcode/generate"),
                headers(host, &[]),
            )?;
            let data = envelope(response(reply.status, &reply.body)?, false)?;
            let url = data["url"]
                .as_str()
                .filter(|url| url.starts_with("https://"))
                .ok_or_else(|| Error::new("invalidData", "account.qr-url"))?;
            let key = data["qrcode_key"]
                .as_str()
                .filter(|key| !key.is_empty() && key.len() <= 256)
                .ok_or_else(|| Error::new("invalidData", "account.qr-key"))?;
            let qr = qrcode::QrCode::new(url)
                .map_err(|_| Error::new("invalidData", "account.qr-encode"))?;
            let svg = qr
                .render::<qrcode::render::svg::Color>()
                .min_dimensions(224, 224)
                .build();
            let image = format!(
                "data:image/svg+xml;base64,{}",
                base64::engine::general_purpose::STANDARD.encode(svg)
            );
            if image.len() > 64 * 1024 {
                return Err(Error::new("invalidData", "account.qr-size"));
            }
            *login = Some(key.to_owned());
            Ok(json!({
                "type":"challenge",
                "data":{"key":key,"image":image}
            }))
        }
        "pollLogin" => {
            let key = request["key"].as_str().unwrap_or("");
            if login.as_deref() != Some(key) {
                return Err(Error::new("staleContext", "account.challenge"));
            }
            let reply = raw_get(
                host,
                &format!("{PASSPORT}/x/passport-login/web/qrcode/poll?qrcode_key={key}"),
                vec![("User-Agent".into(), MOBILE_UA.into())],
            )?;
            let data = envelope(response(reply.status, &reply.body)?, false)?;
            let status = data["code"].as_i64().unwrap_or(-1);
            let (code, message) = match status {
                86101 => (801, "等待扫码"),
                86090 => (802, "已扫码，请在手机确认"),
                86038 => (800, "二维码已过期"),
                0 => (803, "登录成功"),
                _ => (800, "登录状态异常"),
            };
            if status == 0 {
                let cookie = login_cookie(&reply.headers)?;
                let data = profile(host, Some(&cookie))?
                    .ok_or_else(|| Error::new("unauthenticated", "account.profile"))?;
                let record = account_record(&data)?;
                host.credential_publish(&record.to_string(), cookie.as_bytes())?;
                *login = None;
            } else if status == 86038 {
                *login = None;
            }
            Ok(json!({"type":"progress","data":{"code":code,"message":message}}))
        }
        "profile" => Ok(match profile(host, None)? {
            Some(data) => json!({"type":"profile","data":account_record(&data)?}),
            None => json!({"type":"profile","data":Value::Null}),
        }),
        "logout" => Err(Error::new("unsupported", "account.logout")),
        _ => Err(Error::new("invalidData", "account.operation")),
    }
}

fn nav(host: &impl Host) -> Result<Value> {
    let value = get(host, "/x/web-interface/nav", "", headers(host, &[]))?;
    envelope(value, true)
}

fn wbi_keys(host: &impl Host) -> Result<(String, String)> {
    let now = host.now_ms();
    let day = now / 86_400_000;
    if let Some(value) = host.state_get("wbi-keys")? {
        if let Ok(value) = serde_json::from_str::<Value>(&value) {
            if value["day"].as_u64() == Some(day) {
                if let (Some(img), Some(sub)) = (value["img"].as_str(), value["sub"].as_str()) {
                    return Ok((img.to_owned(), sub.to_owned()));
                }
            }
        }
    }
    let value = nav(host)?;
    let (img, sub) = wbi::keys(&value)?;
    host.state_set(
        "wbi-keys",
        &json!({"img":img,"sub":sub,"day":day}).to_string(),
    )?;
    Ok((img, sub))
}

fn signed(host: &impl Host, params: BTreeMap<&str, String>) -> Result<String> {
    let (img, sub) = wbi_keys(host)?;
    Ok(wbi::sign(params, &img, &sub, host.now_ms() / 1000))
}

pub fn search(host: &impl Host, query: &str, state: &Page) -> Result<Value> {
    let params = signed(
        host,
        BTreeMap::from([
            ("keyword", query.to_owned()),
            ("page", state.page.to_string()),
            ("search_type", "video".to_owned()),
        ]),
    )?;
    let value = envelope(
        get(
            host,
            "/x/web-interface/wbi/search/type",
            &params,
            headers(host, &[]),
        )?,
        false,
    )?;
    let pages = integer(&value["numPages"]).unwrap_or(0);
    Ok(json!({"value":value,"pages":pages}))
}

pub fn view(host: &impl Host, bvid: &str) -> Result<Value> {
    envelope(
        get(
            host,
            "/x/web-interface/view",
            &format!("bvid={bvid}"),
            headers(host, &[]),
        )?,
        false,
    )
}

fn first_cid(host: &impl Host, bvid: &str) -> Result<u64> {
    let value = envelope(
        get(
            host,
            "/x/player/pagelist",
            &format!("bvid={bvid}"),
            headers(host, &[]),
        )?,
        false,
    )?;
    value
        .as_array()
        .and_then(|pages| pages.first())
        .and_then(|page| integer(&page["cid"]))
        .ok_or_else(|| Error::new("notFound", "playback.cid"))
}

fn cid(host: &impl Host, bvid: &str, explicit: Option<u64>) -> Result<u64> {
    explicit
        .or_else(|| integer(&view(host, bvid).ok()?["cid"]))
        .map_or_else(|| first_cid(host, bvid), Ok)
}

fn stream_item(value: &Value) -> Option<(String, Vec<String>)> {
    let url = text(first(value, &["base_url", "baseUrl", "url"]));
    if url.is_empty() {
        return None;
    }
    let backups = first(value, &["backup_url", "backupUrl"])
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|value| value.as_str())
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .collect::<Vec<_>>();
    Some((url, backups))
}

fn quality(preferred: &str) -> Result<(u16, bool, bool)> {
    match preferred {
        "standard" => Ok((30216, false, false)),
        "higher" => Ok((30232, false, false)),
        "exhigh" => Ok((30280, false, false)),
        "lossless" => Ok((30251, false, true)),
        "hires" => Ok((30251, true, true)),
        other => Err(Error::new("invalidData", &format!("quality.{other}"))),
    }
}

fn playurl(host: &impl Host, bvid: &str, cid: u64, preferred: &str) -> Result<Value> {
    let (qlt, dolby, hires) = quality(preferred)?;
    let params = signed(
        host,
        BTreeMap::from([
            ("bvid", bvid.to_owned()),
            ("cid", cid.to_string()),
            ("fnval", "4048".to_owned()),
            ("fnver", "0".to_owned()),
            ("fourk", "1".to_owned()),
            ("qlt", qlt.to_string()),
            ("voice_balance", "1".to_owned()),
        ]),
    )?;
    let value = envelope(
        get(
            host,
            "/x/player/wbi/playurl",
            &params,
            headers(host, &[("User-Agent", WEB_UA)]),
        )?,
        false,
    )?;
    Ok(json!({"value":value,"dolby":dolby,"hires":hires,"qlt":qlt}))
}

fn audio_quality(id: u64) -> &'static str {
    match id {
        30216 => "64k",
        30232 => "132k",
        30250 => "dolby",
        30251 => "hires",
        _ => "192k",
    }
}

fn resource(
    host: &impl Host,
    url: String,
    backups: Vec<String>,
    quality: &str,
    expiry_ms: u64,
) -> Value {
    json!({
        "metadata": {
            "actualQuality": quality,
            "extent": {"kind":"full"},
            "expiresAtMs": host.now_ms().saturating_add(expiry_ms)
        },
        "url": url,
        "backupUrls": backups,
        "headers": headers(host, &[("User-Agent", WEB_UA)])
    })
}

pub fn resolve_playback(host: &impl Host, request: &Value) -> Result<Value> {
    let id = check_reference(&request["track"], "track")?;
    let (bvid, explicit) =
        parse_video_id(id).ok_or_else(|| Error::new("invalidData", "playback.video-id"))?;
    let cid = cid(host, bvid, explicit)?;
    let preferred = request["preferredQuality"].as_str().unwrap_or("exhigh");
    let response = playurl(host, bvid, cid, preferred)?;
    let data = &response["value"];
    let Some(dash) = data["dash"].as_object() else {
        let Some(stream) = data["durl"]
            .as_array()
            .and_then(|items| items.first())
            .and_then(stream_item)
        else {
            return Err(Error::new("sourceUnavailable", "playback.streams"));
        };
        return Ok(resource(host, stream.0, stream.1, "mp4", 60_000));
    };
    if response["dolby"] == true {
        if let Some(stream) = dash
            .get("dolby")
            .and_then(|value| value["audio"].as_array())
            .and_then(|items| items.first())
            .and_then(stream_item)
        {
            return Ok(resource(host, stream.0, stream.1, "dolby", 60_000));
        }
    }
    if response["hires"] == true {
        if let Some(stream) = dash
            .get("flac")
            .and_then(|value| value["audio"].as_object())
            .and_then(|_| stream_item(&dash["flac"]["audio"]))
        {
            return Ok(resource(host, stream.0, stream.1, "hires", 60_000));
        }
    }
    let Some(items) = dash.get("audio").and_then(Value::as_array) else {
        return Err(Error::new("sourceUnavailable", "playback.audio"));
    };
    let target = response["qlt"]
        .as_u64()
        .and_then(|qlt| items.iter().find(|item| integer(&item["id"]) == Some(qlt)))
        .or_else(|| items.first())
        .and_then(stream_item)
        .ok_or_else(|| Error::new("sourceUnavailable", "playback.audio"))?;
    let selected = items
        .iter()
        .find(|item| stream_item(item).is_some_and(|value| value.0 == target.0));
    let quality = selected
        .and_then(|item| integer(&item["id"]))
        .map(audio_quality)
        .unwrap_or("audio");
    Ok(resource(host, target.0, target.1, quality, 60_000))
}

fn video_quality(id: u64) -> &'static str {
    match id {
        16 => "360p",
        32 => "480p",
        64 => "720p",
        74 => "720p60",
        80 => "1080p",
        112 => "1080p+",
        116 => "1080p60",
        120 => "4k",
        _ => "video",
    }
}

pub fn resolve_mv(host: &impl Host, request: &Value) -> Result<Value> {
    let id = check_reference(&request["mv"], "mv")?;
    let (bvid, explicit) =
        parse_video_id(id).ok_or_else(|| Error::new("invalidData", "mv.video-id"))?;
    let cid = cid(host, bvid, explicit)?;
    let response = playurl(host, bvid, cid, "exhigh")?;
    let data = &response["value"];
    if let Some(stream) = data["durl"]
        .as_array()
        .and_then(|items| items.first())
        .and_then(stream_item)
    {
        return Ok(resource(
            host,
            stream.0,
            stream.1,
            "mp4",
            2 * 60 * 60 * 1000,
        ));
    }
    let Some(items) = data["dash"]["video"].as_array() else {
        return Err(Error::new("sourceUnavailable", "mv.video"));
    };
    let avc = items
        .iter()
        .filter(|item| integer(&item["codecid"]) == Some(7))
        .collect::<Vec<_>>();
    let selected = avc
        .iter()
        .copied()
        .filter(|item| integer(&item["id"]).unwrap_or(u64::MAX) <= 80)
        .max_by_key(|item| integer(&item["id"]).unwrap_or(0))
        .or_else(|| {
            avc.into_iter()
                .max_by_key(|item| integer(&item["id"]).unwrap_or(0))
        })
        .or_else(|| {
            items
                .iter()
                .max_by_key(|item| integer(&item["id"]).unwrap_or(0))
        })
        .ok_or_else(|| Error::new("sourceUnavailable", "mv.video"))?;
    let stream =
        stream_item(selected).ok_or_else(|| Error::new("sourceUnavailable", "mv.video"))?;
    let quality = integer(&selected["id"])
        .map(video_quality)
        .unwrap_or("video");
    Ok(resource(
        host,
        stream.0,
        stream.1,
        quality,
        2 * 60 * 60 * 1000,
    ))
}

pub fn read_track(host: &impl Host, reference: &Value) -> Result<Value> {
    let id = check_reference(reference, "track")?;
    let (bvid, cid) =
        parse_video_id(id).ok_or_else(|| Error::new("invalidData", "track.video-id"))?;
    let value = view(host, bvid)?;
    if text(&value["bvid"]) != bvid {
        return Err(Error::new("notFound", "track.identity"));
    }
    Ok(track(&value, reference, cid))
}

fn bracketed_title(value: &str) -> &str {
    for (open, close) in [('《', '》'), ('「', '」')] {
        if let Some(start) = value.find(open) {
            let rest = &value[start + open.len_utf8()..];
            if let Some(end) = rest.find(close) {
                let inner = rest[..end].trim();
                if !inner.is_empty() {
                    return inner;
                }
            }
        }
    }
    value.trim()
}

pub fn lyric_match(host: &impl Host, request: &Value) -> Result<Value> {
    let id = check_reference(&request["track"]["reference"], "track")?;
    let (bvid, explicit) =
        parse_video_id(id).ok_or_else(|| Error::new("invalidData", "lyric.video-id"))?;
    let cid = match explicit {
        Some(cid) => cid,
        None => cid(host, bvid, None)?,
    };
    let params = signed(
        host,
        BTreeMap::from([("bvid", bvid.to_owned()), ("cid", cid.to_string())]),
    )?;
    let value = envelope(
        get(host, "/x/player/wbi/v2", &params, headers(host, &[]))?,
        false,
    )?;
    let title = text(&value["bgm_info"]["music_title"]);
    if title.is_empty() {
        return Err(Error::new("notFound", "lyric.bgm-title"));
    }
    Ok(json!({"title": bracketed_title(&title)}))
}

pub fn search_tracks(host: &impl Host, request: &Value) -> Result<Value> {
    let query = request["keyword"].as_str().unwrap_or("");
    let state = page(&request["page"], format!("search:track:{query}"))?;
    let response = search(host, query, &state)?;
    let items = response["value"]["result"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|item| item["type"] != "ketang")
        .filter_map(|item| {
            let bvid = text(&item["bvid"]);
            valid_bvid(&bvid).then(|| track(item, &Value::Null, None))
        })
        .take(state.limit)
        .collect::<Vec<_>>();
    Ok(json!({
        "type": "tracks",
        "data": {
            "items": items,
            "nextCursor": (state.page < response["pages"].as_u64().unwrap_or(0) && !items.is_empty())
                .then(|| json!({"page":state.page+1,"limit":state.limit,"scope":state.scope}).to_string()),
            "total": Value::Null,
            "description": Value::Null
        }
    }))
}

pub fn search_mvs(host: &impl Host, request: &Value) -> Result<Value> {
    let query = request["query"].as_str().unwrap_or("");
    let state = page(&request["page"], format!("search:mv:{query}"))?;
    let response = search(host, query, &state)?;
    let items = response["value"]["result"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|item| item["type"] != "ketang")
        .filter_map(mv)
        .take(state.limit)
        .collect::<Vec<_>>();
    Ok(page_result(
        items,
        &state,
        response["pages"].as_u64().unwrap_or(0),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Response;
    use std::cell::RefCell;

    #[derive(Default)]
    struct Fixture {
        responses: RefCell<Vec<Reply>>,
        state: RefCell<BTreeMap<String, String>>,
        credential: RefCell<Vec<u8>>,
        published: RefCell<Option<(String, Vec<u8>)>>,
        invalidated: RefCell<bool>,
    }
    struct Reply {
        status: u16,
        headers: Vec<(String, String)>,
        value: Value,
    }
    fn reply(value: Value) -> Reply {
        Reply {
            status: 200,
            headers: vec![],
            value,
        }
    }
    impl Host for Fixture {
        fn http(
            &self,
            _: &str,
            _: &str,
            _: Vec<(String, String)>,
            _: Vec<u8>,
            _: u32,
        ) -> Result<Response> {
            let reply = self.responses.borrow_mut().remove(0);
            Ok(Response {
                status: reply.status,
                headers: reply.headers,
                body: reply.value.to_string().into_bytes(),
            })
        }
        fn now_ms(&self) -> u64 {
            1_700_000_000_000
        }
        fn state_get(&self, key: &str) -> Result<Option<String>> {
            Ok(self.state.borrow().get(key).cloned())
        }
        fn state_set(&self, key: &str, value: &str) -> Result<()> {
            self.state.borrow_mut().insert(key.into(), value.into());
            Ok(())
        }
        fn credential_read(&self) -> Result<Vec<u8>> {
            Ok(self.credential.borrow().clone())
        }
        fn credential_publish(&self, metadata: &str, data: &[u8]) -> Result<()> {
            *self.published.borrow_mut() = Some((metadata.into(), data.to_vec()));
            Ok(())
        }
        fn credential_replace(&self, data: &[u8]) -> Result<()> {
            *self.credential.borrow_mut() = data.to_vec();
            Ok(())
        }
        fn credential_invalidate(&self) -> Result<()> {
            *self.invalidated.borrow_mut() = true;
            Ok(())
        }
        fn diagnostic(&self, _: &str, _: &str, _: &str) {}
    }

    fn envelope(data: Value) -> Value {
        json!({"code":0,"message":"0","data":data})
    }

    #[test]
    fn selects_requested_audio_and_keeps_backup_urls() {
        let host = Fixture::default();
        host.responses.borrow_mut().extend([
            reply(envelope(json!({"wbi_img":{"img_url":"https://i0.hdslb.com/a.png","sub_url":"https://i0.hdslb.com/b.png"}}))),
            reply(envelope(json!({"dash":{"audio":[{"id":30280,"base_url":"https://upos.example/audio.m4s","backup_url":["https://backup.example/audio.m4s"]}]}}))),
        ]);
        let value = resolve_playback(
            &host,
            &json!({"track":{"source":"bilibili","kind":"track","id":"BV1xx411c7mD:1"},"preferredQuality":"exhigh","allowDowngrade":true}),
        )
        .unwrap();
        assert_eq!(value["metadata"]["actualQuality"], "192k");
        assert_eq!(value["backupUrls"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn mv_prefers_avc_1080p_over_higher_codecs() {
        let host = Fixture::default();
        host.responses.borrow_mut().extend([
            reply(envelope(json!({"wbi_img":{"img_url":"https://i0.hdslb.com/a.png","sub_url":"https://i0.hdslb.com/b.png"}}))),
            reply(envelope(json!({"dash":{"video":[
                {"id":120,"codecid":13,"base_url":"https://example/4k.m4s"},
                {"id":80,"codecid":7,"base_url":"https://example/1080.m4s"}
            ]}}))),
        ]);
        let value = resolve_mv(
            &host,
            &json!({"mv":{"source":"bilibili","kind":"mv","id":"BV1xx411c7mD:1"}}),
        )
        .unwrap();
        assert_eq!(value["metadata"]["actualQuality"], "1080p");
        assert_eq!(value["url"], "https://example/1080.m4s");
    }

    #[test]
    fn qr_login_publishes_cookie_only_after_profile_and_unwraps_account_record() {
        let host = Fixture::default();
        host.responses.borrow_mut().extend([
            reply(envelope(json!({
                "url":"https://www.bilibili.com/h5/mobile-appeal",
                "qrcode_key":"qr-key"
            }))),
            reply(envelope(json!({"code":86090}))),
            Reply {
                status: 200,
                headers: vec![
                    (
                        "set-cookie".into(),
                        "SESSDATA=sess; Path=/; HttpOnly".into(),
                    ),
                    (
                        "set-cookie".into(),
                        "bili_jct=csrf; Path=/; HttpOnly".into(),
                    ),
                    (
                        "set-cookie".into(),
                        "DedeUserID=42; Path=/; HttpOnly".into(),
                    ),
                ],
                value: envelope(json!({"code":0})),
            },
            reply(envelope(
                json!({"mid":42,"name":"Listener","face":"//i0.hdslb.com/avatar.jpg"}),
            )),
        ]);
        let mut login = None;
        let challenge = account(
            &host,
            &mut login,
            &json!({"operation":"beginLogin","method":"qr"}),
        )
        .unwrap();
        assert_eq!(challenge["type"], "challenge");
        assert_eq!(challenge["data"]["key"], "qr-key");
        assert!(challenge["data"]["image"]
            .as_str()
            .unwrap()
            .starts_with("data:image/svg+xml;base64,"));
        let waiting = account(
            &host,
            &mut login,
            &json!({"operation":"pollLogin","key":"qr-key"}),
        )
        .unwrap();
        assert_eq!(waiting["data"]["code"], 802);
        let success = account(
            &host,
            &mut login,
            &json!({"operation":"pollLogin","key":"qr-key"}),
        )
        .unwrap();
        assert_eq!(success["data"]["code"], 803);
        let (metadata, credential) = host.published.borrow().clone().unwrap();
        assert_eq!(credential, b"DedeUserID=42; SESSDATA=sess; bili_jct=csrf");
        let record: Value = serde_json::from_str(&metadata).unwrap();
        assert_eq!(record["reference"]["id"], "42");
        assert_eq!(
            record["avatar"]["original"],
            "https://i0.hdslb.com/avatar.jpg"
        );
        assert!(login.is_none());
    }

    #[test]
    fn profile_invalidates_rejected_cookie_and_returns_null() {
        let host = Fixture::default();
        *host.credential.borrow_mut() = b"SESSDATA=old; bili_jct=csrf".to_vec();
        host.responses
            .borrow_mut()
            .push(reply(json!({"code":-101,"data":{"isLogin":false}})));
        let mut login = None;
        let profile = account(&host, &mut login, &json!({"operation":"profile"})).unwrap();
        assert!(profile["data"].is_null());
        assert!(*host.invalidated.borrow());
    }

    #[test]
    fn login_cookie_requires_session_and_csrf_and_drops_attributes() {
        let headers = vec![
            (
                "set-cookie".into(),
                "SESSDATA=a%2Cb; Path=/; HttpOnly".into(),
            ),
            ("set-cookie".into(), "bili_jct=csrf; Path=/".into()),
            (
                "set-cookie".into(),
                "DedeUserID=42; Expires=Wed, 04 Nov 2026 00:00:00 GMT".into(),
            ),
        ];
        assert_eq!(
            login_cookie(&headers).unwrap(),
            "DedeUserID=42; SESSDATA=a%2Cb; bili_jct=csrf"
        );
        assert_eq!(
            login_cookie(&headers[..1]).unwrap_err().code,
            "unauthenticated"
        );
    }
}
