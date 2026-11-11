using CampusNotice.Models;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;

namespace CampusNotice.Controls;

/// <summary>数据源表单共用的字段编辑器。显隐矩阵由 SourceViewModel 驱动。</summary>
public sealed partial class SourceEditor : UserControl
{
    private bool _syncing;

    public SourceEditor()
    {
        InitializeComponent();
        DataContextChanged += (_, _) => SyncTypeFromViewModel();
        Loaded += (_, _) => SyncTypeFromViewModel();
    }

    private SourceViewModel? ViewModel => DataContext as SourceViewModel;

    private void SyncTypeFromViewModel()
    {
        var vm = ViewModel;
        if (vm is null) return;
        _syncing = true;
        try
        {
            int index = vm.EditType switch { "rss" => 1, "json" => 2, _ => 0 };
            if (TypeCombo.SelectedIndex != index) TypeCombo.SelectedIndex = index;
            // 密码一律不回显：新增/编辑时 CookieBox 始终为空，用户输入后写回草稿。
            if (CookieBox.Password != string.Empty) CookieBox.Password = string.Empty;
        }
        finally
        {
            _syncing = false;
        }
    }

    private void CookieBox_PasswordChanged(object sender, RoutedEventArgs e)
    {
        if (_syncing) return;
        if (ViewModel is { } vm) vm.EditCookie = CookieBox.Password;
    }

    private void TypeCombo_SelectionChanged(object sender, SelectionChangedEventArgs e)
    {
        if (_syncing) return;
        var vm = ViewModel;
        if (vm is null) return;
        string tag = (TypeCombo.SelectedItem as ComboBoxItem)?.Tag as string ?? "html";
        // 隐藏字段保留已存原值，不渲染、保存时不显式传空串覆盖。
        vm.SetEditType(tag);
    }
}
