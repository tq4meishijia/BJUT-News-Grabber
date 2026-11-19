//! 附件下载编排：白名单链接 → 去重 → 三次重试 → 原子落盘 → 级联删除 → 单条重试。
//!
//! 异常隔离铁律：任何解析 / 下载 / 落盘失败只写 `AttachmentRecord.status/error` 与日志，
//! 禁止 `?` 冒泡阻断通知入库、去重、LLM、建日程。

use std::path::Path;
use std::time::Duration;

use cn_core::{extract_attachment_links, normalize_attachment_url, AttachmentLink};
use cn_model::{AttachmentStatus, Clock};
use cn_net::{restore_urls_in_text, Fetcher, NetError, WebvpnConfig, ATTACHMENT_MAX_BYTES};
use sha1::Digest;
use url::Url;

use crate::pipeline::{build_request, DetailBundle};
use crate::state::{new_id, AttachmentRecord, NoticeRecord, SourceRecord, StoreData};

/// 下载尝试次数（首次 + 重试 2 次）。
const MAX_ATTEMPTS: u8 = 3;
/// 失败原因最多保留 300 字。
const ERROR_MAX_CHARS: usize = 300;
/// 单次详情 / 附件请求超时（毫秒）。
const REQUEST_TIMEOUT_MS: u64 = 20_000;

/// 一次通知的附件处理结果统计。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AttachmentStats {
    pub downloaded: usize,
    pub failed: usize,
    pub skipped: usize,
}

/// 单文件下载结果（内部使用，绝不向 poll 主流程冒泡）。
enum DownloadResult {
    Ok(Vec<u8>),
    TooLarge(String),
    Failed(String),
}

impl DownloadResult {
    fn failed(reason: impl AsRef<str>) -> Self {
        Self::Failed(truncate_error(reason))
    }
}

/// 从详情 bundle 中发现附件、下载并写入 store.attachments。
/// `id_seed` 由调用方从 CampusService 的单调计数器取得，保证同毫秒多附件 id 唯一。
#[allow(clippy::too_many_arguments)]
pub async fn discover_and_download<F>(
    fetcher: &dyn Fetcher,
    vpn: &WebvpnConfig,
    dir: &Path,
    store: &mut StoreData,
    source: &SourceRecord,
    notice: &NoticeRecord,
    bundle: &DetailBundle,
    clock: &dyn Clock,
    // 本次抓取范围内已请求过的 norm_url（跨通知共享，同一 URL 本轮只下一遍）。
    attempted: &mut std::collections::HashSet<String>,
    id_seed: u64,
    log: &mut Vec<String>,
    mut progress: F,
) -> AttachmentStats
where
    F: FnMut(usize, usize, String) + Send,
{
    let mut stats = AttachmentStats::default();
    if bundle.html.trim().is_empty() {
        return stats;
    }
    let base = Url::parse(&bundle.base_url)
        .or_else(|_| Url::parse(&source.url))
        .ok();
    let Some(base) = base else {
        push_log(log, format!("附件链接解析失败：详情地址不是合法网址（{}）", bundle.base_url));
        return stats;
    };

    let links = extract_attachment_links(&bundle.html, &base);
    let total = links.len();
    if total == 0 {
        return stats;
    }
    // 同一 norm_url 在本次抓取中只实际下载一次；后续通知只共享记录。
    // attempted 由调用方跨通知共享，保证多通知引用同一附件时本轮只下载一次。

    for (index, link) in links.iter().enumerate() {
        let norm_url = normalize_attachment_url(&link.url);
        let existing = store
            .attachments
            .iter()
            .position(|record| record.norm_url == norm_url);

        let slot: usize = if let Some(pos) = existing {
            let record = &mut store.attachments[pos];
            if !record.notice_ids.iter().any(|id| id == &notice.id) {
                record.notice_ids.push(notice.id.clone());
            }
            if record.status == AttachmentStatus::Downloaded && is_file(dir, &record.local_path) {
                stats.skipped += 1;
                progress(index + 1, total, record.file_name.clone());
                continue;
            }
            if record.status == AttachmentStatus::TooLarge {
                // 过大：本轮与后续都不重试（设计 §4(b)）。
                stats.skipped += 1;
                progress(index + 1, total, record.file_name.clone());
                continue;
            }
            if attempted.contains(&norm_url) {
                // 本轮已为该共享附件下载过（无论成功失败），不再重复请求。
                stats.skipped += 1;
                progress(index + 1, total, record.file_name.clone());
                continue;
            }
            // 共享记录仍在，但文件缺失 / 上次失败：复用同一条记录重下，不产生重复记录。
            record.source_id = source.id.clone();
            record.source_url = link.url.clone();
            record.file_name = link.file_name.clone();
            record.status = AttachmentStatus::Missing;
            record.error.clear();
            pos
        } else {
            let local_path = local_file_name(&link.file_name, &link.ext, &norm_url);
            let record = AttachmentRecord {
                id: new_id(
                    "a",
                    &norm_url,
                    clock.now().timestamp_millis(),
                    id_seed.saturating_add(index as u64),
                ),
                notice_ids: vec![notice.id.clone()],
                source_id: source.id.clone(),
                source_url: link.url.clone(),
                norm_url: norm_url.clone(),
                file_name: link.file_name.clone(),
                local_path: local_path.clone(),
                status: AttachmentStatus::Missing,
                ..AttachmentRecord::default()
            };
            store.attachments.push(record);
            store.attachments.len() - 1
        };

        let result = download_one(fetcher, vpn, source, &link.url, &link.file_name).await;
        attempted.insert(norm_url.clone());
        let record = &mut store.attachments[slot];
        apply_result(record, result, dir, clock, &mut stats, log);
        progress(index + 1, total, record.file_name.clone());
    }
    stats
}

