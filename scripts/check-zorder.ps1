#Requires -Version 5.1
<#
.SYNOPSIS
    rust-sticky-notes 窗口层级回归断言（一键、无人工、失败返回非 0）。

.DESCRIPTION
    先用 --dump / --check 把"窗口层级"变成 JSON，再对下面 5 个场景逐条断言：

      1. 基线      ：便签数 ≥ 2，6 条不变量全部通过
      2. 前台覆盖  ：把普通应用（记事本）拉到前台，未置顶便签不得压住它
      3. 切置顶    ：--set-pin auto true/false，置顶生效、且未置顶那张仍被前台覆盖
      4. 兄弟不连坐：两张一钉一不钉，反复切 2 轮，未置顶那张的 z 始终在前台之后
      5. 重启一致性：关进程 → 重启 → 置顶状态与 store 完全一致

    全程不靠人眼看截图，判定只用 dump / check 里的数值。
    结束时清理：关掉自己启动的记事本、恢复改动过的置顶状态、删除自己建的临时便签。

.PARAMETER Exe
    被断言的 exe。默认 <仓库>\src-tauri\target\release\rust-sticky-notes.exe

.PARAMETER TimeoutSec
    单次命令等待结果文件的超时（秒）。默认 10。

.PARAMETER SkipRestart
    跳过"重启一致性"场景（不想动正在跑的应用时用）。

.PARAMETER KeepArtifacts
    保留 %TEMP% 下的 sticky-dump.json / sticky-check.*（默认失败时也会保留）。

.EXAMPLE
    pwsh -File scripts\check-zorder.ps1

.EXAMPLE
    pwsh -File scripts\check-zorder.ps1 -Exe 'D:\build\rust-sticky-notes.exe' -SkipRestart
#>
[CmdletBinding()]
param(
    [string]$Exe = '',
    [int]$TimeoutSec = 10,
    [switch]$SkipRestart,
    [switch]$KeepArtifacts
)

$ErrorActionPreference = 'Stop'
$script:Results = New-Object System.Collections.ArrayList
$script:StickyExe = $null
$script:CreatedNoteId = $null
$script:ForegroundProc = $null
$script:ForegroundProcIsNew = $false
$script:OriginalPins = @{}
$script:ChangedPins = @{}
$script:TempDir = [System.IO.Path]::GetTempPath()
$script:DumpPath = Join-Path $script:TempDir 'sticky-dump.json'
$script:CheckJson = Join-Path $script:TempDir 'sticky-check.json'
$script:CheckTxt = Join-Path $script:TempDir 'sticky-check.txt'
$script:StartedByUs = $false
$script:Clock = [System.Diagnostics.Stopwatch]::StartNew()

# ---------------------------------------------------------------- 输出

function Write-Step {
    param([string]$Text)
    Write-Host ""
    Write-Host ("== " + $Text) -ForegroundColor Cyan
}

function Add-Result {
    param([string]$Name, [bool]$Ok, [string]$Detail = '')
    $tag = '[FAIL]'
    if ($Ok) { $tag = '[PASS]' }
    $line = '{0} {1}' -f $tag, $Name
    if ($Detail) { $line += '  ' + $Detail }
    if ($Ok) {
        Write-Host $line -ForegroundColor Green
    } else {
        Write-Host $line -ForegroundColor Red
    }
    [void]$script:Results.Add([pscustomobject]@{ Name = $Name; Ok = $Ok; Detail = $Detail })
}

# ---------------------------------------------------------------- Win32（把窗口拉到前台）

