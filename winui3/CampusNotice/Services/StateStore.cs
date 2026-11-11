using CampusNotice.Models;
using Microsoft.UI.Xaml;

namespace CampusNotice.Services;

public enum StateOrigin
{
    /// <summary>启动后首次 GET /api/state。</summary>
    Startup,
    /// <summary>10s 静默刷新 GET /api/state。</summary>
    Poll,
    /// <summary>POST /api/action 回传的 state（唯一真值）。</summary>
    Action,
    /// <summary>手动重试 / 重新连接后的 GET /api/state。</summary>
    Reconnect,
    /// <summary>通用 GET /api/state 刷新。</summary>
    Refresh
}

public sealed class StateChangedEventArgs : EventArgs
{
    public required AppState State { get; init; }
    public required long Version { get; init; }
    public required StateOrigin Origin { get; init; }
    public ActionResponse? Response { get; init; }
}

/// <summary>
/// 全局单一数据源（架构确认项 A2）。
/// * 所有 op 回传全量 state（含 ok:false），成功后与失败后**都**应用 response.State。
/// * 附加单调版本号：只应用版本更新的 state，防止静默刷新覆盖较新的写操作结果。
/// * 写操作在途期间暂停合并静默刷新（R6），并串行化所有写请求（A4）。
/// * 唯一 10s 轮询 /api/state，兼顾 UI 静默刷新与 meta.reminders 增量检测；
///   后端 checkReminders 每 15s 独立运行属服务端事实，客户端不另起提醒轮询；
///   reminder.id 持久且 state 保留前 20 条，不会漏提醒。
/// </summary>
public sealed class StateStore
{
    public const int PollIntervalSeconds = 10;

    private readonly ApiClient _api;
    private readonly DispatcherTimer _pollTimer;
    private readonly SemaphoreSlim _writeGate = new(1, 1);
    private long _version;
    private bool _writeInFlight;
    private bool _pollingEnabled;

    public StateStore(ApiClient api)
    {
        _api = api;
        // DispatcherTimer 必须在 UI 线程构造（App.OnLaunched 中）。
        _pollTimer = new DispatcherTimer { Interval = TimeSpan.FromSeconds(PollIntervalSeconds) };
        _pollTimer.Tick += OnPollTick;
    }

    public AppState? Current { get; private set; }
    public long Version => _version;
    public bool IsConnected { get; private set; }
    public bool WriteInFlight => _writeInFlight;
    public bool PollingEnabled => _pollingEnabled;

    public event EventHandler<StateChangedEventArgs>? StateChanged;
    public event EventHandler<bool>? ConnectionChanged;
    public event EventHandler<ActionResponse>? ActionCompleted;

    private async void OnPollTick(object? sender, object e)
    {
        if (!_pollingEnabled || _writeInFlight || !IsConnected) return;
        await RefreshAsync(StateOrigin.Poll);
    }

    public void StartPolling()
    {
        _pollingEnabled = true;
        if (!_pollTimer.IsEnabled) _pollTimer.Start();
    }

    public void StopPolling()
    {
        _pollingEnabled = false;
        if (_pollTimer.IsEnabled) _pollTimer.Stop();
    }

    /// <summary>暂停/恢复静默刷新合并（写操作在途期间）。</summary>
    public void PausePolling() => _pollingEnabled = false;
    public void ResumePolling() => _pollingEnabled = true;

    public async Task<bool> RefreshAsync(StateOrigin origin = StateOrigin.Refresh, CancellationToken ct = default)
    {
        try
        {
            var state = await Wrap(_api.GetStateAsync(ct));
            ApplyState(state, origin, null);
            SetConnected(true);
            return true;
        }
        catch
        {
            SetConnected(false);
            return false;
        }
    }

    /// <summary>
    /// 发起一个写 op。请求在途串行化 + 暂停静默刷新；无论 ok:true / ok:false，
    /// 只要响应带 state 就整体刷新；请求异常（无 state）才进入断连流程。
    /// </summary>
    public async Task<ActionResponse?> DispatchAsync(
        string op,
        Dictionary<string, object?>? parameters = null,
        CancellationToken ct = default)
    {
        await _writeGate.WaitAsync(ct).ConfigureAwait(true);
        bool wasPolling = _pollingEnabled;
        _writeInFlight = true;
        PausePolling();
        try
        {
            ActionResponse response;
            try
            {
                response = await Wrap(_api.PostActionAsync(op, parameters, ct));
                SetConnected(true);
            }
            catch (Exception ex) when (ex is ApiException or OperationCanceledException or HttpRequestException)
            {
                SetConnected(false);
                return null;
            }

            // 失败也必须应用返回的 state（失败常伴随 source.status='error' / lastLogin / lastLlm 更新）。
            if (response.State is not null)
                ApplyState(response.State, StateOrigin.Action, response);
            ActionCompleted?.Invoke(this, response);
            return response;
        }
        finally
        {
            _writeInFlight = false;
            if (wasPolling) ResumePolling();
            _writeGate.Release();
        }
    }

    private void ApplyState(AppState state, StateOrigin origin, ActionResponse? response)
    {
        Current = state;
        long next = Interlocked.Increment(ref _version);
        StateChanged?.Invoke(this, new StateChangedEventArgs
        {
            State = state,
            Version = next,
            Origin = origin,
            Response = response,
        });
    }

    private void SetConnected(bool connected)
    {
        if (IsConnected == connected) return;
        IsConnected = connected;
        ConnectionChanged?.Invoke(this, connected);
    }

    /// <summary>把底层网络/解析异常统一包装，便于上层只捕获 ApiException。</summary>
    private static async Task<T> Wrap<T>(Task<T> task)
    {
        try
        {
            return await task.ConfigureAwait(true);
        }
        catch (OperationCanceledException)
        {
            throw new ApiException("连接超时");
        }
        catch (HttpRequestException ex)
        {
            throw new ApiException("无法连接本地服务", ex);
        }
        catch (System.Text.Json.JsonException ex)
        {
            throw new ApiException("服务返回了无法解析的数据", ex);
        }
        catch (ApiException)
        {
            throw;
        }
        catch (Exception ex)
        {
            throw new ApiException(ex.Message, ex);
        }
    }
}
