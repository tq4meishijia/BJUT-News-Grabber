using CampusNotice.Helpers;
using CampusNotice.Models;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Automation;

namespace CampusNotice.Controls;

/// <summary>
/// 重要度徽标。唯一输入是 Score（int?）；档位、颜色、形状、讲述人文本全部由
/// Helpers/ScorePalette.cs 计算，避免各页各自硬编码阈值与色值。
/// 阈值：70 / 50 / 35；「重要」筛选阈值 = 50（D1）。
/// </summary>
public sealed partial class ScoreBadge : UserControl
{
    public static readonly DependencyProperty ScoreProperty = DependencyProperty.Register(
        nameof(Score), typeof(int?), typeof(ScoreBadge),
        new PropertyMetadata(null, OnScoreChanged));

    public ScoreBadge()
    {
        InitializeComponent();
        Update(null);
    }

    public int? Score
    {
        get => (int?)GetValue(ScoreProperty);
        set => SetValue(ScoreProperty, value);
    }

    private static void OnScoreChanged(DependencyObject d, DependencyPropertyChangedEventArgs e)
        => ((ScoreBadge)d).Update(e.NewValue as int?);

    private void Update(int? score)
    {
        var visual = ScorePalette.For(score);
        Root.Background = visual.SubtleBrush;
        Label.Foreground = visual.ForegroundBrush;
        Label.Text = visual.BadgeText;

        var fill = visual.FillBrush;
        SolidCircle.Background = fill;
        SolidDiamond.Background = fill;
        HollowCircle.BorderBrush = fill;
        SmallSquare.Background = fill;

        SolidCircle.Visibility = Visibility.Collapsed;
        SolidDiamond.Visibility = Visibility.Collapsed;
        HollowCircle.Visibility = Visibility.Collapsed;
        SmallSquare.Visibility = Visibility.Collapsed;

        switch (visual.Shape)
        {
            case ScoreShape.SolidCircle: SolidCircle.Visibility = Visibility.Visible; break;
            case ScoreShape.SolidDiamond: SolidDiamond.Visibility = Visibility.Visible; break;
            case ScoreShape.HollowCircle: HollowCircle.Visibility = Visibility.Visible; break;
            case ScoreShape.SmallSquare: SmallSquare.Visibility = Visibility.Visible; break;
            case ScoreShape.None: break;
        }

        // 完整、自足的讲述人文本；高对比度下颜色被覆盖时，文字 + 形状是唯一保底。
        AutomationProperties.SetName(this, visual.AutomationName);
        AutomationProperties.SetHelpText(this,
            visual.BandText == "未评分" ? "未评分" : $"重要度 {visual.ScoreText} 分，{visual.BandText}，形状 {NoticeViewModel.ShapeTextOf(visual.Shape)}");
    }
}
