using CampusNotice.Models;

namespace CampusNotice.Helpers;

/// <summary>
/// 纯展示格式化：状态行、日程分组、时间/倒计时/提醒档位、计数。
/// 分组判定沿用 Web 版 public/app.js dayLabel() L50-60（按自然日 delta）。
/// 所有数字由 FontNumeric 样式做等宽右对齐退化方案（Typography.xaml）。
/// </summary>
public static class Formatters
{
    private static readonly string[] GroupOrder =
        { "已逾期", "今天", "明天", "本周内", "更远", "未定时间", "已完成" };

    public static IReadOnlyList<string> GroupNames => GroupOrder;

    /// <summary>解析 dueAt。支持 "YYYY-MM-DDTHH:mm" / "YYYY-MM-DD HH:mm" / 纯日期(补 T23:59)。</summary>
    public static DateTime? ParseDue(string? dueAt)
    {
        if (string.IsNullOrWhiteSpace(dueAt)) return null;
        string text = dueAt.Trim().Replace(' ', 'T');
        if (text.Length == 10) text += "T23:59";
        if (DateTime.TryParse(text, out var dt)) return dt;
        return null;
    }

    public static string DayLabel(string? dueAt)
    {
        var date = ParseDue(dueAt);
        if (date is null) return "未定时间";
        return DayLabel(date.Value);
    }

    public static string DayLabel(DateTime date)
    {
        // 按自然日比较（与 app.js dayLabel 的 startOf/86400000 完全等价）。
        int days = (date.Date - DateTime.Today).Days;
        if (days < 0) return "已逾期";
        if (days == 0) return "今天";
        if (days == 1) return "明天";
        if (days <= 7) return "本周内";
        return "更远";
    }

    /// <summary>分组头取色键（Colors.xaml）。</summary>
    public static string GroupBrushKey(string group) => group switch
    {
        "已逾期" => "GroupOverdueBrush",
        "今天" => "GroupTodayBrush",
        "明天" => "GroupTomorrowBrush",
        "本周内" => "GroupWeekBrush",
        "已完成" => "GroupDoneBrush",
        _ => "GroupFarBrush"
    };

    public static string FormatDue(string? dueAt)
    {
        var dt = ParseDue(dueAt);
        return dt is null ? "—" : dt.Value.ToString("MM月dd日 HH:mm");
    }

    public static string FormatDueShort(string? dueAt)
    {
        var dt = ParseDue(dueAt);
        return dt is null ? "—" : dt.Value.ToString("MM-dd HH:mm");
    }

    public static string CountdownText(string? dueAt)
    {
        var dt = ParseDue(dueAt);
        if (dt is null) return "未定时间";
        var span = dt.Value - DateTime.Now;
        bool overdue = span.TotalSeconds < 0;
        if (overdue) span = span.Negate();
        int days = (int)span.TotalDays;
        int hours = span.Hours;
        int minutes = span.Minutes;
        string body;
        if (days > 0) body = $"{days} 天 {hours} 小时";
        else if (hours > 0) body = $"{hours} 小时 {minutes} 分";
        else body = $"{Math.Max(0, minutes)} 分";
        return overdue ? $"已过 {body}" : $"还有 {body}";
    }

    public static string DeadlineBrushKey(string? dueAt)
    {
        var dt = ParseDue(dueAt);
        if (dt is null) return "DeadlineNoneBrush";
        var span = dt.Value - DateTime.Now;
        if (span.TotalSeconds < 0) return "DeadlineOverdueBrush";
        if (DayLabel(dt.Value) == "今天" || span.TotalHours <= 2) return "DeadlineTodayBrush";
        if (span.TotalHours <= 24) return "DeadlineSoonBrush";
        if (DayLabel(dt.Value) == "本周内") return "DeadlineWeekBrush";
        return "DeadlineFarBrush";
    }

    /// <summary>提醒档位 chip 文案：沿用 core.mjs describeMinutes 的中文口径。</summary>
    public static string RemindText(IReadOnlyList<int>? minutes)
    {
        if (minutes is null || minutes.Count == 0) return "无提醒";
        return "提前 " + string.Join(" · ", minutes.Select(DescribeMinutes));
    }

