[README.md](https://github.com/user-attachments/files/32138291/README_qsl-attachments.md)

# QSL Attachments

**The attachment plane for QSL: a service that stores encrypted file parts it cannot read.**

Files sent through QSL are encrypted on the sending device before they reach this service.
What arrives here is ciphertext in numbered parts. The runtime stores them, tracks upload
sessions, and hands them back — with **no plaintext attachment handling on any service
surface**.

> [!WARNING]
> **Research-stage. Not independently audited. Not production-ready.**
> This is a single-node, local-disk runtime. There is no deployment automation and no
> multi-node storage backend.

---

## Why a separate service

Attachments have a different shape from messages. They are large, they arrive in parts over
time, an upload can be resumed or abandoned, and a recipient may fetch one long after it was
sent. Folding that into the message relay would put lifecycle state and storage pressure into
a component whose whole value is being a dumb pipe.

So attachments get their own plane, with its own contracts, and the
[relay](https://github.com/QuantumShieldLabs/qsl-server) stays transport-only.

## What it implements

This repository is the runtime for three canonical contracts defined in
[qsl-protocol](https://github.com/QuantumShieldLabs/qsl-protocol):

| Contract | Scope |
|---|---|
| [`DOC-CAN-005`](https://github.com/QuantumShieldLabs/qsl-protocol/blob/main/docs/canonical) | Attachment descriptor and control plane |
| [`DOC-CAN-006`](https://github.com/QuantumShieldLabs/qsl-protocol/blob/main/docs/canonical) | Attachment service contract |
| [`DOC-CAN-007`](https://github.com/QuantumShieldLabs/qsl-protocol/blob/main/docs/canonical) | Encryption context and part cipher |

**qsl-protocol remains the canonical source of truth.** Where this repository and those
documents disagree, the documents win.

---

## Design posture

- **Opaque ciphertext only.** Part files on disk are ciphertext. The service has no key
  material and no way to obtain any.
- **No capability-like secrets in canonical URLs.** A secret in a URI ends up in logs,
  proxies, and browser history. Credentials travel in bodies and headers.
- **Capabilities are scoped to one resource.** A `resume_token` authorizes exactly one open
  session — resume, status, upload, commit, abort — until commit, abort, or expiry
  invalidates it. A `fetch_capability` authorizes repeated fetches of exactly one committed
  object until that object expires. Neither is a general credential, and neither crosses to
  another resource.
- **Fail-closed recovery.** On startup the storage root is reconciled explicitly: only
  coherent open sessions and committed objects are re-exposed, orphaned staged artifacts are
  discarded, and the service emits an operator-safe recovery summary rather than inventing
  stronger crash semantics than it can deliver.
- **Storage-headroom rejection.** The runtime refuses work before a weak host exhausts its
  disk, instead of failing halfway through a write.

## Lifecycle

`create` → `upload` (parts) → `status` → `commit`, with `abort` available throughout, and
retrieval afterwards including single-range ciphertext fetch. The full state machine is
normative in `DOC-CAN-006`.

---

## Current state

- Single-node, **local-disk** runtime.
- Opaque ciphertext part files on disk; local metadata and session journals persisted as JSON.
- Deterministic contract-faithfulness tests and a minimal runtime CI.
- Client integration exists upstream in qsl-protocol.
- Evidence exists in-repo for constrained-host validation, reference-deployment validation, and
  bounded stress, soak, and chaos runs. See [`tests/`](tests) and [`docs/`](docs).

**Authorization, stated precisely:** current service authorization is operator-scoped
deployment policy plus per-session and per-object capabilities. Quotas are deployment-global,
resource references are not principals, `Authorization` remains reserved and undefined, and
**there is no separate end-user service principal today**. Many transfers are permitted when
deployment policy and quota allow them; what each capability constrains is *which* session or
object it can touch, not who is asking.

**Durability boundary, stated precisely:** one local storage root on one node, and
graceful same-root restart is in scope. The only supported backup shape is
cold full-root backup/restore plus matching service configuration;
hot/live backup and partial restore remain unsupported. Abrupt-crash and open-session
recovery remains fail-closed plus bounded operator cleanup rather than cross-file
transactional durability.

---

## Status and honest limits

- **Not independently audited.**
- **Not production-ready.** No deployment automation, no multi-node backend, no
  high-availability story.
- Single point of failure by construction at this stage.
- The canonical contracts it implements are marked **DRAFT**.
- Open defects exist and are tracked in the open.

---

## Security reporting

Please do **not** file security-sensitive reports in public issues. Use GitHub private
vulnerability reporting on this repository, or follow [`SECURITY.md`](SECURITY.md). If private
reporting is unavailable, open a minimal public issue with **no exploit details**, stating
that you can share specifics privately.

## Related repositories

- [**qsl-protocol**](https://github.com/QuantumShieldLabs/qsl-protocol) — canonical contracts
  and specifications, including DOC-CAN-005/006/007
- [**qsl-desktop**](https://github.com/QuantumShieldLabs/qsl-desktop) — the desktop client
- [**qsl-server**](https://github.com/QuantumShieldLabs/qsl-server) — the transport-only relay,
  a separate surface

## License

`AGPL-3.0-only` — see [`LICENSE`](LICENSE). Any future commercial services or support
offerings are separate from this repository and do not replace the AGPL terms on the source
published here.
