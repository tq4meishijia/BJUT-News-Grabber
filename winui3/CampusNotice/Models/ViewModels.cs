using System.Collections.ObjectModel;
using System.ComponentModel;
using System.Runtime.CompilerServices;
using System.Text;
using CampusNotice.Helpers;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media;
using Windows.UI.Text;

namespace CampusNotice.Models;

public abstract class ObservableObject : INotifyPropertyChanged
{
    public event PropertyChangedEventHandler? PropertyChanged;

    protected bool Set<T>(ref T field, T value, [CallerMemberName] string? name = null)
    {
        if (EqualityComparer<T>.Default.Equals(field, value)) return false;
        field = value;
        OnPropertyChanged(name);
        return true;
    }

    protected void OnPropertyChanged([CallerMemberName] string? name = null) =>
        PropertyChanged?.Invoke(this, new PropertyChangedEventArgs(name));
}

/// <summary>
/// 通知卡片视图模型。state 刷新时**就地更新属性**而不是重建整个 ItemsSource，
/// 以保留焦点与滚动位置（UX §8.3 针对 Web 版 render() 全量重建的痛点）。
/// </summary>
public sealed class NoticeViewModel : ObservableObject
{
    private string _id = string.Empty;
    private string _title = string.Empty;
    private string _sourceName = string.Empty;
    private string _publishedAt = string.Empty;
    private string _summary = string.Empty;
    private string _content = string.Empty;
    private bool _read;
    private int? _score;
    private string _badgeText = "未评分";
    private string _scoreText = string.Empty;
    private string _bandText = "未评分";
    private string _automationName = "未评分";
    private ScoreShape _shape = ScoreShape.None;
    private Brush _fillBrush = ThemeBrush.Get("ScoreNoneFillBrush");
    private Brush _foregroundBrush = ThemeBrush.Get("ScoreNoneForegroundBrush");
    private Brush _subtleBrush = ThemeBrush.Get("ScoreNoneSubtleBrush");
    private string _shapeText = "无";
    private string _metaLine = string.Empty;
    private string _usefulText = string.Empty;
    private string _tagsText = string.Empty;
    private string _deadlineText = string.Empty;
    private string _contactText = string.Empty;
    private string _aiText = string.Empty;
    private string _readButtonText = "标已读";
    private string _inlineError = string.Empty;
    private string _scheduleButtonText = "加入日程";
    private bool _isExpanded;
    private bool _showUseful;
    private bool _isHidden;
    private bool _isSummarizing;
    private bool _alreadyScheduled;
    private bool _hasBody;
    private bool _hasUrl;

    public Notice Model { get; private set; } = new();
    public string Id { get => _id; set => Set(ref _id, value); }
    public string Title { get => _title; set => Set(ref _title, value); }
    public string SourceName { get => _sourceName; set => Set(ref _sourceName, value); }
    public string PublishedAt { get => _publishedAt; set => Set(ref _publishedAt, value); }
    public string Summary { get => _summary; set => Set(ref _summary, value); }
    public string Content { get => _content; set => Set(ref _content, value); }
    public bool Read { get => _read; set => Set(ref _read, value); }
    public int? Score { get => _score; set => Set(ref _score, value); }
    public string BadgeText { get => _badgeText; set => Set(ref _badgeText, value); }
    public string ScoreText { get => _scoreText; set => Set(ref _scoreText, value); }
    public string BandText { get => _bandText; set => Set(ref _bandText, value); }
    public string AutomationName { get => _automationName; set => Set(ref _automationName, value); }
    public ScoreShape Shape { get => _shape; set => Set(ref _shape, value); }
    public Brush FillBrush { get => _fillBrush; set => Set(ref _fillBrush, value); }
    public Brush ForegroundBrush { get => _foregroundBrush; set => Set(ref _foregroundBrush, value); }
    public Brush SubtleBrush { get => _subtleBrush; set => Set(ref _subtleBrush, value); }

