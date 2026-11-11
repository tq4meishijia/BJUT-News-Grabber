using System.Text.Json;
using System.Text.Json.Serialization;

namespace CampusNotice.Models;

/// <summary>
/// state.sources[] —— 逐字段对齐 store.mjs state() L977-999。
/// 注意：state() 不返回 cookie 明文，只返回 hasCookie；密码/凭据一律 PasswordBox 不回显。
/// </summary>
public sealed class SourceItem
{
    [JsonPropertyName("id")] public string Id { get; set; } = string.Empty;
    [JsonPropertyName("name")] public string Name { get; set; } = string.Empty;

    /// <summary>'html' | 'rss' | 'json'（source.add 只接受这三种，不接受 wechat）。</summary>
    [JsonPropertyName("type")] public string Type { get; set; } = "html";

    [JsonPropertyName("url")] public string Url { get; set; } = string.Empty;
    [JsonPropertyName("enabled")] public bool Enabled { get; set; }
    [JsonPropertyName("note")] public string Note { get; set; } = string.Empty;

    /// <summary>已保存 Cookie（永远不回显内容）。</summary>
    [JsonPropertyName("hasCookie")] public bool HasCookie { get; set; }

    /// <summary>D2：Warning 橙 + 必有「重新登录」动作，不用红色。</summary>
    [JsonPropertyName("loginExpired")] public bool LoginExpired { get; set; }

    [JsonPropertyName("resolveIp")] public string ResolveIp { get; set; } = string.Empty;
    [JsonPropertyName("linkPattern")] public string LinkPattern { get; set; } = string.Empty;
    [JsonPropertyName("itemsPath")] public string ItemsPath { get; set; } = string.Empty;
    [JsonPropertyName("pages")] public int Pages { get; set; } = 1;
    [JsonPropertyName("pageUrl")] public string PageUrl { get; set; } = string.Empty;
    [JsonPropertyName("urlTemplate")] public string UrlTemplate { get; set; } = string.Empty;
    [JsonPropertyName("detailUrlTemplate")] public string DetailUrlTemplate { get; set; } = string.Empty;
    [JsonPropertyName("contentPath")] public string ContentPath { get; set; } = string.Empty;

    /// <summary>JSON 源字段映射；值可能是任意 JSON 类型，故保留 JsonElement 避免反序列化失败。</summary>
    [JsonPropertyName("map")] public JsonElement? Map { get; set; }

    /// <summary>'idle' | 'ok' | 'error'。</summary>
    [JsonPropertyName("status")] public string Status { get; set; } = "idle";

    [JsonPropertyName("statusText")] public string StatusText { get; set; } = string.Empty;
    [JsonPropertyName("lastFetchAt")] public string LastFetchAt { get; set; } = string.Empty;
    [JsonPropertyName("lastCount")] public int LastCount { get; set; }

    [JsonIgnore]
    public bool IsHtml => Type == "html";

    [JsonIgnore]
    public bool IsRss => Type == "rss";

    [JsonIgnore]
    public bool IsJson => Type == "json";
}