if (-not ('StickyCheckNative' -as [type])) {
    Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;

public static class StickyCheckNative
{
    [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr hWnd, IntPtr lpdwProcessId);
    [DllImport("kernel32.dll")] public static extern uint GetCurrentThreadId();
    [DllImport("user32.dll")] public static extern bool AttachThreadInput(uint idAttach, uint idAttachTo, bool fAttach);
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr hWnd);
    [DllImport("user32.dll")] public static extern bool BringWindowToTop(IntPtr hWnd);
    [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr hWnd, int nCmdShow);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern int GetClassNameW(IntPtr hWnd, System.Text.StringBuilder buf, int len);

    // 后台进程默认没有"允许前台"的权限，先 AttachThreadInput 到当前前台线程再抢，
    // 这是脚本能在不点鼠标的情况下把记事本拉到前台的关键。
    public static bool ForceForeground(IntPtr target)
    {
        IntPtr fg = GetForegroundWindow();
        if (fg == target) return true;
        uint fgThread = GetWindowThreadProcessId(fg, IntPtr.Zero);
        uint myThread = GetCurrentThreadId();
        bool attached = false;
        if (fgThread != 0 && fgThread != myThread) attached = AttachThreadInput(myThread, fgThread, true);
        ShowWindow(target, 9); // SW_RESTORE
        SetForegroundWindow(target);
        BringWindowToTop(target);
        if (attached) AttachThreadInput(myThread, fgThread, false);
        return GetForegroundWindow() == target;
    }

    public static IntPtr Foreground() { return GetForegroundWindow(); }

    public static string ClassOf(IntPtr h)
    {
        var buf = new System.Text.StringBuilder(256);
        GetClassNameW(h, buf, 256);
        return buf.ToString();
    }
}
'@
}

# ---------------------------------------------------------------- 基础工具

function Get-StickyProcesses {
    $name = [System.IO.Path]::GetFileNameWithoutExtension($script:StickyExe)
    return @(Get-Process -Name $name -ErrorAction SilentlyContinue)
}

function Get-Stamp {
    param([string]$Path)
    $item = Get-Item -LiteralPath $Path -ErrorAction SilentlyContinue
    if ($null -eq $item) { return '<missing>' }
    return ('{0}:{1}' -f $item.LastWriteTimeUtc.Ticks, $item.Length)
}

function Wait-StampChanged {
    param([string]$Path, [string]$Before, [int]$Timeout = $TimeoutSec)
    $deadline = (Get-Date).AddSeconds($Timeout)
    while ($true) {
        if ((Get-Stamp $Path) -ne $Before) { return $true }
        if ((Get-Date) -ge $deadline) { return $false }
        Start-Sleep -Milliseconds 25
    }
}

# 启动一条诊断命令（不等待它退出）。
#
# 必须用 ShellExecute（UseShellExecute=true）：否则子进程会继承本脚本的 stdout 句柄，
# 于是应用不退出，`... | Select-Object`、`Tee-Object` 这类管道就永远等不到 EOF。
function Start-StickyProcess {
    param([string[]]$Arguments)
    $quoted = @()
    foreach ($arg in $Arguments) {
        if ($arg -match '[\s"]') { $quoted += ('"{0}"' -f $arg) } else { $quoted += $arg }
    }
    $info = New-Object System.Diagnostics.ProcessStartInfo
    $info.FileName = $script:StickyExe
    $info.Arguments = ($quoted -join ' ')
    $info.UseShellExecute = $true
    $info.WindowStyle = [System.Diagnostics.ProcessWindowStyle]::Hidden
    [void][System.Diagnostics.Process]::Start($info)
}

# 发起一条诊断命令，并等它的结果文件刷新。
# 说明：命令会通过单实例插件转发给已运行的实例，插件统一 exit(0)，
#       所以**退出码不可信**，这里一律以结果文件的最后写入时间为准。
function Invoke-StickyCommand {
    param([string[]]$Arguments, [string]$Artifact)
    $before = Get-Stamp $Artifact
    Start-StickyProcess -Arguments $Arguments
    if (-not (Wait-StampChanged -Path $Artifact -Before $before)) {
        throw ('执行 "{0}" 后 {1} 秒内 {2} 没有更新（实例没响应？版本太旧？）' -f ($Arguments -join ' '), $TimeoutSec, $Artifact)
    }
    # 必须显式 -Encoding UTF8：Windows PowerShell 5.1 的 Get-Content 默认按 ANSI(GBK) 读，
    # 会把 Rust 写出的 UTF-8 中文读成乱码，ConvertFrom-Json 直接解析失败。
    return (Get-Content -LiteralPath $Artifact -Raw -Encoding UTF8 | ConvertFrom-Json)
}

function Invoke-StickyCheck {
    return (Invoke-StickyCommand -Arguments @('--check') -Artifact $script:CheckJson)
}

function Invoke-StickyDump {
    return (Invoke-StickyCommand -Arguments @('--dump') -Artifact $script:DumpPath)
}