    /// <summary>形状的文字冗余（高对比度保底）：实心圆 / 实心菱形 / 空心圆 / 小方点 / 无。</summary>
    public string ShapeText { get => _shapeText; set => Set(ref _shapeText, value); }

    public string MetaLine { get => _metaLine; set => Set(ref _metaLine, value); }
    public string UsefulText { get => _usefulText; set => Set(ref _usefulText, value); }
    public string TagsText { get => _tagsText; set => Set(ref _tagsText, value); }
    public string DeadlineText { get => _deadlineText; set => Set(ref _deadlineText, value); }
    public string ContactText { get => _contactText; set => Set(ref _contactText, value); }
    public string AiText { get => _aiText; set => Set(ref _aiText, value); }
    public string ReadButtonText { get => _readButtonText; set => Set(ref _readButtonText, value); }
    public string InlineError { get => _inlineError; set => Set(ref _inlineError, value); }
    public string ScheduleButtonText { get => _scheduleButtonText; set => Set(ref _scheduleButtonText, value); }
    public bool IsExpanded { get => _isExpanded; set => Set(ref _isExpanded, value); }
    public bool ShowUseful { get => _showUseful; set => Set(ref _showUseful, value); }
    public bool IsHidden { get => _isHidden; set => Set(ref _isHidden, value); }
    public bool IsSummarizing { get => _isSummarizing; set => Set(ref _isSummarizing, value); }
    public bool AlreadyScheduled { get => _alreadyScheduled; set => Set(ref _alreadyScheduled, value); }
    public bool HasBody { get => _hasBody; set => Set(ref _hasBody, value); }
    public bool HasUrl { get => _hasUrl; set => Set(ref _hasUrl, value); }

    public Visibility BodyVisibility => IsExpanded && HasBody ? Visibility.Visible : Visibility.Collapsed;
    public Visibility ReadStripVisibility => !Read ? Visibility.Visible : Visibility.Collapsed;
    public Visibility InlineErrorVisibility => string.IsNullOrEmpty(InlineError) ? Visibility.Collapsed : Visibility.Visible;

    /// <summary>用后端返回的 Notice + 全局 state 就地刷新；不重建对象即可保留焦点。</summary>
    public void Apply(Notice model, AppState state)
    {
        Model = model;
        var visual = ScorePalette.For(model.Score);

        Id = model.Id;
        Title = model.Title;
        SourceName = model.SourceName;
        PublishedAt = model.PublishedAt;
        Summary = model.Summary;
        Content = model.Content;
        Read = model.Read;
        Score = model.Score;
        BadgeText = visual.BadgeText;
        ScoreText = visual.ScoreText;
        BandText = visual.BandText;
        AutomationName = $"{model.Title}，{visual.BandText} {visual.ScoreText} 分，{(model.Read ? "已读" : "未读")}，来源 {model.SourceName}，{model.PublishedAt}";
        Shape = visual.Shape;
        ShapeText = ShapeTextOf(visual.Shape);
        FillBrush = visual.FillBrush;
        ForegroundBrush = visual.ForegroundBrush;
        SubtleBrush = visual.SubtleBrush;

        var meta = new StringBuilder();
        meta.Append(model.SourceName);
        if (!string.IsNullOrEmpty(model.PublishedAt)) meta.Append(" · ").Append(model.PublishedAt);
        MetaLine = meta.ToString();

        UsefulText = model.Useful is { Count: > 0 }
            ? string.Join("\n", model.Useful.Take(3).Select(x => "· " + x))
            : string.Empty;
        TagsText = model.Tags is { Count: > 0 }
            ? string.Join(" · ", model.Tags.Take(4)) + (model.Tags.Count > 4 ? $" · +{model.Tags.Count - 4}" : string.Empty)
            : string.Empty;
        DeadlineText = string.IsNullOrEmpty(model.Deadline) ? string.Empty : "截止 " + Formatters.FormatDueShort(model.Deadline);
        ContactText = string.IsNullOrEmpty(model.Contact) ? string.Empty : "找谁办 " + model.Contact;
        AiText = model.Ai ? "AI 摘要" : string.Empty;
        ReadButtonText = model.Read ? "标未读" : "标已读";
        HasBody = model.HasBody;
        HasUrl = model.HasUrl;
        AlreadyScheduled = state.Items.Any(i => !string.IsNullOrEmpty(model.Id) && i.NoticeId == model.Id);
        ScheduleButtonText = AlreadyScheduled ? "已在日程中 · 查看" : "加入日程";

        OnPropertyChanged(nameof(BodyVisibility));
        OnPropertyChanged(nameof(ReadStripVisibility));
        OnPropertyChanged(nameof(InlineErrorVisibility));
    }

