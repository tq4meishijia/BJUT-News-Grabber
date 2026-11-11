using System.Text.Json.Serialization;

namespace CampusNotice.Models;

/// <summary>
/// state.notices[]（后端最多返回 200 条）。
/// 字段逐项对齐 store.mjs fresh.push() L429-450 与 public/app.js 渲染所用字段。
/// </summary>
public sealed class Notice
{
    [JsonPropertyName("id")] public string Id { get; set; } = string.Empty;
    [JsonPropertyName("dedupeKey")] public string DedupeKey { get; set; } = string.Empty;
    [JsonPropertyName("sourceId")] public string SourceId { get; set; } = string.Empty;
    [JsonPropertyName("sourceName")] public string SourceName { get; set; } = string.Empty;
    [JsonPropertyName("title")] public string Title { get; set; } = string.Empty;
    [JsonPropertyName("url")] public string Url { get; set; } = string.Empty;
    [JsonPropertyName("sourceItemId")] public string SourceItemId { get; set; } = string.Empty;

    /// <summary>发布日期 YYYY-MM-DD。</summary>
    [JsonPropertyName("publishedAt")] public string PublishedAt { get; set; } = string.Empty;

    /// <summary>JSON 源的原始条目上下文（rescore 时可能作为正文）。</summary>
    [JsonPropertyName("context")] public string Context { get; set; } = string.Empty;

    /// <summary>详情页正文（可能为空；展开正文入口在 length &gt; 40 时才出现）。</summary>
    [JsonPropertyName("content")] public string Content { get; set; } = string.Empty;

    [JsonPropertyName("fetchedAt")] public string FetchedAt { get; set; } = string.Empty;

    /// <summary>一句话摘要 / AI 摘要。</summary>
    [JsonPropertyName("summary")] public string Summary { get; set; } = string.Empty;

    /// <summary>「对我有什么用」清单。</summary>
    [JsonPropertyName("useful")] public List<string> Useful { get; set; } = new();

    [JsonPropertyName("tags")] public List<string> Tags { get; set; } = new();

    /// <summary>识别到的截止时间，形如 2026-11-05T14:30；空串表示未识别。</summary>
    [JsonPropertyName("deadline")] public string Deadline { get; set; } = string.Empty;

    [JsonPropertyName("contact")] public string Contact { get; set; } = string.Empty;

    /// <summary>0–100；缺失按 0 / 未评分处理，绝不显示为红色。</summary>
    [JsonPropertyName("score")] public int? Score { get; set; }

    [JsonPropertyName("read")] public bool Read { get; set; }

    [JsonPropertyName("relevant")] public bool Relevant { get; set; } = true;

    /// <summary>true = 已由 AI 生成摘要（rescore 会跳过）。</summary>
    [JsonPropertyName("ai")] public bool Ai { get; set; }

    [JsonIgnore]
    public bool HasBody => (Content?.Length ?? 0) > 40;

    [JsonIgnore]
    public bool HasUrl => !string.IsNullOrWhiteSpace(Url);
}
