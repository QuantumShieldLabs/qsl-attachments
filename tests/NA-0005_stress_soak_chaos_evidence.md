# NA-0005 Stress / Soak / Chaos Evidence

Status: Recorded

Purpose:
- Record the bounded kitchen-sink validation lane for `NA-0201A`.
- Capture message-only, attachment-only, and mixed traffic behavior under weak-host / weak-relay and stronger reference-host profiles.
- Distinguish correctness failure, bounded saturation/degradation, and deployment immaturity truthfully.

Non-goals:
- No default attachment-path promotion in this item.
- No legacy `<= 4 MiB` deprecation in this item.
- No qsl-server source changes.
- No attachment semantic redesign.

## Host roles and topology

Weak-host / weak-relay baseline:
- host: `qsl`
- role: constrained host and real relay baseline
- observed baseline snapshot during validation:
  - `1` vCPU
  - `MemTotal: 977672 kB`
  - `/` filesystem available: `24244109312 B`
- attachment service path used for weak-host comparison: local forward `http://127.0.0.1:13001`

Stronger reference host:
- host: `qatt`
- public name: `qatt.ddnsfree.com`
- observed baseline snapshot during validation:
  - `4` vCPU
  - `MemTotal: 16080392 kB`
  - `/` filesystem available: `100054773760 B`
- attachment service endpoint: `https://qatt.ddnsfree.com`
- service binds on `127.0.0.1:3000` behind Caddy TLS

Shared transport:
- real relay: `https://qsl.ddnsfree.com`
- qsl-server remained transport-only and source-untouched

Storage posture:
- single-node local-disk runtime remained unchanged
- storage roots stayed on local disk under `/var/lib/qsl-attachments/data`

## Mandatory relay preflight

The hard-coded relay compatibility guard from qsl-server `NA-0011` was rerun before live stages began.

Result:
- canonical loopback: `401`
- canonical public: `401`
- `QSL_RELAY_COMPAT_RESULT PASS code=canonical_ok legacy_compat=present`

Interpretation:
- the live relay still served the current canonical header-based API
- live validation began only after that preflight passed

## Matrix and stop rules

Executed stage list:
1. message-only control flow over the real relay
2. mixed message + `5 MiB` attachment on weak-host `qsl`
3. mixed message + `5 MiB` attachment on reference host `qatt`
4. threshold ladder on weak-host / weak-relay baseline:
   - `< 4 MiB`
   - `= 4 MiB`
5. large-file service-backed ladder on `qatt`:
   - `16 MiB`
   - `64 MiB`
   - `100 MiB` target class
6. interruption / restart on `qatt`:
   - service restart before receive
   - client-side upload interruption, then resumed send
7. bounded concurrency ramps:
   - `2` mixed pairs on `qsl`
   - `2`, `4`, and `8` mixed pairs on `qatt`
8. short soak / endurance:
   - `30` minutes mixed traffic on `qatt`
   - `31` completed iterations across `4` rotating pairs
9. secret-hygiene audit under real traffic

Measurements captured for every bounded stage where applicable:
- qsc message send, file send, receive, and confirm timings
- `qsl` / `qatt` `load1`
- `MemAvailable`
- `qsl-attachments` RSS on each host
- relay RSS on `qsl`
- disk delta on the attachment-service host
- explicit reject / retry / timeout codes
- restart window timing where applicable

Stop / escalation rules used:
- do not escalate concurrency until the prior stage stays bounded and honestly classifiable
- stop if correctness failure occurs
- stop if bounded saturation becomes unbounded
- stop if the environment no longer supports truthful classification

Classification rules used:
- `correctness failure`: false delivery state, secret-bearing URL or token leakage, plaintext on service surfaces, broken restart/resume semantics, or dishonest mixed-traffic behavior
- `bounded saturation/degradation`: measured slowdown or queue pressure with honest fail-closed behavior and no integrity lies
- `deployment immaturity`: load-bearing install/ops gap on `qsl` or `qatt`
- `unknown`: load-bearing ambiguity; would stop the directive

## Stage-by-stage results

