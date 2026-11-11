using System.Text.Json.Serialization;

namespace CampusNotice.Models;

/// <summary>
/// POST /api/action 的统一响应：{ ok, message?, error?, preview?, id?, state }。
/// **绝大多数 op 回传全量 state，包括 ok:false 的失败响应**（store.mjs act() L646-969）。
/// 因此 StateStore 对成功与失败都应用 response.State；只有请求异常（无 state）才走断连流程。
/// </summary>
public sealed class ActionResponse
{
    [JsonPropertyName("ok")] public bool Ok { get; set; }
    [JsonPropertyName("message")] public string? Message { get; set; }
    [JsonPropertyName("error")] public string? Error { get; set; }

    /// <summary>仅 item.add 成功时返回，用于「撤销」= item.remove{id}。</summary>
    [JsonPropertyName("id")] public string? Id { get; set; }

    /// <summary>仅 source.test 成功时返回，最多 5 条 {title,url,date}。</summary>
    [JsonPropertyName("preview")] public List<PreviewItem>? Preview { get; set; }

    [JsonPropertyName("state")] public AppState? State { get; set; }

    [JsonIgnore]
    public string FailureText => Error ?? Message ?? "操作失败";

    [JsonIgnore]
    public string SuccessText => Message ?? "操作成功";
}

/// <summary>source.test 成功返回的预览条目（store.mjs L915）。</summary>
public sealed class PreviewItem
{
    [JsonPropertyName("title")] public string Title { get; set; } = string.Empty;
    [JsonPropertyName("url")] public string Url { get; set; } = string.Empty;
    [JsonPropertyName("date")] public string Date { get; set; } = string.Empty;
}

/// <summary>GET /api/state 的外层包装。</summary>
public sealed class StateEnvelope
{
    [JsonPropertyName("ok")] public bool Ok { get; set; }
    [JsonPropertyName("state")] public AppState? State { get; set; }
    [JsonPropertyName("error")] public string? Error { get; set; }
}

/// <summary>GET /api/health 的响应（server.mjs L340）。</summary>
public sealed class HealthResponse
{
    [JsonPropertyName("ok")] public bool Ok { get; set; }
    [JsonPropertyName("port")] public int Port { get; set; }
    [JsonPropertyName("pid")] public int Pid { get; set; }
    [JsonPropertyName("dataPath")] public string DataPath { get; set; } = string.Empty;
    [JsonPropertyName("notices")] public int Notices { get; set; }
}

/// <summary>POST /api/shutdown 的响应。</summary>
public sealed class ShutdownResponse
{
    [JsonPropertyName("ok")] public bool Ok { get; set; }
    [JsonPropertyName("message")] public string? Message { get; set; }
}