/// 单条附件重新下载（`attachment.retry`）。返回是否找到记录。
pub async fn retry_attachment(
    fetcher: &dyn Fetcher,
    vpn: &WebvpnConfig,
    dir: &Path,
    store: &mut StoreData,
    id: &str,
    clock: &dyn Clock,
    log: &mut Vec<String>,
) -> bool {
    let Some(index) = store.attachments.iter().position(|a| a.id == id) else {
        return false;
    };
    let record = store.attachments[index].clone();
    let Some(source) = store
        .sources
        .iter()
        .find(|s| s.id == record.source_id)
        .cloned()
    else {
        store.attachments[index].status = AttachmentStatus::Failed;
        store.attachments[index].error = truncate_error("找不到附件对应的数据源，无法重建请求");
        return true;
    };
    let ext = cn_core::extension_of(&record.file_name);
    if !cn_core::is_allowed_extension(&ext) {
        store.attachments[index].status = AttachmentStatus::Failed;
        store.attachments[index].error = truncate_error("附件文件名没有可用的白名单扩展名");
        return true;
    }
    let link = AttachmentLink {
        url: record.source_url.clone(),
        file_name: record.file_name.clone(),
        ext,
    };
    store.attachments[index].status = AttachmentStatus::Missing;
    store.attachments[index].error.clear();
    let result = download_one(fetcher, vpn, &source, &link.url, &link.file_name).await;
    let mut stats = AttachmentStats::default();
    let record = &mut store.attachments[index];
    apply_result(record, result, dir, clock, &mut stats, log);
    true
}

/// 删除通知的级联清理：只有最后一个引用被移除时才删本地文件与记录。
pub fn cascade_remove(store: &mut StoreData, dir: &Path, notice_id: &str, log: &mut Vec<String>) {
    let mut remove_indexes: Vec<usize> = Vec::new();
    for (index, record) in store.attachments.iter_mut().enumerate() {
        if !record.notice_ids.iter().any(|id| id == notice_id) {
            continue;
        }
        record.notice_ids.retain(|id| id != notice_id);
        if !record.notice_ids.is_empty() {
            continue;
        }
        if is_safe_relative(&record.local_path) {
            let path = dir.join(&record.local_path);
            if path.is_file() && std::fs::remove_file(&path).is_err() {
                push_log(log, format!("删除附件文件失败：{}", path.to_string_lossy()));
            }
        } else {
            push_log(log, format!("拒绝删除越界附件路径：{}", record.local_path));
        }
        remove_indexes.push(index);
    }
    for index in remove_indexes.into_iter().rev() {
        store.attachments.remove(index);
    }
}

