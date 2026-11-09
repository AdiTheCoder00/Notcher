# Measures the top shoulders on the live Murmur Control Centre window.
#
# Emits a single JSON object and exits. status = "skipped" when the desktop app
# is not running, so the check can run headless without failing; "ok" carries
# the measured geometry. The cursor is put back where it was found.
#
#   powershell -NoProfile -ExecutionPolicy Bypass -File scripts/check-notch-native.ps1

$ErrorActionPreference = "Stop"
Add-Type -AssemblyName System.Drawing
Add-Type -AssemblyName System.Windows.Forms

Add-Type -TypeDefinition @'
using System;
using System.Text;
using System.Runtime.InteropServices;

public class MurmurNative
{
    [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
    [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
    [DllImport("user32.dll")] public static extern void mouse_event(uint flags, uint dx, uint dy, int data, IntPtr extra);
    [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc callback, IntPtr param);
    public delegate bool EnumProc(IntPtr handle, IntPtr param);
    [DllImport("user32.dll")] public static extern int GetWindowText(IntPtr handle, StringBuilder text, int count);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr handle);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr handle, out Rect rect);
    public struct Rect { public int Left, Top, Right, Bottom; }

    public static string Find()
    {
        string found = "";
        EnumWindows((handle, param) =>
        {
            var text = new StringBuilder(256);
            GetWindowText(handle, text, 256);
            if (text.Length > 0 && IsWindowVisible(handle) && text.ToString().Contains("Murmur"))
            {
                Rect rect;
                GetWindowRect(handle, out rect);
                found += rect.Left + "," + rect.Top + "," + (rect.Right - rect.Left) + "," + (rect.Bottom - rect.Top) + ";";
            }
            return true;
        }, IntPtr.Zero);
        return found;
    }
}
'@

function Get-MurmurWindows {
    $windows = @()
    foreach ($entry in ([MurmurNative]::Find() -split ";")) {
        if ($entry -match "^(-?\d+),(-?\d+),(\d+),(\d+)$") {
            $windows += [pscustomobject]@{
                X = [int]$Matches[1]
                Y = [int]$Matches[2]
                W = [int]$Matches[3]
                H = [int]$Matches[4]
            }
        }
    }
    return $windows
}

function Test-Near($colour, $target, $tolerance) {
    return ([math]::Abs($colour.R - $target.R) -le $tolerance) -and
           ([math]::Abs($colour.G - $target.G) -le $tolerance) -and
           ([math]::Abs($colour.B - $target.B) -le $tolerance)
}

function Send-Json($payload) {
    $payload | ConvertTo-Json -Compress
    exit 0
}

[MurmurNative]::SetProcessDPIAware() | Out-Null

if (-not (Get-Process murmur -ErrorAction SilentlyContinue)) {
    Send-Json @{ status = "skipped"; reason = "Murmur is not running" }
}

$cursor = [System.Windows.Forms.Cursor]::Position
$windows = Get-MurmurWindows
$panel = $windows | Where-Object { $_.W -ge 600 -and $_.H -ge 400 -and $_.H -le 700 } | Select-Object -First 1

if (-not $panel) {
    $notch = $windows | Sort-Object W | Select-Object -First 1
    if (-not $notch) {
        Send-Json @{ status = "skipped"; reason = "no Murmur window found" }
    }

    # Real cursor steps, so the webview receives mouseover like a person's would.
    $targetX = $notch.X + [int]($notch.W / 2)
    $targetY = $notch.Y + [int]($notch.H / 2)

    # Park the cursor off the notch first: a move that starts on the notch emits
    # no mouseover, which is why hovering did nothing when it was already there.
    $startX = $targetX + 80
    $startY = $targetY + 160
    [MurmurNative]::SetCursorPos($startX, $startY) | Out-Null
    Start-Sleep -Milliseconds 150

    for ($step = 1; $step -le 12; $step++) {
        [MurmurNative]::SetCursorPos(
            [int]($startX + ($targetX - $startX) * $step / 12),
            [int]($startY + ($targetY - $startY) * $step / 12)
        ) | Out-Null
        Start-Sleep -Milliseconds 35
    }
    [MurmurNative]::SetCursorPos($targetX, $targetY) | Out-Null

    for ($try = 0; $try -lt 30 -and -not $panel; $try++) {
        Start-Sleep -Milliseconds 200
        $panel = Get-MurmurWindows | Where-Object { $_.W -ge 600 -and $_.H -ge 400 -and $_.H -le 700 } | Select-Object -First 1
    }

    if (-not $panel) {
        [MurmurNative]::mouse_event(2, 0, 0, 0, [IntPtr]::Zero)
        Start-Sleep -Milliseconds 60
        [MurmurNative]::mouse_event(4, 0, 0, 0, [IntPtr]::Zero)
        for ($try = 0; $try -lt 25 -and -not $panel; $try++) {
            Start-Sleep -Milliseconds 200
            $panel = Get-MurmurWindows | Where-Object { $_.W -ge 600 -and $_.H -ge 400 -and $_.H -le 700 } | Select-Object -First 1
        }
    }

    [MurmurNative]::SetCursorPos($targetX, $targetY) | Out-Null
}

