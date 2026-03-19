# NA-0004 Reference Deployment Validation Evidence

Status: Recorded

Purpose:
- Record the stronger-than-`qsl` reference deployment used for `NA-0201`.
- Capture mixed message + attachment validation over the real relay and the deployed `qsl-attachments` reference host.
- Distinguish bounded saturation/degradation, correctness failure, and deployment immaturity truthfully.

Non-goals:
- No default-path promotion in this item.
- No legacy deprecation in this item.
- No qsl-server source changes.
- No attachment semantic redesign.

## Reference deployment target

Reference host:
- SSH alias: `qatt`
- Public name: `qatt.ddnsfree.com`
- Materially stronger than constrained-host `qsl`
- Observed baseline snapshot during validation:
  - `4` vCPU
  - `MemTotal: 16080392 kB`
  - `/` filesystem: `102888095744` bytes total, `100381609984` bytes available

Constrained baseline retained for comparison:
- relay host remains `qsl`
- constrained-host profile remains the weak-host / weak-relay baseline from `NA-0003`

Ingress / topology:
- real relay: `https://qsl.ddnsfree.com`
- reference attachment service: `https://qatt.ddnsfree.com`
- `qsl-attachments` binds on `127.0.0.1:3000`
- Caddy terminates TLS on `qatt.ddnsfree.com`
- storage root remains local disk at `/var/lib/qsl-attachments/data`

Install / update path:
- documented in `docs/NA-0004_reference_deployment_runbook.md`
- local build + binary copy was used for the first install
- no deployment automation was introduced in this item

## Relay preflight

The hard-coded relay compatibility guard from `NA-0011` was rerun before reference traffic validation.

Result:
- canonical loopback `204`
- legacy loopback `204`
- canonical public `204`
- legacy public `204`
- `QSL_RELAY_COMPAT_RESULT PASS code=canonical_ok legacy_compat=present`

This preserved the invariant that reference deployment validation does not begin against a stale relay.

## Validation matrix

Executed required categories:
1. message-only flows over the real relay
2. attachment-only flows over the real relay + reference deployment
3. mixed message + attachment flows
4. threshold ladder: `< 4 MiB`, `= 4 MiB`, `> 4 MiB`
5. large-file ladder: `16 MiB`, `64 MiB`, `100 MiB` target class
6. interruption / restart: upload interruption-resume and `qsl-attachments` restart
7. bounded concurrency ramp
8. short soak / endurance window with mixed traffic
9. secret-hygiene audit under real traffic

Measurements captured:
- qsc send / receive / confirm durations
- `qatt` `load1`, `MemAvailable`, and `qsl-attachments` RSS
- `qsl` `load1` and relay RSS
- disk delta on the attachment service host
- explicit reject / timeout / retry outcomes where they occurred

## Stage-by-stage results

### Message-only flow

Case: `msgonly`
- result: pass
- send: `2537 ms`
- receive: `1679 ms`
- confirm: `581 ms`
- classification: success

### Attachment-only service-backed flows

Case: `att5m`
- result: pass
- send: `59574 ms`
- receive: `10136 ms`
- confirm: `1519 ms`
- classification: success

Case: `att16m`
- result: pass
- send: `169476 ms`
- receive: `12709 ms`
- confirm: `1565 ms`
- classification: success

Case: `att64m`
- result: pass
- send: `263823 ms`
- receive: `42663 ms`
- confirm: `1918 ms`
- classification: success

Case: `att100m`
- result: pass
- send: `241331 ms`
- receive: `57683 ms`
- confirm: `1840 ms`
- classification: success

### Mixed message + attachment flow

Case: `mix16m`
- result: pass
- message send: `2630 ms`
- file send: `176497 ms`
- receive: `13652 ms`
- confirm: `1664 ms`
- classification: success

### Threshold ladder

Case: corrected `attlt4` rerun (`< 4 MiB`)
- result: sender completed; receiver timed out at the fixed harness window while still advancing chunks
- send: `445424 ms`
- receive: timed out at `180618 ms`
- classification: bounded saturation/degradation on the weak relay / legacy threshold path
- reasoning: the receive side kept making forward progress, retries stayed bounded, and the attachment service path remained idle; this was not a `qatt` correctness failure

Case: fresh `eq4ref` rerun (`= 4 MiB`)
- result: sender-side final chunk rejected after bounded retries with `relay_inbox_queue_full`; receive never started
- send: `603655 ms`
- classification: bounded saturation/degradation on the weak relay / legacy threshold path
- reasoning: the attachment service host stayed effectively idle (`qatt` max `load1` `0.0`, max RSS `26392 kB`, disk delta `4096 B`) while the relay queue saturated on the final in-message chunk

Case: `att5m` (`> 4 MiB`)
- result: pass via the attachment service path
- classification: success

### Restart / recovery

Case: `restart16`
- result: pass
- file send: `184746 ms`
- service restart executed before receive
- receive: `15441 ms`
- confirm: `1732 ms`
- classification: success

Case: `abort16`
- result: pass
- intentional upload interruption recorded as `alice.file_send_abort_1 rc=1`
- resumed send: `184795 ms`
- receive: `21940 ms`
- confirm: `1760 ms`
- classification: success
- reasoning: resumed upload continued the same attachment id and completed without exposing an incomplete object to the receiver

