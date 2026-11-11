using CampusNotice.Helpers;
using CampusNotice.Models;
using Microsoft.UI.Dispatching;
using Microsoft.Windows.AppNotifications;
using Microsoft.Windows.AppNotifications.Builder;

namespace CampusNotice.Services;

/// <summary>
/// 原生 Toast 的**唯一通道**：Windows App SDK AppNotification（决策 3 / 架构确认项 A1）。
///
/// * 后端 server.mjs 以 --no-toast 托管：onNotify 只记日志并返回 true，不调 PowerShell toast()。
///   store.mjs recordReminder() L627-641 是「先写 meta.reminders 再判断 settings.notify」，
///   所以静默化后端**不会丢提醒记录**；不带 --no-toast 参数时后端行为完全不变。
/// * 客户端 StateStore 每 10s 轮询 /api/state，读 meta.reminders
///   （state 只回前 20 条），按 reminder.id 去重，**只对本次会话启动后出现的新 id** 发通知。
/// * settings.notify === false 时客户端也不弹（两端都尊重）。
/// * 设置页「测试桌面通知」走 ShowTestNotification()，**绝不调用 notify.test op**
///   （那个 op 会走后端 PowerShell 通道，绕过单一通道）。
/// * 通知只带一个「打开日程」按钮：激活窗口 → 日程页 → 定位 itemId，只定位不改状态。
/// </summary>
public sealed class ToastService
{
    private readonly HashSet<string> _seen = new(StringComparer.Ordinal);
    private readonly DateTime _sessionStartUtc = DateTime.UtcNow;
    private DispatcherQueue? _dispatcher;
    private bool _baselineDone;
    private bool _registered;

    /// <summary>AppNotification 是否可用；unpackaged 注册失败时为 false（见文档「未验证项」）。</summary>
    public bool IsAvailable => _registered;

    /// <summary>「打开日程」激活（前台 NotificationInvoked 或冷启动 AppInstance.Activated 都汇到这里）。</summary>
    public event EventHandler<string>? OpenScheduleRequested;

    public void Initialize(DispatcherQueue dispatcher)
    {
        _dispatcher = dispatcher;
        try
        {
            AppNotificationManager.Default.NotificationInvoked += OnNotificationInvoked;
            AppNotificationManager.Default.Register();
            _registered = true;
        }
        catch (Exception ex)
        {
            // unpackaged 应用在部分 Windows App SDK 版本/系统组合下注册 COM 激活器会失败。
            // 降级：不发系统 Toast，但仍在应用内状态行与日程分组显示提醒，功能不阻断。
            _registered = false;
            System.Diagnostics.Debug.WriteLine($"[ToastService] AppNotification 注册失败：{ex.Message}");
        }
    }

    public void Unregister()
    {
        if (!_registered) return;
        try
        {
            AppNotificationManager.Default.NotificationInvoked -= OnNotificationInvoked;
            AppNotificationManager.Default.Unregister();
        }
        catch
        {
            // 忽略：进程即将退出。
        }
        _registered = false;
    }

    /// <summary>
    /// 供 App 处理冷启动激活参数（AppInstance.Activated，Kind=AppNotification）。
    /// 用反射取 Arguments，避免与不同 SDK 版本的投影类型强耦合。
    /// </summary>
    public void HandleActivation(object? data)
    {
        if (data is null) return;
        try
        {
            var prop = data.GetType().GetProperty("Arguments");
            if (prop?.GetValue(data) is IReadOnlyDictionary<string, string> dict)
                HandleActivationArguments(dict);
        }
        catch (Exception ex)
        {
            System.Diagnostics.Debug.WriteLine($"[ToastService] 解析 Toast 激活参数失败：{ex.Message}");
        }
    }

    public void HandleActivationArguments(IReadOnlyDictionary<string, string> arguments)
    {
        arguments.TryGetValue("action", out var action);
        arguments.TryGetValue("itemId", out var itemId);
        if (action == "openSchedule" && !string.IsNullOrEmpty(itemId))
            _dispatcher?.TryEnqueue(() => OpenScheduleRequested?.Invoke(this, itemId));
    }

    private void OnNotificationInvoked(object? sender, AppNotificationActivatedEventArgs e)
    {
        if (e.Arguments is null) return;
        var dict = new Dictionary<string, string>(StringComparer.Ordinal);
        foreach (var pair in e.Arguments) dict[pair.Key] = pair.Value;
        HandleActivationArguments(dict);
    }

    /// <summary>每次 state 刷新后调用（Startup / Poll / Action 均可）。同一 reminder.id 全程只发一条。</summary>
    public void ProcessReminders(AppState state)
    {
        bool notifyEnabled = state.Settings.Notify && LocalPrefs.Current.NotifyEnabledLocal;
        foreach (var reminder in state.Meta.Reminders)
        {
            if (string.IsNullOrEmpty(reminder.Id)) continue;
            if (_seen.Contains(reminder.Id)) continue;

            // 会话启动时已存在的历史提醒只做基线，不弹；只有启动后新出现的 id 才弹。
            bool isNew = _baselineDone || IsAfterSessionStart(reminder.At);
            _seen.Add(reminder.Id);
            if (!isNew || !notifyEnabled || !_registered) continue;
            ShowReminder(reminder);
        }
        _baselineDone = true;
    }

    private bool IsAfterSessionStart(string at)
    {
        if (string.IsNullOrEmpty(at)) return false;
        return DateTime.TryParse(at, out var dt) && dt.ToUniversalTime() > _sessionStartUtc;
    }

    private void ShowReminder(Reminder reminder)
    {
        try
        {
            var button = new AppNotificationButton("打开日程")
                .AddArgument("action", "openSchedule")
                .AddArgument("itemId", reminder.ItemId);
            var notification = new AppNotificationBuilder()
                .AddText($"日程提醒 · {reminder.Title}")
                .AddText(Formatters.Truncate(reminder.Note, 120))
                .AddButton(button)
                .SetTag(reminder.Id)
                .SetGroup("schedule-reminders")
                .BuildNotification();
            AppNotificationManager.Default.Show(notification);
        }
        catch (Exception ex)
        {
            System.Diagnostics.Debug.WriteLine($"[ToastService] 显示提醒失败：{ex.Message}");
        }
    }

    /// <summary>
    /// 设置页「测试桌面通知」：**客户端自测 Toast**，不调用 notify.test op。
    /// 若 AppNotification 不可用，直接返回 false，由页面用 Informational InfoBar 说明。
    /// </summary>
    public bool ShowTestNotification()
    {
        if (!_registered) return false;
        try
        {
            var notification = new AppNotificationBuilder()
                .AddText("校园通知 · 日程")
                .AddText("这是一条来自客户端的测试桌面通知，说明系统通知通道可用。")
                .SetTag("self-test")
                .BuildNotification();
            AppNotificationManager.Default.Show(notification);
            return true;
        }
        catch
        {
            return false;
        }
    }
}