/// 构造一次下载请求并重试；不抛错，错误分类成 DownloadResult。
async fn download_one(
    fetcher: &dyn Fetcher,
    vpn: &WebvpnConfig,
    source: &SourceRecord,
    raw_url: &str,
    file_name: &str,
) -> DownloadResult {
    // 先还原网关地址，再由 build_request 统一重写出站 URL 并附加同源 Cookie。
    let restored_url = restore_urls_in_text(raw_url, vpn);
    let mut last_error = String::new();
    for attempt in 1..=MAX_ATTEMPTS {
        let mut req = build_request(&restored_url, source, vpn, REQUEST_TIMEOUT_MS);
        req.max_bytes = ATTACHMENT_MAX_BYTES;
        match fetcher.fetch(req).await {
            Ok(response) if (200..300).contains(&response.status) && !response.body.is_empty() => {
                return DownloadResult::Ok(response.body);
            }
            Ok(response) if (200..300).contains(&response.status) => {
                last_error = format!("附件「{file_name}」响应内容为空");
            }
            Ok(response) => {
                last_error = format!("附件「{file_name}」返回 HTTP {}", response.status);
            }
            Err(NetError::TooLarge(_)) => {
                return DownloadResult::TooLarge(format!(
                    "附件「{file_name}」超过 {} MiB，已跳过",
                    ATTACHMENT_MAX_BYTES / 1024 / 1024
                ));
            }
            Err(error) => {
                last_error = format!("附件「{file_name}」下载失败：{error}");
            }
        }
        if attempt < MAX_ATTEMPTS {
            let delay = if attempt == 1 { 150 } else { 300 };
            tokio::time::sleep(Duration::from_millis(delay)).await;
        }
    }
    DownloadResult::failed(last_error)
}

/// 把下载结果落到记录与统计；文件 IO 失败同样只记状态。
fn apply_result(
    record: &mut AttachmentRecord,
    result: DownloadResult,
    dir: &Path,
    clock: &dyn Clock,
    stats: &mut AttachmentStats,
    log: &mut Vec<String>,
) {
    match result {
        DownloadResult::Ok(bytes) => match write_atomic(dir, &record.local_path, &bytes) {
            Ok(()) => {
                record.status = AttachmentStatus::Downloaded;
                record.size_bytes = bytes.len() as u64;
                record.fetched_at = clock.now().to_rfc3339();
                record.error.clear();
                stats.downloaded += 1;
            }
            Err(error) => {
                record.status = AttachmentStatus::Failed;
                record.error = truncate_error(error);
                stats.failed += 1;
                push_log(log, format!("附件「{}」落盘失败：{}", record.file_name, record.error));
            }
        },
        DownloadResult::TooLarge(reason) => {
            record.status = AttachmentStatus::TooLarge;
            record.error = truncate_error(reason);
            stats.skipped += 1;
            push_log(log, record.error.clone());
        }
        DownloadResult::Failed(reason) => {
            record.status = AttachmentStatus::Failed;
            record.error = truncate_error(reason);
            stats.failed += 1;
            push_log(log, record.error.clone());
        }
    }
}

/// tmp + rename 原子写；首次写入自动建目录。
fn write_atomic(dir: &Path, local_path: &str, bytes: &[u8]) -> Result<(), String> {
    if !is_safe_relative(local_path) {
        return Err(format!("非法附件文件名：{local_path}"));
    }
    std::fs::create_dir_all(dir).map_err(|e| format!("创建附件目录失败：{e}"))?;
    let target = dir.join(local_path);
    let tmp = dir.join(format!("{local_path}.tmp"));
    std::fs::write(&tmp, bytes).map_err(|e| format!("写入附件临时文件失败：{e}"))?;
    std::fs::rename(&tmp, &target).map_err(|e| format!("附件临时文件改名失败：{e}"))?;
    Ok(())
}

fn is_file(dir: &Path, local_path: &str) -> bool {
    is_safe_relative(local_path) && dir.join(local_path).is_file()
}

fn is_safe_relative(name: &str) -> bool {
    !name.is_empty()
        && !name.contains("..")
        && Path::new(name).file_name().and_then(|s| s.to_str()) == Some(name)
}

/// `{stem}_{sha1(norm_url)[0..8]}.{ext}`，始终带 hash，天然无冲突且可稳定复用。
fn local_file_name(file_name: &str, ext: &str, norm_url: &str) -> String {
    let mut hasher = sha1::Sha1::new();
    hasher.update(norm_url.as_bytes());
    let digest = hasher.finalize();
    let hash: String = digest.iter().take(4).map(|b| format!("{b:02x}")).collect();
    let suffix = format!(".{ext}");
    let stem = if file_name.to_ascii_lowercase().ends_with(&suffix) {
        file_name[..file_name.len() - suffix.len()].to_string()
    } else {
        file_name.to_string()
    };
    let stem: String = stem.chars().take(80).collect();
    let stem = if stem.is_empty() {
        "attachment".to_string()
    } else {
        stem
    };
    format!("{stem}_{hash}.{ext}")
}

fn truncate_error(text: impl AsRef<str>) -> String {
    text.as_ref().chars().take(ERROR_MAX_CHARS).collect()
}

fn push_log(log: &mut Vec<String>, text: impl AsRef<str>) {
    log.push(cn_core::LogLine::new("warn", text).text);
}
