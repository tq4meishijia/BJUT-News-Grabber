using System.Text;
using System.Text.Json;
using CampusNotice.Models;

namespace CampusNotice.Services;

/// <summary>
/// 全部后端 op 常量表。**只收录 server.mjs / store.mjs 中真实存在的 op，禁止虚构。**
/// 逐一比对结论见 docs/winui3-engineering.md「op × 页面映射与静态比对」。
/// </summary>
public static class Ops
{
    // store.mjs act() L646-969
    public const string Poll = "poll";
    public const string NoticeRead = "notice.read";
    public const string NoticeReadAll = "notice.readAll";
    public const string NoticeRemove = "notice.remove";
    public const string NoticeSummarize = "notice.summarize";
    public const string NoticesRescore = "notices.rescore";
    public const string NoticesRefill = "notices.refill";
    public const string ItemAdd = "item.add";
    public const string ItemUpdate = "item.update";
    public const string ItemDone = "item.done";
    public const string ItemRemove = "item.remove";
    public const string ItemRefire = "item.refire";
    public const string SourceAdd = "source.add";
    public const string SourceUpdate = "source.update";
    public const string SourceRemove = "source.remove";
    public const string SourceTest = "source.test";
    public const string SettingsSave = "settings.save";

    // server.mjs L241-310 单独处理的 5 个（+ notify.test）
    public const string LlmKey = "llm.key";
    public const string LlmTest = "llm.test";
    public const string AccountSave = "account.save";
    public const string AccountTest = "account.test";
    public const string AccountClear = "account.clear";

    /// <summary>
    /// 仅后端 PowerShell 通道，**本 WinUI 客户端禁止调用**（决策 3：原生 Toast 单一通道）。
    /// 仅 Web 版 / 复用他实例服务时使用；本客户端设置页「测试桌面通知」走 ToastService.ShowTestNotification()，
    /// 不调用此 op。
    /// </summary>
    public const string NotifyTest = "notify.test";
}

public sealed class ApiException : Exception
{
    public ApiException(string message, Exception? inner = null) : base(message, inner) { }
}

/// <summary>
/// 本地 HTTP 客户端。服务只监听 127.0.0.1:8737。
///
/// 同源约束：server.mjs sameOrigin() L189-198 只在请求带 Origin 时校验，
/// HttpClient 默认**不带 Origin 头**，因此这里绝不添加 Origin / Referer。
///
/// 统一异常与超时：所有网络/解析异常包成 ApiException；StateStore 据此进入「服务断连」流程。
/// </summary>
public sealed class ApiClient
{
    public const int Port = 8737;
    public static string BaseAddress => $"http://127.0.0.1:{Port}/";
    public string RssUrl => $"{BaseAddress}rss.xml";

    private readonly HttpClient _http;
    private static readonly JsonSerializerOptions JsonOptions = new()
    {
        PropertyNameCaseInsensitive = true,
        NumberHandling = System.Text.Json.Serialization.JsonNumberHandling.AllowReadingFromString,
    };

    public ApiClient()
    {
        var handler = new SocketsHttpHandler
        {
            // 本地回环，不需要代理；避免系统代理把 127.0.0.1 请求转发出去。
            UseProxy = false,
            AllowAutoRedirect = false,
            ConnectTimeout = TimeSpan.FromSeconds(3),
        };
        _http = new HttpClient(handler, disposeHandler: true)
        {
            BaseAddress = new Uri(BaseAddress),
            Timeout = TimeSpan.FromSeconds(20),
        };
        // 不设置任何 Origin / Referer 头。
    }

    /// <summary>GET /api/health → { ok, port, pid, dataPath, notices }。</summary>
    public async Task<HealthResponse> HealthAsync(CancellationToken ct = default)
    {
        using var response = await _http.GetAsync("api/health", ct).ConfigureAwait(false);
        response.EnsureSuccessStatusCode();
        var json = await response.Content.ReadAsStringAsync(ct).ConfigureAwait(false);
        return JsonSerializer.Deserialize<HealthResponse>(json, JsonOptions)
               ?? throw new ApiException("健康检查响应为空");
    }

    /// <summary>GET /api/state → { ok, state }。仅用于启动与 10s 静默刷新。</summary>
    public async Task<AppState> GetStateAsync(CancellationToken ct = default)
    {
        using var response = await _http.GetAsync("api/state", ct).ConfigureAwait(false);
        response.EnsureSuccessStatusCode();
        var json = await response.Content.ReadAsStringAsync(ct).ConfigureAwait(false);
        var envelope = JsonSerializer.Deserialize<StateEnvelope>(json, JsonOptions)
                       ?? throw new ApiException("状态响应为空");
        if (!envelope.Ok || envelope.State is null)
            throw new ApiException(envelope.Error ?? "服务返回了无效状态");
        return envelope.State;
    }

    /// <summary>
    /// POST /api/action，请求体 { op, ...params }。
    /// 响应恒为 { ok, message?, error?, preview?, id?, state }；
    /// **ok:false 时也带 state**，调用方必须一并应用。
    /// </summary>
    public async Task<ActionResponse> PostActionAsync(
        string op,
        Dictionary<string, object?>? parameters = null,
        CancellationToken ct = default)
    {
        var body = new Dictionary<string, object?> { ["op"] = op };
        if (parameters is not null)
        {
            foreach (var pair in parameters) body[pair.Key] = pair.Value;
        }

        string json = JsonSerializer.Serialize(body, JsonOptions);
        using var content = new StringContent(json, Encoding.UTF8, "application/json");
        using var response = await _http.PostAsync("api/action", content, ct).ConfigureAwait(false);
        response.EnsureSuccessStatusCode();
        var text = await response.Content.ReadAsStringAsync(ct).ConfigureAwait(false);
        return JsonSerializer.Deserialize<ActionResponse>(text, JsonOptions)
               ?? throw new ApiException("操作响应为空");
    }

    /// <summary>POST /api/shutdown（后端立即回 ok:true，80ms 后自行退出）。只在「退出」且托管了 server 时调用。</summary>
    public async Task<ShutdownResponse?> ShutdownAsync(CancellationToken ct = default)
    {
        using var content = new StringContent("{}", Encoding.UTF8, "application/json");
        using var response = await _http.PostAsync("api/shutdown", content, ct).ConfigureAwait(false);
        response.EnsureSuccessStatusCode();
        var text = await response.Content.ReadAsStringAsync(ct).ConfigureAwait(false);
        return JsonSerializer.Deserialize<ShutdownResponse>(text, JsonOptions);
    }
}
