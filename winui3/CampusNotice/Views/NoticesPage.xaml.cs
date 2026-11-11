using System.Collections.ObjectModel;
using CampusNotice.Controls;
using CampusNotice.Helpers;
using CampusNotice.Models;
using CampusNotice.Services;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;

namespace CampusNotice.Views;

/// <summary>
/// 通知页（UX §3 / §10.9.1）。
/// * 三态筛选「重要」阈值 = score >= 50（D1，与 Web 版 55 是有意差异），三处明示。
/// * 列表增量同步：state 刷新时**就地更新 ViewModel 属性**，只在实际增删时才动
///   ObservableCollection，保留焦点与滚动位置（§8.3 针对 Web 版全量重建的痛点）。
/// * 移除 = 8s 延迟提交 + TeachingTip 撤销（撤销窗内零写请求）。
/// </summary>
public sealed partial class NoticesPage : Page, IPageView
{
    public ObservableCollection<NoticeViewModel> ViewModels { get; } = new();

    private readonly Dictionary<string, NoticeViewModel> _all = new(StringComparer.Ordinal);
    private AppState? _state;
    private string _filter = "all";
    private bool _compact;

    public NoticesPage()
    {
        InitializeComponent();
        NoticeList.ItemsSource = ViewModels;
        NoticeCard.Command += OnCardCommand;
        _filter = LocalPrefs.Current.NoticeFilter is "all" or "important" or "unread"
            ? LocalPrefs.Current.NoticeFilter
            : "all";
        UpdateFilterButtons();
    }

    // --------------------------- IPageView ---------------------------

    public void ApplyState(AppState state, StateOrigin origin)
    {
        _state = state;
        var next = new Dictionary<string, NoticeViewModel>(StringComparer.Ordinal);
        foreach (var notice in state.Notices)
        {
            if (_all.TryGetValue(notice.Id, out var vm)) vm.Apply(notice, state);
            else
            {
                vm = new NoticeViewModel();
                vm.Apply(notice, state);
            }
            next[notice.Id] = vm;
        }
        // 撤销窗口内被本地隐藏的卡片要保留引用，撤销后可原地恢复。
        foreach (var pair in _all)
        {
            if (!next.ContainsKey(pair.Key) && pair.Value.IsHidden) next[pair.Key] = pair.Value;
        }
        _all.Clear();
        foreach (var pair in next) _all[pair.Key] = pair.Value;

        FilterAll.Content = $"全部 {state.Notices.Count}";
        FilterImportant.Content = $"重要 {state.ImportantCount}";
        FilterUnread.Content = $"未读 {state.UnreadCount}";
        ReadAllButton.IsEnabled = state.Notices.Count > 0;

        SyncCollection();
        UpdateEmptyState(state);
        LimitHint.Visibility = state.Notices.Count >= 200 ? Visibility.Visible : Visibility.Collapsed;
    }

    public void ApplyFilter(string filter)
    {
        if (filter is not ("all" or "important" or "unread")) return;
        _filter = filter;
        LocalPrefs.Current.NoticeFilter = filter;
        LocalPrefs.Current.Save();
        UpdateFilterButtons();
        SyncCollection();
        if (_state is not null) UpdateEmptyState(_state);
    }

    public void ApplyCompact(bool compact)
    {
        _compact = compact;
        PageRoot.MaxWidth = compact ? double.PositiveInfinity : 940;
        PageRoot.Padding = compact ? new Thickness(12, 16, 12, 24) : new Thickness(24, 8, 24, 32);
        // <640：命令栏次要项由 MainWindow 的 CommandBar 溢出承担；本页隐藏次要 meta。
        ImportantExplanation.Visibility = compact
            ? Visibility.Collapsed
            : (_filter == "important" ? Visibility.Visible : Visibility.Collapsed);
    }

    public void OnPageActivated()
    {
        UpdateFilterButtons();
        SyncCollection();
    }

    public void HandleCommand(string command)
    {
        if (command == "escape")
        {
            foreach (var vm in _all.Values)
            {
                if (!vm.IsExpanded) continue;
                vm.IsExpanded = false;
                vm.RefreshVisualState();
            }
        }
    }

    // --------------------------- 列表增量同步 ---------------------------

    private IEnumerable<NoticeViewModel> Desired()
    {
        if (_state is null) return Enumerable.Empty<NoticeViewModel>();
        return _state.Notices
            .Where(notice => _filter switch
            {
                "important" => ScorePalette.IsImportant(notice),   // score >= 50，唯一判定入口
                "unread" => !notice.Read,
                _ => true
            })
            .Select(notice => _all.TryGetValue(notice.Id, out var vm) ? vm : null)
            .Where(vm => vm is not null && !vm!.IsHidden)
            .Select(vm => vm!);
    }

