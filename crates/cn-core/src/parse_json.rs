//! JSON 列表解析（对照 `core.mjs::parseJsonList / pickPath / guessList`）。

use chrono::{DateTime, Local};
use cn_model::{Clock, FieldMap, RawItem, SourceSpec};
use serde_json::Value;
use url::Url;

use crate::date::find_date;
use crate::text::{clean_title, encode_uri_component, excerpt_of};

/// `core.mjs::pickPath(value, path)`：点分路径（数组下标用数字键）。
pub fn pick_path<'a>(value: &'a Value, path: &str) -> Option<&'a Value> {
    if path.is_empty() {
        return Some(value);
    }
    let mut current = Some(value);
    for key in path.split('.') {
        let cur = current?;
        if let Some(arr) = cur.as_array() {
            if let Ok(idx) = key.parse::<usize>() {
                current = arr.get(idx);
                continue;
            }
        }
        current = cur.get(key);
    }
    current
}

/// JS `String(x)`（x 非 null/undefined）。
fn js_string(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                i.to_string()
            } else if let Some(u) = n.as_u64() {
                u.to_string()
            } else {
                n.to_string()
            }
        }
        Value::Bool(b) => b.to_string(),
        Value::Null => "null".to_string(),
        Value::Array(items) => items
            .iter()
            .map(js_string)
            .collect::<Vec<_>>()
            .join(","),
        Value::Object(_) => "[object Object]".to_string(),
    }
}

/// JS `String(x ?? '')`：null/undefined → ''，其余走 `String(x)`。
fn js_string_or_empty(value: Option<&Value>) -> String {
    match value {
        None | Some(Value::Null) => String::new(),
        Some(v) => js_string(v),
    }
}

/// JS `String(x || '')`：falsy（null/undefined/0/''/false）→ ''，其余走 `String(x)`。
fn js_string_truthy(value: Option<&Value>) -> String {
    match value {
        None => String::new(),
        Some(Value::Null) => String::new(),
        Some(Value::Bool(false)) => String::new(),
        Some(Value::Number(n)) if n.as_f64() == Some(0.0) => String::new(),
        Some(Value::String(s)) if s.is_empty() => String::new(),
        Some(v) => js_string(v),
    }
}

fn is_object_row(value: &Value) -> bool {
    // JS `row && typeof row === 'object'`：null 排除，数组与对象都算。
    !value.is_null() && (value.is_object() || value.is_array())
}

/// `core.mjs::guessList`：在响应里找第一个「对象数组」。
pub fn guess_list(data: &Value) -> Option<&Vec<Value>> {
    if let Some(arr) = data.as_array() {
        return Some(arr);
    }
    if !data.is_object() {
        return None;
    }
    const CANDIDATES: [&str; 18] = [
        "d.data",
        "d.list",
        "d.rows",
        "d.items",
        "d.records",
        "d.result",
        "d",
        "data",
        "list",
        "rows",
        "items",
        "records",
        "result",
        "content",
        "data.list",
        "data.rows",
        "data.items",
        "data.records",
    ];
    for path in CANDIDATES {
        if let Some(value) = pick_path(data, path) {
            if let Some(arr) = value.as_array() {
                if arr.iter().any(is_object_row) {
                    return Some(arr);
                }
            }
        }
    }
    // 广度优先找第一个对象数组。
    let mut queue: Vec<&Value> = vec![data];
    let mut guard = 0;
    while !queue.is_empty() && guard < 200 {
        guard += 1;
        let current = queue.remove(0);
        if !current.is_object() {
            continue;
        }
        for value in current.as_object().expect("is object").values() {
            if let Some(arr) = value.as_array() {
                if arr.iter().any(is_object_row) {
                    return Some(arr);
                }
            }
            if value.is_object() {
                queue.push(value);
            }
        }
    }
    None
}

/// `core.mjs::parseJsonList(text, source)`。
pub fn parse_json_list(body: &str, spec: &SourceSpec, now: &dyn Clock) -> Vec<RawItem> {
    let now_dt: DateTime<Local> = now.now();
    let data: Value = match serde_json::from_str(body) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };
    // 填了 itemsPath 就按它来，取不到数组就自动猜一个。
    let list: Option<&Vec<Value>> = spec
        .items_path
        .as_deref()
        .filter(|p| !p.is_empty())
        .and_then(|p| pick_path(&data, p).and_then(|v| v.as_array()))
        .or_else(|| guess_list(&data));
    let Some(list) = list else {
        return Vec::new();
    };
    let map: &FieldMap = &spec.map;
    let title_key = FieldMap::field(&map.title, "title");
    // JS：map.url || map.link || 'url'
    // JS：map.url || map.link || 'url'
    let url_key = FieldMap::field(&map.url, FieldMap::field(&map.link, "url"));
    let id_key = FieldMap::field(&map.id, "id");
    let date_key = FieldMap::field(&map.date, "date");
    let summary_key = FieldMap::field(&map.summary, "summary");

    let base = Url::parse(&spec.url).ok();
    let mut out: Vec<RawItem> = Vec::new();
    for row in list.iter().take(200) {
        let title = clean_title(&js_string_truthy(pick_path(row, title_key)));
        if title.is_empty() {
            continue;
        }
        let mut url = js_string_truthy(pick_path(row, url_key));
        let item_id = js_string_or_empty(pick_path(row, id_key));
        // 接口不给链接时，用模板拼一条人能点开的页面（{id} / {index} / {title}）。
        if url.is_empty() {
            if let Some(template) = spec.url_template.as_deref().filter(|t| !t.is_empty()) {
                url = template
                    .replace("{id}", &encode_uri_component(&item_id))
                    .replace("{index}", &(out.len() + 1).to_string())
                    .replace("{title}", &encode_uri_component(&title));
            }
        }
        if !url.is_empty() && !(url.starts_with("http://") || url.starts_with("https://")) {
            url = match &base {
                Some(b) => b
                    .join(&url)
                    .map(|u| u.to_string())
                    .unwrap_or_default(),
                None => String::new(),
            };
        }
        out.push(RawItem {
            item_id,
            title,
            url,
            published_at: find_date(&js_string_or_empty(pick_path(row, date_key)), now_dt),
            context: excerpt_of(&js_string_or_empty(pick_path(row, summary_key))),
        });
    }
    out
}
