using Windows.UI.StartScreen;

namespace CampusNotice.Services;

/// <summary>
/// 任务栏跳转列表（视觉规范 §7.13 / UX 规范 §1.5）：分组「常用」3 项。
///   「通知」  → nav:notices   （保留上次筛选）
///   「日程」  → nav:schedule
///   「立即抓取」→ nav:poll    （断连时只激活窗口显示 Error，不重复触发 poll）
/// **不含退出项、不含任何启动类入口。**
///
/// ⚠️ 已知限制（见 docs/winui3-engineering.md / winui3-build-and-run.md）：
/// Windows.UI.StartScreen.JumpList 需要**包标识**。默认 WindowsPackageType=None（unpackaged）
/// 运行时 LoadCurrentAsync() 会抛 COMException（无包标识），此时本服务降级为 no-op，
/// 三个入口仅通过系统托盘提供；把项目改为 MSIX 打包并在 VS 里以打包方式启动后即可生效。
/// 这是 WinRT API 的固有限制，本实现不虚构「unpackaged 也支持」的承诺。
/// </summary>
public sealed class JumpListService
{
    public const string ArgNotices = "nav:notices";
    public const string ArgSchedule = "nav:schedule";
    public const string ArgPoll = "nav:poll";

    public bool IsSupported { get; private set; }
    public string? LastError { get; private set; }

    public async Task InitializeAsync()
    {
        try
        {
            var jumpList = await JumpList.LoadCurrentAsync();
            jumpList.SystemGroupKind = JumpListSystemGroupKind.None;
            jumpList.Items.Clear();

            var notices = JumpListItem.CreateWithArguments(ArgNotices, "通知");
            notices.Description = "打开通知页，保留上次筛选";
            notices.GroupName = "常用";

            var schedule = JumpListItem.CreateWithArguments(ArgSchedule, "日程");
            schedule.Description = "打开日程页";
            schedule.GroupName = "常用";

            var poll = JumpListItem.CreateWithArguments(ArgPoll, "立即抓取");
            poll.Description = "立刻抓取一次通知";
            poll.GroupName = "常用";

            jumpList.Items.Add(notices);
            jumpList.Items.Add(schedule);
            jumpList.Items.Add(poll);

            await jumpList.SaveAsync();
            IsSupported = true;
        }
        catch (Exception ex)
        {
            // unpackaged：无包标识，跳转列表不可用。静默降级，不弹错误、不阻断启动。
            IsSupported = false;
            LastError = ex.Message;
            System.Diagnostics.Debug.WriteLine($"[JumpListService] 跳转列表不可用：{ex.Message}");
        }
    }
}
