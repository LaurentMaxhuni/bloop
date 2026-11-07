# Draws bloop's app icon and installer art from the mascot's 24x24 design grid
# (same geometry as src/mascot.rs, idle mood). Deterministic: rerun after changing it.
#   powershell -ExecutionPolicy Bypass -File installer/make-art.ps1
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing

$art = Join-Path $PSScriptRoot 'art'
New-Item -ItemType Directory -Force $art | Out-Null

function New-RoundRect([single]$x, [single]$y, [single]$w, [single]$h, [single]$r) {
    $p = New-Object System.Drawing.Drawing2D.GraphicsPath
    $d = 2 * $r
    $p.AddArc($x, $y, $d, $d, 180, 90)
    $p.AddArc($x + $w - $d, $y, $d, $d, 270, 90)
    $p.AddArc($x + $w - $d, $y + $h - $d, $d, $d, 0, 90)
    $p.AddArc($x, $y + $h - $d, $d, $d, 90, 90)
    $p.CloseFigure()
    $p
}

# Mascot in a square of side $side at ($ox,$oy); $body/$face are colors.
function Draw-Mascot($g, [single]$ox, [single]$oy, [single]$side, $body, $face) {
    $u = $side / 24.0
    $bodyBrush = New-Object System.Drawing.SolidBrush $body
    $faceBrush = New-Object System.Drawing.SolidBrush $face
    # Antenna.
    $stem = New-Object System.Drawing.Pen $body, ([single](1.3 * $u))
    $stem.StartCap = 'Round'; $stem.EndCap = 'Round'
    $g.DrawLine($stem, $ox + 12 * $u, $oy + 5 * $u, $ox + 14 * $u, $oy + 1.6 * $u)
    $g.FillEllipse($bodyBrush, $ox + (14 - 1.5) * $u, $oy + (1.6 - 1.5) * $u, 3 * $u, 3 * $u)
    # Body.
    $g.FillPath($bodyBrush, (New-RoundRect ($ox + 2.5 * $u) ($oy + 5 * $u) (19 * $u) (16.5 * $u) (8 * $u)))
    # Eyes with a highlight.
    foreach ($x in 8.5, 15.5) {
        $g.FillEllipse($faceBrush, $ox + ($x - 1.9) * $u, $oy + (12.5 - 1.9) * $u, 3.8 * $u, 3.8 * $u)
        $g.FillEllipse($bodyBrush, $ox + ($x + 0.6 - 0.55) * $u, $oy + (11.8 - 0.55) * $u, 1.1 * $u, 1.1 * $u)
    }
    # Smile.
    $mouth = New-Object System.Drawing.Pen $face, ([single](1.2 * $u))
    $mouth.StartCap = 'Round'; $mouth.EndCap = 'Round'
    $g.DrawBezier($mouth, $ox + 10.4 * $u, $oy + 16.2 * $u, $ox + 11.2 * $u, $oy + 17.6 * $u,
        $ox + 12.8 * $u, $oy + 17.6 * $u, $ox + 13.6 * $u, $oy + 16.2 * $u)
}

function New-Canvas([int]$w, [int]$h, $format) {
    $bmp = New-Object System.Drawing.Bitmap $w, $h, $format
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $g.SmoothingMode = 'AntiAlias'
    $g.PixelOffsetMode = 'HighQuality'
    $g.TextRenderingHint = 'AntiAliasGridFit'
    @($bmp, $g)
}

$black = [System.Drawing.Color]::FromArgb(255, 10, 10, 10)
$white = [System.Drawing.Color]::FromArgb(255, 250, 250, 250)

