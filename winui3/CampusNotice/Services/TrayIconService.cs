using System.Runtime.InteropServices;
using CampusNotice.Helpers;
using CampusNotice.Models;
using Microsoft.UI.Dispatching;
using Microsoft.UI.Xaml;

namespace CampusNotice.Services;

/// <summary>
/// 系统托盘（视觉规范 §7.13 / UX 规范 §1.4）。
/// 用 Win32 Shell_NotifyIcon + 系统 TrackPopupMenuEx 菜单，**不自绘皮肤**。
/// 专用 STA 后台线程跑消息循环 + HWND_MESSAGE 隐藏窗口接收托盘回调。
///
/// 菜单 8 项（顺序固定，「退出」恒在最后）：
///   1 灰显标题「校园通知 · 日程 — 服务运行中 / 服务未连接」(不可点)
///   2 显示主窗口        3 立即抓取(poll)        4 通知 N 未读(N=0 隐藏)
///   5 日程 N 项今日到期(N=0 隐藏)              6 分隔线
///   7 关闭窗口（最小化到托盘，不发 shutdown）  8 退出
///
/// **硬约束：菜单里没有任何启动类入口（不写启动文件夹 / 注册表启动项 / 计划任务）。**
/// </summary>
public sealed class TrayIconService : IDisposable
{
    private const uint WM_USER = 0x0400;
    private const uint WM_TRAYICON = WM_USER + 1;
    private const uint WM_LBUTTONUP = 0x0202;
    private const uint WM_LBUTTONDBLCLK = 0x0203;
    private const uint WM_RBUTTONUP = 0x0205;
    private const uint WM_APP_TRAY_QUIT = 0x8001;

    private const int NIM_ADD = 0;
    private const int NIM_DELETE = 2;
    private const int NIF_MESSAGE = 0x01;
    private const int NIF_ICON = 0x02;
    private const int NIF_TIP = 0x04;

    private const uint MF_STRING = 0x0000;
    private const uint MF_GRAYED = 0x0001;
    private const uint MF_DISABLED = 0x0002;
    private const uint MF_SEPARATOR = 0x0800;

    private const uint TPM_RETURNCMD = 0x0100;
    private const uint TPM_RIGHTBUTTON = 0x0002;
    private const uint TPM_NONOTIFY = 0x0080;

    private const uint IMAGE_ICON = 1;
    private const uint LR_LOADFROMFILE = 0x0010;
    private static readonly IntPtr IDI_APPLICATION = new(32512);
    private static readonly IntPtr HWND_MESSAGE = new(-3);

    private const int IdTitle = 1001;
    private const int IdShow = 1002;
    private const int IdPoll = 1003;
    private const int IdNotices = 1004;
    private const int IdSchedule = 1005;
    private const int IdSeparator = 1006;
    private const int IdHide = 1007;
    private const int IdExit = 1008;

    [UnmanagedFunctionPointer(CallingConvention.Winapi)]
    private delegate IntPtr WndProcDelegate(IntPtr hWnd, uint msg, IntPtr wParam, IntPtr lParam);

    private static WndProcDelegate? _wndProcKeepAlive;

    private readonly DispatcherQueue _dispatcher;
    private Thread? _thread;
    private IntPtr _hwnd;
    private IntPtr _hIcon;
    private bool _iconAdded;
    private bool _disposed;

    // 由 Update() 从 UI 线程写入、托盘线程读取（简单标量，volatile 足够）。
    private volatile bool _connected;
    private volatile bool _polling;
    private volatile bool _windowVisible;
    private volatile int _unread;
    private volatile int _dueToday;

    public event EventHandler? ShowWindowRequested;
    public event EventHandler? PollRequested;
    public event EventHandler? ShowNoticesRequested;
    public event EventHandler? ShowScheduleRequested;
    public event EventHandler? HideWindowRequested;
    public event EventHandler? ExitRequested;

    public TrayIconService(DispatcherQueue dispatcher) => _dispatcher = dispatcher;