    public void RefreshVisualState()
    {
        OnPropertyChanged(nameof(BodyVisibility));
        OnPropertyChanged(nameof(ReadStripVisibility));
        OnPropertyChanged(nameof(InlineErrorVisibility));
    }

    public static string ShapeTextOf(ScoreShape shape) => shape switch
    {
        ScoreShape.SolidCircle => "实心圆",
        ScoreShape.SolidDiamond => "实心菱形",
        ScoreShape.HollowCircle => "空心圆",
        ScoreShape.SmallSquare => "小方点",
        _ => "无"
    };
}

/// <summary>日程分组（顺序恒为 已逾期→今天→明天→本周内→更远→未定时间→(已完成)）。</summary>
public sealed class ScheduleGroupViewModel : ObservableObject
{
    private string _name = string.Empty;
    private int _count;
    private Brush _headerBrush = ThemeBrush.Get("GroupFarBrush");
    private Brush _headerFill = ThemeBrush.Get("GroupFarBrush");

    public string Name { get => _name; set => Set(ref _name, value); }
    public int Count { get => _count; set => Set(ref _count, value); }
    public Brush HeaderBrush { get => _headerBrush; set => Set(ref _headerBrush, value); }
    public Brush HeaderFill { get => _headerFill; set => Set(ref _headerFill, value); }
    public string AutomationName => $"{Name}，共 {Count} 项";
    public ObservableCollection<ScheduleItemViewModel> Items { get; } = new();
}

public sealed class ScheduleItemViewModel : ObservableObject
{
    private string _id = string.Empty;
    private string _title = string.Empty;
    private string _note = string.Empty;
    private bool _done;
    private string _timeText = "—";
    private string _countdownText = string.Empty;
    private string _remindText = "无提醒";
    private string _groupName = string.Empty;
    private string _automationName = string.Empty;
    private string _checkBoxName = string.Empty;
    private string _deadlineForegroundKey = "DeadlineFarBrush";
    private string _deadlineSubtleKey = "DeadlineFarSubtleBrush";
    private bool _fromNotice;
    private bool _hasUrl;
    private bool _canRefire;
    private bool _isEditing;
    private bool _isHidden;
    private double _opacity = 1.0;
    private TextDecorations _textDecorations = TextDecorations.None;
    private string _doneText = string.Empty;

