using CampusNotice.Helpers;
using CampusNotice.Models;
using CampusNotice.Services;
using Microsoft.UI.Dispatching;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;

namespace CampusNotice.Views;

/// <summary>
/// 设置页（UX §6 / §10.9.4）：五个分组、720 宽、组间 24。
/// * 分组 1 表单**显式一次性保存**（settings.save 会 restartTimers，绝不逐字符保存）。
/// * 4 个行为开关即时保存：防抖 400ms + StateStore 串行 + 在途禁用；失败由返回 state 回滚。
/// * 凭据（cookie / apiKey / password）一律 PasswordBox，绝不回显；DPAPI 说明常驻。
/// * 「测试桌面通知」走客户端 ToastService 自测，**绝不调用 notify.test op**。
/// * 整页不含任何启动类入口：不写启动文件夹 / 注册表启动项 / 计划任务，也不出现相关文案。
/// </summary>
public sealed partial class SettingsPage : Page, IPageView
{
    private readonly DispatcherTimer _debounceTimer = new();
    private readonly Dictionary<string, bool> _pendingToggles = new(StringComparer.Ordinal);
    private AppState? _state;
    private bool _formInitialized;
    private bool _suppressToggles;
    private bool _compact;
    private bool _toggleSaveInFlight;

    public SettingsPage()
    {
        InitializeComponent();
        _debounceTimer.Interval = TimeSpan.FromMilliseconds(400);
        _debounceTimer.Tick += OnDebounceTick;
    }

    // --------------------------- IPageView ---------------------------

    public void ApplyState(AppState state, StateOrigin origin)
    {
        _state = state;
        if (!_formInitialized)
        {
            PopulateForm(state);
            _formInitialized = true;
        }
        UpdateReadOnly(state);
        SyncToggles(state);
    }

    public void HandleCommand(string command)
    {
        switch (command)
        {
            case "saveSettings":
                _ = SaveGroup1Async();
                break;
            case "focusAccount":
                BackLink.Visibility = Visibility.Visible;
                UsernameBox.Focus(FocusState.Programmatic);
                break;
            case "escape":
                DirtyHint.Visibility = Visibility.Collapsed;
                break;
        }
    }

    public void ApplyCompact(bool compact)
    {
        _compact = compact;
        PageRoot.MaxWidth = compact ? double.PositiveInfinity : 720;
        PageRoot.Padding = compact ? new Thickness(12, 16, 12, 24) : new Thickness(24, 8, 24, 32);
    }

    public void OnPageActivated() { }

    // --------------------------- 表单填充 / 只读状态 ---------------------------

    private void PopulateForm(AppState state)
    {
        PollMinutesBox.Value = state.Settings.PollMinutes;
        KeywordsBox.Text = string.Join(", ", state.Settings.Keywords ?? new List<string>());
        AutoRemindBox.Text = string.Join(",", state.Settings.AutoRemindBefore ?? new List<int>());
        LlmMaxBox.Value = state.Settings.LlmMaxPerPoll;
        DetailLimitBox.Value = state.Settings.DetailLimit;
        MaxItemsBox.Value = state.Settings.MaxItemsPerSource;

        CasBaseBox.Text = state.Meta.Account.CasBase ?? string.Empty;
        ServiceBox.Text = state.Meta.Account.Service ?? string.Empty;
        UsernameBox.Text = state.Meta.Account.Username ?? string.Empty;
        PasswordBox.Password = string.Empty;   // 从不回显
        ApiKeyBox.Password = string.Empty;     // 从不回显
        PasswordBox.PlaceholderText = state.Meta.Account.Configured
            ? "已保存（留空不修改）"
            : "首次保存需要填写密码";
        ApiKeyBox.PlaceholderText = state.Meta.LlmKeySet
            ? "已保存（留空不修改）"
            : "尚未保存 API Key";
        DirtyHint.Visibility = Visibility.Collapsed;
    }

