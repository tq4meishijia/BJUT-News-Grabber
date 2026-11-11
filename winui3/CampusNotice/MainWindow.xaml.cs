using System.Runtime.InteropServices;
using CampusNotice.Helpers;
using CampusNotice.Models;
using CampusNotice.Services;
using Microsoft.UI.Dispatching;
using Microsoft.UI.Windowing;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Automation;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Input;
using Microsoft.UI.Xaml.Media;
using Windows.System;
using WinRT.Interop;

namespace CampusNotice;

/// <summary>四页共享的最小契约。默认实现让各页只实现自己关心的钩子。</summary>
public interface IPageView
{
    void ApplyState(AppState state, StateOrigin origin);
    void HandleCommand(string command) { }
    void ApplyCompact(bool compact) { }
    void OnPageActivated() { }
    void LocateItem(string itemId) { }
    void LocateByNoticeId(string noticeId) { }
    void ApplyFilter(string filter) { }
}

public sealed partial class MainWindow : Window
{
    private const int SwHide = 0;
    private const int SwRestore = 9;

    private readonly IntPtr _hwnd;
    private readonly List<IPageView> _pages = new();
    private readonly DispatcherTimer _successTimer = new();
    private readonly DispatcherTimer _errorTimer = new();
    private readonly DispatcherTimer _tipTimer = new();
    private readonly DispatcherTimer _undoTimer = new();
    private readonly List<PendingUndo> _pendingUndos = new();

    private IPageView? _activePage;
    private string _activeTag = "notices";
    private bool _suppressNav;
    private bool _exiting;
    private bool _starting;
    private bool _disconnected;
    private bool _pollInFlight;
    private bool _compact;
    private AppState? _state;
    private string? _transientSuccess;
    private (string Title, string Message)? _transientError;
    private int _undoSecondsLeft;
    private string _undoMultiLabel = "已移除 N 项";
    private Action? _infoAction;
    private Action? _tipAction;
    private Action? _tipClose;
    private bool _pendingTrayHint;
    private int _firstRunStep = -1;

    /// <summary>
    /// 一条待撤销项（视觉规范 v1.1 组件 7.14 / UX §9.2）。
    /// Commit == null 表示操作**已经提交**（如 notice.readAll，撤销需逐条回放），
    /// 只保留撤销动作；Commit != null 表示 8s 内零写请求、超时/退出前才真正提交。
    /// </summary>
    private sealed class PendingUndo
    {
        public required string Label { get; init; }
        public required Action Undo { get; init; }
        public Func<Task>? Commit { get; init; }
    }

    public MainWindow()
    {
        InitializeComponent();
        _hwnd = WindowNative.GetWindowHandle(this);
        Title = "校园通知 · 日程";

        _pages.Add(NoticesPage);
        _pages.Add(SchedulePage);
        _pages.Add(SourcesPage);
        _pages.Add(SettingsPage);

        ApplyBackdrop();
        SetupAccelerators();
        SetupWindowLifecycle();
        SetupTimers();

        GlobalInfoBar.MaxWidth = 940;
        GlobalInfoBar.HorizontalAlignment = HorizontalAlignment.Left;

        ContentLayer.SizeChanged += (_, _) => ApplyBreakpoints();
        ContentLayer.ActualThemeChanged += (_, _) =>
        {
            ApplyBackdrop();
            // 主题切换后重新解析 C 层语义色（ScoreBadge / 状态条等由代码取色）。
            if (_state is not null) foreach (var page in _pages) page.ApplyState(_state, StateOrigin.Refresh);
        };

        App.State.StateChanged += (_, e) => ApplyState(e.State, e.Origin);
        App.Status.PropertyChanged += (_, _) => { /* GlobalStatus 由 MainWindow 统一驱动，无需反向绑定 */ };

        NavigateTo(LocalPrefs.Current.LastPage is "notices" or "schedule" or "sources" or "settings"
            ? LocalPrefs.Current.LastPage
            : "notices");
        ApplyBreakpoints();
    }

    public bool IsWindowVisible => NativeIsWindowVisible(_hwnd);

    /// <summary>进入设置页之前的页面（CAS 深链的「返回上一页」用，不建返回栈）。</summary>
    public string PreviousPageTag { get; private set; } = "notices";

    // --------------------------- 材质 / 主题 ---------------------------

