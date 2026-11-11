using System.Collections.ObjectModel;
using System.Text.Json;
using CampusNotice.Helpers;
using CampusNotice.Models;
using CampusNotice.Services;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;

namespace CampusNotice.Views;

/// <summary>
/// 数据源页（UX §5 / §10.9.3）。
/// * loginExpired = Warning 橙 + 必有「重新登录」按钮；红色只留给 error 状态条/整体阻断。
/// * 测试成功内联展示 ≤5 条 preview；失败内联显示原因且失败仍应用返回 state。
/// * 新增/编辑用内联表单（10+ 字段不用 ContentDialog）；删除用 ContentDialog 二次确认。
/// * type 只提供 html/rss/json；切换 type 隐藏字段保留原值、不显式传空串覆盖。
/// </summary>
public sealed partial class SourcesPage : Page, IPageView
{
    public ObservableCollection<SourceViewModel> Sources { get; } = new();

    private readonly Dictionary<string, SourceViewModel> _all = new(StringComparer.Ordinal);
    private readonly Dictionary<string, string> _editOriginalType = new(StringComparer.Ordinal);
    private readonly SourceViewModel _addDraft = new();
    private AppState? _state;
    private bool _compact;
    private bool _suppressToggle;

    public SourcesPage()
    {
        InitializeComponent();
        SourceList.ItemsSource = Sources;
        _addDraft.EditType = "html";
        _addDraft.Type = "html";
        _addDraft.EditPages = "1";
        AddEditor.DataContext = _addDraft;
    }

    // --------------------------- IPageView ---------------------------

    public void ApplyState(AppState state, StateOrigin origin)
    {
        _state = state;
        var next = new Dictionary<string, SourceViewModel>(StringComparer.Ordinal);
        foreach (var source in state.Sources)
        {
            if (_all.TryGetValue(source.Id, out var vm)) vm.Apply(source);
            else
            {
                vm = new SourceViewModel();
                vm.Apply(source);
            }
            next[source.Id] = vm;
        }
        _all.Clear();
        foreach (var pair in next) _all[pair.Key] = pair.Value;

        var desired = state.Sources.Select(s => _all[s.Id]).ToList();
        var keep = new HashSet<string>(desired.Select(v => v.Id), StringComparer.Ordinal);
        for (int i = Sources.Count - 1; i >= 0; i--)
        {
            if (!keep.Contains(Sources[i].Id)) Sources.RemoveAt(i);
        }
        for (int i = 0; i < desired.Count; i++)
        {
            if (i < Sources.Count && Sources[i].Id == desired[i].Id) continue;
            int existing = -1;
            for (int j = 0; j < Sources.Count; j++)
            {
                if (Sources[j].Id == desired[i].Id) { existing = j; break; }
            }
            if (existing >= 0) Sources.Move(existing, i);
            else Sources.Insert(i, desired[i]);
        }

        EmptyState.Visibility = state.Sources.Count == 0 ? Visibility.Visible : Visibility.Collapsed;
    }

    public void HandleCommand(string command)
    {
        switch (command)
        {
            case "addSource":
                AddForm.Visibility = AddForm.Visibility == Visibility.Visible
                    ? Visibility.Collapsed
                    : Visibility.Visible;
                if (AddForm.Visibility == Visibility.Visible) ResetAddDraft();
                break;
            case "escape":
                AddForm.Visibility = Visibility.Collapsed;
                foreach (var vm in _all.Values.Where(v => v.IsEditing))
                {
                    vm.IsEditing = false;
                    vm.RefreshVisualState();
                }
                break;
        }
    }

    public void ApplyCompact(bool compact)
    {
        _compact = compact;
        PageRoot.MaxWidth = compact ? double.PositiveInfinity : 940;
        PageRoot.Padding = compact ? new Thickness(12, 16, 12, 24) : new Thickness(24, 8, 24, 32);
    }

    public void OnPageActivated() { }

    private SourceViewModel? Find(string id) => _all.TryGetValue(id, out var vm) ? vm : null;

