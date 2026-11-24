# Xentrium node supervisor — restart-on-exit loop.
# Run by Task Scheduler at boot (SYSTEM account, no login required).
# The node itself writes <data_dir>\xentrium.log; this wrapper logs its
# own supervisor events to <data_dir>\xentrium-service.log.

param(
    [Parameter(Mandatory=$true)][string]$WorkingDir,
    [Parameter(Mandatory=$true)][string]$Config,
    [string]$Bootstrap = "",
    [string]$DataDir = ".\data"
)

Set-Location $WorkingDir
$exe = Join-Path $WorkingDir "target\release\xentrium.exe"
$logDir = Split-Path -Parent $Config | Split-Path -Parent
$supLog = Join-Path $WorkingDir "data\service-supervisor.log"

function Log($msg) {
    $line = "$(Get-Date -Format 'yyyy-MM-ddTHH:mm:ssZ') $msg"
    Add-Content -Path $supLog -Value $line
}

Log "supervisor starting: exe=$exe config=$Config bootstrap=$Bootstrap"

$backoff = 5
while ($true) {
    $argsList = @('--config', $Config)
    if ($Bootstrap -ne "") { $argsList += @('-b', $Bootstrap) }

    Log "launching node"
    $proc = Start-Process -FilePath $exe -ArgumentList $argsList `
        -WorkingDirectory $WorkingDir -NoNewWindow -PassThru -Wait
    Log "node exited (code $($proc.ExitCode)); restarting in ${backoff}s"
    Start-Sleep -Seconds $backoff
    # Reset backoff after a stable run (>60s alive handled implicitly:
    # fast-crash loops just get the fixed 5s delay)
}