    private void ApplyBackdrop()
    {
        if (ThemeBrush.IsHighContrast())
        {
            // §9.2：高对比度显式关闭 Mica / Acrylic，回退不透明系统底。
            App.CurrentThemeName = "HighContrast";
            SystemBackdrop = null;
            ContentLayer.Background = SystemBrush("SolidBackgroundFillColorBaseBrush",
                new SolidColorBrush(Microsoft.UI.Colors.Black));
            return;
        }

        bool dark = ContentLayer.ActualTheme == ElementTheme.Dark;
        App.CurrentThemeName = dark ? "Dark" : "Light";
        ContentLayer.Background = SystemBrush("LayerFillColorDefaultBrush",
            new SolidColorBrush(Microsoft.UI.Colors.Gray));

        // Light = MicaKind.Base / Dark = MicaKind.BaseAlt。
        // 用反射设置 Kind：不同 Windows App SDK 版本该属性可能存在差异，
        // 反射可保证在 1.5/1.6 上都能编译；若属性不存在则退化为默认 Base。
        var backdrop = new MicaBackdrop();
        var kindProperty = typeof(MicaBackdrop).GetProperty("Kind");
        if (kindProperty?.PropertyType.IsEnum == true)
        {
            try
            {
                kindProperty.SetValue(backdrop, Enum.Parse(kindProperty.PropertyType, dark ? "BaseAlt" : "Base"));
            }
            catch
            {
                // 枚举成员名不同则保留默认值。
            }
        }
        SystemBackdrop = backdrop;
    }

    private static Brush SystemBrush(string key, Brush fallback)
    {
        var resources = Application.Current?.Resources;
        if (resources is not null && resources.TryGetValue(key, out var value) && value is Brush brush)
            return brush;
        return fallback;
    }

    // --------------------------- 生命周期 ---------------------------

    private void SetupWindowLifecycle()
    {
        // 关闭窗口(X / Alt+F4) = Hide 到托盘，**不发 /api/shutdown**；只有「退出」才发。
        AppWindow.Closing += (_, e) =>
        {
            if (_exiting) return;
            e.Cancel = true;
            HideToTray();
        };
    }

    private void SetupTimers()
    {
        _successTimer.Interval = TimeSpan.FromSeconds(5);
        _successTimer.Tick += (_, _) =>
        {
            _successTimer.Stop();
            _transientSuccess = null;
            RecomputeInfoBar();
        };
        _tipTimer.Tick += (_, _) =>
        {
            _tipTimer.Stop();
            GlobalTip.IsOpen = false;
        };

        _undoTimer.Interval = TimeSpan.FromSeconds(1);
        _undoTimer.Tick += OnUndoTick;

        _errorTimer.Interval = TimeSpan.FromSeconds(5);
        _errorTimer.Tick += (_, _) =>
        {
            _errorTimer.Stop();
            _transientError = null;
            RecomputeInfoBar();
        };
    }

    private void SetupAccelerators()
    {
        AddAccelerator(VirtualKey.Number1, VirtualKeyModifiers.Menu, () => NavigateTo("notices"));
        AddAccelerator(VirtualKey.Number2, VirtualKeyModifiers.Menu, () => NavigateTo("schedule"));
        AddAccelerator(VirtualKey.Number3, VirtualKeyModifiers.Menu, () => NavigateTo("sources"));
        AddAccelerator(VirtualKey.Number4, VirtualKeyModifiers.Menu, () => NavigateTo("settings"));
        AddAccelerator(VirtualKey.F5, VirtualKeyModifiers.None, () => _ = TriggerPollAsync());
        AddAccelerator(VirtualKey.R, VirtualKeyModifiers.Control, () => _ = TriggerPollAsync());
        AddAccelerator(VirtualKey.S, VirtualKeyModifiers.Control, () =>
        {
            if (_activeTag == "settings") _activePage?.HandleCommand("saveSettings");
        });
        AddAccelerator(VirtualKey.Escape, VirtualKeyModifiers.None, () =>
        {
            GlobalTip.IsOpen = false;
            _activePage?.HandleCommand("escape");
            if (_compact) NavView.IsPaneOpen = false;
        });
        // Alt+← 不绑定：四页平级、无返回栈（UX §8.2）。
    }

    private void AddAccelerator(VirtualKey key, VirtualKeyModifiers modifiers, Action action)
    {
        var accelerator = new KeyboardAccelerator { Key = key, Modifiers = modifiers };
        accelerator.Invoked += (_, e) => { action(); e.Handled = true; };
        RootGrid.KeyboardAccelerators.Add(accelerator);
    }

    // --------------------------- 导航 ---------------------------