    private void ResetAddDraft()
    {
        _addDraft.EditName = string.Empty;
        _addDraft.EditUrl = string.Empty;
        _addDraft.EditCookie = string.Empty;
        _addDraft.EditResolveIp = string.Empty;
        _addDraft.EditNote = string.Empty;
        _addDraft.EditLinkPattern = string.Empty;
        _addDraft.EditItemsPath = string.Empty;
        _addDraft.EditPages = "1";
        _addDraft.EditPageUrl = string.Empty;
        _addDraft.EditUrlTemplate = string.Empty;
        _addDraft.EditDetailUrlTemplate = string.Empty;
        _addDraft.EditContentPath = string.Empty;
        _addDraft.EditMap = string.Empty;
        _addDraft.SetEditType("html");
        AddError.Visibility = Visibility.Collapsed;
        AddEditor.DataContext = null;
        AddEditor.DataContext = _addDraft;   // 强制刷新 TwoWay 绑定
    }

    private void ShowAddError(string? message)
    {
        AddError.Text = message ?? string.Empty;
        AddError.Visibility = string.IsNullOrEmpty(message) ? Visibility.Collapsed : Visibility.Visible;
    }

    // --------------------------- 新增 ---------------------------

    private void AddCancel_Click(object sender, RoutedEventArgs e)
    {
        AddForm.Visibility = Visibility.Collapsed;
        ShowAddError(null);
    }

    private void EmptyAdd_Click(object sender, RoutedEventArgs e) => HandleCommand("addSource");

    private async void AddSave_Click(object sender, RoutedEventArgs e)
    {
        string url = _addDraft.EditUrl?.Trim() ?? string.Empty;
        if (!url.StartsWith("http://", StringComparison.OrdinalIgnoreCase) &&
            !url.StartsWith("https://", StringComparison.OrdinalIgnoreCase))
        {
            ShowAddError("请填写 http(s) 开头的网址");
            return;
        }

        var payload = CommonFields(_addDraft);
        payload["enabled"] = true;
        AddTypeFields(payload, _addDraft);

        var response = await App.State.DispatchAsync(Ops.SourceAdd, payload);
        if (response?.Ok != true)
        {
            ShowAddError(response?.FailureText ?? "本地服务未响应");
            return;
        }
        ShowAddError(null);
        AddForm.Visibility = Visibility.Collapsed;
        App.Window?.ShowTeachingTip("已添加数据源", "可点该卡上的「测试」立即验证解析是否正常。",
            null, null, null, "知道了", null, seconds: 5, lightDismiss: true);
    }

    private static Dictionary<string, object?> CommonFields(SourceViewModel vm) => new()
    {
        ["name"] = vm.EditName ?? string.Empty,
        ["type"] = vm.EditType,
        ["url"] = (vm.EditUrl ?? string.Empty).Trim(),
        ["cookie"] = vm.EditCookie ?? string.Empty,
        ["resolveIp"] = vm.EditResolveIp ?? string.Empty,
        ["note"] = Formatters.Truncate(vm.EditNote, 300),
    };

    private static void AddTypeFields(Dictionary<string, object?> payload, SourceViewModel vm)
    {
        switch (vm.EditType)
        {
            case "html":
                payload["linkPattern"] = vm.EditLinkPattern ?? string.Empty;
                payload["pageUrl"] = vm.EditPageUrl ?? string.Empty;
                payload["pages"] = ParsePages(vm.EditPages);
                payload["detailUrlTemplate"] = vm.EditDetailUrlTemplate ?? string.Empty;
                break;
            case "json":
                payload["itemsPath"] = vm.EditItemsPath ?? string.Empty;
                payload["pageUrl"] = vm.EditPageUrl ?? string.Empty;
                payload["pages"] = ParsePages(vm.EditPages);
                payload["urlTemplate"] = vm.EditUrlTemplate ?? string.Empty;
                payload["detailUrlTemplate"] = vm.EditDetailUrlTemplate ?? string.Empty;
                payload["contentPath"] = vm.EditContentPath ?? string.Empty;
                if (TryParseMap(vm.EditMap, out var map)) payload["map"] = map;
                break;
            // rss：只提交公共字段，隐藏字段不渲染、不参与提交。
        }
    }

    private static int ParsePages(string? raw) =>
        int.TryParse(raw, out var n) ? Math.Clamp(n, 1, 5) : 1;

