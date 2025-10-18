# Sicherheitsrichtlinie

## 1. Überblick
Dieses Dokument beschreibt die Sicherheitsrichtlinien der BKG-Plattform, einschließlich Governance, Schutzmaßnahmen und Audit-Anforderungen für Admin-CAVEs und Namespace-CAVEs.

## 2. Governance-Grundsätze
- **Verantwortlichkeiten:** Admin-CAVEs sind für Modell-Hosting, Schlüsselverwaltung und Replika-Koordination zuständig.
- **Zugriffsmodell:** RBAC gemäß README v1.8.2 (Admin, Namespace, Session, Model-Access).
- **Compliance:** Alle Änderungen benötigen SBOM-, SLSA- und cosign-Signaturen gemäß CI-Richtlinien.

## 3. Schutzmaßnahmen
- **Sandbox-Isolation:** Nutzung von Linux-Namespaces, cgroups v2, Seccomp und Overlay-Dateisystemen.
- **Schlüsselverwaltung:** TTLs und Rotationsrichtlinien entsprechend Abschnitt "Schlüssel-Lifecycle" der README.
- **Telemetry:** OpenTelemetry mit konfigurierbarer Sampling-Rate über `CAVE_OTEL_SAMPLING_RATE`.

## 4. Auditing & Logging
- **Audit-Logs:** Append-only JSON-Lines, signiert; Zugriff über Admin-CAVE Audit-UI.
- **Webhook-Überwachung:** Rotation-Webhooks sind HMAC-signiert und werden im Audit-Log erfasst.

## 5. CI- und Testanforderungen
- **OpenAPI-Validierung:** `make api-schema` + `openapi-cli validate`.
- **cave.yaml-Validierung:** `ajv validate -s schema/cave.schema.json -d cave.yaml`.
- **Supply-Chain:** `make sbom`, `make slsa`, `cosign sign-blob` für SBOM-Artefakte.

## 22. Threat-Matrix
Die Threat-Matrix dokumentiert Angriffsvektoren für Sandbox, Admin-CAVE, P2P und Supply-Chain. Für jede Bedrohung sind Auswirkungen, Eintrittswahrscheinlichkeiten und Gegenmaßnahmen hinterlegt.

Die Threat-Matrix in §22 ist verpflichtend CI-geprüft (via pytest security/).

## 23. Anhänge
- **Glossar:** Siehe README Appendix A.
- **Verweise:** docs/architecture.md, docs/env.md, schema/cave.schema.json.