    public void NavigateTo(string tag)
    {
        if (tag is not ("notices" or "schedule" or "sources" or "settings")) tag = "notices";
        if (tag != _activeTag) PreviousPageTag = _activeTag;
        _activeTag = tag;
        _suppressNav = true;
        try
        {
            NoticesPage.Visibility = tag == "notices" ? Visibility.Visible : Visibility.Collapsed;
            SchedulePage.Visibility = tag == "schedule" ? Visibility.Visible : Visibility.Collapsed;
            SourcesPage.Visibility = tag == "sources" ? Visibility.Visible : Visibility.Collapsed;
            SettingsPage.Visibility = tag == "settings" ? Visibility.Visible : Visibility.Collapsed;

            _activePage = tag switch
            {
                "notices" => NoticesPage,
                "schedule" => SchedulePage,
                "sources" => SourcesPage,
                _ => SettingsPage
            };

            NavNotices.IsSelected = tag == "notices";
            NavSchedule.IsSelected = tag == "schedule";
            NavSources.IsSelected = tag == "sources";
            NavSettings.IsSelected = tag == "settings";
            NavView.SelectedItem = tag switch
            {
                "notices" => NavNotices,
                "schedule" => NavSchedule,
                "sources" => NavSources,
                _ => null
            };

            PageTitle.Text = tag switch
            {
                "notices" => "通知",
                "schedule" => "日程",
                "sources" => "数据源",
                _ => "设置"
            };
            UpdateCommandVisibility(tag);
            if (tag == "schedule") CmdShowDone.IsChecked = LocalPrefs.Current.ShowDoneItems;
            _activePage.OnPageActivated();
            LocalPrefs.Current.LastPage = tag;
            LocalPrefs.Current.Save();
        }
        finally
        {
            _suppressNav = false;
        }
    }

    private void UpdateCommandVisibility(string tag)
    {
        CmdPoll.Visibility = tag == "settings" ? Visibility.Collapsed : Visibility.Visible;
        CmdAddItem.Visibility = tag == "schedule" ? Visibility.Visible : Visibility.Collapsed;
        CmdShowDone.Visibility = tag == "schedule" ? Visibility.Visible : Visibility.Collapsed;
        CmdAddSource.Visibility = tag == "sources" ? Visibility.Visible : Visibility.Collapsed;
        CmdEnableNotifications.Visibility = tag == "notices" ? Visibility.Visible : Visibility.Collapsed;
        CmdReloginAll.Visibility = tag == "sources" ? Visibility.Visible : Visibility.Collapsed;
        CmdGoSettings.Visibility = tag == "settings" ? Visibility.Collapsed : Visibility.Visible;
    }

    private void NavView_SelectionChanged(NavigationView sender, NavigationViewSelectionChangedEventArgs args)
    {
        if (_suppressNav) return;
        if (args.SelectedItem is NavigationViewItem item && item.Tag is string tag)
            NavigateTo(tag);
    }

    private void NavView_Loaded(object sender, RoutedEventArgs e)
    {
        if (NavView.SelectedItem is null) NavNotices.IsSelected = true;
    }

    private void NavSettings_Tapped(object sender, TappedRoutedEventArgs e) => NavigateTo("settings");

    // --------------------------- 响应式断点（DIP） ---------------------------

    private void ApplyBreakpoints()
    {
        double width = Content.XamlRoot?.Size.Width ?? 1024;
        NavView.PaneDisplayMode = width >= 1008
            ? NavigationViewPaneDisplayMode.Left
            : width >= 640
                ? NavigationViewPaneDisplayMode.LeftCompact
                : NavigationViewPaneDisplayMode.LeftMinimal;

        bool compact = width < 640;
        if (compact == _compact) return;
        _compact = compact;
        foreach (var page in _pages) page.ApplyCompact(compact);
    }

    // --------------------------- 状态应用 ---------------------------

    public void ApplyState(AppState state, StateOrigin origin)
    {
        _state = state;
        string line = _disconnected ? "本地服务未运行" : Formatters.StatusLine(state);
        if (StatusLine.Text != line) StatusLine.Text = line;   // 只在变化时更新，避免 LiveRegion 反复播报
        App.Status.StatusLine = line;
        App.Status.Polling = state.Meta.Polling || _pollInFlight;

        PaneStatus.Text = $"服务运行中 · 端口 {state.Meta.Port}";
        PaneFooterStatus.Text = $"数据只存在这台电脑 · 端口 {state.Meta.Port}";

        BadgeNotices.Value = Math.Min(state.UnreadCount, 99);
        BadgeNotices.Visibility = state.UnreadCount > 0 ? Visibility.Visible : Visibility.Collapsed;
        int due = Formatters.DueIn24Hours(state);
        BadgeSchedule.Value = Math.Min(due, 99);
        BadgeSchedule.Visibility = due > 0 ? Visibility.Visible : Visibility.Collapsed;

        App.Tray?.Update(state, App.State.IsConnected, IsWindowVisible);
        foreach (var page in _pages) page.ApplyState(state, origin);
        RecomputeInfoBar();
    }