    public void Initialize()
    {
        if (_thread is not null) return;
        _thread = new Thread(MessageLoop) { IsBackground = true, Name = "CampusNotice.Tray" };
        try { _thread.SetApartmentState(ApartmentState.STA); } catch { /* 非 Windows 平台忽略 */ }
        _thread.Start();
    }

    /// <summary>由 MainWindow 在每次 state 刷新 / 连接状态变化时调用，更新菜单文案与禁用态。</summary>
    public void Update(AppState? state, bool connected, bool windowVisible)
    {
        _connected = connected;
        _windowVisible = windowVisible;
        if (state is null) return;
        _unread = state.UnreadCount;
        _dueToday = Formatters.DueTodayCount(state);
        _polling = state.Meta.Polling;
        UpdateTip();
    }

    public void SetPolling(bool polling) => _polling = polling;

    private void UpdateTip()
    {
        if (!_iconAdded || _hwnd == IntPtr.Zero) return;
        try
        {
            var data = MakeNotifyData();
            data.szTip = _connected ? "校园通知 · 日程 — 服务运行中" : "校园通知 · 日程 — 服务未连接";
            Shell_NotifyIcon(1 /* NIM_MODIFY */, ref data);
        }
        catch { /* 忽略 */ }
    }

    private void MessageLoop()
    {
        _wndProcKeepAlive = WndProc;
        var wndClass = new WNDCLASSEX
        {
            cbSize = Marshal.SizeOf<WNDCLASSEX>(),
            style = 0,
            lpfnWndProc = _wndProcKeepAlive,
            cbClsExtra = 0,
            cbWndExtra = 0,
            hInstance = GetModuleHandle(null),
            hIcon = IntPtr.Zero,
            hCursor = IntPtr.Zero,
            hbrBackground = IntPtr.Zero,
            lpszMenuName = string.Empty,
            lpszClassName = "CampusNoticeTrayWindow",
            hIconSm = IntPtr.Zero,
        };

        ushort atom = RegisterClassEx(ref wndClass);
        if (atom == 0) return;

        _hwnd = CreateWindowEx(0, "CampusNoticeTrayWindow", "CampusNoticeTray", 0,
            0, 0, 0, 0, HWND_MESSAGE, IntPtr.Zero, wndClass.hInstance, IntPtr.Zero);
        if (_hwnd == IntPtr.Zero) return;

        _hIcon = ResolveIcon();
        var notify = MakeNotifyData();
        notify.szTip = "校园通知 · 日程 — 服务连接中";
        notify.hIcon = _hIcon;
        _iconAdded = Shell_NotifyIcon(NIM_ADD, ref notify);

        while (GetMessage(out var msg, IntPtr.Zero, 0, 0) > 0)
        {
            TranslateMessage(ref msg);
            DispatchMessage(ref msg);
        }

        if (_iconAdded) Shell_NotifyIcon(NIM_DELETE, ref notify);
        if (_hIcon != IntPtr.Zero && _hIcon != IDI_APPLICATION) DestroyIcon(_hIcon);
        DestroyWindow(_hwnd);
    }

    private NOTIFYICONDATA MakeNotifyData() => new()
    {
        cbSize = Marshal.SizeOf<NOTIFYICONDATA>(),
        hWnd = _hwnd,
        uID = 1,
        uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP,
        uCallbackMessage = WM_TRAYICON,
        hIcon = _hIcon,
        szTip = string.Empty,
        szInfo = string.Empty,
        szInfoTitle = string.Empty,
    };

    private IntPtr WndProc(IntPtr hWnd, uint msg, IntPtr wParam, IntPtr lParam)
    {
        if (msg == WM_TRAYICON)
        {
            // 低字 = 鼠标消息；右键/左键单击弹系统菜单，双击显示主窗口。
            uint mouse = (uint)(lParam.ToInt64() & 0xFFFF);
            if (mouse is WM_RBUTTONUP or WM_LBUTTONUP) ShowContextMenu();
            else if (mouse == WM_LBUTTONDBLCLK) Raise(ShowWindowRequested);
            return IntPtr.Zero;
        }
        if (msg == WM_APP_TRAY_QUIT)
        {
            PostQuitMessage(0);
            return IntPtr.Zero;
        }
        return DefWindowProc(hWnd, msg, wParam, lParam);
    }

