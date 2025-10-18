# BKG Security Policy

## 1. Overview
This document summarizes the security posture, mandatory controls, and validation workflows that govern the BKG platform.

## 2. Threat Matrix (§22)
The Threat Matrix enumerates adversarial scenarios across the CAVE runtime, Admin-CAVE hosting surface, telemetry plane, and supply chain. Each threat entry captures mitigations, detection hooks, and ownership for rapid response.

Die Threat-Matrix in §22 ist verpflichtend CI-geprüft (via pytest security/).

## 3. Validation & Continuous Compliance
- **CI Enforcement:** Security regression suites must be executed via `pytest security/` and form part of the required CI gating jobs.
- **Documentation Sync:** Any update to the Threat Matrix requires a corresponding update to mitigation owners and automation coverage.
- **Supply Chain Integrity:** SBOM and SLSA provenance artifacts must be generated (`make sbom`, `make slsa`) and the SBOM signed (`cosign sign-blob`).

## 4. Incident Response Hooks
- **Audit Logging:** Append-only, signed JSON Lines logs must be streamed to the compliance archive with a 30-day minimum retention policy.
- **Rotation Webhooks:** Credential rotation events must be HMAC signed and replay protected; alerts are routed to the security operations queue.
- **Telemetry Sampling:** Operators may tune `CAVE_OTEL_SAMPLING_RATE` to adjust trace volume without disabling telemetry, ensuring investigations retain relevant signals.

## 5. Secure Development Requirements
- **Sandbox Hardening:** Enforce seccomp profiles, namespace isolation, and cgroup limits as outlined in the Phase-0 gating criteria.
- **Key Governance:** Follow TTL and rotation rules for Admin, Namespace, Session, and Model-Access keys, including webhook notifications.
- **Configuration Hygiene:** Sensitive environment variables (e.g., `BKG_API_KEY`, `BKG_DB_DSN`, TLS materials) must be sourced from managed secret stores.

## 6. References
- `README.md` — Canonical system prompt and implementation policy.
- `docs/env.md` — Environment variable reference and sensitivity flags.
- `schema/cave.schema.json` — Validation schema for `cave.yaml` deployments.
