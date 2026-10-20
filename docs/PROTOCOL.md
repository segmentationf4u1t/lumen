# Protocol notes — Vulcan II Mini (1E7D:2F09)

All values below were verified against firmware 1.26 on the target unit. The
implementation is host-side only; there is no firmware/DFU path.

## Interfaces

| Collection | Usage page | Report sizes |
|------------|-----------:|--------------|
| Control | `0xFF01` | feature 1044 max |
| LED | `0xFF00` | input 65, output 65 |

The HID paths contain `MI_01&COL05` and `MI_03` on this unit. Selection is by
VID/PID + usage page, never by path.

## Feature reports

| ID | Length | Purpose | Notes |
|----|-------:|---------|-------|
| `0x04` | ≥4 | ready/status | byte 1 == 1 means ready. 3 bytes is rejected by Windows |
| `0x09` | 9 | identity | byte 2 = firmware code (126 = 1.26); byte 6 = layout (0 here) |
| `0x0E` | 5 | direct mode | `0E 05 01 00 00` on, `0E 05 00 00 00` off |
| `0x11` | 332 | static frame | write-only; checksum in last two bytes |
| `0x13` | 123 | static keepalive | sent 3× with 25 ms gaps to commit `0x11` |

If `OutputReportByteLength` is 64 instead of 65, the collection has no report
IDs and the writer sends the 64-byte payload without the leading `0x00` byte.
The target unit reports 65.

## Direct mode

Enter streaming mode with two reports, in this order:

1. `0x11` with mode byte 4 = `0x0B` (same 332-byte layout as static, colors may
   be zero)
2. `0x0E 05 01 00 00`

Sending only `0x0E 05 01` can leave the firmware effect in control while HID
writes still return success, so both are sent. There is no keepalive for direct
mode.

## Direct frame

Six output reports of 65 bytes:

```
byte 0      report ID 0x00
byte 1      0xA1
byte 2      packet number (1..6)
byte 3..4   payload length 345, little-endian, first packet only
byte 5..64  60 payload bytes
```

Payload is 345 bytes. Logical key `i` maps through `SLOT_MAP[i]`; its RGB is
stored at `slot*3`, `slot*3+1`, `slot*3+2`. Unused payload bytes are zero.

## Static report

```
byte 0      0x11
byte 1..2   332 little-endian
byte 3      0x02
byte 4      mode 0x01 (static)
byte 5      speed 1..11
byte 6      brightness 1..69
byte 7      0xFB
byte 8      0x00
byte 9..    RGB per slot, same slot map as direct frames
last 2      sum of bytes 0..329, little-endian
```

## Safety

Only `0x04`, `0x09`, `0x0E`, `0x11`, `0x13` and the LED output report are
written. Any new report ID is out of scope until separately reverse-engineered.
