//! 正文定位（对照 `core.mjs::extractMainContent`）：先按常见 CMS 容器定位，再兜底整页。
//! JS 的 `slice` 下标是 UTF-16 单元；这里用字符下标（fixtures 全 BMP，两者一致）。

use regex::Regex;

use crate::text::{char_slice, html_to_text, js_len};

/// 尽量只留正文；输出经 `htmlToText`，长度 ≤ 6000 字符。
pub fn extract_main_content(html: &str) -> String {
    let source = html;
    let markers: [(&str, &str); 5] = [
        ("vsb_content", r#"id=["']?vsb_content"#),
        ("v_news", r#"class=["'][^"']*v_news_content"#),
        ("trs", r#"class=["'][^"']*TRS_Editor"#),
        (
            "article",
            r#"class=["'][^"']*(?:article[-_]?content|article|news[-_]?content|content[-_]?box)[^"']*["']"#,
        ),
        ("zoom", r#"id=["']?(?:content|article|zoom)["']?"#),
    ];
    let mut start: Option<usize> = None;
    for (_name, pattern) in markers {
        let re = Regex::new(&format!("(?i){pattern}")).expect("marker pattern");
        if let Some(m) = re.find(source) {
            start = Some(m.start());
            break;
        }
    }
    let mut slice = source.to_string();
    if let Some(start) = start {
        let tag_end = source[start..].find('>').map(|i| start + i);
        let from_char = match tag_end {
            Some(te) => source[..te + 1].chars().count(),
            None => source[..start].chars().count(),
        };
        let to_char = source[..start].chars().count() + 40_000;
        slice = char_slice(source, from_char, to_char);
    }
    const TAILS: [&str; 8] = [
        "上一篇",
        "下一篇",
        "分享到",
        "打印本页",
        "关闭窗口",
        "相关附件",
        "版权所有",
        "京公网安备",
    ];
    let mut end = js_len(&slice);
    for tail in TAILS {
        if let Some(byte_index) = slice.find(tail) {
            let char_index = slice[..byte_index].chars().count();
            if char_index > 200 && char_index < end {
                end = char_index;
            }
        }
    }
    let slice = char_slice(&slice, 0, end);
    html_to_text(&slice).chars().take(6000).collect()
}