    private void SyncCollection()
    {
        var desired = Desired().ToList();
        var keep = new HashSet<string>(desired.Select(v => v.Id), StringComparer.Ordinal);

        for (int i = ViewModels.Count - 1; i >= 0; i--)
        {
            if (!keep.Contains(ViewModels[i].Id)) ViewModels.RemoveAt(i);
        }

        for (int i = 0; i < desired.Count; i++)
        {
            if (i < ViewModels.Count && ViewModels[i].Id == desired[i].Id) continue;
            int existing = -1;
            for (int j = 0; j < ViewModels.Count; j++)
            {
                if (ViewModels[j].Id == desired[i].Id) { existing = j; break; }
            }
            if (existing >= 0) ViewModels.Move(existing, i);
            else ViewModels.Insert(i, desired[i]);
        }
    }

    private void UpdateEmptyState(AppState state)
    {
        bool empty = ViewModels.Count == 0;
        EmptyState.Visibility = empty ? Visibility.Visible : Visibility.Collapsed;
        if (!empty) return;

        bool noData = state.Notices.Count == 0;
        EmptyTitle.Text = noData ? "还没有抓到通知。" : "当前筛选下没有通知。";
        EmptyBody.Text = noData
            ? "先到「数据源」启用或添加一个源，再点右上角「立即抓取」。"
            : "切回「全部」看看，或调整筛选条件。";
        EmptyGoSources.Visibility = noData ? Visibility.Visible : Visibility.Collapsed;
        EmptyPoll.Visibility = noData ? Visibility.Visible : Visibility.Collapsed;
        EmptyShowAll.Visibility = noData ? Visibility.Collapsed : Visibility.Visible;
    }

    private void UpdateFilterButtons()
    {
        FilterAll.IsChecked = _filter == "all";
        FilterImportant.IsChecked = _filter == "important";
        FilterUnread.IsChecked = _filter == "unread";
        ImportantExplanation.Visibility = _filter == "important" && !_compact
            ? Visibility.Visible
            : Visibility.Collapsed;
    }

    private NoticeViewModel? Find(string id) => _all.TryGetValue(id, out var vm) ? vm : null;

    // --------------------------- 交互 → 真实 op ---------------------------

    private void Filter_Click(object sender, RoutedEventArgs e)
    {
        if (sender is ToggleButton button && button.Tag is string tag) ApplyFilter(tag);
    }

    private void EmptyGoSources_Click(object sender, RoutedEventArgs e) => App.Window?.NavigateTo("sources");
    private void EmptyPoll_Click(object sender, RoutedEventArgs e) => _ = App.Window?.TriggerPollAsync();
    private void EmptyShowAll_Click(object sender, RoutedEventArgs e) => ApplyFilter("all");

    private async void ReadAll_Click(object sender, RoutedEventArgs e)
    {
        var ids = _state?.Notices.Where(n => !n.Read).Select(n => n.Id).ToList() ?? new List<string>();
        if (ids.Count == 0) return;
        var response = await App.State.DispatchAsync(Ops.NoticeReadAll);
        if (response?.Ok != true) return;

        // 无反向批量 op：缓存未读 id 集合，撤销时逐条 notice.read{read:false}（§9.2）。
        // 已提交型（readAll 立刻生效）→ 用 8s 底部撤销条，Commit = null；撤销由 Undo 回放。
        App.Window?.PushUndo(
            $"已将 {ids.Count} 条通知标为已读",
            async () =>
            {
                foreach (var id in ids)
                {
                    await App.State.DispatchAsync(Ops.NoticeRead,
                        new Dictionary<string, object?> { ["id"] = id, ["read"] = false });
                }
            },
            onCommit: null,
            multiLabel: "已批量标记 N 项");
    }

    private async void OnCardCommand(object? sender, NoticeCardCommandEventArgs e)
    {
        var vm = Find(e.Id);
        if (vm is null) return;

        switch (e.Command)
        {
            case NoticeCardCommand.OpenOriginal:
            {
                string url = vm.Model.Url;
                await App.State.DispatchAsync(Ops.NoticeRead,
                    new Dictionary<string, object?> { ["id"] = e.Id, ["read"] = true });
                await Formatters.OpenUrlAsync(url);
                break;
            }
            case NoticeCardCommand.ToggleRead:
                await App.State.DispatchAsync(Ops.NoticeRead,
                    new Dictionary<string, object?> { ["id"] = e.Id, ["read"] = !vm.Read });
                break;

            case NoticeCardCommand.ToggleBody:
                vm.IsExpanded = !vm.IsExpanded;
                vm.RefreshVisualState();
                break;

            case NoticeCardCommand.Summarize:
                await SummarizeAsync(e.Id);
                break;

            case NoticeCardCommand.Remove:
                RemoveWithUndo(e.Id, vm);
                break;

            case NoticeCardCommand.AddSchedule:
                await AddScheduleAsync(vm);
                break;
        }
    }