# App icon: white mascot on a black rounded tile, so it reads on light and dark taskbars.
$frames = foreach ($size in 16, 32, 48, 256) {
    $bmp, $g = New-Canvas $size $size ([System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
    $g.Clear([System.Drawing.Color]::Transparent)
    $g.FillPath((New-Object System.Drawing.SolidBrush $black), (New-RoundRect 0 0 $size $size ([single]($size * 0.22))))
    $pad = $size * 0.1
    Draw-Mascot $g $pad $pad ($size - 2 * $pad) $white $black
    $g.Dispose()
    $ms = New-Object System.IO.MemoryStream
    if ($size -ge 256) {
        # Large frame: PNG-compressed (Vista+).
        $bmp.Save($ms, [System.Drawing.Imaging.ImageFormat]::Png)
    } else {
        # Small frames: classic 32-bit DIB (BITMAPINFOHEADER, bottom-up BGRA, then an AND mask),
        # which every icon consumer reads.
        $bw = New-Object System.IO.BinaryWriter $ms
        $bw.Write([uint32]40); $bw.Write([int32]$size); $bw.Write([int32]($size * 2))
        $bw.Write([uint16]1); $bw.Write([uint16]32); $bw.Write([uint32]0)
        $maskStride = [int]([math]::Ceiling($size / 32.0) * 4)
        $bw.Write([uint32]($size * $size * 4 + $maskStride * $size))
        $bw.Write([int32]0); $bw.Write([int32]0); $bw.Write([uint32]0); $bw.Write([uint32]0)
        for ($y = $size - 1; $y -ge 0; $y--) {
            for ($x = 0; $x -lt $size; $x++) {
                $c = $bmp.GetPixel($x, $y)
                $bw.Write([byte]$c.B); $bw.Write([byte]$c.G); $bw.Write([byte]$c.R); $bw.Write([byte]$c.A)
            }
        }
        # All-zero AND mask: the alpha channel carries transparency.
        $bw.Write((New-Object byte[] ($maskStride * $size)))
        $bw.Flush()
    }
    $bmp.Dispose()
    , @($size, $ms.ToArray())
}
# ICO container: DIB frames for small sizes, PNG for 256.
$ico = New-Object System.IO.MemoryStream
$w = New-Object System.IO.BinaryWriter $ico
$w.Write([uint16]0); $w.Write([uint16]1); $w.Write([uint16]$frames.Count)
$offset = 6 + 16 * $frames.Count
foreach ($f in $frames) {
    $dim = if ($f[0] -ge 256) { 0 } else { $f[0] }
    $w.Write([byte]$dim); $w.Write([byte]$dim); $w.Write([byte]0); $w.Write([byte]0)
    $w.Write([uint16]1); $w.Write([uint16]32)
    $w.Write([uint32]$f[1].Length); $w.Write([uint32]$offset)
    $offset += $f[1].Length
}
foreach ($f in $frames) { $w.Write([byte[]]$f[1]) }
$w.Flush()
[System.IO.File]::WriteAllBytes((Join-Path $art 'bloop.ico'), $ico.ToArray())

$rgb = [System.Drawing.Imaging.PixelFormat]::Format24bppRgb

# Wizard side image (2x of Inno's 164x314): black field, big mascot, wordmark.
$bmp, $g = New-Canvas 328 628 $rgb
$g.Clear($black)
Draw-Mascot $g 64 300 200 $white $black
$font = New-Object System.Drawing.Font 'Segoe UI Semibold', 40, ([System.Drawing.FontStyle]::Regular), ([System.Drawing.GraphicsUnit]::Pixel)
$fmt = New-Object System.Drawing.StringFormat
$fmt.Alignment = 'Center'
$g.DrawString('bloop', $font, (New-Object System.Drawing.SolidBrush $white), (New-Object System.Drawing.RectangleF 0, 520, 328, 60), $fmt)
# A notch hanging from the top edge, like on screen.
$g.FillPath((New-Object System.Drawing.SolidBrush $white), (New-RoundRect 84 -40 160 76 28))
$g.Dispose()
$bmp.Save((Join-Path $art 'wizard.bmp'), [System.Drawing.Imaging.ImageFormat]::Bmp)
$bmp.Dispose()

# Wizard header image (2x of 55x55): black mascot on white.
$bmp, $g = New-Canvas 110 110 $rgb
$g.Clear([System.Drawing.Color]::White)
Draw-Mascot $g 10 10 90 $black $white
$g.Dispose()
$bmp.Save((Join-Path $art 'wizard-small.bmp'), [System.Drawing.Imaging.ImageFormat]::Bmp)
$bmp.Dispose()

Get-ChildItem $art | ForEach-Object { '{0}  {1} bytes' -f $_.Name, $_.Length }