    public static string DescribeMinutes(int value)
    {
        if (value == 0) return "准时";
        if (value % 1440 == 0) return $"{value / 1440} 天";
        if (value % 60 == 0) return $"{value / 60} 小时";
        return $"{value} 分钟";
    }

    /// <summary>24 小时内到期（未完成）数：导航 InfoBadge 与状态行使用。</summary>
    public static int DueIn24Hours(AppState state)
    {
        var now = DateTime.Now;
        return state.Items.Count(i =>
        {
            if (i.Done) return false;
            var dt = ParseDue(i.DueAt);
            return dt is not null && dt.Value >= now.AddHours(-24) && dt.Value <= now.AddHours(24);
        });
    }

    /// <summary>今日到期（含已逾期，未完成）数：托盘菜单第 5 项使用。</summary>
    public static int DueTodayCount(AppState state)
    {
        var today = DateTime.Today;
        return state.Items.Count(i =>
        {
            if (i.Done) return false;
            var dt = ParseDue(i.DueAt);
            return dt is not null && dt.Value.Date <= today;
        });
    }

    /// <summary>
    /// 状态行（12px，常驻）：「上次抓取 MM-DD HH:mm · 每 N 分钟自动抓一次 · N 项 24 小时内到期」。
    /// 断连时由 MainWindow 覆写为「本地服务未运行」。
    /// 仅在文本实际变化时更新，避免 10s 静默刷新反复触发 LiveRegion 播报（UX §8.4）。
    /// </summary>
    public static string StatusLine(AppState state)
    {
        string first = string.IsNullOrEmpty(state.Meta.LastPollAt)
            ? "还没抓取过"
            : $"上次抓取 {FormatTimestamp(state.Meta.LastPollAt)}";
        string interval = $"每 {state.Settings.PollMinutes} 分钟自动抓一次";
        string soon = $"{DueIn24Hours(state)} 项 24 小时内到期";
        return $"{first} · {interval} · {soon}";
    }

    public static string FormatTimestamp(string iso)
    {
        if (DateTime.TryParse(iso, out var dt)) return dt.ToString("MM-dd HH:mm");
        if (iso.Length >= 16) return iso.Replace('T', ' ').Substring(0, 16);
        return iso;
    }

    public static string ShortTime(string? iso)
    {
        if (string.IsNullOrEmpty(iso)) return "—";
        return DateTime.TryParse(iso, out var dt) ? dt.ToString("HH:mm") : iso;
    }

    /// <summary>系统默认浏览器打开原文（点击通知标题 / 「查看通知原文 ↗」）。</summary>
    public static async Task OpenUrlAsync(string? url)
    {
        if (string.IsNullOrWhiteSpace(url)) return;
        if (!Uri.TryCreate(url, UriKind.Absolute, out var uri)) return;
        try
        {
            await Windows.System.Launcher.LaunchUriAsync(uri);
        }
        catch
        {
            // 打开浏览器失败不构成阻断性错误，静默忽略（数据仍在应用内）。
        }
    }

    public static string Truncate(string? text, int max) =>
        string.IsNullOrEmpty(text) ? string.Empty : (text.Length <= max ? text : text[..max] + "…");

    /// <summary>item.add 的无截止时间兜底：明天 09:00（沿用 app.js L718-719）。</summary>
    public static string TomorrowNine()
    {
        var t = DateTime.Today.AddDays(1).AddHours(9);
        return t.ToString("yyyy-MM-ddTHH:mm");
    }

    /// <summary>手动添加表单的时间归一化：纯日期补 T23:59。</summary>
    public static string NormalizeDueInput(string? input)
    {
        if (string.IsNullOrWhiteSpace(input)) return string.Empty;
        string text = input.Trim().Replace(' ', 'T');
        if (text.Length == 10) text += "T23:59";
        return DateTime.TryParse(text, out var dt) ? dt.ToString("yyyy-MM-ddTHH:mm") : string.Empty;
    }
}
