using Microsoft.UI.Xaml.Automation;
using Microsoft.UI.Xaml.Controls;

namespace CampusNotice.Helpers;

/// <summary>
/// code-behind 动态创建 FontIcon 时使用的字形常量，与 Themes/IconGlyphs.xaml 一一对应。
/// ⚠️ 同样需要按 Segoe Fluent Icons 官方字形表核对（视觉规范 §6.1 / §12）。
/// 凡 Symbol 枚举能表达的语义，优先用 <see cref="MakeSymbol"/> 返回 SymbolIcon。
/// </summary>
public static class Icons
{
    public const string Ringer = "\uEA8F";
    public const string Calendar = "\uE787";
    public const string Globe = "\uE774";
    public const string Setting = "\uE713";
    public const string Refresh = "\uE72C";
    public const string Add = "\uE710";
    public const string Filter = "\uE71C";
    public const string Edit = "\uE70F";
    public const string Delete = "\uE74D";
    public const string Play = "\uE768";
    public const string More = "\uE712";
    public const string Link = "\uE71B";
    public const string Cancel = "\uE711";
    public const string Tag = "\uE8EC";
    public const string Contact = "\uE77B";
    public const string Completed = "\uE73E";
    public const string ErrorBadge = "\uEA39";
    public const string Clock = "\uE823";
    public const string Warning = "\uE7BA";
    public const string Info = "\uE946";
    public const string ChevronDown = "\uE70D";
    public const string Save = "\uE74E";
    public const string Download = "\uE896";
    public const string Sparkle = "\uE734"; // 参考码位，落地前核对
    public const string Permission = "\uE7EF"; // 参考码位，落地前核对

    public static FontIcon Make(string glyph, double size = 14, string? automationName = null)
    {
        var icon = new FontIcon
        {
            Glyph = glyph,
            FontSize = size,
            FontFamily = new Microsoft.UI.Xaml.Media.FontFamily("Segoe Fluent Icons"),
        };
        if (!string.IsNullOrEmpty(automationName))
            AutomationProperties.SetName(icon, automationName);
        return icon;
    }

    public static SymbolIcon MakeSymbol(Symbol symbol, double size = 16, string? automationName = null)
    {
        var icon = new SymbolIcon { Symbol = symbol, FontSize = size };
        if (!string.IsNullOrEmpty(automationName))
            AutomationProperties.SetName(icon, automationName);
        return icon;
    }
}
