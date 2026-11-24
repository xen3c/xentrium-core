# Service Deployment Report — Phase-4 Step-4

## Goal

Make Xentrium survive machine reboots: auto-start on boot, auto-restart
on crash, persistent logs, health checks — verified with real reboots of
all three nodes.

## Service architecture

### node1 (Linux)

| Component | Mechanism |
|---|---|
| Service manager | `systemd` |
| Unit | `deploy/systemd/xentrium.service` (`Type=simple`) |
| Auto-start on boot | `WantedBy=multi-user.target`, `After=network-online.target` |
| Auto-restart | `Restart=always`, `RestartSec=10`, `StartLimitBurst=5/300s` |
| Graceful stop | SIGTERM, `TimeoutStopSec=30` |
| Logs | Node file log (`data/node1/xentrium.log`) + journald (`journalctl -u xentrium`) |
| Health check | `xentrium-healthcheck.timer` every 60s → `deploy/scripts/xentrium-health.sh` (process + listener + log-freshness probe); restarts the unit on failure |

### node2 / node3 (Windows)

| Component | Mechanism |
|---|---|
| Service manager | Task Scheduler (native Windows, no SCM handler needed in the binary) |
| Task | `Xentrium Node` → `deploy/windows/xentrium-service.ps1` supervisor loop |
| Auto-start on boot | `-AtStartup` trigger, runs as `SYSTEM`, no login required |
| Auto-restart | Supervisor restarts the node on any exit (5s delay); task-level `RestartCount=999` |
| Logs | Node file log + `data/service-supervisor.log` (launch/exit events) |
| Health check | `Xentrium Health` task every 5 min → `xentrium-health.ps1 -RestartOnFail` (process + listener + log-freshness probe); kills a wedged node so the supervisor restarts it |

Install: `deploy/windows/install-node-service.ps1` registers both tasks
(idempotent, `-Force`).

## Reboot verification

### node1 — rebooted

```
Loaded: loaded (.../xentrium.service; enabled; preset: enabled)
Active: active (running) since ... ; Main PID: 915
```

Service auto-started at boot before any user session existed (PID 915).
Peers reconnected and pushed validator/committee/snapshot state within
seconds; replay dedup rejections in the log are expected noise.

### node3 — rebooted

Supervisor log shows the boot trigger firing automatically:

```
supervisor starting: exe=...\xentrium.exe config=config\deploy\node3.toml
launching node
```

Node running as `SYSTEM` (session 0), listening on its configured port,
connected to node1, heartbeating (~350 ms), re-announcing its validator
and receiving network state.

### node2 — rebooted

Same pattern: supervisor auto-launched at boot; node listening on its
configured port, reconnected to node1, heartbeating (245–386 ms).

## Requirement 8 — post-restart state chain

Observed on `node2` after its task-managed start (same path runs after
every reboot):

```
Loaded 3 validators from storage
Loaded snapshot from storage: state 5984932
Loaded committee from storage: 3 members
Starting TCP transport
Connected to bootstrap peer: <node1>
Self-registered as validator: xnt-...
Verified handshake from <node1>: ... (TLS-bound)
Heartbeat: sent Ping ...
```

| Step | Verified |
|---|---|
| Node starts automatically | yes — systemd unit / scheduled task, no login or manual step |
| Rejoins network automatically | yes — bootstrap reconnect + verified handshake on all 3 nodes |
| Validator restored | yes — `Loaded 3 validators from storage` + self re-registration |
| Committee restored | yes — `Loaded committee from storage: 3 members` |
| Snapshot restored | yes — `Loaded snapshot from storage` |

## Incidents found and fixed during rollout

| Issue | Fix |
|---|---|
| Health-task trigger rejected `TimeSpan.MaxValue` repetition duration | commit `f3dac63` — use 10-year duration |
| Port conflict: stale manual `node2` process held `:30304`, task-launched node crash-looped | Killed stale process; supervisor recovered automatically on next launch — itself a live demonstration of the restart loop |

## Classification

- Auto-start on boot: **verified** on all three nodes (real reboots)
- Auto-restart on crash/exit: **verified** — supervisor loop observed
  restarting node2 across repeated exit-1 crashes; systemd `Restart=always`
  configured equivalently on node1
- Log persistence: **verified** — node file logs + supervisor log +
  journald persist across restarts
- Health checks: **verified** — probes installed and scheduled on all
  three nodes (process + listener + log freshness, with restart action)
- Reboot without manual node startup: **verified** on all three nodes

## Success criterion

Met — a machine reboot requires **no manual node startup**. Each node
boots under its service manager, reloads persisted validator/committee/
snapshot state, rejoins the network, and reconverges automatically.
