using System.Text.Json.Serialization;

namespace CampusNotice.Models;

/// <summary>
/// state.items[]（后端最多返回 300 条）。字段对齐 store.mjs makeItem() L579-597
/// 及 act('item.update'/'item.done') 写入的 dueAt / doneAt / fired。
/// </summary>
public sealed class ScheduleItem
{
    [JsonPropertyName("id")] public string Id { get; set; } = string.Empty;
    [JsonPropertyName("title")] public string Title { get; set; } = string.Empty;
    [JsonPropertyName("note")] public string Note { get; set; } = string.Empty;

    /// <summary>YYYY-MM-DDTHH:mm（normalizeDue 归一化后的字符串），空 = 未定时间。</summary>
    [JsonPropertyName("dueAt")] public string DueAt { get; set; } = string.Empty;

    /// <summary>提前提醒档位（分钟数组，如 [1440,120]）。</summary>
    [JsonPropertyName("remindBefore")] public List<int> RemindBefore { get; set; } = new();

    /// <summary>已触发过的档位；length &gt; 0 时「重置提醒」(item.refire) 才出现。</summary>
    [JsonPropertyName("fired")] public List<int> Fired { get; set; } = new();

    [JsonPropertyName("done")] public bool Done { get; set; }
    [JsonPropertyName("doneAt")] public string DoneAt { get; set; } = string.Empty;
    [JsonPropertyName("createdAt")] public string CreatedAt { get; set; } = string.Empty;

    /// <summary>'manual' | 'notice' | 自动建日程。</summary>
    [JsonPropertyName("source")] public string Source { get; set; } = "manual";

    /// <summary>来源通知 id（source='notice' 时非空），用于「已在日程中」判断与去重。</summary>
    [JsonPropertyName("noticeId")] public string NoticeId { get; set; } = string.Empty;

    /// <summary>通知原文链接（「查看通知原文 ↗」）。</summary>
    [JsonPropertyName("url")] public string Url { get; set; } = string.Empty;

    [JsonIgnore]
    public bool FromNotice => !string.IsNullOrEmpty(NoticeId) || Source == "notice";

    [JsonIgnore]
    public bool CanRefire => Fired.Count > 0;

    [JsonIgnore]
    public bool HasUrl => !string.IsNullOrWhiteSpace(Url);
}