    private void UpdatePollingUi(bool polling)
    {
        CmdPoll.Label = polling ? "抓取中…" : "立即抓取";
        CmdPoll.IsEnabled = !polling;
        PollingBar.Visibility = polling ? Visibility.Visible : Visibility.Collapsed;
        App.Tray?.SetPolling(polling);
        App.Status.Polling = polling;
    }

    // --------------------------- InfoBar（全局唯一） ---------------------------

    private void RecomputeInfoBar()
    {
        // 优先级 Error > Warning > Informational > Success，同一时刻最多 1 条。
        if (_disconnected)
        {
            SetInfo(InfoBarSeverity.Error, "本地服务未运行",
                "本地服务未运行（127.0.0.1:8737）。检查程序是否被关闭，或点「重试」重新连接。数据已保存在本机，不会丢失。",
                closable: false, "重试", () => _ = App.RetryConnectionAsync());
            return;
        }
        if (_transientError is not null)
        {
            // 开关即时保存失败 → 顶部 InfoBar Error（不弹窗），与表单字段内联错误区分（v1.1 §7.8 ④ / §13.7）。
            SetInfo(InfoBarSeverity.Error, _transientError.Value.Title, _transientError.Value.Message,
                closable: true, null, null);
            return;
        }
        if (_starting)
        {
            SetInfo(InfoBarSeverity.Informational, "正在连接本地服务…",
                "127.0.0.1:8737 · 正在探测或启动本地服务，最多等待 15 秒。",
                closable: true, null, null);
            return;
        }

        if (_state is not null)
        {
            int expired = _state.LoginExpiredCount;
            if (expired > 0)
            {
                // D2：loginExpired 恒为 Warning 橙，绝不升 Error；必有「重新登录」动作。
                SetInfo(InfoBarSeverity.Warning, "登录态失效",
                    $"有 {expired} 个数据源登录态失效，抓取可能不完整。点「重新登录」用已保存的校园账号自动更新 Cookie。",
                    closable: true, "重新登录", () => _ = ReloginAsync());
                return;
            }

            string pollError = _state.Meta.LastPollError ?? string.Empty;
            if (!string.IsNullOrEmpty(pollError))
            {
                int enabled = _state.Sources.Count(s => s.Enabled);
                bool overall = enabled > 0 && _state.ErrorSourceCount >= enabled;
                SetInfo(overall ? InfoBarSeverity.Error : InfoBarSeverity.Warning,
                    overall ? "整体抓取失败" : "部分数据源抓取失败",
                    overall ? $"抓取失败：{pollError}" : $"抓取完成，{_state.ErrorSourceCount} 个数据源失败：{pollError} 其他源正常。",
                    closable: !overall, "查看数据源", () => NavigateTo("sources"));
                return;
            }
            if (_state.NoEnabledSource)
            {
                SetInfo(InfoBarSeverity.Warning, "没有启用任何数据源",
                    "先到「数据源」启用或添加至少一个源，再点「立即抓取」。",
                    closable: true, "查看数据源", () => NavigateTo("sources"));
                return;
            }
        }

        if (_transientSuccess is not null)
        {
            SetInfo(InfoBarSeverity.Success, "操作成功", _transientSuccess, closable: true, null, null);
            return;
        }

        GlobalInfoBar.IsOpen = false;
    }

    private void SetInfo(InfoBarSeverity severity, string title, string message,
        bool closable, string? actionText, Action? action)
    {
        GlobalInfoBar.Severity = severity;
        GlobalInfoBar.Title = title;
        GlobalInfoBar.Message = message;
        GlobalInfoBar.IsClosable = closable;
        _infoAction = action;
        InfoBarAction.Content = actionText ?? string.Empty;
        InfoBarAction.Visibility = string.IsNullOrEmpty(actionText) ? Visibility.Collapsed : Visibility.Visible;
        GlobalInfoBar.IsOpen = true;
    }

    public void ShowSuccess(string message)
    {
        // v1.1 §13.3：撤销条存在的 8s 内不得再弹 Success InfoBar（避免叠加）。
        if (_pendingUndos.Count > 0) return;
        _transientSuccess = message;
        RecomputeInfoBar();
        _successTimer.Stop();
        _successTimer.Start();
    }

