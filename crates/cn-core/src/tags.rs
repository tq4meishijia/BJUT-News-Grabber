//! 关键词标签（对照 `core.mjs::pickTags`）。

/// 顺序扫描关键词，命中即收（去重、上限 6）。
pub fn pick_tags(text: &str, keywords: &[String]) -> Vec<String> {
    let mut hit: Vec<String> = Vec::new();
    for word in keywords {
        if !word.is_empty() && text.contains(word.as_str()) && !hit.contains(word) {
            hit.push(word.clone());
        }
        if hit.len() >= 6 {
            break;
        }
    }
    hit
}