# 切换置顶：总是跟一个 --dump，这样"文件刷新"就代表"命令已执行完"，
# 不必猜命令什么时候生效（单实例转发是异步的）。
function Set-NotePin {
    param([string]$NoteId, [bool]$Pinned)
    $truth = ([string]$Pinned).ToLowerInvariant()
    Invoke-StickyCommand -Arguments @('--set-pin', $NoteId, $truth, '--dump') -Artifact $script:DumpPath | Out-Null
}

# 删除便签同样落到 dump 文件上
function Remove-NoteById {
    param([string]$NoteId)
    Invoke-StickyCommand -Arguments @('--delete-note', $NoteId, '--dump') -Artifact $script:DumpPath | Out-Null
}

function Read-Dump {
    return (Get-Content -LiteralPath $script:DumpPath -Raw -Encoding UTF8 | ConvertFrom-Json)
}

function Get-CheckItem {
    param($Check, [string]$Id)
    return ($Check.checks | Where-Object { $_.id -eq $Id } | Select-Object -First 1)
}

function Test-CheckPassed {
    param($Check, [string]$Id)
    $item = Get-CheckItem -Check $Check -Id $Id
    if ($null -eq $item) { return $false }
    return ($item.status -eq 'pass')
}

function Describe-Check {
    param($Check, [string]$Id)
    $item = Get-CheckItem -Check $Check -Id $Id
    if ($null -eq $item) { return '（没有这条结果）' }
    return ('{0}：{1}' -f $item.status, $item.detail)
}

function Get-PinMap {
    param($Dump)
    $map = @{}
    foreach ($note in $Dump.notes) { $map[$note.id] = [bool]$note.pinned }
    return $map
}

function Get-WindowOf {
    param($Dump, [string]$NoteId)
    return ($Dump.windows | Where-Object { $_.note_id -eq $NoteId } | Select-Object -First 1)
}

# ---------------------------------------------------------------- 前置条件

function Resolve-StickyExe {
    if ($Exe) {
        $candidate = $Exe
        if (-not [System.IO.Path]::IsPathRooted($candidate)) {
            $candidate = Join-Path (Get-Location).Path $candidate
        }
        if (-not (Test-Path -LiteralPath $candidate)) { throw "找不到 exe：$candidate" }
        return (Resolve-Path -LiteralPath $candidate).Path
    }
    $repo = Split-Path -Parent $PSScriptRoot
    $default = Join-Path $repo 'src-tauri\target\release\rust-sticky-notes.exe'
    if (Test-Path -LiteralPath $default) { return $default }
    throw "找不到 release exe：$default`n先跑 npm run tauri build -- --no-bundle，或用 -Exe 指定路径。"
}

function Ensure-StickyRunning {
    if ((Get-StickyProcesses).Count -gt 0) { return }
    Write-Host '   没有已运行实例，先启动一份（--dump 会等所有便签窗口建好）' -ForegroundColor DarkGray
    $before = Get-Stamp $script:DumpPath
    Start-StickyProcess -Arguments @('--dump')
    if (-not (Wait-StampChanged -Path $script:DumpPath -Before $before -Timeout ([Math]::Max($TimeoutSec, 20)))) {
        throw '启动应用后没有等到 sticky-dump.json（应用没起来？）'
    }
    $script:StartedByUs = $true
}