    /// <summary>开关即时保存失败：顶部 InfoBar Error（可关闭），5s 后由状态重算接管。</summary>
    public void ShowError(string title, string message)
    {
        _transientError = (title, message);
        RecomputeInfoBar();
        _errorTimer.Stop();
        _errorTimer.Start();
    }

    public void ShowWarning(string title, string message)
    {
        SetInfo(InfoBarSeverity.Warning, title, message, closable: true, null, null);
    }

    private void InfoBarAction_Click(object sender, RoutedEventArgs e) => _infoAction?.Invoke();

    private void GlobalInfoBar_Closed(InfoBar sender, InfoBarClosedEventArgs args)
    {
        // Error 断连 InfoBar 的 IsClosable=False，不会走到这里；这里只处理可关闭项。
        if (_disconnected) GlobalInfoBar.IsOpen = true;
    }

    // --------------------------- 后端状态 ---------------------------

    public void ShowBackendStarting()
    {
        _starting = true;
        _disconnected = false;
        StartingOverlay.Visibility = Visibility.Visible;
        StatusLine.Text = "正在连接本地服务…";
        RecomputeInfoBar();
    }

    public void ShowDisconnected()
    {
        _starting = false;
        _disconnected = true;
        StartingOverlay.Visibility = Visibility.Collapsed;
        StatusLine.Text = "本地服务未运行";
        App.Status.StatusLine = "本地服务未运行";
        PaneStatus.Text = "服务未连接 · 端口 8737";
        RecomputeInfoBar();
    }

    public void OnReconnected()
    {
        _disconnected = false;
        _starting = false;
        StartingOverlay.Visibility = Visibility.Collapsed;
        RecomputeInfoBar();
    }

    public void OnBackendReady()
    {
        _starting = false;
        _disconnected = false;
        StartingOverlay.Visibility = Visibility.Collapsed;
        RecomputeInfoBar();

        // 首次运行 3 步引导：本地标记 + 从未抓取过 + health 已连通（UX §7.1）。
        if (!LocalPrefs.Current.FirstRunCompleted &&
            string.IsNullOrEmpty(App.State.Current?.Meta.LastPollAt) &&
            App.State.IsConnected)
        {
            StartFirstRun();
        }
    }

    // --------------------------- 命令栏动作 ---------------------------

    private void CmdPoll_Click(object sender, RoutedEventArgs e) => _ = TriggerPollAsync();

    private void CmdAddItem_Click(object sender, RoutedEventArgs e) => _activePage?.HandleCommand("addItem");
    private void CmdAddSource_Click(object sender, RoutedEventArgs e) => _activePage?.HandleCommand("addSource");

    private void CmdShowDone_Click(object sender, RoutedEventArgs e)
    {
        LocalPrefs.Current.ShowDoneItems = CmdShowDone.IsChecked == true;
        LocalPrefs.Current.Save();
        _activePage?.HandleCommand("showDone");
    }

    private async void CmdEnableNotifications_Click(object sender, RoutedEventArgs e)
    {
        if (!App.State.IsConnected) { ShowDisconnected(); return; }
        if (App.State.Current?.Settings.Notify == false)
        {
            var response = await App.State.DispatchAsync(Ops.SettingsSave,
                Settings.TogglePatch("notify", true));
            LocalPrefs.Current.NotifyEnabledLocal = true;
            LocalPrefs.Current.Save();
            if (response?.Ok == true) ShowSuccess("已开启桌面通知（到点提醒，与开机启动无关）");
            return;
        }
        bool ok = App.Toast.ShowTestNotification();
        ShowSuccess(ok
            ? "桌面通知已开启，已发出一条自测通知"
            : "桌面通知通道当前不可用，请检查系统通知设置");
    }

    private void CmdTestToast_Click(object sender, RoutedEventArgs e)
    {
        // 走客户端 ToastService 自测，**不调用 notify.test op**（决策 3 单一通道）。
        bool ok = App.Toast.ShowTestNotification();
        ShowSuccess(ok
            ? "已发出一条自测桌面通知"
            : "系统通知通道当前不可用（AppNotification 未注册或系统设置关闭）");
    }

    private async void CmdRescore_Click(object sender, RoutedEventArgs e)
    {
        if (!App.State.IsConnected) { ShowDisconnected(); return; }
        var response = await App.State.DispatchAsync(Ops.NoticesRescore);
        if (response?.Ok == true) ShowSuccess(response.SuccessText + "（AI 摘要保留）");
        else if (response is not null) ShowWarning("重算未完成", response.FailureText);
    }

