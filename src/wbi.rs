use crate::model::{Error, Result};
use md5::{Digest, Md5};
use std::collections::BTreeMap;

const MIXIN_KEY_ENC_TAB: [usize; 64] = [
    46, 47, 18, 2, 53, 8, 23, 32, 15, 50, 10, 31, 58, 3, 45, 35, 27, 43, 5, 49, 33, 9, 42, 19, 29,
    28, 14, 39, 12, 38, 41, 13, 37, 48, 7, 16, 24, 55, 40, 61, 26, 17, 0, 1, 60, 51, 30, 4, 22, 25,
    54, 21, 56, 59, 6, 63, 57, 62, 11, 36, 20, 34, 44, 52,
];

fn mixin_key(orig: &str) -> String {
    MIXIN_KEY_ENC_TAB
        .iter()
        .filter_map(|index| orig.as_bytes().get(*index))
        .map(|byte| char::from(*byte))
        .take(32)
        .collect()
}

fn encode(value: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut output = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            output.push(char::from(byte));
        } else {
            output.push('%');
            output.push(char::from(HEX[(byte >> 4) as usize]));
            output.push(char::from(HEX[(byte & 15) as usize]));
        }
    }
    output
}

pub fn sign(params: BTreeMap<&str, String>, img_key: &str, sub_key: &str, seconds: u64) -> String {
    let mut params = params;
    params.insert("wts", seconds.to_string());
    let query = params
        .into_iter()
        .map(|(key, value)| {
            let value = value
                .chars()
                .filter(|character| !matches!(character, '!' | '\'' | '(' | ')' | '*'))
                .collect::<String>();
            format!("{}={}", encode(key), encode(&value))
        })
        .collect::<Vec<_>>()
        .join("&");
    let signature = format!("{query}{}", mixin_key(&format!("{img_key}{sub_key}")));
    let digest = Md5::digest(signature.as_bytes());
    format!("{query}&w_rid={digest:x}")
}

pub fn keys(nav: &serde_json::Value) -> Result<(String, String)> {
    let img = nav["wbi_img"]["img_url"]
        .as_str()
        .ok_or_else(|| Error::new("sourceUnavailable", "wbi.img-key"))?;
    let sub = nav["wbi_img"]["sub_url"]
        .as_str()
        .ok_or_else(|| Error::new("sourceUnavailable", "wbi.sub-key"))?;
    let img = img
        .rsplit('/')
        .next()
        .and_then(|value| value.split('.').next())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| Error::new("sourceUnavailable", "wbi.img-key"))?;
    info_assert();
    let sub = sub
        .rsplit('/')
        .next()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| Error::new("sourceUnavailable", "wbi.sub-key"))?;
    Ok((img.to_owned(), sub.to_owned()))
}

fn info_assert() {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_bbplayer_sort_filter_and_encoding_order() {
        let mut params = BTreeMap::new();
        params.insert("keyword", "晴天 A!'()*".into());
        params.insert("page", "1".into());
        let key = "7cd084941338484aae1ad9425b84077c";
        let sub = "4932caff0ff746eab6f01bf08b70ac45";
        let first = sign(params.clone(), key, sub, 1_700_000_000);
        let second = sign(params, key, sub, 1_700_000_000);
        assert_eq!(first, second);
        assert!(first.starts_with("keyword=%E6%99%B4%E5%A4%A9%20A&page=1&wts=1700000000&w_rid="));
    }

    #[test]
    fn keys_match_mobile_implementation_quirks() {
        let nav = serde_json::json!({
            "wbi_img": {
                "img_url": "https://i0.hdslb.com/bfs/wbi/abcdef.png",
                "sub_url": "https://i0.hdslb.com/bfs/wbi/123456.png"
            }
        });
        assert_eq!(keys(&nav).unwrap(), ("abcdef".into(), "123456.png".into()));
    }
}