    public ScheduleItem Model { get; private set; } = new();
    public string Id { get => _id; set => Set(ref _id, value); }
    public string Title { get => _title; set => Set(ref _title, value); }
    public string Note { get => _note; set => Set(ref _note, value); }
    public bool Done { get => _done; set => Set(ref _done, value); }
    public string TimeText { get => _timeText; set => Set(ref _timeText, value); }
    public string CountdownText { get => _countdownText; set => Set(ref _countdownText, value); }
    public string RemindText { get => _remindText; set => Set(ref _remindText, value); }
    public string GroupName { get => _groupName; set => Set(ref _groupName, value); }
    public string AutomationName { get => _automationName; set => Set(ref _automationName, value); }
    public string CheckBoxName { get => _checkBoxName; set => Set(ref _checkBoxName, value); }
    public string DeadlineForegroundKey { get => _deadlineForegroundKey; set => Set(ref _deadlineForegroundKey, value); }
    public string DeadlineSubtleKey { get => _deadlineSubtleKey; set => Set(ref _deadlineSubtleKey, value); }
    public bool FromNotice { get => _fromNotice; set => Set(ref _fromNotice, value); }
    public bool HasUrl { get => _hasUrl; set => Set(ref _hasUrl, value); }
    public bool CanRefire { get => _canRefire; set => Set(ref _canRefire, value); }
    public bool IsEditing { get => _isEditing; set => Set(ref _isEditing, value); }
    public bool IsHidden { get => _isHidden; set => Set(ref _isHidden, value); }
    public double Opacity { get => _opacity; set => Set(ref _opacity, value); }
    public TextDecorations TextDecorations { get => _textDecorations; set => Set(ref _textDecorations, value); }

    /// <summary>「已完成」文字：普通主题与高对比度都必须出现，Opacity 不承载语义（§4.6）。</summary>
    public string DoneText { get => _doneText; set => Set(ref _doneText, value); }

    public Visibility FromNoticeVisibility => FromNotice ? Visibility.Visible : Visibility.Collapsed;
    public Visibility UrlVisibility => HasUrl ? Visibility.Visible : Visibility.Collapsed;
    public Visibility RefireVisibility => CanRefire ? Visibility.Visible : Visibility.Collapsed;
    public Visibility DoneTextVisibility => Done ? Visibility.Visible : Visibility.Collapsed;
    public Visibility EditVisibility => IsEditing ? Visibility.Visible : Visibility.Collapsed;
    public Visibility HiddenVisibility => IsHidden ? Visibility.Collapsed : Visibility.Visible;

    // 内联编辑草稿（改时间 / 标题 / 提醒档位）
    public string EditTitle { get; set; } = string.Empty;
    public string EditDue { get; set; } = string.Empty;
    public string EditRemind { get; set; } = string.Empty;

    public void Apply(ScheduleItem model, string group, bool highContrast)
    {
        Model = model;
        Id = model.Id;
        Title = model.Title;
        Note = model.Note;
        Done = model.Done;
        GroupName = group;
        TimeText = string.IsNullOrEmpty(model.DueAt) ? "—" : Formatters.FormatDue(model.DueAt);
        CountdownText = Formatters.CountdownText(model.DueAt);
        RemindText = Formatters.RemindText(model.RemindBefore);
        FromNotice = model.FromNotice;
        HasUrl = model.HasUrl;
        CanRefire = model.CanRefire;
        DeadlineForegroundKey = Formatters.DeadlineBrushKey(model.DueAt);
        DeadlineSubtleKey = DeadlineForegroundKey.Replace("Brush", "SubtleBrush");

        // 完成态 = 删除线 + 「已完成」文字 + Regular 字重；高对比度禁用 Opacity 表意。
        DoneText = model.Done ? "已完成" : string.Empty;
        TextDecorations = model.Done ? TextDecorations.Strikethrough : TextDecorations.None;
        Opacity = highContrast ? 1.0 : (model.Done ? 0.55 : 1.0);

        CheckBoxName = model.Done ? $"将「{model.Title}」标记为未完成" : $"将「{model.Title}」标记为完成";
        AutomationName = model.Done
            ? $"{model.Title}，已完成，到期 {TimeText}，{group}"
            : $"{model.Title}，{group}，到期 {TimeText}，{CountdownText}，{RemindText}，未完成{(model.FromNotice ? "，来自通知" : string.Empty)}";

        if (!IsEditing)
        {
            EditTitle = model.Title;
            EditDue = model.DueAt;
            EditRemind = string.Join(",", model.RemindBefore ?? new List<int>());
        }

        OnPropertyChanged(nameof(FromNoticeVisibility));
        OnPropertyChanged(nameof(UrlVisibility));
        OnPropertyChanged(nameof(RefireVisibility));
        OnPropertyChanged(nameof(DoneTextVisibility));
        OnPropertyChanged(nameof(EditVisibility));
        OnPropertyChanged(nameof(HiddenVisibility));
    }