# 启动一个普通应用并把它拉到前台；返回 $true 表示前台确实是外部应用。
# 已经拉起过的那个会复用（Win11 的记事本会把新文档开进同一个进程，反复拉起会白等）。
function Start-ForegroundApp {
    if ($null -ne $script:ForegroundProc) {
        $alive = Get-Process -Id $script:ForegroundProc.Id -ErrorAction SilentlyContinue
        if ($alive) {
            $alive.Refresh()
            if ($alive.MainWindowHandle -ne 0) {
                $script:ForegroundProc = $alive
                for ($i = 0; $i -lt 6; $i++) {
                    if ([StickyCheckNative]::ForceForeground([IntPtr]$alive.MainWindowHandle)) { return $true }
                    Start-Sleep -Milliseconds 80
                }
                return $false
            }
        }
        $script:ForegroundProc = $null
    }

    $existing = @(Get-Process -Name Notepad -ErrorAction SilentlyContinue | Select-Object -ExpandProperty Id)
    Start-Process notepad -ErrorAction SilentlyContinue | Out-Null

    $target = $null
    for ($i = 0; $i -lt 30; $i++) {
        Start-Sleep -Milliseconds 80
        $candidates = @(Get-Process -Name Notepad -ErrorAction SilentlyContinue | Where-Object { $_.MainWindowHandle -ne 0 })
        $fresh = @($candidates | Where-Object { $existing -notcontains $_.Id })
        if ($fresh.Count -gt 0) { $target = $fresh[0]; break }
    }
    if ($null -eq $target) {
        # Win11 的记事本会把新文档开进已有进程（或进程名不是 Notepad），退一步找任意一个记事本窗口
        $target = @(Get-Process -Name Notepad -ErrorAction SilentlyContinue | Where-Object { $_.MainWindowHandle -ne 0 } |
            Sort-Object Id | Select-Object -First 1)
    }
    if ($null -eq $target) {
        # 最后兜底：自己起一个 cmd 窗口当"前台应用"，保证场景能跑
        Write-Host '   没能拉起记事本，改用 cmd 窗口当前台应用' -ForegroundColor Yellow
        $target = Start-Process -FilePath $env:ComSpec -ArgumentList @('/c', 'title STICKY-CHECK & timeout /t 600 >nul') -PassThru
        for ($i = 0; $i -lt 40; $i++) {
            Start-Sleep -Milliseconds 80
            $target.Refresh()
            if ($target.MainWindowHandle -ne 0) { break }
        }
        if ($target.MainWindowHandle -eq 0) { throw '没能得到前台应用的窗口句柄' }
    }

    $hwnd = [IntPtr]$target.MainWindowHandle
    $script:ForegroundProc = $target
    $script:ForegroundProcIsNew = ($existing -notcontains $target.Id)

    for ($i = 0; $i -lt 10; $i++) {
        if ([StickyCheckNative]::ForceForeground($hwnd)) { return $true }
        Start-Sleep -Milliseconds 100
    }
    return ([StickyCheckNative]::Foreground() -eq $hwnd)
}

function Get-ForegroundInfo {
    param($Dump)
    $fg = $Dump.foreground
    return ('hwnd={0} class="{1}" is_ours={2} z={3}' -f $fg.hwnd, $fg.class, $fg.is_ours, $fg.z)
}

# ---------------------------------------------------------------- 场景

function Step-Baseline {
    Write-Step '场景 1/5：基线（便签数 ≥ 2 + 六条不变量）'

    $check = Invoke-StickyCheck
    $dump = Read-Dump

    $count = @($dump.notes).Count
    if ($count -lt 2) {
        Write-Host ("   只有 {0} 张便签，用 --new-note 补一张（结束时删除）" -f $count) -ForegroundColor DarkGray
        $before = @($dump.notes | ForEach-Object { $_.id })
        # 带上 --dump：新建完成后才会写文件，这样"文件刷新"就等于"便签已经建好"
        Invoke-StickyCommand -Arguments @('--new-note', '--dump') -Artifact $script:DumpPath | Out-Null
        $probe = Read-Dump
        $freshNote = @($probe.notes | Where-Object { $before -notcontains $_.id })
        if ($freshNote.Count -eq 0) { throw '--new-note 之后没有出现新便签' }
        $script:CreatedNoteId = $freshNote[0].id
        # 便签变了，基线断言要重新采一次
        $check = Invoke-StickyCheck
        $dump = Read-Dump
        $count = @($dump.notes).Count
    }

    Add-Result '基线：便签数 ≥ 2（可以分两张做一钉一不钉）' ($count -ge 2) ("当前 $count 张")
    Add-Result '基线：6 条不变量全部通过' ([bool]$check.ok) ("{0}/{1}" -f $check.passed, $check.total)

    $dump = Read-Dump
    $noteIds = @($dump.notes | ForEach-Object { $_.id } | Sort-Object)
    $windowIds = @($dump.windows | ForEach-Object { $_.note_id } | Sort-Object)
    $same = (($noteIds -join ',') -eq ($windowIds -join ','))
    Add-Result '基线：便签与窗口 id 一一对应' $same ("notes={0} windows={1}" -f $noteIds.Count, $windowIds.Count)

    foreach ($note in $dump.notes) { $script:OriginalPins[$note.id] = [bool]$note.pinned }
}