    private async void CmdRefill_Click(object sender, RoutedEventArgs e)
    {
        if (!App.State.IsConnected) { ShowDisconnected(); return; }
        var response = await App.State.DispatchAsync(Ops.NoticesRefill,
            new Dictionary<string, object?> { ["limit"] = 40 });
        if (response?.Ok == true) ShowSuccess(response.SuccessText);
        else if (response is not null) ShowWarning("补齐正文未完成", response.FailureText);
    }

    private void CmdReloginAll_Click(object sender, RoutedEventArgs e) => _ = ReloginAsync();

    private void CmdGoSettings_Click(object sender, RoutedEventArgs e) => NavigateTo("settings");

    /// <summary>
    /// D2 主路径：configured=true → account.test（后端 autoLogin 写回 Cookie）；
    /// configured=false → 跳设置页「自动登录 CAS」分组由用户填 account.save。
    /// </summary>
    public async Task ReloginAsync()
    {
        if (!App.State.IsConnected) { ShowDisconnected(); return; }
        bool configured = App.State.Current?.Meta.Account.Configured == true;
        if (!configured)
        {
            NavigateTo("settings");
            _activePage?.HandleCommand("focusAccount");
            ShowWarning("需要填写校园账号", "还没保存统一身份认证账号密码，请在「自动登录（CAS）」分组填写后点「保存并登录」。");
            return;
        }
        var response = await App.State.DispatchAsync(Ops.AccountTest);
        if (response?.Ok == true) ShowSuccess(response.SuccessText);
        else ShowWarning("重新登录未完成", response?.FailureText ?? "自动登录失败，请到设置页检查账号与密码。");
    }

    /// <summary>立即抓取。断连时仅激活窗口 + Error InfoBar，不重复发 poll（R4）。</summary>
    public async Task TriggerPollAsync(bool activateOnDisconnect = false)
    {
        if (!App.State.IsConnected)
        {
            if (activateOnDisconnect) ActivateFromTray();
            ShowDisconnected();
            return;
        }
        if (_pollInFlight) return;   // 在途直接忽略，不排队（§9.5）
        _pollInFlight = true;
        UpdatePollingUi(true);
        int before = App.State.Current?.Notices.Count ?? 0;
        try
        {
            var response = await App.State.DispatchAsync(Ops.Poll);
            if (response is null) { ShowDisconnected(); return; }
            if (!response.Ok)
            {
                string text = response.FailureText;
                if (text.Contains("没有启用")) ShowWarning("没有启用任何数据源", text);
                // 「正在抓取中」等静默失败不弹提示。
                return;
            }
            int after = App.State.Current?.Notices.Count ?? 0;
            int added = Math.Max(0, after - before);
            ShowSuccess($"抓取完成 · 新增 {added} 条通知");
        }
        finally
        {
            _pollInFlight = false;
            UpdatePollingUi(false);
        }
    }

    // --------------------------- 托盘 / 深链 ---------------------------

    public void ActivateFromTray()
    {
        NativeShowWindow(_hwnd, SwRestore);
        NativeSetForegroundWindow(_hwnd);
        Activate();
        if (_pendingTrayHint)
        {
            _pendingTrayHint = false;
            ShowTeachingTip("已最小化到托盘",
                "程序仍在后台抓取与提醒；要完全停止请在托盘菜单选「退出」。",
                null, null, null, "知道了", null, seconds: 6, lightDismiss: true);
        }
    }

    public void HideToTray()
    {
        NativeShowWindow(_hwnd, SwHide);
        App.Tray?.Update(_state, App.State.IsConnected, false);
        if (!LocalPrefs.Current.CloseToTrayHintShown)
        {
            LocalPrefs.Current.CloseToTrayHintShown = true;
            LocalPrefs.Current.Save();
            _pendingTrayHint = true;
        }
    }

    public void ActivateFromTrayAndNavigate(string tag, string? filter)
    {
        ActivateFromTray();
        NavigateTo(tag);             // 深链不压入返回栈（§1.3）
        if (!string.IsNullOrEmpty(filter)) _activePage?.ApplyFilter(filter);
        _activePage?.OnPageActivated();
    }

    public void ActivateFromTrayAndLocate(string itemId)
    {
        ActivateFromTray();
        NavigateTo("schedule");
        SchedulePage.LocateItem(itemId);   // 只定位、不改状态（§1.6）
    }

