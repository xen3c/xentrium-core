# Xentrium health probe (Windows). Exits 0 healthy, 1 unhealthy.
# Checks: process alive, TCP listener bound, node log written <3min ago.
#
# Usage:
#   powershell -File deploy\windows\xentrium-health.ps1 -Port 30304 `
#       -LogPath C:\xentrium\xentrium-core\data\node3\xentrium.log

param(
    [int]$Port = 30304,
    [string]$LogPath = ".\data\node\xentrium.log"
)

function Fail($msg) { Write-Host "HEALTH FAIL: $msg"; exit 1 }

$proc = Get-Process xentrium -ErrorAction SilentlyContinue
if (!$proc) { Fail "no xentrium process" }

$listener = Get-NetTCPConnection -State Listen -LocalPort $Port -ErrorAction SilentlyContinue
if (!$listener) { Fail "nothing listening on :$Port" }

if (!(Test-Path $LogPath)) { Fail "log missing: $LogPath" }
$age = (Get-Date) - (Get-Item $LogPath).LastWriteTime
if ($age.TotalMinutes -gt 3) { Fail "log stale ($([int]$age.TotalMinutes)min without writes)" }

Write-Host "HEALTH OK: process alive, :$Port listening, log fresh"
exit 0
