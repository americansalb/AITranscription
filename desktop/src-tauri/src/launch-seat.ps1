param(
    [Parameter(Mandatory=$true)] [string]$Role,
    [Parameter(Mandatory=$true)] [int]$Instance,
    [Parameter(Mandatory=$true)] [string]$ProjectDir,
    [Parameter(Mandatory=$true)] [string]$ClaudeExe,
    [Parameter(Mandatory=$true)] [string]$JoinPrompt
)

# vaak Layer 1 per-seat self-healing wrapper.
# Invoked by AITranscription/desktop/src-tauri/src/launcher.rs (do_spawn_member).
# Mirrors AITranscription/launch-team.ps1 per-seat body so vaak.exe-spawned and
# script-spawned teams share one resilience surface.
#
# Pins addressed (this round):
#   #634(1) Sentinel exit:        .vaak/stop-<role>-<instance> file -> graceful drain, no relaunch
#   #634(2) Port-probe:           wait up to 60s for 127.0.0.1:7865 before relaunching claude
#   #635    Externalized:         this file ships separately, not embedded in Rust string literals
#   pin M   --resume <session-id>: wrapper owns session_id (UUIDv4 generated on fresh)
#                                   per architect #648 P3 pivot — no Layer 3 dependency
#   #1+18   Backoff + crash-loop: 2,4,8,16,30s exp; 10 restarts in 60s -> 60s pause (Q3)
#   J(deg)  Time-only fresh trigger: >=4h since last_fresh_at -> drop --resume (DC-2 degrade)
#   UX P1   Seat .json refresh on EVERY iteration (wrapper_pid + last_active_at_ms)
#   D       JSONL log:            .vaak/logs/<role>-<instance>.jsonl

Set-Location $ProjectDir
$env:VAAK_ROLE        = $Role
$env:VAAK_INSTANCE    = "$Instance"
$env:VAAK_PROJECT_DIR = $ProjectDir

$seatLabel    = "$Role-$Instance"
$seatsDir     = Join-Path $ProjectDir '.vaak\sessions'
$logsDir      = Join-Path $ProjectDir '.vaak\logs'
New-Item -ItemType Directory -Force -Path $seatsDir | Out-Null
New-Item -ItemType Directory -Force -Path $logsDir  | Out-Null

$seatJsonPath = Join-Path $seatsDir "$seatLabel.json"
$logPath      = Join-Path $logsDir  "$seatLabel.jsonl"
$sentinelPath = Join-Path $ProjectDir ".vaak\stop-$seatLabel"
$resumeNudge  = "Re-engage in autonomous team mode. Handle any unread team messages, then call mcp__vaak__project_wait to enter standby. Continue indefinitely."

function Write-VaakLog($ev, $payload) {
    $entry = [pscustomobject]@{
        ts      = (Get-Date -Format 'o')
        seat    = $seatLabel
        event   = $ev
        payload = $payload
    } | ConvertTo-Json -Compress -Depth 6
    try { Add-Content -Path $logPath -Value $entry -Encoding UTF8 } catch { }
}

function Test-VaakHostAlive {
    try {
        $c = New-Object System.Net.Sockets.TcpClient
        $r = $c.BeginConnect('127.0.0.1', 7865, $null, $null)
        $ok = $r.AsyncWaitHandle.WaitOne(500)
        $c.Close()
        return $ok
    } catch { return $false }
}

$restartTimes = @()
$attempt = 0

