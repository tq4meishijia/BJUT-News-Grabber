using System.Text.Json.Serialization;

namespace CampusNotice.Models;

/// <summary>
/// GET /api/state → { ok:true, state:{ settings, sources, notices, items, meta } } 的 state 部分。
/// 也是所有 POST /api/action 响应里回传的同一形状（含 ok:false 的失败响应）。
/// 这是全局唯一数据源，见 Services/StateStore.cs。
/// </summary>
public sealed class AppState
{
    [JsonPropertyName("settings")]
    public Settings Settings { get; set; } = new();

    [JsonPropertyName("sources")]
    public List<SourceItem> Sources { get; set; } = new();

    [JsonPropertyName("notices")]
    public List<Notice> Notices { get; set; } = new();

    [JsonPropertyName("items")]
    public List<ScheduleItem> Items { get; set; } = new();

    [JsonPropertyName("meta")]
    public Meta Meta { get; set; } = new();

    // ---- 只读派生值（不参与反序列化，供绑定与命令栏计数使用）----

    /// <summary>未读通知数（导航 InfoBadge / 托盘菜单第 4 项）。</summary>
    [JsonIgnore]
    public int UnreadCount => Notices.Count(n => !n.Read);

    /// <summary>
    /// 「重要」= score >= 50（主理人拍板 D1；与现有 Web 版 app.js 的 >= 55 是有意差异）。
    /// 四档边界 70 / 50 / 35 见 Helpers/ScorePalette.cs。
    /// </summary>
    public const int ImportantThreshold = 50;

    [JsonIgnore]
    public int ImportantCount => Notices.Count(n => (n.Score ?? 0) >= ImportantThreshold);

    /// <summary>未完成日程数。</summary>
    [JsonIgnore]
    public int PendingItemCount => Items.Count(i => !i.Done);

    /// <summary>存在登录态失效的数据源数量（D2：只驱动 Warning 橙，不升 Error）。</summary>
    [JsonIgnore]
    public int LoginExpiredCount => Sources.Count(s => s.LoginExpired);

    /// <summary>有 status:'error' 的数据源（部分源失败 → Warning；整体失败 → Error）。</summary>
    [JsonIgnore]
    public int ErrorSourceCount => Sources.Count(s => s.Status == "error");

    /// <summary>没有启用任何数据源。</summary>
    [JsonIgnore]
    public bool NoEnabledSource => Sources.All(s => !s.Enabled);
}
