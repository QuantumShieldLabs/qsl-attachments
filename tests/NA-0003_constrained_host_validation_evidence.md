# NA-0003 Constrained-Host Validation Evidence

Goals: G4, G5

## Scope

`NA-0003` implements the smallest operational controls needed to execute the constrained-host ladder from qsl-protocol `DOC-ATT-002` without changing canonical attachment semantics.

Runtime controls added in this item:
- storage-headroom reserve before create/upload/commit so weak hosts reject deterministically before disk exhaustion
- operator-safe startup configuration logging for deployment/runtime visibility
- `101 MiB` ciphertext ceiling so the `100 MiB` target class remains truthful after part-cipher overhead

Controls intentionally deferred:
- deployment automation
- multi-node or remote-object storage
- broader metrics pipeline / alerting system
- any default-path promotion or legacy deprecation step

## Environment

- constrained host: `qsl` via sanctioned `ssh qsl`
- host profile observed during validation:
  - `1` vCPU
  - `977668 kB` total memory
  - `554184 kB` available memory at preflight
  - root disk `30083776512` bytes total, `25112563712` bytes free at preflight
- real relay path: `https://qsl.ddnsfree.com`
- relay compatibility guard: `qsl-server` `verify_remote.sh` passed on the live host before ladder execution
- deployed qsl-attachments service:
  - binary: `/opt/qsl-attachments/bin/qsl-attachments`
  - host loopback bind: `127.0.0.1:3000`
  - local validation endpoint: `http://127.0.0.1:13000` through a sanctioned SSH local forward to the host loopback service
  - storage root: `/var/lib/qsl-attachments/data`
  - deployed `QATT_MAX_CIPHERTEXT_BYTES=105906176`
  - deployed `QATT_STORAGE_RESERVE_BYTES=134217728`

## Ladder Results

| Stage | Result | Classification | Evidence |
| --- | --- | --- | --- |
| Single-flow deployed service + real relay smoke (`> 4 MiB`) | Pass | contract-faithful | `stage2_over4m_service_*.log`; `stage2_over4m_service_summary.txt` |
| `< 4 MiB` legacy-path success (`24 KiB`) | Pass | contract-faithful | `stage2_small24_*.log` |
| `= 4 MiB` exact-threshold legacy path | Rejects with bounded retries and `relay_inbox_queue_full` | bounded weak-relay saturation | `stage2_exact4m_clean_send.log`; `stage2_exact4m_host.log` |
| `> 4 MiB` without attachment service | Immediate reject `attachment_service_required` | contract-faithful reject | `stage2_over4m_reject_send.log`; `stage2_over4m_reject_summary.txt` |
| `16 MiB` service-backed transfer | Pass | contract-faithful | `stage3_16m_*.log`; `stage3_16m_summary.txt` |
| `64 MiB` service-backed transfer | Pass | contract-faithful | `stage3_64m_*.log`; `stage3_64m_summary.txt` |
| `100 MiB` target-class transfer | Pass after ciphertext-ceiling correction | contract-faithful | `stage3_100m_*.log`; `stage3_100m_summary.txt` |
| Upload interrupt then resume (`6 MiB`) | Pass; interrupted upload stayed invisible until resumed | contract-faithful | `stage4_upload_resume_*.log`; `stage4_upload_resume_summary.txt` |
| Direct service restart (`6 MiB`) | Pass; post-restart probe returned `422` and flow completed | contract-faithful | `stage4_restart_only_*.log`; `stage4_restart_only_summary.txt` |
| Direct API quota reject | `413` `REJECT_QATTSVC_QUOTA` | contract-faithful reject | `quota_reject_body.json` |
| Direct API session expiry | `410` `REJECT_QATTSVC_EXPIRED` | contract-faithful reject | `session_expiry_body.json` |
| Direct API committed-object expiry | `410` `REJECT_QATTSVC_EXPIRED` | contract-faithful reject | `object_expiry_only_v2_body.json`; `object_expiry_only_v2_summary.txt` |
| Limited concurrency (`2 x 16 MiB`) | Pass | bounded concurrent load, contract-faithful | `stage5_conc_v2_*`; `stage5_concurrency_v2_summary.txt`; `stage5_concurrency_v2_host.log` |

## Delivery-State Truthfulness

- Legacy `< 4 MiB` path:
  - send logged `file_xfer_manifest` and `file_xfer_complete`
  - send progressed only to `accepted_by_relay` and `awaiting_confirmation`
  - later confirm logged `file_confirm_recv kind=coarse_complete` and then `peer_confirmed`
  - no `attachment_service_commit` marker appeared on the legacy-path run
- Service-backed `> 4 MiB` path:
  - send logged `attachment_service_commit`
  - send progressed only to `accepted_by_relay` and `awaiting_confirmation`
  - receive logged `attachment_confirm_send`
  - confirm logged `attachment_confirm_recv` and then `peer_confirmed`
