# Installs the Xentrium node as an auto-start scheduled task plus a
# periodic health-check task. Run elevated (admin). Example:
#   powershell -ExecutionPolicy Bypass -File deploy\windows\install-node-service.ps1 `
#       -WorkingDir C:\xentrium\xentrium-core -Config config\deploy\node3.toml `
#       -Bootstrap "1.2.3.4:443" -Port 30305 -LogPath C:\xentrium\xentrium-core\data\node3\xentrium.log

param(
    [Parameter(Mandatory=$true)][string]$WorkingDir,
    [Parameter(Mandatory=$true)][string]$Config,
    [string]$Bootstrap = "",
    [int]$Port = 30304,
    [string]$LogPath = "",
    [string]$TaskName = "Xentrium Node",
    [string]$HealthTaskName = "Xentrium Health"
)

$supervisor = Join-Path $WorkingDir "deploy\windows\xentrium-service.ps1"
$health = Join-Path $WorkingDir "deploy\windows\xentrium-health.ps1"
if (!(Test-Path $supervisor)) { throw "supervisor script not found: $supervisor" }
if (!(Test-Path $health)) { throw "health script not found: $health" }
if ($LogPath -eq "") { $LogPath = Join-Path $WorkingDir "data\xentrium.log" }

# Ensure supervisor log directory exists
New-Item -ItemType Directory -Force -Path (Join-Path $WorkingDir "data") | Out-Null

$principal = New-ScheduledTaskPrincipal -UserId "SYSTEM" -LogonType ServiceAccount -RunLevel Highest

# --- Node supervisor task: runs at boot, supervisor loop restarts node on exit ---
$argLine = "-NoProfile -ExecutionPolicy Bypass -File `"$supervisor`" -WorkingDir `"$WorkingDir`" -Config `"$Config`" -Bootstrap `"$Bootstrap`""
$action = New-ScheduledTaskAction -Execute "powershell.exe" -Argument $argLine
$trigger = New-ScheduledTaskTrigger -AtStartup
$settings = New-ScheduledTaskSettingsSet -RestartCount 999 -RestartInterval (New-TimeSpan -Minutes 1) `
    -ExecutionTimeLimit ([TimeSpan]::Zero) -StartWhenAvailable

Register-ScheduledTask -TaskName $TaskName -Action $action -Trigger $trigger `
    -Principal $principal -Settings $settings -Force -ErrorAction Stop | Out-Null

# --- Health task: every 5 minutes; kills a wedged node so supervisor restarts it ---
$healthArgs = "-NoProfile -ExecutionPolicy Bypass -File `"$health`" -Port $Port -LogPath `"$LogPath`" -RestartOnFail"
$healthAction = New-ScheduledTaskAction -Execute "powershell.exe" -Argument $healthArgs
# TimeSpan.MaxValue serializes to a duration the task schema rejects;
# 10 years is effectively "forever" and validates fine.
$healthTrigger = New-ScheduledTaskTrigger -Once -At (Get-Date) `
    -RepetitionInterval (New-TimeSpan -Minutes 5) -RepetitionDuration (New-TimeSpan -Days 3650)
$healthSettings = New-ScheduledTaskSettingsSet -StartWhenAvailable -ExecutionTimeLimit (New-TimeSpan -Minutes 2)

Register-ScheduledTask -TaskName $HealthTaskName -Action $healthAction -Trigger $healthTrigger `
    -Principal $principal -Settings $healthSettings -Force -ErrorAction Stop | Out-Null

Write-Host "Installed scheduled tasks '$TaskName' and '$HealthTaskName'"
Write-Host "Node starts at boot as SYSTEM; supervisor restarts it on exit;"
Write-Host "health probe runs every 5 min and force-restarts a wedged node."
Write-Host "Start it now with:  Start-ScheduledTask -TaskName `"$TaskName`""
Write-Host "Remove both with:   Unregister-ScheduledTask -TaskName `"$TaskName`", `"$HealthTaskName`" -Confirm:`$false"
