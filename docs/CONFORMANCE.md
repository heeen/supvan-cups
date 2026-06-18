# IPP Everywhere conformance audit

Run of the canonical `ipptool` suite against the live server:

```sh
ipptool -t ipp://127.0.0.1:8631/ipp/print/<queue> \
    /usr/share/cups/ipptool/ipp-everywhere.test
```

against `ipp-printer-app` 0.3.0. Three suite sections fail:
RFC 8011 §4.1.1, PWG 5100.12 §6.2, PWG 5100.14 §5.1/5.2. Everything below
is a framework (`ipp-printer-app`) responsibility unless tagged
**[supvan]**. Tiers are ordered by value/effort.

## Tier 1 — Conformance *bugs* (wrong type / behaviour; cheap, high-value)

1. **`operations-supported` is `1setOf keyword`, must be `1setOf enum`.**
   `attributes.rs` emits `"Print-Job"`, `"Validate-Job"`, … as keywords;
   IPP requires the operation *codes* (`0x0002`, `0x0004`, `0x000b`, …).
   Every `operations-supported WITH-VALUE "0x…"` assertion fails because of
   this. CUPS tolerates it; conformance tools and stricter clients don't.
2. **`finishings-supported` is `keyword`, must be `1setOf enum`.** We emit
   `"none"`; spec wants enum `3`. (`finishings-default` is already `Enum(3)`
   — inconsistent today.)
3. **`media-col-supported` is a collection, must be `1setOf keyword`.** It
   should list the *member names* a client may set (`media-size`,
   `media-top-margin`, …), not the collections themselves. We conflated it
   with `media-col-database`. (Real values still belong in
   `media-col-database` / `media-col-ready`.)
4. **`requested-attributes` is ignored.** Get-Printer-Attributes always
   returns the full set; the suite's filtered query catches this
   (`NOT EXPECTED: printer-uri-supported`). Must return only the requested
   attributes (plus support the `all` / group-name magic values).
5. **request-id 0 not rejected.** RFC 8011 §4.1.1 requires
   `client-error-bad-request`; we answer `successful-ok`. Validate in the
   IPP request handler.

## Tier 2 — Missing REQUIRED operations (PWG 5100.14 §5.1)

We implement Print-Job, Validate-Job, Get-Printer-Attributes, Get-Jobs,
Get-Job-Attributes, Cancel-Job. Missing required ones:

- **Identify-Printer** (`0x3c`) + `identify-actions-supported` /
  `identify-actions-default`. **[supvan]** can map this to a beep/LED via the
  proto `CHECK_DEVICE` or a buzzer command.
- **Create-Job** (`0x05`) + **Send-Document** (`0x06`) — the multi-document
  job flow many clients prefer over Print-Job.
- **Close-Job** (`0x3b`), **Cancel-My-Jobs** (`0x39`).

## Tier 3 — Missing REQUIRED descriptor attributes (mostly static)

Cheap to emit; no behaviour change. Add to `get_printer_attributes`:

- Media geometry: `media-size-supported`,
  `media-{top,bottom,left,right}-margin-supported`.
- Job/limits: `multiple-document-jobs-supported`,
  `multiple-operation-time-out` (+`-action`), `which-jobs-supported`,
  `job-ids-supported`, `preferred-attributes-supported`,
  `overrides-supported`, `printer-get-attributes-supported`.
- Rendering: `orientation-requested-supported`,
  `print-rendering-intent-default` / `-supported`,
  `pwg-raster-document-sheet-back` (`normal` for a one-sided printer).
- Identity/admin: `printer-geo-location` (`unknown` ok),
  `printer-organization`, `printer-organizational-unit` (empty ok),
  `printer-icons` (serve a PNG from the HTTP server), `pages-per-minute`.
- Change tracking: `printer-config-change-date-time` / `-time`,
  `printer-state-change-date-time` / `-time`.

## Tier 4 — Dynamic, device-fed (we already have the data) **[supvan]**

We poll `RETURN_MAT` every cycle — the values are sitting unused:

- **`media-ready` / `media-col-ready`** — the *currently loaded* roll
  (width/height/type from `MaterialInfo`), not just the static catalogue.
  This is what printer dialogs show as "loaded media".
- **`printer-supply` / `printer-supply-description` / `-info-uri`** — map
  the labels-remaining counter to an IPP supply level (a "media" supply unit
  with `level`/`maxcapacity`). Surfaces the remaining-labels gauge in GUIs.

Needs a small framework API so the device backend can publish per-poll
"ready media" + "supply" into the registry, then the attribute builder emits
them.

## Tier 5 — Real capability gap

- **`document-format-supported` should include `image/jpeg`.** IPP Everywhere
  requires JPEG decode; we only accept PWG/CUPS raster. Either add a
  JPEG→raster path or document the limitation. Larger; defer.

## Suggested sequencing

- **0.4.0 (framework)**: Tier 1 + Tier 3 — all attribute/encoding fixes and
  the request-id guard. Pure `attributes.rs` / request-handler work, no new
  device plumbing. Knocks out most of the suite failures.
- **0.5.0 (framework + supvan)**: Tier 2 (Identify-Printer first — small and
  user-visible) and Tier 4 (media-ready / printer-supply from `MaterialInfo`,
  which also finishes the "labels remaining in the GUI" thread).
- **Later**: Tier 5 JPEG support if a real client needs it.

Re-run `ipp-everywhere.test` after each tier; track the pass delta.
