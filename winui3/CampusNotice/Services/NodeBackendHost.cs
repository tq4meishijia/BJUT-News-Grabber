using System.Diagnostics;

namespace CampusNotice.Services;

/// <summary>
/// 自动托管 server.mjs（架构确认项 A3 · 「谁启动谁关闭」）。
///
/// 启动：
///   1. 先 GET /api/health；已健康（Web 版 / 另一客户端 / ctl.mjs 起的实例）→ **复用，不 spawn**，
///      避免 EADDRINUSE 导致 server exit(1)（server.mjs L356-360）。
///   2. 未健康 → spawn `node server.mjs --no-toast --no-open`（隐藏窗口，无 Origin 头问题）。
///      --no-toast 是本次唯一后端改动：后端 onNotify 只记日志并返回 true，
///      提醒仍写入 meta.reminders，系统 Toast 由客户端 AppNotification 单一通道发送。
///   3. 以 400ms 间隔轮询 health，15s 超时 → 抛 TimeoutException，UI 进入断连 Error + 「重试」。
///
/// 退出：仅当 OwnsServer == true 才 POST /api/shutdown；复用他实例服务时只解绑、不停止。
/// shutdown 超时/失败可降级放行（数据在每个写 op 后已逐次落盘）。
/// </summary>
public sealed class NodeBackendHost
{
    public const int StartupTimeoutSeconds = 15;
    private const int HealthIntervalMs = 400;

    private readonly ApiClient _api;
    private Process? _process;

    public NodeBackendHost(ApiClient api) => _api = api;

    /// <summary>true = 本实例 spawn 了 node，退出时才发 /api/shutdown。</summary>
    public bool OwnsServer { get; private set; }

    public event EventHandler<string>? Log;
    public event EventHandler? BackendExited;

    /// <summary>
    /// 定位仓库根目录（含 server.mjs）。
    /// 开发调试时 exe 在 winui3/CampusNotice/bin/.../net8.0-windows10.0.19041.0/，
    /// 需要从 AppContext.BaseDirectory 逐级向上找；也支持 CAMPUS_NOTICE_ROOT 环境变量显式指定。
    /// MSIX 打包部署时的限制见 docs/winui3-engineering.md「已知限制」。
    /// </summary>
    public static string? ResolveBackendRoot()
    {
        var candidates = new List<string>();
        string? env = Environment.GetEnvironmentVariable("CAMPUS_NOTICE_ROOT");
        if (!string.IsNullOrWhiteSpace(env)) candidates.Add(env);
        candidates.Add(Environment.CurrentDirectory);
        candidates.Add(AppContext.BaseDirectory);

        foreach (var start in candidates)
        {
            var dir = new DirectoryInfo(start);
            for (int i = 0; i < 8 && dir is not null; i++)
            {
                if (File.Exists(Path.Combine(dir.FullName, "server.mjs"))) return dir.FullName;
                dir = dir.Parent;
            }
        }
        return null;
    }

    public async Task<bool> EnsureAsync(CancellationToken ct = default)
    {
        if (await ProbeAsync(ct).ConfigureAwait(true))
        {
            OwnsServer = false;
            Log?.Invoke(this, "检测到已有健康的本地服务，复用它（本实例不负责关闭）。");
            return true;
        }

        string? root = ResolveBackendRoot();
        if (root is null)
        {
            Log?.Invoke(this, "找不到 server.mjs；请设置 CAMPUS_NOTICE_ROOT 指向仓库根目录。");
            return false;
        }

        if (!Spawn(root)) return false;

        var deadline = DateTime.UtcNow.AddSeconds(StartupTimeoutSeconds);
        while (DateTime.UtcNow < deadline && !ct.IsCancellationRequested)
        {
            if (await ProbeAsync(ct).ConfigureAwait(true))
            {
                OwnsServer = true;
                Log?.Invoke(this, "本地服务已就绪（127.0.0.1:8737）。");
                return true;
            }
            await Task.Delay(HealthIntervalMs, ct).ConfigureAwait(true);
        }

        // 超时：避免残留半启动的 node 进程；下次「重试」会重新探测/托管。
        Log?.Invoke(this, "本地服务 15s 内未就绪。");
        TryKillSpawned();
        return false;
    }