| Case | Role | Payload | Result | Classification | Key timings / notes |
| --- | --- | ---: | --- | --- | --- |
| `msgonly_relay` | relay | `64 B` | pass | success | send `1152 ms`, receive `1595 ms`, confirm `920 ms`, total `3671 ms` |
| `mix5m_qsl` | weak-host | `5 MiB` | pass | success | msg `1245 ms`, file `27719 ms`, receive `7466 ms`, confirm `1301 ms`, total `37738 ms` |
| `mix5m_qatt` | reference-host | `5 MiB` | pass | success | msg `1259 ms`, file `53846 ms`, receive `7724 ms`, confirm `1522 ms`, total `64360 ms` |
| `threshold_lt4_qsl` | weak-host | `4194303 B` | bounded degraded | bounded saturation | explicit `relay_inbox_queue_full` reject after bounded retries; file-send window `1752 ms` |
| `threshold_eq4_qsl` | weak-host | `4194304 B` | bounded degraded | bounded saturation | sender failed closed after `310347 ms` with bounded `relay_inbox_queue_full` retries |
| `att16_qatt` | reference-host | `16 MiB` | pass | success | file `149795 ms`, receive `12698 ms`, confirm `1055 ms`, total `163553 ms` |
| `att64_qatt` | reference-host | `64 MiB` | pass | success | file `207629 ms`, receive `29261 ms`, confirm `1007 ms`, total `237902 ms` |
| `att100_qatt` | reference-host | `100 MiB` | pass | success | file `134404 ms`, receive `42088 ms`, confirm `996 ms`, total `177493 ms` |
| `restart16_qatt` | reference-host | `16 MiB` | pass | success | file `152535 ms`, receive `9109 ms`, confirm `1208 ms`, total `165372 ms`, restart window `2516 ms` |
| `resume16_qatt` | reference-host | `16 MiB` | pass | success | resumed file `146126 ms`, receive `10241 ms`, confirm `1054 ms`, total `157424 ms`, interrupted send exit `143` |
| `conc2_qsl` | weak-host | `2 x 5 MiB mixed` | pass | success | pair timings `01:1516/29779/15425/1647`, `02:1543/29787/15379/1601` |
| `conc2_qatt` | reference-host | `2 x 5 MiB mixed` | pass | success | pair timings `01:1582/49561/11130/1569`, `02:1552/49530/7988/1525` |
| `conc4_qatt` | reference-host | `4 x 5 MiB mixed` | pass | success | all 4 pairs bounded and successful |
| `conc8_qatt` | reference-host | `8 x 5 MiB mixed` | pass | success | all 8 pairs bounded and successful |
| `soak30_qatt` | reference-host | `30 min mixed` | pass | success | `1800000 ms`, `31` iterations, `4` rotating pairs |

## Exact metrics captured

Representative resource metrics from the authoritative bundle:

Case: `mix5m_qsl`
- `qsl` max `load1`: `0.08`
- `qsl` min `MemAvailable`: `593460 kB`
- `qsl` max `qsl-attachments` RSS: `14924 kB`
- relay RSS: `6344 kB`
- `qsl` disk delta: `5308416 B`

Case: `mix5m_qatt`
- `qsl` max `load1`: `0.04`
- `qsl` min `MemAvailable`: `593500 kB`
- relay RSS: `6344 kB`
- `qatt` max `load1`: `0.08`
- `qatt` min `MemAvailable`: `15494420 kB`
- `qatt` max `qsl-attachments` RSS: `15524 kB`
- `qatt` disk delta: `5283840 B`

Case: `att100_qatt`
- `qsl` max `load1`: `0.20`
- `qsl` min `MemAvailable`: `558404 kB`
- relay RSS: `35908 kB`
- `qatt` max `load1`: `0.00`
- `qatt` min `MemAvailable`: `15309696 kB`
- `qatt` max `qsl-attachments` RSS: `179176 kB`
- `qatt` disk delta: `104943616 B`

Case: `restart16_qatt`
- `qsl` max `load1`: `0.08`
- `qsl` min `MemAvailable`: `558424 kB`
- relay RSS: `35908 kB`
- `qatt` max `load1`: `0.16`
- `qatt` min `MemAvailable`: `15419300 kB`
- `qatt` max `qsl-attachments` RSS: `106628 kB`
- `qatt` disk delta: `16846848 B`

Case: `conc2_qatt`
- `qsl` summary: max `load1 0.00`, min `MemAvailable 551896 kB`, max `qsl-attachments` RSS `15996 kB`, relay RSS `35916 kB`, disk delta `28672 B`
- `qatt` summary: max `load1 0.00`, min `MemAvailable 15460840 kB`, max `qsl-attachments` RSS `32516 kB`, disk delta `10534912 B`

