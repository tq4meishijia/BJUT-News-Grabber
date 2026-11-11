using System.Collections.ObjectModel;
using CampusNotice.Helpers;
using CampusNotice.Models;
using CampusNotice.Services;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media;
using Windows.Foundation;

namespace CampusNotice.Views;

/// <summary>
/// 日程页（UX §4 / §10.9.2）。
/// 分组顺序恒定：已逾期→今天→明天→本周内→更远→未定时间→(已完成)，空组不渲染。
/// 完成态 = 删除线 + 「已完成」文字 + AutomationProperties.Name，Opacity 只是装饰（§4.6）。
/// 删除 = 8s 延迟提交 + TeachingTip 撤销；改时间 / 提醒档位会由后端清空 fired[]。
/// </summary>
public sealed partial class SchedulePage : Page, IPageView
{
    public ObservableCollection<ScheduleGroupViewModel> Groups { get; } = new();

    private readonly Dictionary<string, ScheduleGroupViewModel> _groupTemplates = new(StringComparer.Ordinal);
    private readonly Dictionary<string, ScheduleItemViewModel> _items = new(StringComparer.Ordinal);
    private AppState? _state;
    private bool _showDone;
    private bool _compact;

    public SchedulePage()
    {
        InitializeComponent();
        GroupList.ItemsSource = Groups;
        foreach (var name in Formatters.GroupNames)
        {
            var key = Formatters.GroupBrushKey(name);
            _groupTemplates[name] = new ScheduleGroupViewModel
            {
                Name = name,
                HeaderBrush = ThemeBrush.Get(key),
                HeaderFill = ThemeBrush.Get(key),
            };
        }
        _showDone = LocalPrefs.Current.ShowDoneItems;
    }

    // --------------------------- IPageView ---------------------------

    public void ApplyState(AppState state, StateOrigin origin)
    {
        _state = state;
        bool highContrast = ThemeBrush.IsHighContrast();

        var buckets = new Dictionary<string, List<ScheduleItem>>(StringComparer.Ordinal);
        foreach (var name in Formatters.GroupNames) buckets[name] = new List<ScheduleItem>();

        foreach (var item in state.Items)
        {
            if (item.Done && !_showDone) continue;
            string group = item.Done ? "已完成" : Formatters.DayLabel(item.DueAt);
            if (!buckets.ContainsKey(group)) group = "未定时间";
            buckets[group].Add(item);
        }

        foreach (var pair in buckets)
        {
            if (pair.Key == "已完成")
                pair.Value.Sort((a, b) => string.CompareOrdinal(b.DoneAt ?? "", a.DoneAt ?? ""));
            else
                pair.Value.Sort((a, b) => string.CompareOrdinal(a.DueAt ?? "", b.DueAt ?? ""));
        }

        var used = new HashSet<string>(StringComparer.Ordinal);
        foreach (var pair in buckets)
        {
            var group = _groupTemplates[pair.Key];
            var desired = new List<ScheduleItemViewModel>();
            foreach (var item in pair.Value)
            {
                if (_items.TryGetValue(item.Id, out var vm)) vm.Apply(item, pair.Key, highContrast);
                else
                {
                    vm = new ScheduleItemViewModel();
                    vm.Apply(item, pair.Key, highContrast);
                    _items[item.Id] = vm;
                }
                used.Add(item.Id);
                if (!vm.IsHidden) desired.Add(vm);
            }
            Sync(group.Items, desired, v => v.Id);
            group.Count = group.Items.Count;
            group.HeaderBrush = ThemeBrush.Get(Formatters.GroupBrushKey(pair.Key));
            group.HeaderFill = ThemeBrush.Get(Formatters.GroupBrushKey(pair.Key));
        }

        foreach (var key in _items.Keys.Where(k => !used.Contains(k) && !_items[k].IsHidden).ToList())
            _items.Remove(key);

        var visibleGroups = Formatters.GroupNames
            .Where(name => (name != "已完成" || _showDone) && _groupTemplates[name].Items.Count > 0)
            .Select(name => _groupTemplates[name])
            .ToList();
        Sync(Groups, visibleGroups, g => g.Name);

        PendingCount.Text = $"共 {state.PendingItemCount} 项待办";
        SoonCount.Text = $"{Formatters.DueIn24Hours(state)} 项 24 小时内到期";
        EmptyState.Visibility = visibleGroups.Count == 0 ? Visibility.Visible : Visibility.Collapsed;
    }

