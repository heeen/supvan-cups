# IPP Everywhere conformance audit

Run of the canonical `ipptool` suite against the live server:

```sh
ipptool -tv ipp://127.0.0.1:<port>/ipp/print/<queue> \
    /usr/share/cups/ipptool/ipp-everywhere.test
```

The suite pulls in `ipp-1.1.test` and `ipp-2.0.test`, so it exercises RFC 8011
§4.x request validation, PWG 5100.12 description attributes, and PWG 5100.14
required operations/attributes.

## Status (ipp-printer-app 0.4.0)

**31 PASS / 1 FAIL.** The single remaining failure is the `image/jpeg`
document-format requirement (PWG 5100.14 §5.2) — a real capability gap, not an
encoding bug. Everything below was closed in 0.4.0 (see the framework
CHANGELOG); the supvan-side `om_` media-name fix lives in
`crates/supvan-app/src/models.rs`.

### Closed in 0.4.0 (framework)

- **Encoding bugs**: `operations-supported` / `finishings-supported` now
  `1setOf enum`; `media-col-supported` now a `1setOf keyword` member list
  (real sizes stay in `media-col-database`).
- **`requested-attributes` honoured** for Get-Printer-Attributes, Get-Jobs
  (default = `job-uri` + `job-id`), and Get-Job-Attributes.
- **Request validation**: reject `request-id` 0 (§4.1.1), missing/misordered
  `attributes-charset` + `attributes-natural-language` (§4.1.4), unsupported
  IPP version (§4.1.8), and missing `printer-uri`/`job-uri` (§4.2).
- **Required operations**: Identify-Printer (via the new
  `DeviceBackend::identify` hook), Create-Job, Send-Document (requires
  `last-document`), Close-Job, Cancel-My-Jobs. Jobs carry their
  `requesting-user-name` owner so `Get-Jobs my-jobs=true` scopes correctly.
- **~25 required descriptor/job attributes**: media margins,
  `media-size-supported`, `media-ready` / `media-col-ready`,
  job/limit descriptors, rendering intent, `pwg-raster-document-sheet-back`,
  identity/admin (`printer-organization`, `printer-icons` via `GET /icon.png`,
  geo-location, supply), change timestamps, `time-at-processing`,
  `job-printer-up-time`. `printer-up-time` floored at 1.

### Closed (supvan config)

- **PWG media names use `om_`** (other-metric), not `oe_` (inches). `oe_…mm`
  failed the IPP Everywhere media-name regex.

## Remaining

### Tier 5 — real capability gap (the one suite failure)

- **`document-format-supported` should include `image/jpeg`.** IPP Everywhere
  requires JPEG decode; we only accept PWG/CUPS raster. Needs a JPEG→raster
  path. Larger; deferred.

### Tier 4 — dynamic, device-fed enrichment **[supvan, optional]**

The framework emits *static* `media-ready` / `media-col-ready` /
`printer-supply` so the required attributes are present and conformant. They
don't yet reflect the live roll. We already poll `RETURN_MAT` every cycle, so:

- **`media-ready` / `media-col-ready`** could publish the *currently loaded*
  roll (width/height/type from `MaterialInfo`) instead of the static default.
- **`printer-supply` level** could track the labels-remaining counter, which
  also finishes the "labels remaining in the GUI" thread.

Needs a small framework API so the device backend can push per-poll "ready
media" + "supply" into the registry, then the attribute builder reads them.

- **Identify-Printer → beep.** The framework dispatches to
  `DeviceBackend::identify`; supvan's backend can map it to a buzzer/`CHECK_DEVICE`
  command (currently the default no-op).

Re-run `ipp-everywhere.test` after each change; track the pass delta.
