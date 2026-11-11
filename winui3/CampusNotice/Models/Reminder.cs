using System.Text.Json.Serialization;

namespace CampusNotice.Models;

/// <summary>
/// state.meta.reminders[] —— 对齐 store.mjs recordReminder() L627-641。
/// 后端 checkReminders() 每 15s 生成一次提醒（服务端事实）；客户端 StateStore 以唯一 10s 轮询
/// /api/state 同时做 UI 静默刷新与提醒增量检测，ToastService 按 Id 去重，
/// 且只对本次会话启动后出现的新 id 发 AppNotification。
///
/// 关键：recordReminder 是「先写 meta.reminders 再判断 settings.notify」，
/// 所以 server.mjs 带 --no-toast 静默化后端 PowerShell 通道不会丢提醒记录。
/// </summary>
public sealed class Reminder
{
    [JsonPropertyName("id")] public string Id { get; set; } = string.Empty;

    /// <summary>用于「打开日程」深链定位，只定位不改状态。</summary>
    [JsonPropertyName("itemId")] public string ItemId { get; set; } = string.Empty;

    [JsonPropertyName("title")] public string Title { get; set; } = string.Empty;

    /// <summary>提醒正文（如「提前1天：11月5日 14:30 到期（还有 X）」），Toast 里 ≤2 行。</summary>
    [JsonPropertyName("note")] public string Note { get; set; } = string.Empty;

    [JsonPropertyName("dueAt")] public string DueAt { get; set; } = string.Empty;

    /// <summary>生成时间 ISO8601，用于判定「本次会话启动后出现的新 id」。</summary>
    [JsonPropertyName("at")] public string At { get; set; } = string.Empty;
}