    /// <summary>「已在日程中 · 查看」：按 noticeId 找到对应的日程项并定位。</summary>
    public void NavigateToScheduleByNoticeId(string noticeId)
    {
        NavigateTo("schedule");
        SchedulePage.LocateByNoticeId(noticeId);
    }

    // --------------------------- 退出 ---------------------------

    public async Task RequestExitAsync()
    {
        if (_exiting) return;
        _exiting = true;

        await FlushDeletesAsync();                 // 退出前 flush 待删除项（§7.5 第 2 步）
        App.State.StopPolling();
        await App.Host.ShutdownAsync();            // 仅本实例托管时发 /api/shutdown
        App.Tray?.Dispose();
        App.Toast.Unregister();
        LocalPrefs.Current.Save();

        Close();
        Environment.Exit(0);
    }

    // --------------------------- 组件 7.14：8s 底部撤销条 ---------------------------

    /// <summary>
    /// 压入一条待撤销项（视觉规范 v1.1 §13.3 / UX §9.2）。
    /// **仅移除型延迟提交**：`notice.remove` / `item.remove` / `notice.readAll` 回滚。
    /// 8s 内连续多条只保留 1 条撤销条，文案变「已移除 N 项」，撤销一次性恢复全部。
    /// `onCommit == null` 表示操作已提交（如 readAll），撤销由 `onUndo` 自行回放。
    /// </summary>
    public void PushUndo(string label, Action onUndo, Func<Task>? onCommit = null,
        string multiLabel = "已移除 N 项")
    {
        _pendingUndos.Add(new PendingUndo { Label = label, Undo = onUndo, Commit = onCommit });
        _undoMultiLabel = multiLabel;
        if (!_undoTimer.IsEnabled)
        {
            _undoSecondsLeft = 8;
            UndoProgress.Maximum = 8;
            UndoProgress.Value = 8;
            _undoTimer.Start();
        }
        UpdateUndoBar();
        UndoBar.Visibility = Visibility.Visible;
    }

    /// <summary>通知 / 日程「移除」便捷封装：8s 内零写请求，超时 / 退出前才真正提交。</summary>
    public void ScheduleDelete(string op, string id, string title, Action onUndo)
    {
        string label = $"已移除「{Formatters.Truncate(title, 20)}…」";
        PushUndo(label, onUndo,
            async () =>
            {
                await App.State.DispatchAsync(op, new Dictionary<string, object?> { ["id"] = id });
            },
            "已移除 N 项");
    }

    private void UpdateUndoBar()
    {
        int count = _pendingUndos.Count;
        UndoLabel.Text = count <= 1
            ? (_pendingUndos.FirstOrDefault()?.Label ?? "已移除 1 项")
            : _undoMultiLabel.Replace("N", count.ToString());
        UndoCountdown.Text = $"{Math.Max(0, _undoSecondsLeft)}s";
        UndoProgress.Value = Math.Max(0, _undoSecondsLeft);
        AutomationProperties.SetName(UndoBar, count <= 1
            ? "已移除 1 项，8 秒内可撤销"
            : $"已移除 {count} 项，{Math.Max(0, _undoSecondsLeft)} 秒内可撤销");
    }

    private async void OnUndoTick(object? sender, object e)
    {
        _undoSecondsLeft -= 1;
        if (_undoSecondsLeft > 0)
        {
            UpdateUndoBar();   // 12px tabular 倒计时「8s…1s」+ 2px 确定性 ProgressBar 线性递减
            return;
        }
        _undoTimer.Stop();
        UndoBar.Visibility = Visibility.Collapsed;   // 条消失后才发移除请求
        await CommitUndosAsync();
    }

    private void UndoButton_Click(object sender, RoutedEventArgs e)
    {
        // 撤销：条立即消失、卡片立刻回原位（无淡入动画）、全程零写请求（已提交场景由 Undo 自行回放）。
        _undoTimer.Stop();
        UndoBar.Visibility = Visibility.Collapsed;
        var snapshot = _pendingUndos.ToList();
        _pendingUndos.Clear();
        foreach (var entry in snapshot) entry.Undo();
    }

    private async Task CommitUndosAsync()
    {
        var snapshot = _pendingUndos.ToList();
        _pendingUndos.Clear();
        foreach (var entry in snapshot)
        {
            if (entry.Commit is not null) await entry.Commit();
        }
    }

    /// <summary>退出时序第 2 步：flush 待删除项，12px 状态行提示、不弹窗（UX §7.5 / 附录 A）。</summary>
    public async Task FlushDeletesAsync()
    {
        _undoTimer.Stop();
        UndoBar.Visibility = Visibility.Collapsed;
        int pending = _pendingUndos.Count;
        if (pending > 0) StatusLine.Text = $"正在保存 {pending} 项变更…";
        await CommitUndosAsync();
    }