    private static bool TryParseMap(string? raw, out Dictionary<string, string> map)
    {
        map = new Dictionary<string, string>();
        if (string.IsNullOrWhiteSpace(raw)) return false;
        try
        {
            var parsed = JsonSerializer.Deserialize<Dictionary<string, string>>(raw);
            if (parsed is null) return false;
            map = parsed;
            return true;
        }
        catch
        {
            return false;
        }
    }

    // --------------------------- 启用 / 停用（即时保存） ---------------------------

    private async void Enabled_Toggled(object sender, RoutedEventArgs e)
    {
        if (_suppressToggle || sender is not ToggleSwitch toggle || toggle.Tag is not string id) return;
        var vm = Find(id);
        if (vm is null) return;
        bool desired = toggle.IsOn;
        if (desired == vm.Model.Enabled) return;   // 绑定回写造成的 Toggled，忽略

        toggle.IsEnabled = false;                  // 在途禁用，防连点（§9.5）
        try
        {
            var response = await App.State.DispatchAsync(Ops.SourceUpdate,
                new Dictionary<string, object?> { ["id"] = id, ["enabled"] = desired });
            if (response is null) App.Window?.ShowDisconnected();
        }
        finally
        {
            toggle.IsEnabled = true;
        }
    }

    // --------------------------- 测试 / 重新登录 / 打开 ---------------------------

    private async void Test_Click(object sender, RoutedEventArgs e)
    {
        if (sender is not FrameworkElement fe || fe.Tag is not string id) return;
        var vm = Find(id);
        if (vm is null || vm.IsTesting) return;
        vm.IsTesting = true;
        vm.InlineError = string.Empty;
        try
        {
            var response = await App.State.DispatchAsync(Ops.SourceTest,
                new Dictionary<string, object?> { ["id"] = id });
            if (response is null) { App.Window?.ShowDisconnected(); return; }

            vm.Preview.Clear();
            if (response.Ok)
            {
                foreach (var item in response.Preview ?? new List<PreviewItem>()) vm.Preview.Add(item);
                vm.PreviewMessage = response.SuccessText;
                vm.ShowPreview = true;
                vm.InlineError = string.Empty;
            }
            else
            {
                vm.ShowPreview = false;
                vm.InlineError = response.FailureText;
                // 后端提示「这个地址返回的是 JSON」时给一键切换到 JSON 接口的动作（§5.2 R2）。
                if (response.FailureText.Contains("JSON", StringComparison.OrdinalIgnoreCase))
                {
                    App.Window?.ShowTeachingTip("这个地址看起来是 JSON 接口",
                        "可以把解析方式改成「JSON 接口」，必要时填 itemsPath。",
                        null, "一键切换到 JSON 接口", () =>
                        {
                            vm.LoadDraftFromModel();
                            vm.SetEditType("json");
                            vm.IsEditing = true;
                            vm.RefreshVisualState();
                            _editOriginalType[id] = vm.Model.Type;
                        }, "知道了", null, seconds: 8, lightDismiss: true);
                }
            }
        }
        finally
        {
            vm.IsTesting = false;
            vm.RefreshVisualState();
        }
    }

    private void Relogin_Click(object sender, RoutedEventArgs e) => _ = App.Window?.ReloginAsync();

    private void OpenUrl_Click(object sender, RoutedEventArgs e)
    {
        if (sender is FrameworkElement fe && fe.Tag is string id)
            _ = Formatters.OpenUrlAsync(Find(id)?.Model.Url);
    }

    // --------------------------- 编辑 ---------------------------

    private void Edit_Click(object sender, RoutedEventArgs e)
    {
        if (sender is not FrameworkElement fe || fe.Tag is not string id) return;
        var vm = Find(id);
        if (vm is null) return;
        _editOriginalType[id] = vm.Model.Type;
        vm.LoadDraftFromModel();
        vm.IsEditing = true;
        vm.RefreshVisualState();
    }

    private void EditCancel_Click(object sender, RoutedEventArgs e)
    {
        if (sender is not FrameworkElement fe || fe.Tag is not string id) return;
        var vm = Find(id);
        if (vm is null) return;
        if (_editOriginalType.TryGetValue(id, out var original)) vm.SetEditType(original);
        vm.IsEditing = false;
        vm.RefreshVisualState();
    }

