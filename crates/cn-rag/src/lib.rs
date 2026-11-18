//! 端侧 RAG 的检索内核。
//!
//! 设计前提：语料规模是「几百条通知、约一千个块」，不是百万级文档。
//! 因此这里**不引入向量库、不引入分词词典、不引入第三方 crate**：
//! 中文用字符 bigram 做词项，BM25 直接线性打分，稠密向量用余弦，
//! 两路结果用 RRF 融合。全部可在 Windows 与 Android 上以同一份代码运行。
//!
//! 边界：本 crate 不做 IO、不做推理、不持有模型句柄。嵌入与生成由上层注入，
//! 这里只负责「怎么切、怎么算、怎么排」。

use std::collections::HashMap;

/// 一个可检索的最小单元。
#[derive(Debug, Clone, PartialEq)]
pub struct Chunk {
    pub id: String,
    pub notice_id: String,
    pub ord: usize,
    pub text: String,
}

/// 判断是否 CJK 统一表意文字（含扩展 A 与兼容区）。
fn is_cjk(ch: char) -> bool {
    matches!(ch as u32,
        0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xF900..=0xFAFF | 0x20000..=0x2FA1F)
}

/// 中文按字符 bigram、西文按小写词切分。
///
/// 不引入 jieba 的理由：一是多一份词典与约 3 MB 体积；二是本场景查询词多为
/// 「考试」「奖学金」「四六级」这类 2–4 字词，bigram 的召回不低于分词，
/// 且永远不会因为词典缺词而漏召回。
pub fn tokenize(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cjk = String::new();
    let mut word = String::new();

    fn flush_cjk(run: &mut String, out: &mut Vec<String>) {
        let chars: Vec<char> = run.chars().collect();
        match chars.len() {
            0 => {}
            1 => out.push(chars[0].to_string()),
            _ => {
                for pair in chars.windows(2) {
                    out.push(format!("{}{}", pair[0], pair[1]));
                }
            }
        }
        run.clear();
    }

    fn flush_word(run: &mut String, out: &mut Vec<String>) {
        if !run.is_empty() {
            out.push(std::mem::take(run));
        }
    }

    for ch in text.chars() {
        if is_cjk(ch) {
            flush_word(&mut word, &mut out);
            cjk.push(ch);
        } else if ch.is_alphanumeric() {
            flush_cjk(&mut cjk, &mut out);
            word.extend(ch.to_lowercase());
        } else {
            flush_cjk(&mut cjk, &mut out);
            flush_word(&mut word, &mut out);
        }
    }
    flush_cjk(&mut cjk, &mut out);
    flush_word(&mut word, &mut out);
    out
}

/// 按句末标点与换行切句，保留标点。
fn split_sentences(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for ch in text.chars() {
        cur.push(ch);
        if matches!(ch, '。' | '！' | '？' | '；' | '\n') {
            let sentence = cur.trim().to_string();
            if !sentence.is_empty() {
                out.push(sentence);
            }
            cur.clear();
        }
    }
    let tail = cur.trim().to_string();
    if !tail.is_empty() {
        out.push(tail);
    }
    out
}

/// 单句超过上限时按字符硬切，避免一个没有句号的段落变成巨块。
fn hard_split(sentence: &str, max_chars: usize) -> Vec<String> {
    let chars: Vec<char> = sentence.chars().collect();
    if chars.len() <= max_chars {
        return vec![sentence.to_string()];
    }
    chars
        .chunks(max_chars)
        .map(|part| part.iter().collect::<String>())
        .collect()
}

/// 把一条通知切成若干块。
///
/// - `max_chars`：单块字符上限，建议 320；
/// - `overlap_sentences`：相邻块重叠的句数，建议 1（保证跨句语义不被切断）。
/// - 正文为空时退化为「标题一块」，标题也为空则返回空。
pub fn chunk_notice(
    notice_id: &str,
    title: &str,
    body: &str,
    max_chars: usize,
    overlap_sentences: usize,
) -> Vec<Chunk> {
    let max_chars = max_chars.max(16);
    let mut parts: Vec<String> = Vec::new();
    for sentence in split_sentences(body) {
        parts.extend(hard_split(&sentence, max_chars));
    }
    if parts.is_empty() {
        let fallback = title.trim();
        if fallback.is_empty() {
            return Vec::new();
        }
        parts.push(fallback.to_string());
    }

    let mut chunks = Vec::new();
    let mut start = 0usize;
    while start < parts.len() {
        let mut text = String::new();
        let mut end = start;
        while end < parts.len() {
            let candidate = text.chars().count() + parts[end].chars().count();
            if !text.is_empty() && candidate > max_chars {
                break;
            }
            text.push_str(&parts[end]);
            end += 1;
        }
        chunks.push(Chunk {
            id: format!("{notice_id}#{}", chunks.len()),
            notice_id: notice_id.to_string(),
            ord: chunks.len(),
            text: text.trim().to_string(),
        });
        if end >= parts.len() {
            break;
        }
        let back = end.saturating_sub(overlap_sentences.min(end - start));
        start = if back > start { back } else { end };
    }
    chunks
}

