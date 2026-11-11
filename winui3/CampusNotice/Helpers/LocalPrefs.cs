using System.Text.Json;

namespace CampusNotice.Helpers;

/// <summary>
/// 纯客户端本地偏好（不写入后端、不新增 API）：
///   - 首次运行 3 步引导完成标记（UX §7.1）
///   - 各页筛选 / 显示已完成开关 / 上次所在页（§1.3.5 切页保留）
///   - 「关闭到托盘」一次性提示标记（§7.5）
///
/// unpackaged 应用不使用 Windows.Storage.ApplicationData（无包标识），
/// 直接写 %APPDATA%\campus-notice\winui3-prefs.json，与后端 store.json 同目录但互不影响。
/// 读写失败一律降级为默认值，绝不阻断启动。
/// </summary>
public sealed class LocalPrefs
{
    private static readonly Lazy<LocalPrefs> Lazy = new(Load);
    public static LocalPrefs Current => Lazy.Value;

    private static string DirectoryPath => Path.Combine(
        Environment.GetFolderPath(Environment.SpecialFolder.ApplicationData), "campus-notice");

    private static string FilePath => Path.Combine(DirectoryPath, "winui3-prefs.json");

    public bool FirstRunCompleted { get; set; }
    public bool CloseToTrayHintShown { get; set; }
    public string LastPage { get; set; } = "notices";
    public string NoticeFilter { get; set; } = "all";
    public bool ShowDoneItems { get; set; }
    public bool NotifyEnabledLocal { get; set; } = true;

    private static LocalPrefs Load()
    {
        try
        {
            if (File.Exists(FilePath))
            {
                var json = File.ReadAllText(FilePath);
                return JsonSerializer.Deserialize<LocalPrefs>(json) ?? new LocalPrefs();
            }
        }
        catch
        {
            // 忽略：本地偏好损坏不应影响应用启动。
        }
        return new LocalPrefs();
    }

    public void Save()
    {
        try
        {
            Directory.CreateDirectory(DirectoryPath);
            File.WriteAllText(FilePath, JsonSerializer.Serialize(this, new JsonSerializerOptions { WriteIndented = true }));
        }
        catch
        {
            // 忽略：无写权限时功能仍可用，只是下次启动不记住偏好。
        }
    }
}
