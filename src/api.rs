use crate::{model::*, wbi, Host};
use serde_json::{json, Value};
use std::collections::BTreeMap;

const MOBILE_UA: &str =
    "Mozilla/5.0 (iPhone; CPU iPhone OS 14_0_1 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Mobile/15E148 BiliApp/6.66.0";
const WEB_UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/155.0.0.0 Safari/537.36 Edg/155.0.0.0";
const REFERER: &str = "https://www.bilibili.com/";
const ORIGIN: &str = "https://www.bilibili.com";

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
        responses: RefCell<Vec<(u16, Value)>>,
        state: RefCell<BTreeMap<String, String>>,
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
            let (status, value) = self.responses.borrow_mut().remove(0);
            Ok(Response {
                status,
                headers: vec![],
                body: value.to_string().into_bytes(),
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
            Ok(vec![])
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
            (200, envelope(json!({"wbi_img":{"img_url":"https://i0.hdslb.com/a.png","sub_url":"https://i0.hdslb.com/b.png"}}))),
            (200, envelope(json!({"dash":{"audio":[{"id":30280,"base_url":"https://upos.example/audio.m4s","backup_url":["https://backup.example/audio.m4s"]}]}}))),
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
            (200, envelope(json!({"wbi_img":{"img_url":"https://i0.hdslb.com/a.png","sub_url":"https://i0.hdslb.com/b.png"}}))),
            (200, envelope(json!({"dash":{"video":[
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
}