- Limited concurrency preserved the same truthful state progression for both simultaneous `16 MiB` transfers.

## Measured Metrics

### Service-backed single-flow stages

| Stage | Send ms | Receive ms | Confirm ms | Total ms | Max qatt RSS KiB | Max relay RSS KiB | Min MemAvailable KiB | Max load1 | Disk delta bytes |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| `> 4 MiB` smoke | `21247` | `8086` | `1083` | `30416` | `14852` | `28312` | `506512` | `0.23` | `12750848` |
| `16 MiB` | `79558` | `12820` | `1053` | `93431` | `31348` | `28312` | `506968` | `0.23` | `17203200` |
| `64 MiB` | `125006` | `33837` | `1061` | `159904` | `200672` | `28312` | `347048` | `0.46` | `67678208` |
| `100 MiB` target class | `84803` | `43401` | `1063` | `129267` | `300652` | `28312` | `245216` | `0.47` | `105250816` |

### Interrupt / restart / limited concurrency stages

| Stage | Timing summary | Max qatt RSS KiB | Max relay RSS KiB | Min MemAvailable KiB | Max load1 | Disk delta bytes |
| --- | --- | ---: | ---: | ---: | ---: | ---: |
| Upload interrupt + resume (`6 MiB`) | abort `1382 ms`, peek `772 ms`, resumed send `32585 ms`, receive `5183 ms`, confirm `1263 ms`, total `41185 ms` | `198260` | `28312` | `320568` | `0.63` | `6971392` |
| Service restart (`6 MiB`) | send `32576 ms`, restart `2483 ms`, receive `5275 ms`, confirm `1443 ms`, total `41777 ms` | `16756` | `28312` | `501096` | `0.22` | `6586368` |
| Limited concurrency (`2 x 16 MiB`) | send `85757/85758 ms`, receive `14722/14744 ms`, confirm `1231/1285 ms`, total `101790 ms` | `38192` | `28316` | `499328` | `0.54` | `34463744` |

### Threshold / weak-relay evidence

- Exact `4 MiB` legacy-path run:
  - all `256` chunks pushed
  - final manifest push hit:
    - `relay_event action=push_fail`
    - `file_push_retry attempt=1 backoff_ms=50 reason=relay_inbox_queue_full`
    - `file_push_retry attempt=2 backoff_ms=100 reason=relay_inbox_queue_full`
    - `file_xfer_reject code=relay_inbox_queue_full`
  - `qatt_rss_kb` remained flat at `14852` throughout the monitored run, so the service path stayed idle

## Integrity Proof

- `> 4 MiB` service-backed smoke source and destination hashes matched
- `16 MiB` source and destination hashes matched
- `64 MiB` source and destination hashes matched
- `100 MiB` target-class source and destination hashes matched
- upload-resume source and destination hashes matched
- direct service-restart source and destination hashes matched
- limited concurrency source and destination hashes matched for both flows

## Secret Hygiene

- Live `qsl-attachments` journal scan for `resume_token`, `fetch_capability`, `X-QATT-Resume-Token`, `X-QATT-Fetch-Capability`, and `Bearer ` returned no matches during the validation window.
- Evidence-bundle scan for concrete token/capability assignments found only probe-script variable references, not stored secret values.
- No raw resume tokens, fetch capabilities, relay bearer tokens, or vault passphrases were recorded in the saved artifacts.

## Saturation vs Correctness Classification

- `= 4 MiB` exact-threshold legacy-path failure:
  - classification: bounded saturation / expected degradation on the weak relay
  - why: retries were bounded, failure reason stayed explicit as `relay_inbox_queue_full`, qsl-attachments stayed idle, and no false delivery progression occurred
  - promotion impact: blocks default-path promotion until broader reference-deployment evidence exists, but it is not a qsl-attachments correctness bug
- `100 MiB` target-class pre-patch ceiling failure:
  - classification: service-local operational gap
  - why: the raw `100 MiB` ciphertext ceiling was too tight for the `100 MiB` plaintext target class after part-cipher overhead
  - resolution: raised the service ceiling to `101 MiB` ciphertext and re-ran successfully
  - promotion impact: resolved in this item
- stricter exploratory receive-abort + restart composite:
  - classification: non-gating exploratory client-side issue outside the minimum required ladder
  - why: the clean service-restart stage passed, but the stricter resumed-download composite later failed to send confirmation with `qsp_pack_failed` / `chainkey_unset` after the file was recovered
  - promotion impact: note for broader reference-deployment evidence, not classified as a qsl-attachments restart failure

## Remaining Promotion Blocker

`NA-0003` does not authorize default-path promotion or legacy deprecation. After constrained-host execution, the honest remaining blocker is broader reference-deployment validation plus promotion-gate evidence across a stronger environment mix. The service-local constrained-host lane is now sufficiently grounded to hand that next decision back to qsl-protocol.