while ($true) {
    if (Test-Path $sentinelPath) {
        Remove-Item $sentinelPath -Force -ErrorAction SilentlyContinue
        Write-VaakLog 'sentinel_exit' @{ reason = 'graceful drain via sentinel file' }
        Write-Host "[vaak-launch][$seatLabel] sentinel observed - exiting wrapper" -ForegroundColor Magenta
        break
    }

    $attempt++
    $now = Get-Date

    # Crash-loop guard (Q3 calibration per architect #643): 10 restarts in 60s -> 60s pause.
    # Looser than the original 5/60s/5min so legit cold-pipe-race relaunches don't trip it.
    $restartTimes = @($restartTimes | Where-Object { ($now - $_).TotalSeconds -lt 60 })
    if ($restartTimes.Count -ge 10) {
        Write-VaakLog 'crash_loop_pause' @{ recent_restarts = $restartTimes.Count; pause_secs = 60 }
        Write-Host "[vaak-launch][$seatLabel] crash-looping (10 restarts within 60s). Pausing 60s." -ForegroundColor Red
        Start-Sleep -Seconds 60
        $restartTimes = @()
    }

    if ($attempt -gt 1) {
        $probeStart = Get-Date
        while (-not (Test-VaakHostAlive)) {
            if (((Get-Date) - $probeStart).TotalSeconds -ge 60) {
                Write-VaakLog 'port_probe_timeout' @{ host = '127.0.0.1:7865' }
                Write-Host "[vaak-launch][$seatLabel] port probe timeout, proceeding anyway" -ForegroundColor Yellow
                break
            }
            Start-Sleep -Seconds 2
        }
    }

    # Decide branch + session-id (architect #648 P3 pivot: wrapper owns session_id, no
    # dependency on Layer 3 hooks). Pin J degraded to time-only (DC-2): 4h since last_fresh_at.
    $useResume    = $false
    $sessionId    = $null
    $reasonFresh  = 'first_launch'
    $lastFreshAt  = $null
    $seatData     = $null

    if (Test-Path $seatJsonPath) {
        try { $seatData = Get-Content $seatJsonPath -Raw -ErrorAction Stop | ConvertFrom-Json } catch { $seatData = $null }
    }

    if ($seatData -ne $null -and $attempt -gt 1) {
        $ageHrs = if ($seatData.last_fresh_at) { (($now - (Get-Date $seatData.last_fresh_at)).TotalHours) } else { 999 }
        if ($ageHrs -ge 4) {
            $reasonFresh = ("wall_clock_{0:N1}h" -f $ageHrs)
        } elseif ($seatData.session_id) {
            $useResume = $true
            $sessionId = $seatData.session_id
        }
    }

    if (-not $useResume) {
        $sessionId   = [guid]::NewGuid().ToString()
        $lastFreshAt = (Get-Date -Format 'o')
    } else {
        $lastFreshAt = if ($seatData.last_fresh_at) { $seatData.last_fresh_at } else { (Get-Date -Format 'o') }
    }

    # UX P1: refresh wrapper_pid + last_active_at_ms on EVERY iteration (both branches).
    # Panel discovery (evil-arch #641 P1) reads this; stale PIDs trigger false [Restart] prompts.
    [pscustomobject]@{
        role              = $Role
        instance          = $Instance
        session_id        = $sessionId
        last_fresh_at     = $lastFreshAt
        wrapper_pid       = $PID
        last_active_at_ms = [int64]([DateTimeOffset]::Now.ToUnixTimeMilliseconds())
    } | ConvertTo-Json -Depth 6 | Set-Content -Path $seatJsonPath -Encoding UTF8

    Write-Host ''
    Write-Host "[vaak-launch][$seatLabel] attempt $attempt @ $(Get-Date -Format 'HH:mm:ss')  use_resume=$useResume session_id=$sessionId" -ForegroundColor Cyan

    $exitCode = 0
    if ($useResume) {
        Write-VaakLog 'claude_resume' @{ session_id = $sessionId; attempt = $attempt }
        & $ClaudeExe --dangerously-skip-permissions --resume $sessionId $resumeNudge
        $exitCode = $LASTEXITCODE
    } else {
        Write-VaakLog 'claude_fresh' @{ attempt = $attempt; reason = $reasonFresh; session_id = $sessionId }
        & $ClaudeExe --dangerously-skip-permissions --session-id $sessionId $JoinPrompt
        $exitCode = $LASTEXITCODE
    }

    Write-VaakLog 'claude_exit' @{ exit_code = $exitCode; attempt = $attempt }
    $restartTimes += $now

    $backoff = [Math]::Min([Math]::Pow(2, [Math]::Min($attempt, 5)), 30)
    Write-Host "[vaak-launch][$seatLabel] claude exited code=$exitCode. Restarting in ${backoff}s..." -ForegroundColor Yellow
    Start-Sleep -Seconds $backoff
}
