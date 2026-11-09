# 测 Tauri 便签的冷/热启动：进程启动 -> 窗口可见 -> 首帧（窗口中心出现便签底色）
param(
  [string]$Exe = "D:\ai\rust-sticky-notes\src-tauri\target\release\rust-sticky-notes.exe",
  [int]$Runs = 5
)

Add-Type -AssemblyName System.Drawing

Add-Type @"
using System;
using System.Runtime.InteropServices;
public class SM {
  public delegate bool EnumProc(IntPtr h, IntPtr l);
  [DllImport("user32.dll")] public static extern bool SetProcessDpiAwarenessContext(IntPtr c);
  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc cb, IntPtr l);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr h, IntPtr after, int x, int y, int cx, int cy, uint flags);
  public struct RECT { public int Left, Top, Right, Bottom; }
  public static IntPtr found = IntPtr.Zero;
  public static void Scan(uint pid) {
    found = IntPtr.Zero;
    EnumWindows((h, l) => {
      uint p; GetWindowThreadProcessId(h, out p);
      if (p == pid && IsWindowVisible(h)) {
        RECT r; GetWindowRect(h, out r);
        if ((r.Right - r.Left) > 400 && (r.Bottom - r.Top) > 400) { found = h; return false; }
      }
      return true;
    }, IntPtr.Zero);
  }
}
"@
[SM]::SetProcessDpiAwarenessContext([IntPtr](-4)) | Out-Null

if (-not (Test-Path $Exe)) { "EXE_NOT_FOUND: $Exe"; exit 2 }

function Is-NoteColor($c) {
  # 便签底色 #FFF8D6 附近（顶部栏偏深，所以取窗口中心偏上的文本区）
  return ($c.R -gt 190 -and $c.G -gt 180 -and $c.B -gt 130 -and ($c.R - $c.B) -gt 20)
}

$results = @()
for ($i = 1; $i -le $Runs; $i++) {
  Get-Process -Name "rust-sticky-notes" -ErrorAction SilentlyContinue | Stop-Process -Force
  Start-Sleep -Milliseconds 900

  $sw = [System.Diagnostics.Stopwatch]::StartNew()
  $proc = Start-Process -FilePath $Exe -PassThru
  $tWin = $null; $tPaint = $null; $hwnd = [IntPtr]::Zero

  while ($sw.Elapsed.TotalMilliseconds -lt 20000) {
    if ($hwnd -eq [IntPtr]::Zero) {
      [SM]::Scan([uint32]$proc.Id)
      if ([SM]::found -ne [IntPtr]::Zero) {
        $hwnd = [SM]::found
        $tWin = [math]::Round($sw.Elapsed.TotalMilliseconds, 1)
        # 提到最前，避免被其它窗口挡住导致采不到色
        [SM]::SetWindowPos($hwnd, [IntPtr]::Zero, 0, 0, 0, 0, 0x0013) | Out-Null
      }
    } else {
      $r = New-Object SM+RECT
      [SM]::GetWindowRect($hwnd, [ref]$r) | Out-Null
      $px = $r.Left + [int](($r.Right - $r.Left) * 0.5)
      $py = $r.Top + [int](($r.Bottom - $r.Top) * 0.35)
      $bmp = [System.Drawing.Bitmap]::new(3, 3)
      $g = [System.Drawing.Graphics]::FromImage($bmp)
      $g.CopyFromScreen($px, $py, 0, 0, $bmp.Size)
      $c = $bmp.GetPixel(1, 1)
      $g.Dispose(); $bmp.Dispose()
      if (Is-NoteColor $c) {
        $tPaint = [math]::Round($sw.Elapsed.TotalMilliseconds, 1)
        break
      }
    }
    Start-Sleep -Milliseconds 6
  }
  $sw.Stop()

  $results += [pscustomobject]@{
    Run        = $i
    窗口可见ms = if ($tWin) { $tWin } else { "timeout" }
    首帧ms     = if ($tPaint) { $tPaint } else { "timeout" }
  }
}

Get-Process -Name "rust-sticky-notes" -ErrorAction SilentlyContinue | Stop-Process -Force
$results | Format-Table -AutoSize
$ok = $results | Where-Object { $_.首帧ms -ne "timeout" }
if ($ok.Count -gt 0) {
  $vals = $ok | ForEach-Object { $_.首帧ms }
  "首帧 min={0}ms  avg={1}ms  max={2}ms  (n={3})" -f ($vals | Measure-Object -Minimum).Minimum, [math]::Round(($vals | Measure-Object -Average).Average, 1), ($vals | Measure-Object -Maximum).Maximum, $vals.Count
}
