# Supvan firmware update: check, download, flash

Reverse-engineered from the vendor Android app (`com.supvan.katasymbol`
v1.4.20, decompiled under `apk-decompiled/`) and confirmed against the live
API. Covers how the app **checks for**, **downloads**, and **flashes** printer
firmware, and what that means for building an independent tool.

> Status: mechanism fully mapped. **No firmware image obtained** — Supvan only
> serves firmware when an *upgrade task* exists for the device, and none was
> active for any probed model at the time of writing (see [Availability](#availability)).

## 1. Download API

Single endpoint, raw OkHttp (no Retrofit), plain JSON, **no authentication**.

```
POST https://api.supvan.com:8789/api/upload/GetFirmwareFile
Content-Type: application/json
User-Agent: com.supvan.katasymbol_Android:<versionName>
```

- Base URL is built in `dal/networkstorage/HttpManagerUtil.java`
  (`https://api.supvan.com:` + port + `/api/upload/`); default port `8789`, but
  the splash screen may override it via `GET /api/upload/getPort?terminalType=1&packageName=…`.
- **TLS is trust-all** in the app (`trustAllCerts`, `verify()→true`) — the
  server may present an invalid cert; a faithful client disables verification.
- The only header added by the interceptor is that `User-Agent`. No token, no
  signature, no timestamp.

### Request — `FirmwareUpdateParam` (plain-Gson, PascalCase keys)

```json
{
  "TerminalType": 1,
  "TerminalPackageName": "com.supvan.katasymbol",
  "TerminalVerNo": "<app versionCode, e.g. 75>",
  "Province": "", "City": "", "Area": "",
  "RibbonVerNo": "0", "LabelVerNo": "0",
  "RibbonPasswordTableVerNo": "0", "LabelPasswordTableVerNo": "0",
  "PcbVerNo": "0",
  "Random": [<16 ints>],          // challenge read from the printer
  "DeviceType": <printerType>,    // e.g. 5002 = T50M Pro / T50 Pro
  "DeviceSn": "<printer serial>", // REQUIRED and non-empty (else "参数为空")
  "Lang": 1,
  "NeedFirmwareData": false,      // app hardcodes false; true also works
  "UserId": "<customerId or ''>",
  "LocalDataVerNo": "0"
}
```

The version fields come from the printer (`READ_FWVER 0xC5` etc.); `Random` is a
16-byte challenge read from the printer (`getRandom`). `DeviceType` values are
the `setPrinterType(...)` ids in `communication/device/*Device.java`.

### Response — envelope + `FirmwareData` under `ResultValue`

```json
{ "ResultCode": 0, "ErrorMsg": null, "TotalCount": 0,
  "ResultValue": {
     "NeedUpdate": true,             // authoritative — no client-side version compare
     "ForceNeedUpdate": false, "ForceNeedUpdateVal": 0,
     "FirmwareData": [<int bytes>],  // firmware image as a JSON int array (NOT base64)
     "RandomCode": [<16 int bytes>], // server's answer to Random (device verify)
     "FirmwareVersionNo": "…", "FirmwareType": 1,  // 1=printer fw, 2=material table
     "FirmwareIndex": 0, "FirmwareRemark": "…", "DownLoadUrl": "…"
  } }
```

The app decides purely on `NeedUpdate`. On `ResultCode=-1` the `ErrorMsg` is a
Chinese status string (`无升级任务` = "no upgrade task", `参数为空` = "empty
parameter") and `ResultValue` is null.

## 2. Firmware authorization: the Random/RandomCode challenge

The printer emits a 16-byte `Random`; the server returns `RandomCode` (its
answer). Before flashing, **some** models verify it on-device (`verifyRM` /
`verifyERM`): P70/MP50, TP80, G7/G15/G21. This is the vendor's anti-tamper gate
— you cannot flash firmware those models will accept without the server's
RandomCode for the current challenge.

**The T50 family does NOT verify.** `T50SignalProcessor.updateFirmware` calls
`T50Print/T50PlusPrint.initThermalPrinter(bytes)` unconditionally — no
`verifyRM`, no signature check. So on a T50-class printer, any well-formed image
is accepted (see [Security](#security-notes)).

## 3. Flash protocol (T50 family)

`T50PlusPrint.initThermalPrinter(byte[])` → reads current fw version (`0xC5`),
and if it differs from the target, calls `updateFirmware(byte[])`:

1. Split the image into `ceil(len/500)` chunks of 500 bytes.
2. **Start:** `sendCmdStartTrans(0xC6, blockSize=512, blockCount=<numChunks>)`
   — the same 16-byte `7E 5A` command frame as `NEXT_ZIPPEDBULK (0x5C)`, opcode
   `0xC6`.
3. **Per chunk** send a 506-byte packet, wrapped in the standard 512-byte data
   frame (`7E 5A 01FC 10 02 <506>`):

   | offset | value |
   |--------|-------|
   | `[0]`  | `0xAA` |
   | `[1]`  | `0xC7` (firmware marker; print data uses `0xBB`) |
   | `[2..4]` | checksum = LE 16-bit sum of bytes `[4..506]` |
   | `[4]`  | chunk index |
   | `[5]`  | total chunk count |
   | `[6..506]` | 500 firmware bytes (last chunk zero-padded) |

The image is **raw** (not LZMA-compressed like print buffers). This maps almost
1:1 onto `supvan-proto`'s existing framing (`cmd::make_cmd_start_trans`,
`data::make_data_packet`) — only the start opcode (`0xC6`) and packet marker
(`0xC7`) differ, and there's no compression step.

## Availability

At the time of writing, `GetFirmwareFile` returned `无升级任务` ("no upgrade
task") for **every** probed model (15, 16, 50, 60, 70, 100, 200, 1501, 3601,
3602, 5001, 5002, 5003, 5005, 8001, 8233) with a valid serial, and `参数为空`
with an empty serial. Firmware is only served when Supvan has an active upgrade
task for the device; none were live. No firmware is bundled in the APK (the
`VERSION_UPGRADE` DB table is app-version data only). **Conclusion:** obtaining
an image requires either an active vendor task for the device or an
out-of-band source.

## Security notes

- The download API has **no authentication or request signing** — anyone can
  query firmware availability for any `DeviceType`/serial.
- On **T50-class printers there is no on-device firmware verification** — the
  Random/RandomCode gate is not enforced before `initThermalPrinter`. If an
  image is obtained (or crafted), the printer will flash it. There is real
  brick risk and no rollback path documented; treat any flasher as destructive.
- Other models (P70/MP50, TP80, G-series) do enforce `verifyRM`, so arbitrary
  firmware is rejected without the server's challenge answer.

## Sources

- `dal/models/FirmwareUpdateParam.java`, `dal/models/FirmwareData.java`,
  `dal/networkstorage/HttpManagerUtil.java`
- `globalsingleton/taskpool/strategy/FirmwareUpgradeStrategy.java`,
  `globalsingleton/taskpool/signalprocessor/T50SignalProcessor.java`
- `communication/print/T50PlusPrint.java` (`initThermalPrinter`, `updateFirmware`),
  and the per-model `getRandom`/`verifyRM` in `G*/MP50/TP80Print.java`