/// 送入嵌入模型时的文本前缀：带上标题，显著改善「块脱离上下文」导致的召回偏差。
pub fn embed_input(title: &str, chunk: &Chunk) -> String {
    if title.trim().is_empty() {
        chunk.text.clone()
    } else {
        format!("{}\n{}", title.trim(), chunk.text)
    }
}

/// BM25（k1=1.2, b=0.75）。语料只有千级块，线性打分足够，无需倒排表。
pub struct Bm25 {
    doc_len: Vec<usize>,
    avg_len: f64,
    df: HashMap<String, usize>,
    tf: Vec<HashMap<String, usize>>,
    n: usize,
}

const K1: f64 = 1.2;
const B: f64 = 0.75;

impl Bm25 {
    pub fn build(docs: &[Vec<String>]) -> Self {
        let mut df: HashMap<String, usize> = HashMap::new();
        let mut tf: Vec<HashMap<String, usize>> = Vec::with_capacity(docs.len());
        let mut doc_len = Vec::with_capacity(docs.len());
        for tokens in docs {
            let mut counts: HashMap<String, usize> = HashMap::new();
            for token in tokens {
                *counts.entry(token.clone()).or_insert(0) += 1;
            }
            for token in counts.keys() {
                *df.entry(token.clone()).or_insert(0) += 1;
            }
            doc_len.push(tokens.len());
            tf.push(counts);
        }
        let n = docs.len();
        let avg_len = if n == 0 {
            0.0
        } else {
            doc_len.iter().sum::<usize>() as f64 / n as f64
        };
        Self { doc_len, avg_len, df, tf, n }
    }

    pub fn len(&self) -> usize {
        self.n
    }

    pub fn is_empty(&self) -> bool {
        self.n == 0
    }

    pub fn score(&self, query: &[String], doc: usize) -> f64 {
        if doc >= self.n || self.avg_len == 0.0 {
            return 0.0;
        }
        let dl = self.doc_len[doc] as f64;
        let mut total = 0.0;
        for term in query {
            let freq = *self.tf[doc].get(term).unwrap_or(&0) as f64;
            if freq == 0.0 {
                continue;
            }
            let df = *self.df.get(term).unwrap_or(&0) as f64;
            let idf = (1.0 + (self.n as f64 - df + 0.5) / (df + 0.5)).ln();
            total += idf * (freq * (K1 + 1.0))
                / (freq + K1 * (1.0 - B + B * dl / self.avg_len));
        }
        total
    }

    /// 返回 `(doc_index, score)`，按分数降序、同分按索引升序，保证可复现。
    pub fn search(&self, query: &[String], top_k: usize) -> Vec<(usize, f64)> {
        let mut hits: Vec<(usize, f64)> = (0..self.n)
            .map(|doc| (doc, self.score(query, doc)))
            .filter(|(_, score)| *score > 0.0)
            .collect();
        hits.sort_by(|a, b| {
            b.1.partial_cmp(&a.1)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then(a.0.cmp(&b.0))
        });
        hits.truncate(top_k);
        hits
    }
}

/// 余弦相似度。维度不一致或任一方为零向量时返回 0，不 panic。
pub fn cosine(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let (mut dot, mut na, mut nb) = (0.0f32, 0.0f32, 0.0f32);
    for i in 0..a.len() {
        dot += a[i] * b[i];
        na += a[i] * a[i];
        nb += b[i] * b[i];
    }
    if na == 0.0 || nb == 0.0 {
        return 0.0;
    }
    dot / (na.sqrt() * nb.sqrt())
}

/// 原地 L2 归一化，便于用点积代替余弦。
pub fn normalize(v: &mut [f32]) {
    let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm > 0.0 {
        for x in v.iter_mut() {
            *x /= norm;
        }
    }
}

/// 对称 int8 量化：512 维从 2 KB 压到 512 B，召回损失通常 < 1%。
pub fn quantize_i8(v: &[f32]) -> (Vec<i8>, f32) {
    let max = v.iter().fold(0.0f32, |m, x| m.max(x.abs()));
    if max == 0.0 {
        return (vec![0; v.len()], 1.0);
    }
    let scale = max / 127.0;
    let q = v
        .iter()
        .map(|x| (x / scale).round().clamp(-127.0, 127.0) as i8)
        .collect();
    (q, scale)
}

pub fn dequantize_i8(q: &[i8], scale: f32) -> Vec<f32> {
    q.iter().map(|x| *x as f32 * scale).collect()
}