    /// <summary>
    /// 只做一次 health 探测，供 App 决定要不要展示「正在连接本地服务…」。
    /// v1.1 §13.2 / §7.12：命中已有健康服务并复用时**不得**闪一下启动态。
    /// </summary>
    public async Task<bool> ProbeHealthAsync(CancellationToken ct = default)
        => await ProbeAsync(ct).ConfigureAwait(true);

    private async Task<bool> ProbeAsync(CancellationToken ct)
    {
        try
        {
            using var cts = CancellationTokenSource.CreateLinkedTokenSource(ct);
            cts.CancelAfter(TimeSpan.FromMilliseconds(1500));
            var health = await _api.HealthAsync(cts.Token).ConfigureAwait(true);
            return health.Ok;
        }
        catch
        {
            return false;
        }
    }

    private bool Spawn(string root)
    {
        try
        {
            var psi = new ProcessStartInfo
            {
                FileName = "node",
                Arguments = "server.mjs --no-toast --no-open",
                WorkingDirectory = root,
                UseShellExecute = false,
                CreateNoWindow = true,
                WindowStyle = ProcessWindowStyle.Hidden,
                RedirectStandardOutput = true,
                RedirectStandardError = true,
                RedirectStandardInput = false,
            };
            // 不设置 Origin：HTTP 客户端侧已保证。
            _process = new Process { StartInfo = psi, EnableRaisingEvents = true };
            _process.OutputDataReceived += (_, e) => { if (!string.IsNullOrEmpty(e.Data)) Log?.Invoke(this, e.Data); };
            _process.ErrorDataReceived += (_, e) => { if (!string.IsNullOrEmpty(e.Data)) Log?.Invoke(this, e.Data); };
            _process.Exited += (_, _) =>
            {
                Log?.Invoke(this, "托管的本地服务进程已退出。");
                BackendExited?.Invoke(this, EventArgs.Empty);
            };

            if (!_process.Start()) return false;
            _process.BeginOutputReadLine();
            _process.BeginErrorReadLine();
            Log?.Invoke(this, $"已托管 server.mjs --no-toast --no-open（PID {_process.Id}）。");
            return true;
        }
        catch (Exception ex)
        {
            Log?.Invoke(this, $"启动本地服务失败：{ex.Message}");
            _process = null;
            return false;
        }
    }

    /// <summary>
    /// 优雅停止：仅本实例托管时发 shutdown；收到 ok 即可继续，再做 waitForExit 兜底。
    /// 复用他实例服务时返回 false，**不发 shutdown**。
    /// </summary>
    public async Task<bool> ShutdownAsync()
    {
        if (!OwnsServer)
        {
            Log?.Invoke(this, "本地服务由其他窗口托管，退出本客户端不影响后台抓取与提醒。");
            return false;
        }

        try
        {
            using var cts = new CancellationTokenSource(TimeSpan.FromSeconds(3));
            await _api.ShutdownAsync(cts.Token).ConfigureAwait(true);
        }
        catch (Exception ex)
        {
            // 降级放行：数据已在每个写 op 后落盘，不因关不掉而阻塞用户。
            Log?.Invoke(this, $"/api/shutdown 未成功（{ex.Message}），仍允许退出。");
        }

        try
        {
            if (_process is { HasExited: false })
            {
                await _process.WaitForExitAsync(
                    new CancellationTokenSource(TimeSpan.FromSeconds(2)).Token).ConfigureAwait(false);
            }
        }
        catch
        {
            TryKillSpawned();
        }
        return true;
    }

    private void TryKillSpawned()
    {
        try
        {
            if (_process is { HasExited: false }) _process.Kill(entireProcessTree: true);
        }
        catch
        {
            // 忽略：进程可能已经退出。
        }
        _process = null;
        OwnsServer = false;
    }
}