    private void UpdateReadOnly(AppState state)
    {
        var account = state.Meta.Account;
        var login = account.LastLogin;
        string loginText = login is null
            ? "尚未尝试自动登录"
            : $"上次登录 {Formatters.FormatTimestamp(login.At)} · {(login.Ok == true ? "成功" : login.Ok == false ? "失败" : "—")} · {login.Message}";
        AccountStatus.Text = $"账号状态：{(account.Configured ? "已配置" : "未配置")} · {loginText}";

        var llm = state.Meta.Llm;
        LlmStatusText.Text = $"API Key：{(state.Meta.LlmKeySet ? "已保存" : "未保存")} · " +
                             (llm is null ? "尚未测试模型" : $"上次测试 {Formatters.FormatTimestamp(llm.At)} · {(llm.Ok == true ? "可用" : llm.Ok == false ? "失败" : "—")} · {llm.Message}");

        RuntimeText.Text =
            $"上次抓取 {Formatters.FormatTimestamp(state.Meta.LastPollAt)} · 累计 {state.Meta.PollCount} 次 · 端口 {state.Meta.Port}\n" +
            $"数据文件 {state.Meta.DataPath}\n" +
            $"RSS 地址 http://127.0.0.1:{state.Meta.Port}/rss.xml";

        bool hasError = !string.IsNullOrEmpty(state.Meta.LastPollError);
        PollErrorInfoBar.IsOpen = hasError;
        if (hasError) PollErrorInfoBar.Message = state.Meta.LastPollError!;
    }

    private void SyncToggles(AppState state)
    {
        // v1.1 §7.8 ①：写队列在途 / 防抖窗口内保持乐观值不回弹，
        // 不因 state 刷新把用户刚点的开关弹回旧值。
        if (_toggleSaveInFlight || _pendingToggles.Count > 0) return;
        ApplyToggleValues(state);
    }

    private void ApplyToggleValues(AppState state)
    {
        _suppressToggles = true;
        try
        {
            ToggleLlm.IsOn = state.Settings.Llm;
            ToggleAutoSchedule.IsOn = state.Settings.AutoSchedule;
            ToggleFetchDetail.IsOn = state.Settings.FetchDetail;
            ToggleNotify.IsOn = state.Settings.Notify;
        }
        finally
        {
            _suppressToggles = false;
        }
    }

    private void Text_MarkDirty(object sender, TextChangedEventArgs e) => MarkDirty();

    private void Number_MarkDirty(NumberBox sender, NumberBoxValueChangedEventArgs args) => MarkDirty();

    private void MarkDirty() => DirtyHint.Visibility = Visibility.Visible;

    // --------------------------- 分组 1：显式一次性保存 ---------------------------

    private async void SaveGroup1_Click(object sender, RoutedEventArgs e) => await SaveGroup1Async();

    private async Task SaveGroup1Async()
    {
        if (!App.State.IsConnected) { App.Window?.ShowDisconnected(); return; }

        if (double.IsNaN(PollMinutesBox.Value) || PollMinutesBox.Value < 5)
        {
            ShowGroup1Error("抓取间隔至少 5 分钟");
            return;
        }

        var keywords = SplitWords(KeywordsBox.Text);
        var oldKeywords = _state?.Settings.Keywords ?? new List<string>();
        if (keywords.Count == 0 && oldKeywords.Count > 0)
        {
            // 清空关键词是高风险：会给二次确认（评分依赖关键词）。
            bool ok = await Dialogs.ConfirmAsync(XamlRoot, "清空全部关注关键词？",
                "清空后，已抓取通知的重要度不会自动改变；可稍后点「按当前关键词重算摘要」。", "仍然清空");
            if (!ok) return;
        }

        var settings = new Dictionary<string, object?>
        {
            ["pollMinutes"] = (int)PollMinutesBox.Value,
            ["keywords"] = keywords,
            ["llmMaxPerPoll"] = (int)(double.IsNaN(LlmMaxBox.Value) ? 8 : LlmMaxBox.Value),
            ["detailLimit"] = (int)(double.IsNaN(DetailLimitBox.Value) ? 25 : DetailLimitBox.Value),
            ["maxItemsPerSource"] = (int)(double.IsNaN(MaxItemsBox.Value) ? 25 : MaxItemsBox.Value),
        };
        var remind = SplitMinutes(AutoRemindBox.Text);
        if (remind.Count > 0) settings["autoRemindBefore"] = remind;

        // 一次点击只发 1 次 settings.save（只重建一次定时器）。
        var response = await App.State.DispatchAsync(Ops.SettingsSave,
            new Dictionary<string, object?> { ["settings"] = settings });
        if (response?.Ok != true)
        {
            ShowGroup1Error(response?.FailureText ?? "本地服务未响应");
            return;
        }
        ShowGroup1Error(null);
        DirtyHint.Visibility = Visibility.Collapsed;
        if (App.State.Current is { } current) PopulateForm(current);
        App.Window?.ShowSuccess("设置已保存。已有通知的重要度不会自动重算，可点「按当前关键词重算摘要」。");
    }

