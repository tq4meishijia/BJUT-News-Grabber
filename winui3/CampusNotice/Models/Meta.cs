using System.Text.Json.Serialization;

namespace CampusNotice.Models;

/// <summary>
/// state.meta —— 逐字段对齐 store.mjs state() L1002-1017 与 core.mjs emptyStore() L125。
/// reminders 是后端 checkReminders()/recordReminder() 生成的持久记录（state 只回前 20 条）。
/// </summary>
public sealed class Meta
{
    [JsonPropertyName("lastPollAt")] public string LastPollAt { get; set; } = string.Empty;
    [JsonPropertyName("lastPollError")] public string LastPollError { get; set; } = string.Empty;
    [JsonPropertyName("pollCount")] public int PollCount { get; set; }

    [JsonPropertyName("reminders")] public List<Reminder> Reminders { get; set; } = new();

    /// <summary>最近一次 LLM 调用结果（store.mjs lastLlm）。</summary>
    [JsonPropertyName("llm")] public LlmStatus? Llm { get; set; }

    /// <summary>从不返回密码；configured = canAutoLogin()。</summary>
    [JsonPropertyName("account")] public AccountInfo Account { get; set; } = new();

    /// <summary>抓取进行中（后端 polling 标志）。</summary>
    [JsonPropertyName("polling")] public bool Polling { get; set; }

    [JsonPropertyName("dataPath")] public string DataPath { get; set; } = string.Empty;
    [JsonPropertyName("port")] public int Port { get; set; } = 8737;
    [JsonPropertyName("llmKeySet")] public bool LlmKeySet { get; set; }
}

public sealed class LlmStatus
{
    [JsonPropertyName("at")] public string At { get; set; } = string.Empty;

    /// <summary>三态：null=未调用，true=成功，false=失败。</summary>
    [JsonPropertyName("ok")] public bool? Ok { get; set; }

    [JsonPropertyName("message")] public string Message { get; set; } = string.Empty;
}

public sealed class AccountInfo
{
    [JsonPropertyName("username")] public string Username { get; set; } = string.Empty;
    [JsonPropertyName("casBase")] public string CasBase { get; set; } = string.Empty;
    [JsonPropertyName("service")] public string Service { get; set; } = string.Empty;

    /// <summary>true → 「重新登录」走 account.test；false → 跳设置页 account.save。</summary>
    [JsonPropertyName("configured")] public bool Configured { get; set; }

    [JsonPropertyName("lastLogin")] public LoginStatus? LastLogin { get; set; }
}

public sealed class LoginStatus
{
    [JsonPropertyName("at")] public string At { get; set; } = string.Empty;
    [JsonPropertyName("ok")] public bool? Ok { get; set; }
    [JsonPropertyName("message")] public string Message { get; set; } = string.Empty;
}
