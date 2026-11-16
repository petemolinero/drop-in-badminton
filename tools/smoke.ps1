# Start the app, give it 20 seconds, and report whether it's still running,
# with a screenshot of the screen and anything it wrote to crash.log.
param([string]$Exe, [string]$Name)
$env:DROP_IN_BADMINTON_NO_UPDATE = '1'
$env:DROP_IN_BADMINTON_NO_DIALOG = '1'
$log = Join-Path $env:APPDATA 'drop-in-badminton\drop-in-badminton\data\crash.log'
Remove-Item $log -ErrorAction SilentlyContinue
$err = "smoke-$Name-stderr.txt"
$p = Start-Process -FilePath $Exe -PassThru -RedirectStandardError $err
Start-Sleep -Seconds 20
Add-Type -AssemblyName System.Windows.Forms, System.Drawing
$b = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds
$bmp = New-Object System.Drawing.Bitmap $b.Width, $b.Height
[System.Drawing.Graphics]::FromImage($bmp).CopyFromScreen($b.Location, [System.Drawing.Point]::Empty, $b.Size)
$bmp.Save("smoke-$Name.png")
"--- stderr"; Get-Content $err -ErrorAction SilentlyContinue
"--- crash.log"; Get-Content $log -ErrorAction SilentlyContinue
if ($p.HasExited) { "FAILED: $Exe exited with code $($p.ExitCode)"; exit 1 }
"OK: $Exe still running after 20 s"
Stop-Process $p