    private void ShowGroup1Error(string? message)
    {
        Group1Error.Text = message ?? string.Empty;
        Group1Error.Visibility = string.IsNullOrEmpty(message) ? Visibility.Collapsed : Visibility.Visible;
    }

    private static List<string> SplitWords(string? text) =>
        (text ?? string.Empty)
            .Split(new[] { ',', '，', ' ', '\t', '\r', '\n', '、', ';' }, StringSplitOptions.RemoveEmptyEntries)
            .Select(x => x.Trim())
            .Where(x => x.Length > 0)
            .Distinct()
            .Take(200)
            .ToList();

    private static List<int> SplitMinutes(string? text) =>
        (text ?? string.Empty)
            .Split(new[] { ',', '，', ' ', '\t', '、' }, StringSplitOptions.RemoveEmptyEntries)
            .Select(x => int.TryParse(x.Trim(), out var n) && n >= 0 ? n : -1)
            .Where(n => n >= 0)
            .Distinct()
            .ToList();

    // --------------------------- 分组 2：防抖即时保存 ---------------------------

    private void Toggle_Toggled(object sender, RoutedEventArgs e)
    {
        if (_suppressToggles || _toggleSaveInFlight) return;
        if (sender is not ToggleSwitch toggle || toggle.Tag is not string key) return;
        var current = App.State.Current;
        if (current is not null && GetToggleValue(current.Settings, key) == toggle.IsOn) return;

        _pendingToggles[key] = toggle.IsOn;
        _debounceTimer.Stop();
        _debounceTimer.Start();
    }

    private static bool GetToggleValue(Settings s, string key) => key switch
    {
        "llm" => s.Llm,
        "autoSchedule" => s.AutoSchedule,
        "fetchDetail" => s.FetchDetail,
        "notify" => s.Notify,
        _ => false
    };

    private async void OnDebounceTick(object? sender, object e)
    {
        _debounceTimer.Stop();
        if (_pendingToggles.Count == 0 || _toggleSaveInFlight) return;
        var batch = new Dictionary<string, bool>(_pendingToggles);
        _pendingToggles.Clear();

        _toggleSaveInFlight = true;
        SetTogglesEnabled(false);
        SetRingsActive(true);     // ② 右侧紧邻 14px ProgressRing，防抖窗口内只出现一个在途态
        try
        {
            // 串行提交单键 patch；StateStore 也会串行化，双保险。
            foreach (var pair in batch)
            {
                var response = await App.State.DispatchAsync(Ops.SettingsSave,
                    Settings.TogglePatch(pair.Key, pair.Value));
                if (response is null)
                {
                    App.Window?.ShowDisconnected();
                    break;
                }
                if (!response.Ok)
                {
                    // ④ 失败 → 回滚乐观值（由返回 state 刷新）+ 顶部 InfoBar Error，不弹窗。
                    ApplyToggleValues(App.State.Current ?? _state!);
                    App.Window?.ShowError("开关保存失败", response.FailureText);
                    break;
                }
            }
            // ⑤ 成功不额外弹 Success 吐司 / TeachingTip，仅由 state 刷新与顶部状态位承担。
        }
        finally
        {
            SetRingsActive(false);
            SetTogglesEnabled(true);
            _toggleSaveInFlight = false;
            // 队列结束后按真值同步一次，清掉乐观态。
            if (App.State.Current is { } current) ApplyToggleValues(current);
        }
    }

    private void SetRingsActive(bool active)
    {
        SetRing(RingLlm, active);
        SetRing(RingAutoSchedule, active);
        SetRing(RingFetchDetail, active);
        SetRing(RingNotify, active);
    }

    private static void SetRing(ProgressRing ring, bool active)
    {
        ring.IsActive = active;
        ring.Visibility = active ? Visibility.Visible : Visibility.Collapsed;
    }

    private void SetTogglesEnabled(bool enabled)
    {
        ToggleLlm.IsEnabled = enabled;
        ToggleAutoSchedule.IsEnabled = enabled;
        ToggleFetchDetail.IsEnabled = enabled;
        ToggleNotify.IsEnabled = enabled;
    }

