using Microsoft.UI;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Media;
using CampusNotice.Models;

namespace CampusNotice.Helpers;

/// <summary>
/// 重要度四档：阈值 70 / 50 / 35，沿用 Web 版 app.js scoreTone() L63-65。
/// 「重要」筛选阈值 = score &gt;= 50（主理人拍板 D1，与 Web 版 &gt;= 55 是有意差异）。
///
/// 三重编码（视觉规范 §2.4 / §7.4、UX 规范 §3.1）：
///   档位文字 + score 数字 + 形状（实心圆 / 实心菱形 / 空心圆 / 小方点）。
/// 高对比度下颜色可能被系统覆盖，形状与文字是唯一保底。
/// </summary>
public enum ScoreShape
{
    /// <summary>实心圆（重要 70–100）。</summary>
    SolidCircle,
    /// <summary>实心菱形（关注 50–69）。</summary>
    SolidDiamond,
    /// <summary>空心圆（一般 35–49）。</summary>
    HollowCircle,
    /// <summary>小方点（低 0–34）。</summary>
    SmallSquare,
    /// <summary>无（未评分 / 0 未评分）。</summary>
    None
}

public sealed record ScoreVisual(
    string BandText,
    string ScoreText,
    ScoreShape Shape,
    string FillKey,
    string ForegroundKey,
    string SubtleKey,
    string AutomationName,
    Brush FillBrush,
    Brush ForegroundBrush,
    Brush SubtleBrush)
{
    /// <summary>「重要 78」「关注 56」「一般 42」「低 18」「未评分」。</summary>
    public string BadgeText => BandText == "未评分" ? "未评分" : $"{BandText} {ScoreText}";
}

public static class ScorePalette
{
    public const int ImportantThreshold = 50;

    /// <summary>「重要：关注度 ≥ 50（关注 50–69 + 重要 70–100）」——三处明示之一。</summary>
    public const string ImportantExplanation = "重要：关注度 ≥ 50（关注 50–69 + 重要 70–100）";

    /// <summary>筛选判定：只用这一个方法，禁止在别处硬编码 55 或 50。</summary>
    public static bool IsImportant(Notice notice) => (notice.Score ?? 0) >= ImportantThreshold;

    public static bool IsImportant(int? score) => (score ?? 0) >= ImportantThreshold;

    public static ScoreVisual For(int? rawScore)
    {
        // score 缺失 / 0 未评分 → 「未评分」，绝不显示为红色（视觉规范 §7.4 硬性要求）。
        if (rawScore is null || rawScore <= 0)
        {
            return Make("未评分", "未评分", ScoreShape.None, "ScoreNone");
        }

        int score = Math.Clamp(rawScore.Value, 0, 100);
        return score switch
        {
            >= 70 => Make("重要", score.ToString(), ScoreShape.SolidCircle, "ScoreCritical"),
            >= 50 => Make("关注", score.ToString(), ScoreShape.SolidDiamond, "ScoreHigh"),
            >= 35 => Make("一般", score.ToString(), ScoreShape.HollowCircle, "ScoreMedium"),
            _ => Make("低", score.ToString(), ScoreShape.SmallSquare, "ScoreLow")
        };
    }

    private static ScoreVisual Make(string band, string number, ScoreShape shape, string prefix)
    {
        string fillKey = prefix + "FillBrush";
        string fgKey = prefix + "ForegroundBrush";
        string subtleKey = prefix + "SubtleBrush";
        string automation = band == "未评分" ? "未评分" : $"重要度 {number} 分，{band}";
        return new ScoreVisual(
            band, number, shape, fillKey, fgKey, subtleKey, automation,
            ThemeBrush.Get(fillKey), ThemeBrush.Get(fgKey), ThemeBrush.Get(subtleKey));
    }

    /// <summary>档位文字（用于 AutomationProperties.Name 模板「{档位} {score} 分」）。</summary>
    public static string Band(int? score) => For(score).BandText;
}

/// <summary>
/// 从 Application.Resources 按语义键取 Brush。
/// 先按当前主题从 MergedDictionaries 里的 ThemeDictionaries 精确取值，
/// 再回退到外层索引器，最后回退到中灰——保证任何主题/资源缺失下都不抛异常。
/// 代码里零裸 HEX：所有颜色都来自 Themes/Colors.xaml。
/// </summary>
public static class ThemeBrush
{
    public static Brush Get(string key)
    {
        var resources = Application.Current?.Resources;
        if (resources is null) return new SolidColorBrush(Colors.Gray);

        string theme = ResolveThemeName();
        foreach (var dict in resources.MergedDictionaries)
        {
            if (dict.ThemeDictionaries is null) continue;
            if (dict.ThemeDictionaries.TryGetValue(theme, out var themed) && themed is ResourceDictionary td)
            {
                if (td.TryGetValue(key, out var value) && value is Brush brush) return brush;
            }
        }

        if (resources.TryGetValue(key, out var direct) && direct is Brush directBrush) return directBrush;
        return new SolidColorBrush(Colors.Gray);
    }

    private static string ResolveThemeName()
    {
        try
        {
            // 高对比度优先：显式关闭 Mica/Acrylic 与 C 层自定义色（视觉规范 §9.2）。
            var accessibility = new Windows.UI.ViewManagement.AccessibilitySettings();
            if (accessibility.HighContrast) return "HighContrast";
        }
        catch
        {
            // 部分 unpackaged 环境查询 HighContrast 可能失败，退化为按实际主题判断。
        }
        // App.CurrentThemeName 由 MainWindow 的 ActualThemeChanged 维护，比
        // Application.RequestedTheme 更准确（后者在未显式设置时未必反映系统主题）。
        return App.CurrentThemeName == "Dark" ? "Dark" : "Light";
    }

    public static bool IsHighContrast()
    {
        try
        {
            return new Windows.UI.ViewManagement.AccessibilitySettings().HighContrast;
        }
        catch
        {
            return false;
        }
    }
}
