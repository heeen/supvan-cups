# IPP golden fixtures

Reference IPP request/response captures for the `ipp-printer-app` framework.

## Capture (live server)

With `supvan-printer-app` running on port 8631 and at least one printer registered:

```sh
./scripts/capture_ipp_golden.sh [printer-name]
```

Writes:

- `get-printer-attributes.req.bin` / `.resp.bin`
- `validate-job.req.bin` / `.resp.bin`

## Synthetic requests

`crates/ipp-printer-app/tests/golden_ipp.rs` (via `cargo test -p ipp-printer-app`) builds
minimal Get-Printer-Attributes requests for regression tests without a running server.

## CUPS acceptance

See [docs/CUPS_ACCEPTANCE.md](../../docs/CUPS_ACCEPTANCE.md).
