using CampusNotice.Models;
using CampusNotice.Services;
using Microsoft.UI.Dispatching;
using Microsoft.UI.Xaml;
using Microsoft.Windows.AppLifecycle;

namespace CampusNotice;

/// <summary>
/// 应用入口：单实例、服务装配、自动托管后端、AppNotification 注册与激活路由。
///
/// 与现有 Node/Web 版的关系：本进程不实现任何抓取/存储逻辑，只做 UI；
/// 通过 127.0.0.1:8737 的 /api/state 与 /api/action 复用 server.mjs + store.mjs。
/// 自动托管时以 `node server.mjs --no-toast --no-open` 启动，谁启动谁关闭。
/// </summary>
public sealed partial class App : Application
{
    private const string SingleInstanceKey = "CampusNotice-WinUI3-SingleInstance";

    public static ApiClient Api { get; private set; } = null!;
    public static StateStore State { get; private set; } = null!;
    public static ToastService Toast { get; private set; } = null!;
    public static NodeBackendHost Host { get; private set; } = null!;
    public static JumpListService JumpList { get; private set; } = null!;
    public static TrayIconService? Tray { get; private set; }
    public static MainWindow? Window { get; private set; }
    public static GlobalStatus Status { get; } = new();

    /// <summary>UI 线程调度器（在 OnLaunched 中取得，供托盘线程 / 激活回调回投）。</summary>
    public static DispatcherQueue UiDispatcher { get; private set; } = null!;

    /// <summary>
    /// 当前实际主题名（"Light" / "Dark" / "HighContrast"），由 MainWindow 的
    /// ActualThemeChanged 事件维护；ThemeBrush 用它从 ThemeDictionaries 精确取色。
    /// </summary>
    public static string CurrentThemeName { get; set; } = "Light";

    private static AppInstance? _mainInstance;

    public App()
    {
        InitializeComponent();
        // 不显式设置 RequestedTheme：跟随系统浅/深色（视觉规范 §9.1）。
    }

    protected override async void OnLaunched(LaunchActivatedEventArgs args)
    {
        UiDispatcher = DispatcherQueue.GetForCurrentThread();

        // ---- 单实例：把激活参数重定向到已有窗口，避免重复 spawn node / EADDRINUSE ----
        _mainInstance = AppInstance.FindOrRegisterForKey(SingleInstanceKey);
        if (!_mainInstance.IsCurrent)
        {
            try
            {
                var redirected = AppInstance.GetCurrent().GetActivatedEventArgs();
                await _mainInstance.RedirectActivationToAsync(redirected);
            }
            catch
            {
                // 重定向失败也直接退出，避免两个实例同时跑。
            }
            Environment.Exit(0);
            return;
        }
        _mainInstance.Activated += OnAppInstanceActivated;

        // ---- 服务装配（顺序固定：StateStore 依赖 ApiClient；MainWindow 订阅 State）----
        Api = new ApiClient();
        State = new StateStore(Api);
        Host = new NodeBackendHost(Api);
        Toast = new ToastService();
        JumpList = new JumpListService();

        Window = new MainWindow();
        Window.Activate();

        Toast.Initialize(UiDispatcher);
        Toast.OpenScheduleRequested += (_, itemId) =>
            Window?.ActivateFromTrayAndLocate(itemId);

        await JumpList.InitializeAsync();

        Tray = new TrayIconService(UiDispatcher);
        WireTray(Tray);
        Tray.Initialize();

        WireHost();
        WireState();

        await StartBackendAsync();

        // 冷启动即由跳转列表 / Toast 拉起时，处理激活参数。
        HandleActivation(AppInstance.GetCurrent().GetActivatedEventArgs());
    }

    private static void WireState()
    {
        State.StateChanged += (_, e) =>
        {
            Toast.ProcessReminders(e.State);
            Tray?.Update(e.State, State.IsConnected, Window?.IsWindowVisible ?? true);
        };

        State.ConnectionChanged += (_, connected) =>
        {
            if (!connected) Window?.ShowDisconnected();
            else Window?.OnReconnected();
        };
    }

    private static void WireHost()
    {
        Host.Log += (_, message) => System.Diagnostics.Debug.WriteLine($"[BackendHost] {message}");
        Host.BackendExited += (_, _) =>
            UiDispatcher.TryEnqueue(() => Window?.ShowDisconnected());
    }