    private async void EditSave_Click(object sender, RoutedEventArgs e)
    {
        if (sender is not FrameworkElement fe || fe.Tag is not string id) return;
        var vm = Find(id);
        if (vm is null) return;

        string url = (vm.EditUrl ?? string.Empty).Trim();
        if (!url.StartsWith("http://", StringComparison.OrdinalIgnoreCase) &&
            !url.StartsWith("https://", StringComparison.OrdinalIgnoreCase))
        {
            vm.InlineError = "请填写 http(s) 开头的网址";
            vm.RefreshVisualState();
            return;
        }

        var model = vm.Model;
        var patch = new Dictionary<string, object?> { ["id"] = id };
        PutIfChanged(patch, "name", vm.EditName, model.Name);
        PutIfChanged(patch, "url", url, model.Url);
        PutIfChanged(patch, "type", vm.EditType, model.Type);
        PutIfChanged(patch, "resolveIp", vm.EditResolveIp ?? string.Empty, model.ResolveIp);
        PutIfChanged(patch, "note", Formatters.Truncate(vm.EditNote, 300), model.Note);
        // Cookie 留空 = 不修改，绝不显式传空串覆盖（§5.2）。
        if (!string.IsNullOrEmpty(vm.EditCookie)) patch["cookie"] = vm.EditCookie;

        if (vm.EditType == "html")
        {
            PutIfChanged(patch, "linkPattern", vm.EditLinkPattern ?? string.Empty, model.LinkPattern);
            PutIfChanged(patch, "pageUrl", vm.EditPageUrl ?? string.Empty, model.PageUrl);
            PutIfChanged(patch, "detailUrlTemplate", vm.EditDetailUrlTemplate ?? string.Empty, model.DetailUrlTemplate);
            if (ParsePages(vm.EditPages) != model.Pages) patch["pages"] = ParsePages(vm.EditPages);
        }
        else if (vm.EditType == "json")
        {
            PutIfChanged(patch, "itemsPath", vm.EditItemsPath ?? string.Empty, model.ItemsPath);
            PutIfChanged(patch, "pageUrl", vm.EditPageUrl ?? string.Empty, model.PageUrl);
            PutIfChanged(patch, "urlTemplate", vm.EditUrlTemplate ?? string.Empty, model.UrlTemplate);
            PutIfChanged(patch, "detailUrlTemplate", vm.EditDetailUrlTemplate ?? string.Empty, model.DetailUrlTemplate);
            PutIfChanged(patch, "contentPath", vm.EditContentPath ?? string.Empty, model.ContentPath);
            if (ParsePages(vm.EditPages) != model.Pages) patch["pages"] = ParsePages(vm.EditPages);
            if (TryParseMap(vm.EditMap, out var map)) patch["map"] = map;
        }

        var response = await App.State.DispatchAsync(Ops.SourceUpdate, patch);
        if (response?.Ok != true)
        {
            vm.InlineError = response?.FailureText ?? "本地服务未响应";
            vm.RefreshVisualState();
            return;
        }
        vm.IsEditing = false;
        vm.InlineError = string.Empty;
        vm.RefreshVisualState();
    }

    private static void PutIfChanged(Dictionary<string, object?> patch, string key, string? value, string original)
    {
        string candidate = value ?? string.Empty;
        if (!string.Equals(candidate, original ?? string.Empty, StringComparison.Ordinal))
            patch[key] = candidate;
    }

    // --------------------------- 删除（无恢复 op → 二次确认） ---------------------------

    private async void Delete_Click(object sender, RoutedEventArgs e)
    {
        if (sender is not FrameworkElement fe || fe.Tag is not string id) return;
        var vm = Find(id);
        if (vm is null) return;

        bool confirmed = await Dialogs.ConfirmDestructiveAsync(
            XamlRoot,
            "删除数据源",
            $"确定删除「{vm.Name}」吗？删除后不可恢复（后端没有恢复数据源的操作），已抓取的通知会保留。",
            "删除");
        if (!confirmed) return;

        var response = await App.State.DispatchAsync(Ops.SourceRemove,
            new Dictionary<string, object?> { ["id"] = id });
        if (response?.Ok != true)
            App.Window?.ShowWarning("删除未完成", response?.FailureText ?? "本地服务未响应");
    }
}
