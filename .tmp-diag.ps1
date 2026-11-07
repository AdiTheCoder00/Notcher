$src = @"
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using System.Text;
public class Win3 {
  public delegate bool EnumProc(IntPtr hWnd, IntPtr lParam);
  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc cb, IntPtr extra);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr hWnd, out uint pid);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr hWnd);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr hWnd, out RECT rect);
  [DllImport("user32.dll")] public static extern IntPtr GetWindowLongPtrW(IntPtr hWnd, int index);
  [DllImport("user32.dll")] public static extern IntPtr WindowFromPoint(POINT p);
  [DllImport("user32.dll")] public static extern int GetWindowTextW(IntPtr hWnd, StringBuilder t, int n);
  [DllImport("user32.dll")] public static extern IntPtr GetParent(IntPtr hWnd);
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left, Top, Right, Bottom; }
  [StructLayout(LayoutKind.Sequential)] public struct POINT { public int X, Y; }
  public static List<IntPtr> Handles(uint pid) {
    var rows = new List<IntPtr>();
    EnumWindows((h, l) => {
      uint owner; GetWindowThreadProcessId(h, out owner);
      if (owner == pid && IsWindowVisible(h)) rows.Add(h);
      return true;
    }, IntPtr.Zero);
    return rows;
  }
  public static string Text(IntPtr h) { var sb = new StringBuilder(128); GetWindowTextW(h, sb, 128); return sb.ToString(); }
}
"@
Add-Type -TypeDefinition $src

$proc = Get-Process murmur -ErrorAction Stop

"windows:"
foreach ($h in [Win3]::Handles([uint32]$proc.Id)) {
  $r = New-Object Win3+RECT
  [Win3]::GetWindowRect($h, [ref]$r) | Out-Null
  $w = $r.Right - $r.Left; $ht = $r.Bottom - $r.Top
  if ($w -lt 40) { continue }
  $ex = [Win3]::GetWindowLongPtrW($h, -20).ToInt64()
  $style = [Win3]::GetWindowLongPtrW($h, -16).ToInt64()
  $flags = @()
  if ($ex -band 0x20) { $flags += "TRANSPARENT" }
  if ($ex -band 0x80000) { $flags += "LAYERED" }
  if ($ex -band 0x8000000) { $flags += "NOACTIVATE" }
  if ($ex -band 0x80) { $flags += "TOOLWINDOW" }
  if ($ex -band 0x8) { $flags += "TOPMOST" }
  "  {0}x{1} at ({2},{3}) exstyle=0x{4:X} [{5}] title='{6}'" -f $w, $ht, $r.Left, $r.Top, $ex, ($flags -join ","), [Win3]::Text($h)
}

$p = New-Object Win3+POINT
$p.X = 1280; $p.Y = 30
$under = [Win3]::WindowFromPoint($p)
$r2 = New-Object Win3+RECT
[Win3]::GetWindowRect($under, [ref]$r2) | Out-Null
"window under (1280,30): hwnd=$under pid=" + $( $x = 0; [Win3]::GetWindowThreadProcessId($under, [ref]$x) | Out-Null; $x ) + " size=$($r2.Right - $r2.Left)x$($r2.Bottom - $r2.Top) title='" + [Win3]::Text($under) + "'"
