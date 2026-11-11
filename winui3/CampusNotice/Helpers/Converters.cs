using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Data;

namespace CampusNotice.Helpers;

/// <summary>XAML 绑定转换器集合。全部在 Converters.xaml 资源之外以代码方式实例化也可，
/// 这里随 Themes 之外提供，供各页 Page.Resources 里声明后使用。</summary>

public sealed class BoolToVisibilityConverter : IValueConverter
{
    public bool Invert { get; set; }
    public object Convert(object value, Type targetType, object parameter, string language)
    {
        bool flag = value is bool b && b;
        if (Invert) flag = !flag;
        return flag ? Visibility.Visible : Visibility.Collapsed;
    }
    public object ConvertBack(object value, Type targetType, object parameter, string language) =>
        value is Visibility v && v == Visibility.Visible;
}

public sealed class InverseBoolConverter : IValueConverter
{
    public object Convert(object value, Type targetType, object parameter, string language) =>
        value is not bool b || !b;
    public object ConvertBack(object value, Type targetType, object parameter, string language) =>
        value is not bool b || !b;
}

/// <summary>非空/非零字符串 → Visible。用于空态、说明行、内联错误、preview 区。</summary>
public sealed class StringToVisibilityConverter : IValueConverter
{
    public bool Invert { get; set; }
    public object Convert(object value, Type targetType, object parameter, string language)
    {
        bool has = !string.IsNullOrWhiteSpace(value as string);
        if (Invert) has = !has;
        return has ? Visibility.Visible : Visibility.Collapsed;
    }
    public object ConvertBack(object value, Type targetType, object parameter, string language) =>
        throw new NotSupportedException();
}

public sealed class IntToVisibilityConverter : IValueConverter
{
    public object Convert(object value, Type targetType, object parameter, string language) =>
        value is int n && n > 0 ? Visibility.Visible : Visibility.Collapsed;
    public object ConvertBack(object value, Type targetType, object parameter, string language) =>
        throw new NotSupportedException();
}

/// <summary>
/// 停用数据源整卡 Opacity = 0.6（视觉规范 §7.7）。
/// 注意：Opacity **不承载语义**，AutomationProperties.Name 必须同时含「已停用」（§9.2）。
/// </summary>
public sealed class EnabledToOpacityConverter : IValueConverter
{
    public object Convert(object value, Type targetType, object parameter, string language) =>
        value is bool enabled && enabled ? 1.0 : 0.6;
    public object ConvertBack(object value, Type targetType, object parameter, string language) =>
        throw new NotSupportedException();
}

/// <summary>完成态装饰性降权（普通主题 Opacity 0.55）；高对比度由代码禁用，改删除线 + 文字。</summary>
public sealed class DoneToOpacityConverter : IValueConverter
{
    public object Convert(object value, Type targetType, object parameter, string language) =>
        value is bool done && done ? 0.55 : 1.0;
    public object ConvertBack(object value, Type targetType, object parameter, string language) =>
        throw new NotSupportedException();
}

public sealed class CountToBadgeConverter : IValueConverter
{
    public object Convert(object value, Type targetType, object parameter, string language)
    {
        int n = value is int i ? i : 0;
        return n > 99 ? "99+" : n.ToString();
    }
    public object ConvertBack(object value, Type targetType, object parameter, string language) =>
        throw new NotSupportedException();
}

public sealed class StringFormatConverter : IValueConverter
{
    public object Convert(object value, Type targetType, object parameter, string language) =>
        string.Format(parameter as string ?? "{0}", value);
    public object ConvertBack(object value, Type targetType, object parameter, string language) =>
        throw new NotSupportedException();
}