Case: `conc4_qatt`
- `qsl` summary: max `load1 0.16`, min `MemAvailable 551916 kB`, relay RSS `35916 kB`
- `qatt` summary: max `load1 0.00`, min `MemAvailable 15456476 kB`, max `qsl-attachments` RSS `46888 kB`, disk delta `21045248 B`

Case: `conc8_qatt`
- `qsl` summary: max `load1 0.15`, min `MemAvailable 552032 kB`, relay RSS `35916 kB`
- `qatt` summary: max `load1 0.00`, min `MemAvailable 15400304 kB`, max `qsl-attachments` RSS `88620 kB`, disk delta `42070016 B`

Case: `soak30_qatt`
- `qsl` summary: max `load1 0.29`, min `MemAvailable 552340 kB`, relay RSS `35920 kB`, disk delta `9162752 B`
- `qatt` summary: max `load1 0.29`, min `MemAvailable 15435400 kB`, max `qsl-attachments` RSS `58328 kB`, disk delta `172019712 B`

## Weak-host vs reference-host comparison

Exact host roles:
- `qsl` remained the weak-host / weak-relay baseline
- `qatt` remained the stronger reference deployment

Comparison summary:
- the weak baseline still exposed threshold-path degradation at `< 4 MiB` and exact `4 MiB`, both as explicit bounded `relay_inbox_queue_full` failure on the relay / legacy path
- the same lane did not reproduce as an attachment-service correctness problem on `qatt`; service-backed `5 MiB`, `16 MiB`, `64 MiB`, and `100 MiB` reference-host transfers all completed
- `qatt` stayed comfortably bounded through restart, resumed upload, concurrency `2/4/8`, and a `30` minute mixed soak window
- the reference host never showed the kind of pressure that would justify classifying the remaining issue as service-host saturation or deployment immaturity

Interpretation:
- weak-host degradation remains real and should stay in the evidence set
- it should not be over-read as a `qsl-attachments` correctness defect
- the remaining blocker after this lane is the product decision about default attachment-path promotion and legacy in-message behavior, not another host-hardening pass

## Mixed message + attachment evidence

This lane was not attachment-only.

Direct mixed-traffic proof:
- `mix5m_qsl` passed on the weak-host baseline
- `mix5m_qatt` passed on the stronger reference host
- `conc2_qsl` passed with two concurrent mixed pairs on the weak baseline
- `conc2_qatt`, `conc4_qatt`, and `conc8_qatt` passed with mixed traffic on the reference host
- `soak30_qatt` sustained mixed traffic for `30` minutes with `31` successful iterations across `4` rotating pairs

This is sufficient to say the integrated message + attachment system remained truthful under the bounded kitchen-sink lane.

## Secret-hygiene audit

Checks performed:
- evidence bundle string scan for bearer / capability / secret markers
- `qatt` `qsl-attachments` journal tail scan
- `qatt` Caddy journal tail scan
- `qsl` `qsl-attachments` journal tail scan
- `qsl` `qsl-server` journal tail scan

Result:
- no raw bearer token values were recorded in the service or proxy journal captures
- no `resume_token`, `fetch_capability`, `X-QATT-Resume-Token`, or `X-QATT-Fetch-Capability` values were found in the captured journals
- bundle hits were limited to variable names and operator-safe helper-script scaffolding inside the private evidence bundle, not stored secret values in committed artifacts
- no plaintext attachment content appeared on service surfaces

## Classification summary

Correctness failures:
- none proven in this lane

Bounded saturation/degradation:
- `threshold_lt4_qsl`
- `threshold_eq4_qsl`
- reason: both stayed on the weak-host / weak-relay legacy threshold path and failed closed with explicit bounded retry / reject behavior

Deployment immaturity:
- none load-bearing in this lane

Unknown:
- none

## Decision impact

This evidence is strong enough to close the bounded kitchen-sink lane.

Why:
- the stronger reference deployment remained bounded through large files, restart/recovery, concurrency up to `8`, and a `30` minute mixed soak window
- weak-host / weak-relay degradation stayed bounded and honestly classifiable
- no direct `qsl-attachments` runtime gap outranked the product decision anymore

Honest next blocker:
- `NA-0202 — Default Attachment Path Promotion + Legacy In-Message Deprecation Decision`