    // --------------------------- 分组 3：CAS ---------------------------

    private void BackLink_Click(object sender, RoutedEventArgs e)
    {
        var target = App.Window?.PreviousPageTag ?? "notices";
        BackLink.Visibility = Visibility.Collapsed;
        App.Window?.NavigateTo(target);
    }

    private async void AccountSave_Click(object sender, RoutedEventArgs e)
    {
        string username = UsernameBox.Text.Trim();
        string casBase = CasBaseBox.Text.Trim();
        string serviceUrl = ServiceBox.Text.Trim();
        string password = PasswordBox.Password;

        if (string.IsNullOrEmpty(username)) { ShowAccountError("账号不能为空"); return; }
        if (!casBase.StartsWith("http://", StringComparison.OrdinalIgnoreCase) &&
            !casBase.StartsWith("https://", StringComparison.OrdinalIgnoreCase) ||
            !serviceUrl.StartsWith("http://", StringComparison.OrdinalIgnoreCase) &&
            !serviceUrl.StartsWith("https://", StringComparison.OrdinalIgnoreCase))
        {
            ShowAccountError("「认证入口地址」和「登录后返回的地址」都要填 http(s) 开头的完整网址");
            return;
        }
        if (string.IsNullOrEmpty(password) && App.State.Current?.Meta.Account.Configured != true)
        {
            ShowAccountError("第一次保存需要填写密码");
            return;
        }

        var payload = new Dictionary<string, object?>
        {
            ["username"] = username,
            ["password"] = password,
            ["casBase"] = casBase,
            ["service"] = serviceUrl,
        };
        AccountSaveButton.IsEnabled = false;
        AccountSaveButton.Content = "登录中…";
        try
        {
            var response = await App.State.DispatchAsync(Ops.AccountSave, payload);
            if (response?.Ok != true)
            {
                ShowAccountError(response?.FailureText ?? "保存失败，请到设置页检查账号与密码");
                return;
            }
            ShowAccountError(null);
            PasswordBox.Password = string.Empty;
            App.Window?.ShowSuccess(response.SuccessText.Length > 0 ? response.SuccessText : "账号已保存并登录");
        }
        finally
        {
            AccountSaveButton.IsEnabled = true;
            AccountSaveButton.Content = "保存并登录";
        }
    }

    private async void AccountTest_Click(object sender, RoutedEventArgs e)
    {
        if (App.State.Current?.Meta.Account.Configured != true)
        {
            ShowAccountError("还没保存统一身份认证账号密码，请先在上面填写并点「保存并登录」。");
            UsernameBox.Focus(FocusState.Programmatic);
            return;
        }
        AccountTestButton.IsEnabled = false;
        try
        {
            var response = await App.State.DispatchAsync(Ops.AccountTest);
            if (response?.Ok == true) { ShowAccountError(null); App.Window?.ShowSuccess(response.SuccessText); }
            else ShowAccountError(response?.FailureText ?? "自动登录失败");
        }
        finally
        {
            AccountTestButton.IsEnabled = true;
        }
    }

    private async void AccountClear_Click(object sender, RoutedEventArgs e)
    {
        bool ok = await Dialogs.ConfirmDestructiveAsync(XamlRoot, "清除已保存的账号",
            "确定清除本机保存的统一身份认证账号与密码吗？此操作不可恢复，之后需要重新填写一次。",
            "清除");
        if (!ok) return;
        var response = await App.State.DispatchAsync(Ops.AccountClear);
        if (response?.Ok == true)
        {
            CasBaseBox.Text = string.Empty;
            ServiceBox.Text = string.Empty;
            UsernameBox.Text = string.Empty;
            PasswordBox.Password = string.Empty;
            PasswordBox.PlaceholderText = "首次保存需要填写密码";
            App.Window?.ShowSuccess("已清除保存的账号密码");
        }
    }

    private void ShowAccountError(string? message)
    {
        AccountError.Text = message ?? string.Empty;
        AccountError.Visibility = string.IsNullOrEmpty(message) ? Visibility.Collapsed : Visibility.Visible;
    }

    // --------------------------- 分组 4：AI 摘要 ---------------------------

