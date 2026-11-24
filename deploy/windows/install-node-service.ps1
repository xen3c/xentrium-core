# Installs the Xentrium node as an auto-start scheduled task.
# Run elevated (admin). Example:
#   powershell -ExecutionPolicy Bypass -File deploy\windows\install-node-service.ps1 `
#       -WorkingDir C:\xentrium\xentrium-core -Config config\deploy\node3.toml `
#       -Bootstrap "1.2.3.4:443"

param(
    [Parameter(Mandatory=$true)][string]$WorkingDir,
    [Parameter(Mandatory=$true)][string]$Config,
    [string]$Bootstrap = "",
    [string]$TaskName = "Xentrium Node"
)

$supervisor = Join-Path $WorkingDir "deploy\windows\xentrium-service.ps1"
if (!(Test-Path $supervisor)) { throw "supervisor script not found: $supervisor" }

# Ensure supervisor log directory exists
New-Item -ItemType Directory -Force -Path (Join-Path $WorkingDir "data") | Out-Null

$argLine = "-NoProfile -ExecutionPolicy Bypass -File `"$supervisor`" -WorkingDir `"$WorkingDir`" -Config `"$Config`" -Bootstrap `"$Bootstrap`""

$action = New-ScheduledTaskAction -Execute "powershell.exe" -Argument $argLine
$trigger = New-ScheduledTaskTrigger -AtStartup
$principal = New-ScheduledTaskPrincipal -UserId "SYSTEM" -LogonType ServiceAccount -RunLevel Highest
$settings = New-ScheduledTaskSettingsSet -RestartCount 999 -RestartInterval (New-TimeSpan -Minutes 1) `
    -ExecutionTimeLimit ([TimeSpan]::Zero) -StartWhenAvailable

Register-ScheduledTask -TaskName $TaskName -Action $action -Trigger $trigger `
    -Principal $principal -Settings $settings -Force -ErrorAction Stop | Out-Null

Write-Host "Installed scheduled task '$TaskName'"
Write-Host "Starts at boot as SYSTEM; supervisor restarts the node on exit."
Write-Host "Start it now with:  Start-ScheduledTask -TaskName `"$TaskName`""
Write-Host "Remove it with:     Unregister-ScheduledTask -TaskName `"$TaskName`" -Confirm:`$false"