    private void ShowContextMenu()
    {
        IntPtr menu = CreatePopupMenu();
        if (menu == IntPtr.Zero) return;
        try
        {
            string title = _connected
                ? "校园通知 · 日程 — 服务运行中"
                : "校园通知 · 日程 — 服务未连接";
            AppendMenu(menu, MF_STRING | MF_GRAYED | MF_DISABLED, new IntPtr(IdTitle), title);

            AppendMenu(menu, MF_STRING | (_windowVisible ? MF_GRAYED | MF_DISABLED : 0),
                new IntPtr(IdShow), "显示主窗口");
            AppendMenu(menu, MF_STRING | (_polling || !_connected ? MF_GRAYED | MF_DISABLED : 0),
                new IntPtr(IdPoll), "立即抓取");

            if (_unread > 0) AppendMenu(menu, MF_STRING, new IntPtr(IdNotices), $"通知 {_unread} 未读");
            if (_dueToday > 0) AppendMenu(menu, MF_STRING, new IntPtr(IdSchedule), $"日程 {_dueToday} 项今日到期");

            AppendMenu(menu, MF_SEPARATOR, new IntPtr(IdSeparator), string.Empty);
            if (_windowVisible) AppendMenu(menu, MF_STRING, new IntPtr(IdHide), "关闭窗口（最小化到托盘）");
            AppendMenu(menu, MF_STRING, new IntPtr(IdExit), "退出");

            GetCursorPos(out var pt);
            SetForegroundWindow(_hwnd);
            int command = TrackPopupMenuEx(menu, TPM_RETURNCMD | TPM_RIGHTBUTTON | TPM_NONOTIFY,
                pt.X, pt.Y, _hwnd, IntPtr.Zero);
            // 让菜单在点击外部时能正常关闭。
            PostMessage(_hwnd, 0, IntPtr.Zero, IntPtr.Zero);

            switch (command)
            {
                case IdShow: Raise(ShowWindowRequested); break;
                case IdPoll: Raise(PollRequested); break;
                case IdNotices: Raise(ShowNoticesRequested); break;
                case IdSchedule: Raise(ShowScheduleRequested); break;
                case IdHide: Raise(HideWindowRequested); break;
                case IdExit: Raise(ExitRequested); break;
            }
        }
        finally
        {
            DestroyMenu(menu);
        }
    }

    private void Raise(EventHandler? handler)
    {
        if (handler is null) return;
        _dispatcher.TryEnqueue(() => handler.Invoke(this, EventArgs.Empty));
    }

    private static IntPtr ResolveIcon()
    {
        string name = Application.Current?.RequestedTheme == ApplicationTheme.Dark
            ? "TrayDark.ico"
            : "TrayLight.ico";
        var dir = new DirectoryInfo(AppContext.BaseDirectory);
        for (int i = 0; i < 6 && dir is not null; i++)
        {
            string candidate = Path.Combine(dir.FullName, "Assets", name);
            if (File.Exists(candidate))
            {
                var icon = LoadImage(IntPtr.Zero, candidate, IMAGE_ICON, 16, 16, LR_LOADFROMFILE);
                if (icon != IntPtr.Zero) return icon;
            }
            dir = dir.Parent;
        }
        // 资源缺失时回退系统图标，不崩溃（见 Assets/README.md）。
        return LoadIcon(IntPtr.Zero, IDI_APPLICATION);
    }

    public void Dispose()
    {
        if (_disposed) return;
        _disposed = true;
        if (_hwnd != IntPtr.Zero) PostMessage(_hwnd, WM_APP_TRAY_QUIT, IntPtr.Zero, IntPtr.Zero);
        _thread?.Join(TimeSpan.FromSeconds(1));
    }

    // --------------------------- Win32 interop ---------------------------

    [StructLayout(LayoutKind.Sequential)]
    private struct POINT { public int X; public int Y; }

