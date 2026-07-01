#!/usr/bin/env python3
"""Check for / download Supvan printer firmware from the vendor API.

Replicates the official app's `GetFirmwareFile` call (see docs/FIRMWARE.md):

    POST https://api.supvan.com:8789/api/upload/GetFirmwareFile

The endpoint has no authentication; firmware, when a vendor "upgrade task"
exists, is returned inline as a JSON int-array. This tool needs no printer — a
model id (`printerType`) and a device serial are enough. TLS verification is
disabled to mirror the app (the server may present an invalid cert).

Examples:
    supvan-fw-check.py --model 5002 --sn T0117A2410211517
    supvan-fw-check.py --all --sn T0117A2410211517 -o /tmp/supvan-fw
"""
import argparse
import json
import os
import random
import ssl
import sys
import urllib.request

URL = "https://api.supvan.com:8789/api/upload/GetFirmwareFile"
UA = "com.supvan.katasymbol_Android:1.4.20"

# printerType -> label (from communication/device/*Device.java setPrinterType()).
MODELS = {
    15: "E10/T10", 16: "G10", 50: "T50/T50M", 60: "TP60i", 70: "TP70",
    100: "LP100B", 200: "T200M", 1501: "E16/T16", 3601: "E11", 3602: "E12",
    5001: "T50Plus", 5002: "T50MPro/Pro", 5003: "T50S", 5005: "T50Max",
    8001: "T80/T80M", 8233: "TP80",
}


def query(model: int, sn: str, timeout: float, out_dir: str, save: bool):
    body = {
        "TerminalType": 1,
        "TerminalPackageName": "com.supvan.katasymbol",
        "TerminalVerNo": "75",
        "Province": "", "City": "", "Area": "",
        "RibbonVerNo": "0", "LabelVerNo": "0",
        "RibbonPasswordTableVerNo": "0", "LabelPasswordTableVerNo": "0",
        "PcbVerNo": "0",
        "Random": [random.randint(0, 255) for _ in range(16)],
        "DeviceType": model,
        "DeviceSn": sn,
        "Lang": 1,
        "NeedFirmwareData": True,
        "UserId": "",
        "LocalDataVerNo": "0",
    }
    ctx = ssl.create_default_context()
    ctx.check_hostname = False
    ctx.verify_mode = ssl.CERT_NONE  # mirror the app's trust-all TLS
    req = urllib.request.Request(
        URL, data=json.dumps(body).encode(),
        headers={"Content-Type": "application/json", "User-Agent": UA})
    with urllib.request.urlopen(req, timeout=timeout, context=ctx) as r:
        env = json.loads(r.read())

    rv = env.get("ResultValue")
    if isinstance(rv, str):
        rv = json.loads(rv) if rv.strip() else None
    if not isinstance(rv, dict):
        return f"rc={env.get('ResultCode')} msg={env.get('ErrorMsg')}"

    fw = rv.get("FirmwareData")
    n = len(fw) if isinstance(fw, list) else 0
    ver = rv.get("FirmwareVersionNo")
    if n and save:
        os.makedirs(out_dir, exist_ok=True)
        path = os.path.join(out_dir, f"dt{model}_v{str(ver).replace('/', '_')}.bin")
        with open(path, "wb") as f:
            f.write(bytes((x & 0xFF) for x in fw))
        return f"UPDATE v{ver} type={rv.get('FirmwareType')} -> saved {n}B to {path}"
    if n:
        return f"UPDATE v{ver} type={rv.get('FirmwareType')} ({n}B, not saved)"
    return f"NeedUpdate={rv.get('NeedUpdate')} v={ver} url={rv.get('DownLoadUrl')}"


def main():
    ap = argparse.ArgumentParser(description=__doc__,
                                 formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--model", type=int, help="printerType id (e.g. 5002 = T50M Pro)")
    ap.add_argument("--sn", required=True, help="device serial (required by the API)")
    ap.add_argument("--all", action="store_true", help="sweep all known models")
    ap.add_argument("-o", "--out", default="/tmp/supvan-fw", help="firmware output dir")
    ap.add_argument("--no-save", action="store_true", help="check only, don't save")
    ap.add_argument("--timeout", type=float, default=25.0)
    args = ap.parse_args()

    if not args.all and args.model is None:
        ap.error("give --model or --all")
    models = sorted(MODELS) if args.all else [args.model]

    rc = 1
    for m in models:
        try:
            res = query(m, args.sn, args.timeout, args.out, not args.no_save)
        except Exception as e:  # noqa: BLE001
            res = f"ERROR {e}"
        if res.startswith("UPDATE"):
            rc = 0
        print(f"{m:>6} {MODELS.get(m, '?'):<12} {res}")
    return rc


if __name__ == "__main__":
    sys.exit(main())