    /// <summary>本地 UI 状态（编辑 / 隐藏）变化后刷新只读可见性属性。</summary>
    public void RefreshVisualState()
    {
        OnPropertyChanged(nameof(EditVisibility));
        OnPropertyChanged(nameof(HiddenVisibility));
    }
}

/// <summary>数据源卡片视图模型（含 test preview 与内联编辑草稿）。</summary>
public sealed class SourceViewModel : ObservableObject
{
    private string _id = string.Empty;
    private string _name = string.Empty;
    private string _type = "html";
    private string _url = string.Empty;
    private string _note = string.Empty;
    private bool _enabled;
    private bool _hasCookie;
    private bool _loginExpired;
    private string _status = "idle";
    private string _statusText = string.Empty;
    private string _statusLabel = "尚未抓取";
    private string _statusBrushKey = "SourceStatusIdleBrush";
    private string _statusSubtleKey = "SourceStatusIdleSubtleBrush";
    private Brush _statusBrush = ThemeBrush.Get("SourceStatusIdleBrush");
    private Brush _statusSubtleBrush = ThemeBrush.Get("SourceStatusIdleSubtleBrush");
    private string _statusIcon = Icons.Clock;
    private string _footerMeta = string.Empty;
    private string _automationName = string.Empty;
    private string _inlineError = string.Empty;
    private string _previewMessage = string.Empty;
    private bool _showPreview;
    private bool _isTesting;
    private bool _isEditing;
    private double _opacity = 1.0;

    public SourceItem Model { get; private set; } = new();
    public string Id { get => _id; set => Set(ref _id, value); }
    public string Name { get => _name; set => Set(ref _name, value); }
    public string Type { get => _type; set => Set(ref _type, value); }
    public string Url { get => _url; set => Set(ref _url, value); }
    public string Note { get => _note; set => Set(ref _note, value); }
    public bool Enabled { get => _enabled; set => Set(ref _enabled, value); }
    public bool HasCookie { get => _hasCookie; set => Set(ref _hasCookie, value); }
    public bool LoginExpired { get => _loginExpired; set => Set(ref _loginExpired, value); }
    public string Status { get => _status; set => Set(ref _status, value); }
    public string StatusText { get => _statusText; set => Set(ref _statusText, value); }
    public string StatusLabel { get => _statusLabel; set => Set(ref _statusLabel, value); }
    public string StatusBrushKey { get => _statusBrushKey; set => Set(ref _statusBrushKey, value); }
    public string StatusSubtleKey { get => _statusSubtleKey; set => Set(ref _statusSubtleKey, value); }
    public Brush StatusBrush { get => _statusBrush; set => Set(ref _statusBrush, value); }
    public Brush StatusSubtleBrush { get => _statusSubtleBrush; set => Set(ref _statusSubtleBrush, value); }
    public string StatusIcon { get => _statusIcon; set => Set(ref _statusIcon, value); }
    public string FooterMeta { get => _footerMeta; set => Set(ref _footerMeta, value); }
    public string AutomationName { get => _automationName; set => Set(ref _automationName, value); }
    public string InlineError { get => _inlineError; set => Set(ref _inlineError, value); }
    public string PreviewMessage { get => _previewMessage; set => Set(ref _previewMessage, value); }
    public bool ShowPreview { get => _showPreview; set => Set(ref _showPreview, value); }
    public bool IsTesting { get => _isTesting; set => Set(ref _isTesting, value); }
    public bool IsEditing { get => _isEditing; set => Set(ref _isEditing, value); }
    public double Opacity { get => _opacity; set => Set(ref _opacity, value); }

    public ObservableCollection<PreviewItem> Preview { get; } = new();