    private async void LlmSave_Click(object sender, RoutedEventArgs e)
    {
        string key = ApiKeyBox.Password.Trim();
        if (string.IsNullOrEmpty(key))
        {
            ShowLlmError("留空表示不修改；如需清空请点「清空 Key」（会二次确认）。");
            return;
        }
        var response = await App.State.DispatchAsync(Ops.LlmKey,
            new Dictionary<string, object?> { ["apiKey"] = key });
        if (response?.Ok != true) { ShowLlmError(response?.FailureText ?? "保存失败"); return; }
        ShowLlmError(null);
        ApiKeyBox.Password = string.Empty;
        ApiKeyBox.PlaceholderText = "已保存（留空不修改）";
        App.Window?.ShowSuccess(response.SuccessText);
    }

    private async void LlmClear_Click(object sender, RoutedEventArgs e)
    {
        bool ok = await Dialogs.ConfirmDestructiveAsync(XamlRoot, "清空 API Key",
            "确定清空已保存的 API Key 吗？清空后 AI 摘要会退回规则摘要，需要重新填写 Key。",
            "清空 Key");
        if (!ok) return;
        var response = await App.State.DispatchAsync(Ops.LlmKey,
            new Dictionary<string, object?> { ["apiKey"] = string.Empty });
        if (response?.Ok == true)
        {
            ApiKeyBox.Password = string.Empty;
            ApiKeyBox.PlaceholderText = "尚未保存 API Key";
            ShowLlmError(null);
            App.Window?.ShowSuccess(response.SuccessText);
        }
    }

    private async void LlmTest_Click(object sender, RoutedEventArgs e)
    {
        LlmTestButton.IsEnabled = false;
        LlmTestButton.Content = "测试中…";
        try
        {
            var response = await App.State.DispatchAsync(Ops.LlmTest);
            // 失败不覆盖已有 Key，也不升级为全局 Error（D2 原则）。
            if (response?.Ok == true) { ShowLlmError(null); App.Window?.ShowSuccess(response.SuccessText); }
            else ShowLlmError(response?.FailureText ?? "模型不可用");
        }
        finally
        {
            LlmTestButton.IsEnabled = true;
            LlmTestButton.Content = "测试模型";
        }
    }

    private void ShowLlmError(string? message)
    {
        LlmError.Text = message ?? string.Empty;
        LlmError.Visibility = string.IsNullOrEmpty(message) ? Visibility.Collapsed : Visibility.Visible;
    }

    // --------------------------- 分组 5：运行状态 / 触发型 op ---------------------------

    private void TestToast_Click(object sender, RoutedEventArgs e)
    {
        // 客户端 AppNotification 单一通道自测；**不调用 notify.test**（决策 3）。
        bool ok = App.Toast.ShowTestNotification();
        App.Window?.ShowSuccess(ok
            ? "已发出一条自测桌面通知"
            : "系统通知通道当前不可用（AppNotification 未注册或系统设置关闭）");
    }

    private async void Rescore_Click(object sender, RoutedEventArgs e)
    {
        RescoreButton.IsEnabled = false;
        RescoreButton.Content = "重算中…";
        try
        {
            var response = await App.State.DispatchAsync(Ops.NoticesRescore);
            if (response?.Ok == true) App.Window?.ShowSuccess(response.SuccessText + "（AI 摘要保留）");
            else if (response is not null) App.Window?.ShowWarning("重算未完成", response.FailureText);
        }
        finally
        {
            RescoreButton.IsEnabled = true;
            RescoreButton.Content = "按当前关键词重算摘要";
        }
    }

    private async void Refill_Click(object sender, RoutedEventArgs e)
    {
        RefillButton.IsEnabled = false;
        RefillButton.Content = "补齐中…";
        try
        {
            // 后端 notices.refill 是一次性返回 message（「补齐 N 条正文」），
            // 无逐条进度事件，故以按钮 loading + 结果 message 呈现，未实现「N / 40」确定性进度条。
            var response = await App.State.DispatchAsync(Ops.NoticesRefill,
                new Dictionary<string, object?> { ["limit"] = 40 });
            if (response?.Ok == true) App.Window?.ShowSuccess(response.SuccessText);
            else if (response is not null) App.Window?.ShowWarning("补齐正文未完成", response.FailureText);
        }
        finally
        {
            RefillButton.IsEnabled = true;
            RefillButton.Content = "补齐通知正文";
        }
    }
}
