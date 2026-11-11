using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;

namespace CampusNotice.Helpers;

/// <summary>
/// ContentDialog 辅助（视觉规范 §7.10 / UX 规范 §9.3）。
/// 破坏性确认 DefaultButton 恒为 Close（回车 / Esc = 取消），确认按钮绝不默认高亮。
/// 只用于：删除数据源 / 清除已保存账号 / 清空 API Key / 清空关键词 等无恢复 op 的场景。
/// 通知与日程的移除走「8s 延迟提交 + TeachingTip 撤销」，不用对话框。
/// </summary>
public static class Dialogs
{
    public static async Task<bool> ConfirmDestructiveAsync(
        XamlRoot root,
        string title,
        string body,
        string confirmText = "删除")
    {
        var dialog = new ContentDialog
        {
            XamlRoot = root,
            Title = title,
            Content = BuildContent(body),
            PrimaryButtonText = confirmText,
            CloseButtonText = "取消",
            DefaultButton = ContentDialogButton.Close,
            MinWidth = 480,
            MaxWidth = 560,
        };
        // 确认按钮用 Critical 前景 + Delete 图标（视觉规范 §7.10）。
        var result = await dialog.ShowAsync();
        return result == ContentDialogResult.Primary;
    }

    public static async Task<bool> ConfirmAsync(
        XamlRoot root,
        string title,
        string body,
        string confirmText = "确定")
    {
        var dialog = new ContentDialog
        {
            XamlRoot = root,
            Title = title,
            Content = BuildContent(body),
            PrimaryButtonText = confirmText,
            CloseButtonText = "取消",
            DefaultButton = ContentDialogButton.Close,
            MinWidth = 420,
            MaxWidth = 520,
        };
        return await dialog.ShowAsync() == ContentDialogResult.Primary;
    }

    public static async Task ShowInfoAsync(XamlRoot root, string title, string body)
    {
        var dialog = new ContentDialog
        {
            XamlRoot = root,
            Title = title,
            Content = BuildContent(body),
            CloseButtonText = "知道了",
            DefaultButton = ContentDialogButton.Close,
            MinWidth = 420,
            MaxWidth = 520,
        };
        await dialog.ShowAsync();
    }

    private static UIElement BuildContent(string body)
    {
        return new TextBlock
        {
            Text = body,
            TextWrapping = TextWrapping.Wrap,
            FontSize = 14,
            LineHeight = 20,
            IsTextSelectionEnabled = true,
        };
    }
}