function Step-ForegroundCover {
    Write-Step '场景 2/5：前台覆盖（未置顶便签不得压住前台应用）'

    $ok = Start-ForegroundApp
    Start-Sleep -Milliseconds 60
    $check = Invoke-StickyCheck
    $dump = Read-Dump

    Add-Result '前台覆盖：前台窗口确实是外部应用' (-not [bool]$dump.foreground.is_ours) (Get-ForegroundInfo $dump)
    Add-Result '前台覆盖：未置顶便签没有压住前台窗口' (Test-CheckPassed -Check $check -Id 'no_cover_foreground') (Describe-Check -Check $check -Id 'no_cover_foreground')
    if (-not $ok) {
        Write-Host '   注意：SetForegroundWindow 没成功，上面的断言可能退化为 skip' -ForegroundColor Yellow
    }
}

function Step-TogglePin {
    Write-Step '场景 3/5：--set-pin 切换置顶（置顶生效 / 取消置顶回落）'

    $dump = Read-Dump
    $sorted = @($dump.notes | Sort-Object id)
    $target = $sorted[0].id
    Write-Host ("   目标便签：{0}" -f $target) -ForegroundColor DarkGray

    # --- 置顶
    Set-NotePin -NoteId $target -Pinned $true
    Start-ForegroundApp | Out-Null
    $check = Invoke-StickyCheck
    $dump = Read-Dump
    $win = Get-WindowOf -Dump $dump -NoteId $target
    $note = $dump.notes | Where-Object { $_.id -eq $target } | Select-Object -First 1
    Add-Result '切置顶 true：目标便签真的进了置顶层' ([bool]$win.topmost -and [bool]$note.pinned) ("exstyle={0} topmost={1} data.pinned={2}" -f $win.exstyle, $win.topmost, $note.pinned)
    Add-Result '切置顶 true：置顶一致 + 置顶在最前' ((Test-CheckPassed -Check $check -Id 'pin_consistency') -and (Test-CheckPassed -Check $check -Id 'pinned_on_top')) ('{0}；{1}' -f (Describe-Check -Check $check -Id 'pin_consistency'), (Describe-Check -Check $check -Id 'pinned_on_top'))
    Add-Result '切置顶 true：未置顶那张仍被前台覆盖' (Test-CheckPassed -Check $check -Id 'no_cover_foreground') (Describe-Check -Check $check -Id 'no_cover_foreground')

    # --- 取消置顶
    Set-NotePin -NoteId $target -Pinned $false
    Start-ForegroundApp | Out-Null
    $check = Invoke-StickyCheck
    $dump = Read-Dump
    $win = Get-WindowOf -Dump $dump -NoteId $target
    $note = $dump.notes | Where-Object { $_.id -eq $target } | Select-Object -First 1
    Add-Result '切置顶 false：目标便签退出置顶层' ((-not [bool]$win.topmost) -and (-not [bool]$note.pinned)) ("exstyle={0} topmost={1} data.pinned={2}" -f $win.exstyle, $win.topmost, $note.pinned)
    Add-Result '切置顶 false：两张便签都在前台之下' (Test-CheckPassed -Check $check -Id 'no_cover_foreground') (Describe-Check -Check $check -Id 'no_cover_foreground')
    Add-Result '切置顶 false：置顶一致（没有连带状态漂移）' (Test-CheckPassed -Check $check -Id 'pin_consistency') (Describe-Check -Check $check -Id 'pin_consistency')
}

function Step-SiblingIsolation {
    Write-Step '场景 4/5：兄弟不连坐（一钉一不钉，切 2 轮）'

    $dump = Read-NoteDump
    $sorted = @($dump.notes | Sort-Object id)
    $a = $sorted[0].id
    $b = $sorted[1].id

    for ($round = 1; $round -le 2; $round++) {
        $pinned = if ($round -eq 1) { $a } else { $b }
        $unpinned = if ($round -eq 1) { $b } else { $a }

        # 只改真正需要改的那张：多余的一次切换不仅慢，还会掩盖"只动一张"的结论
        $pins = Get-PinMap (Read-Dump)
        if (-not $pins[$pinned]) { Set-NotePin -NoteId $pinned -Pinned $true }
        if ($pins[$unpinned]) { Set-NotePin -NoteId $unpinned -Pinned $false }
        Start-ForegroundApp | Out-Null
        $check = Invoke-StickyCheck
        $dump = Read-Dump

        $fgZ = $dump.foreground.z
        $unpinnedWin = Get-WindowOf -Dump $dump -NoteId $unpinned
        $belowForeground = ($null -ne $fgZ) -and ($null -ne $unpinnedWin.z) -and ([int]$unpinnedWin.z -gt [int]$fgZ)
        Add-Result ("兄弟不连坐 第{0}轮：未置顶 {1} 的 z 仍大于前台 z" -f $round, $unpinned.Substring(0, 8)) $belowForeground ("未置顶 z={0} 前台 z={1} class={2}" -f $unpinnedWin.z, $fgZ, $dump.foreground.class)
        Add-Result ("兄弟不连坐 第{0}轮：置顶一致" -f $round) (Test-CheckPassed -Check $check -Id 'pin_consistency') (Describe-Check -Check $check -Id 'pin_consistency')

        $script:ChangedPins[$a] = $true
        $script:ChangedPins[$b] = $true
    }
}

