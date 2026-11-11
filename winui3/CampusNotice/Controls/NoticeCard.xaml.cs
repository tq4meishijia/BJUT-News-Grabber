using CampusNotice.Helpers;
using CampusNotice.Models;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Automation;
using Microsoft.UI.Xaml.Controls;
using Windows.Foundation;

namespace CampusNotice.Controls;

public enum NoticeCardCommand
{
    AddSchedule,
    ToggleRead,
    Summarize,
    Remove,
    ToggleBody,
    OpenOriginal
}

public sealed class NoticeCardCommandEventArgs : EventArgs
{
    public required string Id { get; init; }
    public required NoticeCardCommand Command { get; init; }
}

/// <summary>
/// 通知卡片只负责「呈现 + 收集交互」，通过静态 Command 事件把动作交给 NoticesPage
/// 统一映射到真实 op。这样 DataTemplate 里不需要处理 DataContext 与事件路由问题。
/// More Flyout 在 code-behind 动态构建，用 Tag 携带 id，避免 Flyout 不在可视化树导致
/// DataContext 丢失（这是 WinUI 3 DataTemplate 里最常见的坑）。
/// </summary>
public sealed partial class NoticeCard : UserControl
{
    public static event EventHandler<NoticeCardCommandEventArgs>? Command;

    public NoticeCard()
    {
        InitializeComponent();
        DataContextChanged += (_, _) =>
        {
            if (ViewModel is { } vm) AutomationProperties.SetName(this, vm.AutomationName);
        };
    }

    private NoticeViewModel? ViewModel => DataContext as NoticeViewModel;

    private void Raise(NoticeCardCommand command)
    {
        var id = ViewModel?.Id;
        if (string.IsNullOrEmpty(id)) return;
        Command?.Invoke(this, new NoticeCardCommandEventArgs { Id = id, Command = command });
    }

    private void Title_Tapped(object sender, Microsoft.UI.Xaml.Input.TappedRoutedEventArgs e)
    {
        if (ViewModel?.HasUrl == true) Raise(NoticeCardCommand.OpenOriginal);
    }

    private void AddSchedule_Click(object sender, RoutedEventArgs e) => Raise(NoticeCardCommand.AddSchedule);

    private void UsefulToggle_Click(object sender, RoutedEventArgs e)
    {
        bool expanded = UsefulTextBlock.MaxLines == 0;
        UsefulTextBlock.MaxLines = expanded ? 2 : 0;
        UsefulToggle.Content = expanded ? "展开更多" : "收起";
    }

    private void More_Click(object sender, RoutedEventArgs e)
    {
        var vm = ViewModel;
        if (vm is null) return;

        var flyout = new MenuFlyout();
        if (vm.HasBody)
        {
            flyout.Items.Add(MakeItem(vm.IsExpanded ? "收起正文" : "展开正文", Icons.ChevronDown,
                NoticeCardCommand.ToggleBody, vm.Id));
        }
        flyout.Items.Add(MakeItem(vm.ReadButtonText, Icons.Completed,
            NoticeCardCommand.ToggleRead, vm.Id));
        flyout.Items.Add(MakeItem("AI 重摘要", Icons.Sparkle,
            NoticeCardCommand.Summarize, vm.Id));
        flyout.Items.Add(new MenuFlyoutSeparator());
        flyout.Items.Add(MakeItem("移除", Icons.Delete,
            NoticeCardCommand.Remove, vm.Id, destructive: true));

        // CommandBarFlyout 是规范首选；这里用系统样式的 MenuFlyout（同为 Acrylic 浮层），
        // 换取可靠的 Tag 路由，取舍记在 docs/winui3-engineering.md。
        flyout.ShowAt(MoreButton, new Point(0, MoreButton.ActualHeight));
    }

    private static MenuFlyoutItem MakeItem(string text, string glyph,
        NoticeCardCommand command, string id, bool destructive = false)
    {
        var item = new MenuFlyoutItem
        {
            Text = text,
            Tag = id,
            Icon = new FontIcon { Glyph = glyph, FontFamily = new Microsoft.UI.Xaml.Media.FontFamily("Segoe Fluent Icons") },
        };
        if (destructive) item.Foreground = ThemeBrush.Get("NetworkErrorBrush");
        item.Click += (_, _) =>
            Command?.Invoke(item, new NoticeCardCommandEventArgs { Id = id, Command = command });
        return item;
    }
}
