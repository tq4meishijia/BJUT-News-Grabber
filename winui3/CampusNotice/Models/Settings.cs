using System.Text.Json.Serialization;

namespace CampusNotice.Models;

/// <summary>
/// state.settings —— 逐字段对齐 core.mjs emptyStore() L88-103 与
/// store.mjs act('settings.save') L940-960 可写字段。
///
/// settings.save 末尾会 restartTimers()：每次调用都清空并重建抓取/提醒定时器。
/// 因此行为开关必须防抖 + 串行 + 在途禁用，表单必须显式一次性保存（UX 规范 §6.2）。
/// </summary>
public sealed class Settings
{
    [JsonPropertyName("pollMinutes")] public int PollMinutes { get; set; } = 30;
    [JsonPropertyName("keywords")] public List<string> Keywords { get; set; } = new();

    /// <summary>新通知用 AI 摘要（关掉走规则摘要）。</summary>
    [JsonPropertyName("llm")] public bool Llm { get; set; } = true;

    [JsonPropertyName("llmMaxPerPoll")] public int LlmMaxPerPoll { get; set; } = 20;

    /// <summary>通知里识别到截止时间时自动建成日程。</summary>
    [JsonPropertyName("autoSchedule")] public bool AutoSchedule { get; set; } = true;

    /// <summary>默认提前提醒档位（分钟），默认 [1440,120]。</summary>
    [JsonPropertyName("autoRemindBefore")] public List<int> AutoRemindBefore { get; set; } = new() { 1440, 120 };

    [JsonPropertyName("maxItemsPerSource")] public int MaxItemsPerSource { get; set; } = 100;

    /// <summary>跟进详情页正文。</summary>
    [JsonPropertyName("fetchDetail")] public bool FetchDetail { get; set; } = true;

    [JsonPropertyName("detailLimit")] public int DetailLimit { get; set; } = 25;

    /// <summary>
    /// 到点弹桌面通知。**只控制提醒通道，不涉及进程启动行为**。
    /// false 时后端 recordReminder 不调 onNotify、客户端 ToastService 也不弹（两端都尊重）。
    /// </summary>
    [JsonPropertyName("notify")] public bool Notify { get; set; } = true;

    /// <summary>生成 settings.save 的单键 patch（行为开关即时保存用，避免重建其它字段）。</summary>
    public static Dictionary<string, object?> TogglePatch(string key, bool value) =>
        new() { ["settings"] = new Dictionary<string, object?> { [key] = value } };
}