### Bounded concurrency

Cases: `conc16a`, `conc16b`
- result: both passed concurrently
- `conc16a`: message send `5166 ms`, file send `180241 ms`, receive `16958 ms`, confirm `2977 ms`
- `conc16b`: message send `6307 ms`, file send `179153 ms`, receive `23726 ms`, confirm `1631 ms`
- classification: success

### Short soak / endurance

Case: `soak5x`
- result: pass
- five consecutive mixed iterations completed cleanly
- per-iteration message send: approximately `2.6-2.9 s`
- per-iteration file send: approximately `57-60 s`
- per-iteration receive: approximately `9-11 s`
- per-iteration confirm: approximately `1.6-3.0 s`
- classification: success

Authoritative threshold note:
- the initial `attlt4` / `atteq4` matrix rows were harness artifacts and are superseded by the corrected `attlt4` rerun and the fresh `eq4ref` rerun recorded here

## Resource observations

Representative aggregate measurements:

Case: `att100m`
- `qatt` max `load1`: `0.02`
- `qatt` min `MemAvailable`: `15405144 kB`
- `qatt` max `qsl-attachments` RSS: `83900 kB`
- `qatt` disk delta: `104882176 B`
- `qsl` max `load1`: `0.68`
- relay RSS: `27908 kB`

Case: `att64m`
- `qatt` max `load1`: `0.11`
- `qatt` min `MemAvailable`: `15406472 kB`
- `qatt` max RSS: `82768 kB`
- `qatt` disk delta: `67137536 B`
- `qsl` max `load1`: `0.46`

Case: `mix16m`
- `qatt` max `load1`: `0.03`
- `qatt` min `MemAvailable`: `15406684 kB`
- `qatt` max RSS: `83900 kB`
- `qatt` disk delta: `16797696 B`
- `qsl` max `load1`: `0.68`

Case: `restart16`
- `qatt` max `load1`: `0.06`
- `qatt` min `MemAvailable`: `15417572 kB`
- `qatt` max RSS: `83900 kB`
- `qatt` disk delta: `16814080 B`
- `qsl` max `load1`: `1.35`

Case: `conc16a` / `conc16b`
- `qatt` max `load1`: `0.34`
- `qatt` min `MemAvailable`: `15463392 kB` / `15469156 kB`
- `qatt` max RSS: `21984 kB`
- `qatt` disk delta: `33595392 B`

Case: `soak5x`
- `qatt` max `load1`: `0.09`
- `qatt` min `MemAvailable`: `15455284 kB`
- `qatt` max RSS: `26392 kB`
- `qatt` disk delta: `26292224 B`

Interpretation:
- the stronger reference host remained substantially under stress during the service-backed ladder
- the remaining observed degradation concentrated on the weak relay / legacy threshold path, not the stronger attachment-service host

## Mixed message + attachment evidence

The reference deployment was not attachment-only.

Mixed-traffic proof:
- `mix16m` passed end-to-end with one message plus one `16 MiB` attachment in the same exchange
- `conc16a` and `conc16b` passed concurrently with both message and attachment traffic
- `soak5x` completed five consecutive mixed message + attachment iterations

This is sufficient to say the stronger reference deployment handled integrated message + attachment traffic cleanly over the real relay.

## Secret-hygiene audit

Checks performed:
- evidence bundle string scan for bearer / capability / secret markers
- `qatt` `qsl-attachments` journal tail scan
- `qatt` Caddy journal tail scan

Result:
- no raw bearer token values were recorded in the evidence bundle
- no `resume_token`, `fetch_capability`, `X-QATT-Resume-Token`, or `X-QATT-Fetch-Capability` values were found in the `qatt` service or proxy journal tails
- bundle hits were limited to variable names and operator-safe script references, not stored secret values
- no plaintext attachment content was exposed on service surfaces

## Classification summary

Successes:
- message-only flow
- `> 4 MiB` attachment-service smoke
- `16 MiB`, `64 MiB`, and `100 MiB` attachment-service runs
- mixed `16 MiB` message + attachment flow
- service restart recovery
- upload interruption-resume
- bounded concurrency ramp
- short mixed soak window
- secret-hygiene audit

Bounded saturation / degradation:
- corrected `< 4 MiB` threshold rerun: weak-relay / legacy-path degradation caused a receive timeout despite continued forward progress
- exact `= 4 MiB` threshold rerun: weak-relay / legacy-path saturation failed closed with `relay_inbox_queue_full` on the sender-side final chunk after bounded retries

Correctness failures:
- none proven in the stronger reference deployment or service-backed path during `NA-0201`

Deployment immaturity:
- none load-bearing on the stronger reference host once the runbook install path and relay preflight were applied

## Promotion-gate conclusion

Reference deployment validation is strong enough to say:
- the stronger attachment-service deployment works well with mixed message + attachment traffic
- the remaining uncertainty is not immediate reference-host hardening
- the honest next blocker before any default-path promotion or legacy deprecation is a broader message + attachment stress / soak / chaos lane that goes beyond this bounded reference run set

That means `NA-0201` should advance to the dedicated kitchen-sink validation lane rather than directly to promotion or back to service-local hardening.