    [StructLayout(LayoutKind.Sequential)]
    private struct MSG
    {
        public IntPtr hwnd;
        public uint message;
        public IntPtr wParam;
        public IntPtr lParam;
        public uint time;
        public POINT pt;
        public uint lPrivate;
    }

    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
    private struct NOTIFYICONDATA
    {
        public int cbSize;
        public IntPtr hWnd;
        public int uID;
        public int uFlags;
        public int uCallbackMessage;
        public IntPtr hIcon;
        [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 128)] public string szTip;
        public int dwState;
        public int dwStateMask;
        [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 256)] public string szInfo;
        public int uTimeoutOrVersion;
        [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 64)] public string szInfoTitle;
        public int dwInfoFlags;
        public Guid guidItem;
        public IntPtr hBalloonIcon;
    }

    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
    private struct WNDCLASSEX
    {
        public int cbSize;
        public int style;
        public WndProcDelegate lpfnWndProc;
        public int cbClsExtra;
        public int cbWndExtra;
        public IntPtr hInstance;
        public IntPtr hIcon;
        public IntPtr hCursor;
        public IntPtr hbrBackground;
        public string lpszMenuName;
        public string lpszClassName;
        public IntPtr hIconSm;
    }

    [DllImport("shell32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    private static extern bool Shell_NotifyIcon(int dwMessage, ref NOTIFYICONDATA lpData);

    [DllImport("user32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    private static extern ushort RegisterClassEx(ref WNDCLASSEX lpwcx);

    [DllImport("user32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    private static extern IntPtr CreateWindowEx(int dwExStyle, string lpClassName, string lpWindowName,
        int dwStyle, int x, int y, int nWidth, int nHeight, IntPtr hWndParent, IntPtr hMenu,
        IntPtr hInstance, IntPtr lpParam);

    [DllImport("user32.dll")]
    private static extern IntPtr DefWindowProc(IntPtr hWnd, uint uMsg, IntPtr wParam, IntPtr lParam);

    [DllImport("user32.dll")]
    private static extern bool DestroyWindow(IntPtr hWnd);

    [DllImport("user32.dll")]
    private static extern int GetMessage(out MSG lpMsg, IntPtr hWnd, uint wMsgFilterMin, uint wMsgFilterMax);

    [DllImport("user32.dll")]
    private static extern bool TranslateMessage(ref MSG lpMsg);

    [DllImport("user32.dll")]
    private static extern IntPtr DispatchMessage(ref MSG lpMsg);

    [DllImport("user32.dll")]
    private static extern void PostQuitMessage(int nExitCode);

    [DllImport("user32.dll")]
    private static extern bool PostMessage(IntPtr hWnd, uint Msg, IntPtr wParam, IntPtr lParam);

    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    private static extern IntPtr CreatePopupMenu();

    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    private static extern bool AppendMenu(IntPtr hMenu, uint uFlags, IntPtr uIDNewItem, string lpNewItem);

    [DllImport("user32.dll")]
    private static extern int TrackPopupMenuEx(IntPtr hMenu, uint uFlags, int x, int y, IntPtr hwnd, IntPtr lptpm);

    [DllImport("user32.dll")]
    private static extern bool DestroyMenu(IntPtr hMenu);

    [DllImport("user32.dll")]
    private static extern bool GetCursorPos(out POINT lpPoint);

    [DllImport("user32.dll")]
    private static extern bool SetForegroundWindow(IntPtr hWnd);

    [DllImport("user32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    private static extern IntPtr LoadImage(IntPtr hInst, string lpszName, uint uType,
        int cxDesired, int cyDesired, uint fuLoad);

    [DllImport("user32.dll")]
    private static extern IntPtr LoadIcon(IntPtr hInstance, IntPtr lpIconName);

    [DllImport("user32.dll")]
    private static extern bool DestroyIcon(IntPtr hIcon);

    [DllImport("kernel32.dll", CharSet = CharSet.Unicode)]
    private static extern IntPtr GetModuleHandle(string? lpModuleName);
}