if (-not $panel) {
    [MurmurNative]::SetCursorPos($cursor.X, $cursor.Y) | Out-Null
    Send-Json @{ status = "skipped"; reason = "the Control Centre is not open - hover the notch, then re-run" }
}

# Let the open animation settle, then re-read in case the window moved.
Start-Sleep -Milliseconds 600
$panel = Get-MurmurWindows | Where-Object { $_.W -ge 600 -and $_.H -ge 400 -and $_.H -le 700 } | Select-Object -First 1

$bitmap = New-Object System.Drawing.Bitmap $panel.W, $panel.H
$graphics = [System.Drawing.Graphics]::FromImage($bitmap)
$graphics.CopyFromScreen($panel.X, $panel.Y, 0, 0, (New-Object System.Drawing.Size $panel.W, $panel.H))
$graphics.Dispose()

# The header just under the top edge is the plain panel surface.
$reference = $bitmap.GetPixel([int]($panel.W / 2), 4)
# Window = 700 px body + the two 12 px shoulder wings.
$scale = [math]::Round($panel.W / 724, 3)

# Canvas run at row `y`, walking in from the left edge of the window.
function Get-Run([int]$y) {
    $run = 0
    while ($run -lt 200 -and -not (Test-Near ($bitmap.GetPixel($run, $y)) $reference 10)) { $run++ }
    return $run
}

function Get-RunFromRight([int]$y) {
    $run = 0
    while ($run -lt 200 -and -not (Test-Near ($bitmap.GetPixel($panel.W - 1 - $run, $y)) $reference 10)) { $run++ }
    return $run
}

# The wings are beside the body, so the body starts one wing in.
$inset = [math]::Round((Get-Run ([int]($panel.H / 2))) / $scale, 1)

# Concave wing: r = (run^2 + depth^2) / (2 * depth).
$depth = [math]::Max(1, [int][math]::Round(6 * $scale))
$runLeft = Get-Run $depth
$runRight = Get-RunFromRight $depth
$shoulderLeft = [math]::Round((($runLeft * $runLeft) + ($depth * $depth)) / (2 * $depth) / $scale, 1)
$shoulderRight = [math]::Round((($runRight * $runRight) + ($depth * $depth)) / (2 * $depth) / $scale, 1)

# The surface must reach the window edge on the top row; a cut shoulder starts a
# whole wing in, which is the hard step this guards.
$stepLeft = [math]::Round((Get-Run 0) / $scale, 1)
$stepRight = [math]::Round((Get-RunFromRight 0) / $scale, 1)

$flank = [math]::Max(1, [int][math]::Round(6 * $scale))
$carved = (-not (Test-Near ($bitmap.GetPixel(1, $flank)) $reference 10)) -and
          (-not (Test-Near ($bitmap.GetPixel($panel.W - 2, $flank)) $reference 10))
$flush = Test-Near ($bitmap.GetPixel([int]($panel.W / 2), 1)) $reference 10

$proof = Join-Path $PSScriptRoot "..\.native-last.png"
$bitmap.Save($proof)
$bitmap.Dispose()

[MurmurNative]::SetCursorPos($cursor.X, $cursor.Y) | Out-Null

Send-Json @{
    status = "ok"
    width = $panel.W
    height = $panel.H
    scale = $scale
    shoulderLeft = $shoulderLeft
    shoulderRight = $shoulderRight
    inset = $inset
    stepLeft = $stepLeft
    stepRight = $stepRight
    carved = $carved
    flush = $flush
    proof = ".native-last.png"
}