    private static void WireTray(TrayIconService tray)
    {
        // 托盘只做「导航 + 触发一个已存在的 op」，不复制页面功能（UX §1.7）。
        tray.ShowWindowRequested += (_, _) => Window?.ActivateFromTray();
        tray.PollRequested += async (_, _) => { if (Window is not null) await Window.TriggerPollAsync(); };
        tray.ShowNoticesRequested += (_, _) => Window?.ActivateFromTrayAndNavigate("notices", "unread");
        tray.ShowScheduleRequested += (_, _) => Window?.ActivateFromTrayAndNavigate("schedule", null);
        tray.HideWindowRequested += (_, _) => Window?.HideToTray();
        tray.ExitRequested += async (_, _) => { if (Window is not null) await Window.RequestExitAsync(); };
    }

    private static async Task StartBackendAsync()
    {
        // v1.1 §13.2 / §7.12：先探测；命中已有健康服务则**不显示启动态**，直接进正常态。
        bool reused = await Host.ProbeHealthAsync();
        if (!reused)
        {
            Status.StatusLine = "正在连接本地服务…（127.0.0.1:8737）";
            Window?.ShowBackendStarting();
        }

        bool ready = await Host.EnsureAsync();
        if (!ready)
        {
            Window?.ShowDisconnected();
            return;
        }

        bool got = await State.RefreshAsync(StateOrigin.Startup);
        if (!got)
        {
            Window?.ShowDisconnected();
            return;
        }

        State.StartPolling();
        Window?.OnBackendReady();
    }

    /// <summary>InfoBar 上的「重试连接」：重新探测 / 重新托管，不重启整个应用。</summary>
    public static async Task RetryConnectionAsync()
    {
        State.StopPolling();
        bool reused = await Host.ProbeHealthAsync();
        if (!reused) Window?.ShowBackendStarting();
        bool ready = await Host.EnsureAsync();
        if (!ready)
        {
            Window?.ShowDisconnected();
            return;
        }
        bool got = await State.RefreshAsync(StateOrigin.Reconnect);
        if (!got)
        {
            Window?.ShowDisconnected();
            return;
        }
        State.StartPolling();
        Window?.OnBackendReady();
    }

    // --------------------------- 激活路由 ---------------------------

    private static void OnAppInstanceActivated(object? sender, AppActivationArguments e)
    {
        UiDispatcher.TryEnqueue(() =>
        {
            Window?.ActivateFromTray();
            HandleActivation(e);
        });
    }

    private static void HandleActivation(AppActivationArguments? args)
    {
        if (args is null) return;

        // AppNotification 冷启动激活：Arguments 是字典，交给 ToastService 解析。
        if (args.Kind.ToString().Contains("AppNotification", StringComparison.OrdinalIgnoreCase))
        {
            Toast.HandleActivation(args.Data);
            return;
        }

        string arguments = ExtractArguments(args.Data);
        if (string.IsNullOrEmpty(arguments)) return;

        switch (arguments)
        {
            case JumpListService.ArgNotices:
                Window?.ActivateFromTrayAndNavigate("notices", null);
                break;
            case JumpListService.ArgSchedule:
                Window?.ActivateFromTrayAndNavigate("schedule", null);
                break;
            case JumpListService.ArgPoll:
                // 断连时仅激活窗口显示 Error，不重复 poll（R4）。
                _ = Window?.TriggerPollAsync(activateOnDisconnect: true);
                break;
            default:
                if (arguments.StartsWith("toast:openSchedule", StringComparison.Ordinal))
                {
                    var itemId = ParseQuery(arguments, "itemId");
                    if (!string.IsNullOrEmpty(itemId)) Window?.ActivateFromTrayAndLocate(itemId);
                }
                break;
        }
    }

    private static string ExtractArguments(object? data)
    {
        if (data is null) return string.Empty;
        try
        {
            return data.GetType().GetProperty("Arguments")?.GetValue(data) as string ?? string.Empty;
        }
        catch
        {
            return string.Empty;
        }
    }

    private static string ParseQuery(string text, string key)
    {
        int idx = text.IndexOf('?');
        string query = idx >= 0 ? text[(idx + 1)..] : text;
        foreach (var pair in query.Split('&', StringSplitOptions.RemoveEmptyEntries))
        {
            var kv = pair.Split('=', 2);
            if (kv.Length == 2 && kv[0] == key) return Uri.UnescapeDataString(kv[1]);
        }
        return string.Empty;
    }
}