    private async Task SummarizeAsync(string id)
    {
        // AI 不可用：不升级为全局 Error，只给 Informational 说明（D2 原则 / §3.3）。
        if (_state?.Settings.Llm == false || _state?.Meta.LlmKeySet == false)
        {
            var target = Find(id);
            if (target is not null) target.InlineError = "未配置 API Key，当前使用规则摘要；可到设置页配置";
            return;
        }

        var before = Find(id);
        if (before is not null) { before.IsSummarizing = true; before.InlineError = string.Empty; }
        var response = await App.State.DispatchAsync(Ops.NoticeSummarize,
            new Dictionary<string, object?> { ["id"] = id });
        var vm = Find(id);
        if (vm is not null) vm.IsSummarizing = false;

        if (response is null) return;
        if (!response.Ok)
        {
            // 失败：保留原摘要/score，只在该卡内联显示原因 + 可重试；不弹全局 Error。
            if (vm is not null) vm.InlineError = response.FailureText + " · 可再次点「AI 重摘要」重试";
        }
        else if (vm is not null)
        {
            vm.InlineError = string.Empty;
        }
    }

    private void RemoveWithUndo(string id, NoticeViewModel vm)
    {
        vm.IsHidden = true;
        SyncCollection();
        if (_state is not null) UpdateEmptyState(_state);

        App.Window?.ScheduleDelete(Ops.NoticeRemove, id, vm.Title, () =>
        {
            var restored = Find(id);
            if (restored is not null) restored.IsHidden = false;
            SyncCollection();                 // 全程零写请求
            if (_state is not null) UpdateEmptyState(_state);
        });
    }

    private async Task AddScheduleAsync(NoticeViewModel vm)
    {
        if (vm.AlreadyScheduled)
        {
            App.Window?.NavigateToScheduleByNoticeId(vm.Id);
            return;
        }

        var notice = vm.Model;
        bool hasDeadline = !string.IsNullOrWhiteSpace(notice.Deadline);
        string dueAt = hasDeadline ? notice.Deadline : Formatters.TomorrowNine();

        if (!hasDeadline)
        {
            // 提交前告知兜底时间（§3.4），用户可取消。
            App.Window?.ShowTeachingTip(
                "未识别到截止时间",
                "默认设为明天 09:00，加入后可到日程页「改时间」。",
                null, "仍然加入", () => _ = CommitAddScheduleAsync(notice.Id, notice, dueAt, true),
                "取消", null, seconds: 0, lightDismiss: false);
        }
        else
        {
            await CommitAddScheduleAsync(notice.Id, notice, dueAt, false);
        }
    }

    private async Task CommitAddScheduleAsync(string noticeId, Notice notice, string dueAt, bool noDeadline)
    {
        var payload = new Dictionary<string, object?>
        {
            ["title"] = Formatters.Truncate(notice.Title, 60),
            ["note"] = $"来自 {notice.SourceName}：{notice.Summary ?? string.Empty}",
            ["dueAt"] = dueAt,
            ["source"] = "notice",
            ["noticeId"] = noticeId,
            ["url"] = notice.Url ?? string.Empty,
        };
        var response = await App.State.DispatchAsync(Ops.ItemAdd, payload);
        if (response?.Ok != true)
        {
            App.Window?.ShowWarning("加入日程失败", response?.FailureText ?? "本地服务未响应");
            return;
        }

        // v1.1 §13.3 / ux-spec §3.4 方案 1：item.add 属「已提交 + 有反向 op」，
        // **不给撤销条、不给「查看日程」按钮**；只用 §7.9 Success InfoBar（自动 4–6s、可关闭）
        // + 返回 state 整体刷新；导航靠日程页 InfoBadge / 计数变化。
        // 返回的 id 仍保存在 response.Id（后端真值），用户反悔时到日程页删除，删除本身走 8s 撤销条。
        string suffix = noDeadline ? "（未识别到截止时间，默认明天 09:00，可到日程页改时间）" : string.Empty;
        App.Window?.ShowSuccess("已加入日程" + suffix);
    }
}
