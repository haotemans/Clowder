param(
  [string]$Out = "D:\weiai\project\clowder\scripts\memlogs\shot-window.png",
  [string]$AppArgs = ""
)

Add-Type @"
using System;
using System.Text;
using System.Runtime.InteropServices;
public static class W {
  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc cb, IntPtr l);
  public delegate bool EnumProc(IntPtr h, IntPtr l);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll")] public static extern int GetWindowTextLength(IntPtr h);
  [DllImport("user32.dll")] public static extern int GetWindowText(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr h, IntPtr hdc, uint flags);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern bool SetProcessDpiAwarenessContext(IntPtr v);
  public struct RECT { public int Left, Top, Right, Bottom; }
}
"@
[void][W]::SetProcessDpiAwarenessContext([IntPtr](-4))  # DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2

Add-Type -AssemblyName System.Windows.Forms, System.Drawing

if ($AppArgs) { $p = Start-Process -FilePath "D:\weiai\project\clowder\target\release\clowder.exe" -ArgumentList $AppArgs -PassThru -WindowStyle Hidden }
else { $p = Start-Process -FilePath "D:\weiai\project\clowder\target\release\clowder.exe" -PassThru -WindowStyle Hidden }
Start-Sleep -Seconds 7

$found = @()
$cb = [W+EnumProc] {
  param($h, $l)
  $pid2 = 0
  [void][W]::GetWindowThreadProcessId($h, [ref]$pid2)
  if ($pid2 -eq $p.Id -and [W]::IsWindowVisible($h)) {
    $len = [W]::GetWindowTextLength($h)
    if ($len -gt 0) {
      $sb = New-Object Text.StringBuilder ($len + 2)
      [void][W]::GetWindowText($h, $sb, $sb.Capacity)
      $script:found += [pscustomobject]@{ Hwnd = $h; Title = $sb.ToString() }
    }
  }
  return $true
}
[void][W]::EnumWindows($cb, [IntPtr]::Zero)

foreach ($w in $found) {
  $r = New-Object W+RECT
  [void][W]::GetWindowRect($w.Hwnd, [ref]$r)
  $wid = $r.Right - $r.Left; $hei = $r.Bottom - $r.Top
  Write-Host ("window '{0}' {1}x{2} at {3},{4}" -f $w.Title, $wid, $hei, $r.Left, $r.Top)
  if ($wid -lt 50 -or $hei -lt 50) { continue }
  $bmp = New-Object Drawing.Bitmap $wid, $hei, ([System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
  $g = [Drawing.Graphics]::FromImage($bmp)
  $hdc = $g.GetHdc()
  [void][W]::PrintWindow($w.Hwnd, $hdc, 2)   # PW_RENDERFULLCONTENT
  $g.ReleaseHdc($hdc)
  $g.Dispose()
  $bmp.Save($Out)
  Write-Host "saved -> $Out"
}
Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue
