# NA-0002 Operational Hardening Contract Evidence

## Current truthful posture
- `qsl-attachments` remains the current single-node local-disk runtime.
- Opaque ciphertext files live on local disk and metadata/session state remains in JSON journals.
- `main` still has only the `rust` required check.
- qsl-protocol remains the canonical semantic source of truth.

## What was missing before this item
- No repo-local deployment profile set.
- No repo-local constrained-host validation ladder.
- No explicit saturation-vs-correctness interpretation rules for weak hosts and weak relays.
- No repo-local next-step definition for operational hardening and real-world validation.

## What this item freezes
- Repo-local alignment with qsl-protocol `DOC-ATT-002`.
- Deployment profiles for local development, constrained-host validation, and reference deployment.
- Readiness categories and constrained-host validation stages.
- Explicit gates blocking default-path promotion and legacy deprecation.
- The exact next repo-local implementation step as `NA-0003`.

## Scope proof
- Docs/governance only.
- No `src/**`, `Cargo.toml`, `Cargo.lock`, or workflow changes.