    public void HandleCommand(string command)
    {
        switch (command)
        {
            case "addItem":
                AddForm.Visibility = AddForm.Visibility == Visibility.Visible
                    ? Visibility.Collapsed
                    : Visibility.Visible;
                break;
            case "showDone":
                _showDone = LocalPrefs.Current.ShowDoneItems;
                Rebuild();
                break;
            case "escape":
                AddForm.Visibility = Visibility.Collapsed;
                foreach (var vm in _items.Values.Where(v => v.IsEditing))
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

    public void OnPageActivated() => Rebuild();

    /// <summary>深链「打开日程」：只定位、不改状态（§1.6）。</summary>
    public void LocateItem(string itemId)
    {
        if (string.IsNullOrEmpty(itemId)) return;
        App.UiDispatcher.TryEnqueue(() => ScrollToItem(itemId));
    }

    public void LocateByNoticeId(string noticeId)
    {
        var item = _items.Values.FirstOrDefault(v => v.Model.NoticeId == noticeId);
        if (item is not null) LocateItem(item.Id);
        else if (_state is not null)
        {
            var found = _state.Items.FirstOrDefault(i => i.NoticeId == noticeId);
            if (found is not null) LocateItem(found.Id);
        }
    }

    private void Rebuild()
    {
        if (_state is not null) ApplyState(_state, StateOrigin.Refresh);
    }

    private static void Sync<T>(ObservableCollection<T> collection, List<T> desired, Func<T, string> key)
    {
        var keep = new HashSet<string>(desired.Select(key), StringComparer.Ordinal);
        for (int i = collection.Count - 1; i >= 0; i--)
        {
            if (!keep.Contains(key(collection[i]))) collection.RemoveAt(i);
        }
        for (int i = 0; i < desired.Count; i++)
        {
            if (i < collection.Count && key(collection[i]) == key(desired[i])) continue;
            int existing = -1;
            for (int j = 0; j < collection.Count; j++)
            {
                if (key(collection[j]) == key(desired[i])) { existing = j; break; }
            }
            if (existing >= 0) collection.Move(existing, i);
            else collection.Insert(i, desired[i]);
        }
    }

    private void ScrollToItem(string id)
    {
        var row = FindDescendant<Border>(GroupList,
            b => b.DataContext is ScheduleItemViewModel vm && vm.Id == id);
        row?.StartBringIntoView();
    }

    private static T? FindDescendant<T>(DependencyObject root, Func<T, bool> predicate) where T : FrameworkElement
    {
        int count = VisualTreeHelper.GetChildrenCount(root);
        for (int i = 0; i < count; i++)
        {
            var child = VisualTreeHelper.GetChild(root, i);
            if (child is T typed && predicate(typed)) return typed;
            if (child is DependencyObject dep)
            {
                var found = FindDescendant(dep, predicate);
                if (found is not null) return found;
            }
        }
        return null;
    }

    private ScheduleItemViewModel? Find(string id) => _items.TryGetValue(id, out var vm) ? vm : null;

    // --------------------------- 手动新增 ---------------------------

    private void AddRemind_SelectionChanged(object sender, SelectionChangedEventArgs e)
    {
        string tag = (AddRemind.SelectedItem as ComboBoxItem)?.Tag as string ?? "1440,120";
        AddCustomRemind.Visibility = tag == "custom" ? Visibility.Visible : Visibility.Collapsed;
    }

    private void AddCancel_Click(object sender, RoutedEventArgs e)
    {
        AddForm.Visibility = Visibility.Collapsed;
        ShowAddError(null);
    }

    private void EmptyAdd_Click(object sender, RoutedEventArgs e) => HandleCommand("addItem");

    private void ShowAddError(string? message)
    {
        AddError.Text = message ?? string.Empty;
        AddError.Visibility = string.IsNullOrEmpty(message) ? Visibility.Collapsed : Visibility.Visible;
    }

    private async void AddSave_Click(object sender, RoutedEventArgs e)
    {
        string title = AddTitle.Text.Trim();
        string dueAt = Formatters.NormalizeDueInput(AddDue.Text);
        // 前置校验：空标题 / 非法时间不发请求（§9.1）。
        if (string.IsNullOrEmpty(title)) { ShowAddError("先写清楚要做的事"); return; }
        if (string.IsNullOrEmpty(dueAt)) { ShowAddError("时间格式不对，用 2026-11-05 14:30 这种"); return; }

        var remind = ParseRemindPreset();
        var payload = new Dictionary<string, object?>
        {
            ["title"] = title,
            ["dueAt"] = dueAt,
            ["note"] = Formatters.Truncate(AddNote.Text, 600),
            ["remindBefore"] = remind,
        };
        var response = await App.State.DispatchAsync(Ops.ItemAdd, payload);
        if (response?.Ok != true)
        {
            ShowAddError(response?.FailureText ?? "本地服务未响应");
            return;
        }

        AddTitle.Text = string.Empty;
        AddDue.Text = string.Empty;
        AddNote.Text = string.Empty;
        ShowAddError(null);
        AddForm.Visibility = Visibility.Collapsed;
        // v1.1 §13.3 / ux-spec §4.2：手动新增日程成功后**零提示**，
        // 只靠列表与 InfoBadge 计数变化；不给撤销条、不给「已加入日程」吐司。
    }

    private List<int> ParseRemindPreset()
    {
        string tag = (AddRemind.SelectedItem as ComboBoxItem)?.Tag as string ?? "1440,120";
        string raw = tag == "custom" ? AddCustomRemind.Text : tag;
        return ParseMinutes(raw);
    }

    private static List<int> ParseMinutes(string? raw)
    {
        return (raw ?? string.Empty)
            .Split(new[] { ',', '，', ' ', '\t' }, StringSplitOptions.RemoveEmptyEntries)
            .Select(part => int.TryParse(part.Trim(), out var n) && n >= 0 ? n : -1)
            .Where(n => n >= 0)
            .Distinct()
            .ToList();
    }

    // --------------------------- 行内交互 → 真实 op ---------------------------

    private async void Done_Click(object sender, RoutedEventArgs e)
    {
        if (sender is not CheckBox checkBox || checkBox.Tag is not string id) return;
        bool done = checkBox.IsChecked == true;
        await App.State.DispatchAsync(Ops.ItemDone,
            new Dictionary<string, object?> { ["id"] = id, ["done"] = done });
    }

    private void OpenNotice_Click(object sender, RoutedEventArgs e)
    {
        if (sender is FrameworkElement fe && fe.Tag is string id)
        {
            var url = Find(id)?.Model.Url;
            _ = Formatters.OpenUrlAsync(url);
        }
    }

    private void RowMore_Click(object sender, RoutedEventArgs e)
    {
        if (sender is not FrameworkElement fe || fe.Tag is not string id) return;
        var vm = Find(id);
        if (vm is null) return;

        var flyout = new MenuFlyout();
        flyout.Items.Add(MakeMenuItem("改时间", Icons.Edit, id, () =>
        {
            vm.IsEditing = true;
            vm.RefreshVisualState();
        }));
        if (vm.CanRefire)
        {
            flyout.Items.Add(MakeMenuItem("重置提醒", Icons.Clock, id, async () =>
                await App.State.DispatchAsync(Ops.ItemRefire,
                    new Dictionary<string, object?> { ["id"] = id })));
        }
        flyout.Items.Add(new MenuFlyoutSeparator());
        flyout.Items.Add(MakeMenuItem("删除", Icons.Delete, id, () => RemoveWithUndo(id, vm), destructive: true));
        flyout.ShowAt(fe, new Point(0, fe.ActualHeight));
    }

    private static MenuFlyoutItem MakeMenuItem(string text, string glyph, string id, Action action,
        bool destructive = false)
    {
        var item = new MenuFlyoutItem
        {
            Text = text,
            Tag = id,
            Icon = new FontIcon { Glyph = glyph, FontFamily = new FontFamily("Segoe Fluent Icons") },
        };
        if (destructive) item.Foreground = ThemeBrush.Get("NetworkErrorBrush");
        item.Click += (_, _) => action();
        return item;
    }

    private void RemoveWithUndo(string id, ScheduleItemViewModel vm)
    {
        vm.IsHidden = true;
        vm.RefreshVisualState();
        Rebuild();

        App.Window?.ScheduleDelete(Ops.ItemRemove, id, vm.Title, () =>
        {
            var restored = Find(id);
            if (restored is not null) { restored.IsHidden = false; restored.RefreshVisualState(); }
            Rebuild();     // 撤销全程零写请求
        });
    }

    private void EditCancel_Click(object sender, RoutedEventArgs e)
    {
        if (sender is FrameworkElement fe && fe.Tag is string id)
        {
            var vm = Find(id);
            if (vm is not null) { vm.IsEditing = false; vm.RefreshVisualState(); }
        }
    }

    private async void EditSave_Click(object sender, RoutedEventArgs e)
    {
        if (sender is not FrameworkElement fe || fe.Tag is not string id) return;
        var vm = Find(id);
        if (vm is null) return;

        // 只提交有变化的字段；改 dueAt / remindBefore 后端会清空 fired[]（自动重新武装提醒）。
        var patch = new Dictionary<string, object?> { ["id"] = id };
        if (!string.Equals(vm.EditTitle, vm.Model.Title, StringComparison.Ordinal))
            patch["title"] = vm.EditTitle;
        string due = Formatters.NormalizeDueInput(vm.EditDue);
        if (!string.IsNullOrEmpty(due) && !string.Equals(due, vm.Model.DueAt, StringComparison.Ordinal))
            patch["dueAt"] = due;
        var remind = ParseMinutes(vm.EditRemind);
        if (!remind.SequenceEqual(vm.Model.RemindBefore))
            patch["remindBefore"] = remind;

        var response = await App.State.DispatchAsync(Ops.ItemUpdate, patch);
        vm.IsEditing = false;
        vm.RefreshVisualState();
        if (response?.Ok != true)
            App.Window?.ShowWarning("修改未保存", response?.FailureText ?? "本地服务未响应");
    }
}
