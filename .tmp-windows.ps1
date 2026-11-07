$src = @"
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using System.Text;
public class Win {
  public delegate bool EnumProc(IntPtr hWnd, IntPtr lParam);
  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc cb, IntPtr extra);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr hWnd, out uint pid);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr hWnd);
  [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern int GetWindowTextW(IntPtr hWnd, StringBuilder text, int max);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr hWnd, out RECT rect);
  [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left, Top, Right, Bottom; }
  public static List<string> List(uint pid) {
    var rows = new List<string>();
    EnumWindows((h, l) => {
      uint owner; GetWindowThreadProcessId(h, out owner);
      if (owner != pid || !IsWindowVisible(h)) return true;
      var sb = new StringBuilder(256); GetWindowTextW(h, sb, 256);
      RECT r; GetWindowRect(h, out r);
      rows.Add(string.Format("{0}x{1} at ({2},{3}) title='{4}'",
        r.Right - r.Left, r.Bottom - r.Top, r.Left, r.Top, sb.ToString()));
      return true;
    }, IntPtr.Zero);
    return rows;
  }
}
"@
Add-Type -TypeDefinition $src
$procs = Get-Process murmur -ErrorAction SilentlyContinue
if (-not $procs) { "murmur.exe is NOT running"; exit 1 }
foreach ($p in $procs) {
  "pid=$($p.Id) workingSetMB=$([math]::Round($p.WorkingSet64/1MB,1)) threads=$($p.Threads.Count)"
  [Win]::List([uint32]$p.Id) | ForEach-Object { "  $_" }
}
"foreground pid=" + (Get-Process -Id ([Win]::GetForegroundWindow() | ForEach-Object { $x = 0; [Win]::GetWindowThreadProcessId($_, [ref]$x) | Out-Null; $x }) -ErrorAction SilentlyContinue).Id