    public Visibility LoginExpiredVisibility => LoginExpired ? Visibility.Visible : Visibility.Collapsed;
    public Visibility CookieOkVisibility => HasCookie && !LoginExpired ? Visibility.Visible : Visibility.Collapsed;
    public Visibility NoCookieVisibility => !HasCookie && !LoginExpired ? Visibility.Visible : Visibility.Collapsed;
    public Visibility PreviewVisibility => ShowPreview ? Visibility.Visible : Visibility.Collapsed;
    public Visibility InlineErrorVisibility => string.IsNullOrEmpty(InlineError) ? Visibility.Collapsed : Visibility.Visible;
    public Visibility EditVisibility => IsEditing ? Visibility.Visible : Visibility.Collapsed;
    public Visibility HtmlFieldsVisibility => Type == "html" ? Visibility.Visible : Visibility.Collapsed;
    public Visibility JsonFieldsVisibility => Type == "json" ? Visibility.Visible : Visibility.Collapsed;
    public Visibility PagingVisibility => Type is "html" or "json" ? Visibility.Visible : Visibility.Collapsed;

    // 内联编辑草稿（type 显隐矩阵见 UX §5.2）
    public string EditName { get; set; } = string.Empty;
    public string EditUrl { get; set; } = string.Empty;
    public string EditType { get; set; } = "html";
    public string EditCookie { get; set; } = string.Empty; // PasswordBox，留空 = 不修改
    public string EditResolveIp { get; set; } = string.Empty;
    public string EditLinkPattern { get; set; } = string.Empty;
    public string EditItemsPath { get; set; } = string.Empty;
    public string EditPages { get; set; } = "1";
    public string EditPageUrl { get; set; } = string.Empty;
    public string EditUrlTemplate { get; set; } = string.Empty;
    public string EditDetailUrlTemplate { get; set; } = string.Empty;
    public string EditContentPath { get; set; } = string.Empty;
    public string EditNote { get; set; } = string.Empty;
    public string EditMap { get; set; } = string.Empty;

    /// <summary>切换解析方式：隐藏字段保留原值、不渲染、不参与空串覆盖（UX §5.2 R2）。</summary>
    public void SetEditType(string type)
    {
        EditType = type;
        Type = type;   // 显隐由 Type 驱动
        OnPropertyChanged(nameof(HtmlFieldsVisibility));
        OnPropertyChanged(nameof(JsonFieldsVisibility));
        OnPropertyChanged(nameof(PagingVisibility));
        OnPropertyChanged(nameof(EditVisibility));
    }

    public void LoadDraftFromModel()
    {
        EditName = Name;
        EditUrl = Url;
        EditType = Type;
        EditCookie = string.Empty; // 绝不回显已存 Cookie
        EditResolveIp = Model.ResolveIp;
        EditLinkPattern = Model.LinkPattern;
        EditItemsPath = Model.ItemsPath;
        EditPages = Model.Pages.ToString();
        EditPageUrl = Model.PageUrl;
        EditUrlTemplate = Model.UrlTemplate;
        EditDetailUrlTemplate = Model.DetailUrlTemplate;
        EditContentPath = Model.ContentPath;
        EditNote = Note;
        EditMap = Model.Map.HasValue ? Model.Map.Value.GetRawText() : string.Empty;
        OnPropertyChanged(nameof(HtmlFieldsVisibility));
        OnPropertyChanged(nameof(JsonFieldsVisibility));
        OnPropertyChanged(nameof(PagingVisibility));
    }