/// Reciprocal Rank Fusion：把多路排序合成一路，k 取 60 是常用默认值。
/// 返回 `(doc_index, score)`，降序且同分按索引升序。
pub fn rrf_fuse(rankings: &[Vec<usize>], k: f64) -> Vec<(usize, f64)> {
    let mut acc: HashMap<usize, f64> = HashMap::new();
    for ranking in rankings {
        for (position, doc) in ranking.iter().enumerate() {
            *acc.entry(*doc).or_insert(0.0) += 1.0 / (k + (position as f64) + 1.0);
        }
    }
    let mut out: Vec<(usize, f64)> = acc.into_iter().collect();
    out.sort_by(|a, b| {
        b.1.partial_cmp(&a.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.0.cmp(&b.0))
    });
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokenizer_handles_mixed_text() {
        let tokens = tokenize("关于2026年期末考试安排的通知");
        assert!(tokens.contains(&"考试".to_string()));
        assert!(tokens.contains(&"2026".to_string()));
        assert!(tokens.contains(&"关于".to_string()));
        assert!(!tokens.contains(&"关".to_string()));
    }

    #[test]
    fn tokenizer_single_char_run() {
        assert_eq!(tokenize("好"), vec!["好".to_string()]);
        assert_eq!(tokenize("A B"), vec!["a".to_string(), "b".to_string()]);
    }

    #[test]
    fn chunk_respects_limit_and_progress() {
        let body = "第一句讲考试安排。第二句讲报名截止时间。第三句讲联系人。第四句讲注意事项。";
        let chunks = chunk_notice("n1", "期末考试通知", body, 24, 1);
        assert!(chunks.len() > 1, "应当被切成多块");
        for chunk in &chunks {
            assert!(chunk.text.chars().count() <= 24);
            assert!(!chunk.text.is_empty());
            assert_eq!(chunk.notice_id, "n1");
        }
        assert_eq!(chunks[0].ord, 0);
        assert_eq!(chunks.last().unwrap().ord, chunks.len() - 1);
        assert!(chunks.last().unwrap().text.contains("注意事项"));
    }

    #[test]
    fn chunk_overlap_keeps_boundary_sentence() {
        let body = "甲句内容在这里。乙句内容在这里。丙句内容在这里。丁句内容在这里。";
        let chunks = chunk_notice("n2", "标题", body, 20, 1);
        assert!(chunks.len() >= 2);
        let first_tail: Vec<char> = chunks[0].text.chars().rev().take(4).collect();
        assert!(!first_tail.is_empty());
        assert!(chunks[1].text.contains("乙句") || chunks[1].text.contains("丙句"));
    }

    #[test]
    fn chunk_empty_body_falls_back_to_title() {
        let chunks = chunk_notice("n3", "只有标题", "", 320, 1);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].text, "只有标题");
        assert!(chunk_notice("n4", "", "", 320, 1).is_empty());
    }

    #[test]
    fn bm25_ranks_matching_doc_first() {
        let docs = vec![
            tokenize("关于期末考试安排的通知"),
            tokenize("关于奖学金评选的通知"),
            tokenize("图书馆开放时间调整"),
        ];
        let index = Bm25::build(&docs);
        let hits = index.search(&tokenize("考试"), 3);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].0, 0);
        assert!(hits[0].1 > 0.0);
    }

    #[test]
    fn bm25_empty_is_safe() {
        let index = Bm25::build(&[]);
        assert!(index.is_empty());
        assert!(index.search(&tokenize("考试"), 5).is_empty());
    }

    #[test]
    fn cosine_basic() {
        assert!((cosine(&[1.0, 0.0], &[1.0, 0.0]) - 1.0).abs() < 1e-6);
        assert!(cosine(&[1.0, 0.0], &[0.0, 1.0]).abs() < 1e-6);
        assert_eq!(cosine(&[1.0], &[1.0, 2.0]), 0.0);
        assert_eq!(cosine(&[0.0, 0.0], &[1.0, 1.0]), 0.0);
    }

    #[test]
    fn quantize_roundtrip_is_close() {
        let v = vec![0.9f32, -0.4, 0.0, 0.33];
        let (q, scale) = quantize_i8(&v);
        let back = dequantize_i8(&q, scale);
        for i in 0..v.len() {
            assert!((v[i] - back[i]).abs() < scale.max(1e-6));
        }
    }

    #[test]
    fn rrf_prefers_item_found_by_both_routes() {
        // 0 号在两路都排第一 → 必然最高；1、2 号各只出现在一路，同分由索引升序决定。
        let fused = rrf_fuse(&[vec![0, 1], vec![0, 2]], 60.0);
        assert_eq!(fused.len(), 3);
        assert_eq!(fused[0].0, 0, "两路都命中的条目应当胜出");
        assert!(fused[0].1 > fused[1].1);
        assert_eq!(fused[1].0, 1, "同分时按索引升序，保证可复现");
    }

    #[test]
    fn normalize_sets_unit_length() {
        let mut v = vec![3.0f32, 4.0];
        normalize(&mut v);
        assert!((v[0] - 0.6).abs() < 1e-6);
        assert!((v[1] - 0.8).abs() < 1e-6);
    }
}