function Read-NoteDump {
    # 保证拿到的是最新快照（重启后旧的 dump 可能还在）
    return (Invoke-StickyDump)
}

function Step-RestartConsistency {
    Write-Step '场景 5/5：重启一致性（置顶状态与 store 一致）'

    $dump = Read-Dump
    $pinsBefore = Get-PinMap $dump
    $countBefore = @($dump.notes).Count

    Write-Host '   关闭进程，重新启动…' -ForegroundColor DarkGray
    foreach ($proc in Get-StickyProcesses) { Stop-Process -Id $proc.Id -Force -ErrorAction SilentlyContinue }
    $deadline = (Get-Date).AddSeconds(10)
    while ((Get-Date) -lt $deadline -and (Get-StickyProcesses).Count -gt 0) { Start-Sleep -Milliseconds 50 }
    if ((Get-StickyProcesses).Count -gt 0) { throw '关不掉已运行的实例' }

    # 用 --dump 当"窗口已就绪"的信号：dump 是在所有便签窗口建完之后才写的
    $before = Get-Stamp $script:DumpPath
    Start-StickyProcess -Arguments @('--dump')
    if (-not (Wait-StampChanged -Path $script:DumpPath -Before $before -Timeout ([Math]::Max($TimeoutSec, 25)))) {
        throw '重启后没有等到 sticky-dump.json（应用没起来）'
    }

    $check = Invoke-StickyCheck
    $dump = Read-Dump
    Add-Result '重启后：置顶一致（数据 pinned 与窗口 TOPMOST 对齐）' (Test-CheckPassed -Check $check -Id 'pin_consistency') (Describe-Check -Check $check -Id 'pin_consistency')

    $pinsAfter = Get-PinMap $dump
    $same = ($pinsAfter.Count -eq $pinsBefore.Count)
    $diff = @()
    foreach ($id in $pinsBefore.Keys) {
        if (-not $pinsAfter.ContainsKey($id) -or $pinsAfter[$id] -ne $pinsBefore[$id]) {
            $same = $false
            $diff += ('{0}: {1} -> {2}' -f $id.Substring(0, 8), $pinsBefore[$id], $pinsAfter[$id])
        }
    }
    Add-Result '重启后：pinned 状态与重启前完全一致' $same ($(if ($diff.Count -gt 0) { $diff -join '；' } else { "共 $($pinsAfter.Count) 张" }))
    Add-Result '重启后：便签数量不变' (@($dump.notes).Count -eq $countBefore) ("{0} -> {1}" -f $countBefore, @($dump.notes).Count)
}

# ---------------------------------------------------------------- 失败上下文