    public void Apply(SourceItem model)
    {
        Model = model;
        Id = model.Id;
        Name = model.Name;
        Type = model.Type;
        Url = model.Url;
        Note = model.Note;
        Enabled = model.Enabled;
        HasCookie = model.HasCookie;
        LoginExpired = model.LoginExpired;
        Status = model.Status;
        StatusText = model.StatusText;

        (StatusLabel, StatusBrushKey, StatusSubtleKey, StatusIcon) = model.Status switch
        {
            "ok" => ($"正常 · {model.LastCount} 条", "SourceStatusOkBrush", "SourceStatusOkSubtleBrush", Icons.Completed),
            "error" => ($"失败 · {Formatters.Truncate(model.StatusText, 24)}", "SourceStatusErrorBrush", "SourceStatusErrorSubtleBrush", Icons.ErrorBadge),
            _ => ("尚未抓取", "SourceStatusIdleBrush", "SourceStatusIdleSubtleBrush", Icons.Clock)
        };

        FooterMeta = $"上次 {Formatters.ShortTime(model.LastFetchAt)} · {model.LastCount} 条";
        StatusBrush = ThemeBrush.Get(StatusBrushKey);
        StatusSubtleBrush = ThemeBrush.Get(StatusSubtleKey);
        Opacity = model.Enabled ? 1.0 : 0.6; // 装饰性降权；Name 里必须含「已停用」
        AutomationName = $"数据源 {model.Name}，{model.Type}，{(model.Enabled ? "已启用" : "已停用")}，状态{StatusLabel}{(model.LoginExpired ? "，Cookie 已失效" : string.Empty)}";

        OnPropertyChanged(nameof(LoginExpiredVisibility));
        OnPropertyChanged(nameof(CookieOkVisibility));
        OnPropertyChanged(nameof(NoCookieVisibility));
        OnPropertyChanged(nameof(PreviewVisibility));
        OnPropertyChanged(nameof(InlineErrorVisibility));
        OnPropertyChanged(nameof(EditVisibility));
    }

    /// <summary>本地 UI 状态（编辑 / 预览 / 错误）变化后刷新只读可见性属性。</summary>
    public void RefreshVisualState()
    {
        OnPropertyChanged(nameof(LoginExpiredVisibility));
        OnPropertyChanged(nameof(CookieOkVisibility));
        OnPropertyChanged(nameof(NoCookieVisibility));
        OnPropertyChanged(nameof(PreviewVisibility));
        OnPropertyChanged(nameof(InlineErrorVisibility));
        OnPropertyChanged(nameof(EditVisibility));
        OnPropertyChanged(nameof(HtmlFieldsVisibility));
        OnPropertyChanged(nameof(JsonFieldsVisibility));
        OnPropertyChanged(nameof(PagingVisibility));
    }
}

/// <summary>
/// 全局 InfoBar + 状态行的唯一状态载体（UX §7.3）。
/// 同一时刻最多 1 条，按 Error &gt; Warning &gt; Informational &gt; Success 择一。
/// </summary>
public sealed class GlobalStatus : ObservableObject
{
    private bool _isOpen;
    private InfoBarSeverity _severity = InfoBarSeverity.Informational;
    private string _title = string.Empty;
    private string _message = string.Empty;
    private bool _isClosable = true;
    private string _actionText = string.Empty;
    private string _statusLine = "正在连接本地服务…";
    private bool _polling;

    public bool IsOpen { get => _isOpen; set => Set(ref _isOpen, value); }
    public InfoBarSeverity Severity { get => _severity; set => Set(ref _severity, value); }
    public string Title { get => _title; set => Set(ref _title, value); }
    public string Message { get => _message; set => Set(ref _message, value); }
    public bool IsClosable { get => _isClosable; set => Set(ref _isClosable, value); }
    public string ActionText { get => _actionText; set => Set(ref _actionText, value); }
    public string StatusLine { get => _statusLine; set => Set(ref _statusLine, value); }
    public bool Polling { get => _polling; set => Set(ref _polling, value); }
    public Visibility ActionVisibility => string.IsNullOrEmpty(ActionText) ? Visibility.Collapsed : Visibility.Visible;

    /// <summary>InfoBar 动作按钮的回调；只保留一个，切换页面/状态时覆盖。</summary>
    public Action? Action { get; set; }
}
