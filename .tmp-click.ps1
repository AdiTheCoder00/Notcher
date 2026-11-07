$src = @"
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using System.Text;
public class Win2 {
  public delegate bool EnumProc(IntPtr hWnd, IntPtr lParam);
  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc cb, IntPtr extra);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr hWnd, out uint pid);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr hWnd);
  [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern int GetWindowTextW(IntPtr hWnd, StringBuilder text, int max);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr hWnd, out RECT rect);
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] public static extern bool GetCursorPos(out POINT p);
  [DllImport("user32.dll")] public static extern void mouse_event(uint flags, uint dx, uint dy, uint data, UIntPtr extra);
  [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr hWnd);
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left, Top, Right, Bottom; }
  [StructLayout(LayoutKind.Sequential)] public struct POINT { public int X, Y; }
  public static List<RECT> Rects(uint pid) {
    var rows = new List<RECT>();
    EnumWindows((h, l) => {
      uint owner; GetWindowThreadProcessId(h, out owner);
      if (owner != pid || !IsWindowVisible(h)) return true;
      RECT r; GetWindowRect(h, out r);
      if (r.Right - r.Left > 40) rows.Add(r);
      return true;
    }, IntPtr.Zero);
    return rows;
  }
}
"@
Add-Type -TypeDefinition $src

$proc = Get-Process murmur -ErrorAction Stop
$before = [Win2]::GetForegroundWindow()
$origin = New-Object Win2+POINT
[Win2]::GetCursorPos([ref]$origin) | Out-Null

# The notch is the 128x44 window at the top centre.
$idle = [Win2]::Rects([uint32]$proc.Id) | Where-Object { ($_.Right - $_.Left) -eq 128 } | Select-Object -First 1
if (-not $idle) { "FAIL: no 128x44 notch window"; exit 1 }
$x = [int](($idle.Left + $idle.Right) / 2)
$y = [int](($idle.Top + $idle.Bottom) / 2)

[Win2]::SetCursorPos($x, $y) | Out-Null
[Win2]::mouse_event(0x0002, 0, 0, 0, [UIntPtr]::Zero)
[Win2]::mouse_event(0x0004, 0, 0, 0, [UIntPtr]::Zero)
Start-Sleep -Milliseconds 1100

"after click:"
[Win2]::Rects([uint32]$proc.Id) | ForEach-Object {
  "  {0}x{1} at ({2},{3})" -f ($_.Right - $_.Left), ($_.Bottom - $_.Top), $_.Left, $_.Top
}

$panel = [Win2]::Rects([uint32]$proc.Id) | Where-Object { ($_.Right - $_.Left) -gt 300 -and ($_.Bottom - $_.Top) -gt 600 } | Select-Object -First 1
if ($panel) {
  # Close with the X in the panel header (24 px in from the right, 26 px down).
  $cx = $panel.Right - 24
  $cy = $panel.Top + 26
  [Win2]::SetCursorPos($cx, $cy) | Out-Null
  [Win2]::mouse_event(0x0002, 0, 0, 0, [UIntPtr]::Zero)
  [Win2]::mouse_event(0x0004, 0, 0, 0, [UIntPtr]::Zero)
  Start-Sleep -Milliseconds 900
  "after close:"
  [Win2]::Rects([uint32]$proc.Id) | ForEach-Object {
    "  {0}x{1} at ({2},{3})" -f ($_.Right - $_.Left), ($_.Bottom - $_.Top), $_.Left, $_.Top
  }
}

[Win2]::SetCursorPos($origin.X, $origin.Y) | Out-Null
if ($before -ne [IntPtr]::Zero) { [Win2]::SetForegroundWindow($before) | Out-Null }
"done"