function Show-FailureContext {
    Write-Host ''
    Write-Host '--- 失败上下文（原始字段，可直接贴给主代理）---' -ForegroundColor Yellow
    if (Test-Path -LiteralPath $script:DumpPath) {
        $dump = Read-Dump
        Write-Host ('前台: hwnd={0} class="{1}" title="{2}" is_ours={3} z={4}' -f $dump.foreground.hwnd, $dump.foreground.class, $dump.foreground.title, $dump.foreground.is_ours, $dump.foreground.z)
        Write-Host '窗口:'
        foreach ($w in $dump.windows) {
            Write-Host ('  {0} hwnd={1} ex={2} topmost={3} owner={4}/{5} z={6}/{7} rect={8}x{9} visible={10}' -f `
                    $w.label, $w.hwnd, $w.exstyle, $w.topmost, $w.owner_hwnd, $w.owner_class, $w.z, $w.z_total, $w.rect.w, $w.rect.h, $w.visible)
        }
        Write-Host '便签:'
        foreach ($n in $dump.notes) {
            $boundsText = 'null'
            if ($null -ne $n.bounds) { $boundsText = '{0}x{1}' -f $n.bounds.width, $n.bounds.height }
            Write-Host ('  {0} pinned={1} collapsed={2} visible={3} bounds={4}' -f $n.id, $n.pinned, $n.collapsed, $n.visible, $boundsText)
        }
    } else {
        Write-Host '（没有 sticky-dump.json）'
    }
    if (Test-Path -LiteralPath $script:CheckTxt) {
        Write-Host '检查结果:'
        Get-Content -LiteralPath $script:CheckTxt | Where-Object { $_ -match '^\[(FAIL|SKIP)\]|^FAIL' } | ForEach-Object { Write-Host ('  ' + $_) }
    }
}

# ---------------------------------------------------------------- 清理

function Clear-ForegroundApp {
    if ($null -eq $script:ForegroundProc) { return }
    if (-not $script:ForegroundProcIsNew) { return }  # 用户原本就开着的记事本不动
    $name = $script:ForegroundProc.ProcessName
    if ($name -eq 'Notepad' -or $name -eq 'cmd') {
        Get-Process -Name $name -ErrorAction SilentlyContinue |
            Where-Object { $_.Id -eq $script:ForegroundProc.Id } |
            Stop-Process -Force -ErrorAction SilentlyContinue
    }
}

function Restore-State {
    if ((Get-StickyProcesses).Count -eq 0) { return }
    # 恢复被脚本动过的置顶状态
    foreach ($id in $script:ChangedPins.Keys) {
        if ($script:OriginalPins.ContainsKey($id)) {
            $want = [bool]$script:OriginalPins[$id]
            try {
                Set-NotePin -NoteId $id -Pinned $want
            } catch {
                Write-Host ('   恢复置顶状态失败：{0}' -f $_.Exception.Message) -ForegroundColor Yellow
            }
        }
    }
    # 删掉脚本自己建的临时便签
    if ($script:CreatedNoteId) {
        try {
            Remove-NoteById -NoteId $script:CreatedNoteId
            Write-Host '   已删除脚本创建的临时便签' -ForegroundColor DarkGray
        } catch {
            Write-Host ('   删除临时便签失败：{0}' -f $_.Exception.Message) -ForegroundColor Yellow
        }
    }
}

function Clear-Artifacts {
    $failed = @($script:Results | Where-Object { -not $_.Ok }).Count
    if ($KeepArtifacts -or $failed -gt 0) {
        Write-Host ("   结果文件保留在 {0}" -f $script:TempDir) -ForegroundColor DarkGray
        return
    }
    Remove-Item -LiteralPath $script:DumpPath, $script:CheckJson, $script:CheckTxt -ErrorAction SilentlyContinue
}

# ---------------------------------------------------------------- main

try {
    Write-Host 'rust-sticky-notes 窗口层级回归断言' -ForegroundColor White
    $script:StickyExe = Resolve-StickyExe
    Write-Host ("   exe: {0}" -f $script:StickyExe)
    Ensure-StickyRunning

    Step-Baseline
    Step-ForegroundCover
    Step-TogglePin
    Step-SiblingIsolation
    if ($SkipRestart) {
        Write-Step '场景 5/5：重启一致性 —— 已按 -SkipRestart 跳过'
    } else {
        Step-RestartConsistency
    }
}
catch {
    Add-Result ('执行异常：' + $_.Exception.Message) $false ''
}
finally {
    Write-Step '清理'
    Clear-ForegroundApp
    Restore-State
    $script:Clock.Stop()
    $total = $script:Results.Count
    $passed = @($script:Results | Where-Object { $_.Ok }).Count
    $failed = $total - $passed
    if ($failed -gt 0) { Show-FailureContext }
    Clear-Artifacts
    Write-Host ''
    $summary = '=> {0}/{1} 通过（耗时 {2:N1}s）' -f $passed, $total, ($script:Clock.ElapsedMilliseconds / 1000.0)
    if ($failed -eq 0) {
        Write-Host $summary -ForegroundColor Green
    } else {
        Write-Host $summary -ForegroundColor Red
    }
    if ($script:StartedByUs) { Write-Host '   （应用是脚本启动的，保持运行中）' -ForegroundColor DarkGray }
}

if (@($script:Results | Where-Object { -not $_.Ok }).Count -gt 0) { exit 1 }
exit 0