    // --------------------------- TeachingTip（单一、串行） ---------------------------

    /// <summary>
    /// 通用 TeachingTip：首次运行 3 步引导、关闭到托盘提示、数据源一键切换 JSON 等。
    /// 一次只显示 1 个、串行。**移除型撤销不在这里**（v1.1 §13.3 已改为底部撤销条）。
    /// </summary>
    public void ShowTeachingTip(string title, string subtitle, FrameworkElement? target,
        string? actionText, Action? action, string? closeText, Action? onClose,
        int seconds = 5, bool lightDismiss = true)
    {
        _tipAction = action;
        _tipClose = onClose;
        GlobalTip.Title = title;
        GlobalTip.Subtitle = subtitle;
        GlobalTip.Target = target?.Visibility == Visibility.Visible ? target : null; // 目标不可见时不指向（R7）
        GlobalTip.ActionButtonContent = actionText;
        GlobalTip.CloseButtonContent = closeText ?? "知道了";
        GlobalTip.IsLightDismissEnabled = lightDismiss;
        _tipTimer.Stop();
        if (seconds > 0)
        {
            _tipTimer.Interval = TimeSpan.FromSeconds(seconds);
            _tipTimer.Start();
        }
        GlobalTip.IsOpen = true;
    }

    private void GlobalTip_Action(TeachingTip sender, object args)
    {
        var action = _tipAction;
        _tipAction = null;
        GlobalTip.IsOpen = false;
        action?.Invoke();
    }

    private void GlobalTip_Close(TeachingTip sender, object args)
    {
        var close = _tipClose;
        _tipClose = null;
        GlobalTip.IsOpen = false;
        close?.Invoke();
    }

    private void GlobalTip_Closed(TeachingTip sender, TeachingTipClosedEventArgs args)
    {
        _tipTimer.Stop();
    }

    // --------------------------- 首次运行 3 步引导（零自启） ---------------------------

    private void StartFirstRun()
    {
        _firstRunStep = 0;
        ShowFirstRunStep();
    }

    private void FinishFirstRun()
    {
        _firstRunStep = -1;
        LocalPrefs.Current.FirstRunCompleted = true;
        LocalPrefs.Current.Save();
        GlobalTip.IsOpen = false;
        NavigateTo("notices");
    }

    private void ShowFirstRunStep()
    {
        switch (_firstRunStep)
        {
            case 0:
                ShowTeachingTip("① 确认本地服务",
                    $"本地服务已就绪。数据只存在这台电脑的 {App.State.Current?.Meta.DataPath ?? "%APPDATA%\\campus-notice"}，端口 8737。状态行会常驻显示上次抓取时间。",
                    PageTitle, "下一步", () => { _firstRunStep = 1; ShowFirstRunStep(); },
                    "跳过引导", FinishFirstRun, seconds: 0, lightDismiss: true);
                break;
            case 1:
                ShowTeachingTip("② 启用数据源",
                    "已为北京工业大学预置 4 个一站式平台数据源。到「数据源」确认至少一个开关是开的。",
                    NavSources, "去数据源", () =>
                    {
                        NavigateTo("sources");
                        _firstRunStep = 2;
                        ShowFirstRunStep();
                    }, "跳过引导", FinishFirstRun, seconds: 0, lightDismiss: true);
                break;
            case 2:
                ShowTeachingTip("③ 立即抓取",
                    $"点「立即抓取」，之后每 {App.State.Current?.Settings.PollMinutes ?? 30} 分钟会自动静默刷新一次。",
                    GlobalCommandBar, "立即抓取", async () =>
                    {
                        FinishFirstRun();
                        await TriggerPollAsync();
                    }, "跳过引导", FinishFirstRun, seconds: 0, lightDismiss: true);
                break;
            default:
                FinishFirstRun();
                break;
        }
    }

    // --------------------------- Win32 ---------------------------

    [DllImport("user32.dll", EntryPoint = "ShowWindow")]
    private static extern bool NativeShowWindow(IntPtr hWnd, int nCmdShow);

    [DllImport("user32.dll", EntryPoint = "SetForegroundWindow")]
    private static extern bool NativeSetForegroundWindow(IntPtr hWnd);

    [DllImport("user32.dll", EntryPoint = "IsWindowVisible")]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool NativeIsWindowVisible(IntPtr hWnd);
}
