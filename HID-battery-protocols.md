
# Battery-report protocols for 2.4 GHz wireless peripherals over HID

### A practical reference for a Rust / hidapi Windows battery viewer

**Scope.** Everything below is about reading a *peripheral's own* battery (mouse, keyboard, headset) over HID — not the system battery. Each section gives: the transport it rides on, the exact request bytes, the exact response layout, how to interpret the battery byte(s), pitfalls, and a source URL.

**Confidence tags.** `[CERTAIN]` = in an official spec or in shipping source code I read directly. `[LIKELY]` = consistent across two independent implementations. `[UNCERTAIN]` = one implementation, or explicitly flagged as unverified by its author.

---

## 0. The single most important architectural fact

There is **no universal vendor-agnostic battery query** for 2.4 GHz gaming peripherals. Windows ships exactly one HID battery client — the **HID UPS Battery** client bound to usage page `0x0084` usage `0x0004` — and gaming mice/keyboards/headsets essentially never expose it.

> `0x0084 | 0x004 | ... | HID UPS Battery | Shared`
> — [HID Clients Supported in Windows](https://raw.githubusercontent.com/MicrosoftDocs/windows-driver-docs/38070fe22fd5182552f89943e5a9099ddb790ac4/windows-driver-docs-pr/hid/hid-clients-supported-in-windows.md) `[CERTAIN]`

So in practice you need a **per-vendor driver table** plus a **generic fallback** (Section 5). This matches how every working open-source tool is built.

Also note the transport split that determines your hidapi code:

| Link | What you open | Notes |
|---|---|---|
| Wired USB | The device's own HID collection (often a vendor page `0xFF00`–`0xFFFF`) | One handle usually suffices |
| 2.4 GHz dongle | The **receiver's** HID collection, *addressed to a device index* | Logitech: index 1..6; Razer/others: the dongle proxies transparently |
| Bluetooth | HID++ rides report IDs 0x10/0x11 on the BT HID interface | Same protocol, different dev index (0xFF) |

---

# 1. Logitech HID++

HID++ is the only one of these protocols with a *published specification*, which makes it the reference implementation for everything else.

## 1.1 Transport framing (shared by 1.0 and 2.0)

Two report IDs, two lengths `[CERTAIN]`:

| Report ID | Total length (incl. ID) | Payload after ID |
|---|---|---|
| `0x10` short | 7 bytes | 6 bytes |
| `0x11` long | 20 bytes | 19 bytes |

Windows exposes these as **two separate HID collections** on the same interface: usage `0x01` = short, usage `0x02` = long. A short request written to the usage-0x01 handle may be answered on *either* handle, so a robust implementation opens both and reads both.

> "a short request is written to the usage-0x01 handle, a long request to the usage-0x02 handle, and replies may arrive on *either* — so we open both and read both."
> — [mouse_tray/drivers/hidpp.py](https://raw.githubusercontent.com/Fan4Metal/mouse_tray/master/mouse_tray/drivers/hidpp.py) `[CERTAIN]` (this is the single best pitfall note in the whole ecosystem)

**20-byte short report layout** (the "0x11" frame, which is what you will use constantly):

~~~
offset  0     1        2            3              4..19
       0x11  dev_idx  feature_idx  fctn<<4 | sw_id  parameters / results (16 bytes)
~~~

**7-byte short report layout:**

~~~
offset  0     1        2            3              4..6
       0x10  dev_idx  feature_idx  fctn<<4 | sw_id  parameters (3 bytes)
~~~

> "Requests 0: 0x10, Dev.index, Feature index, Fctn/ASE, Sw.id., Parameters … 0x11, Dev.index, Feature index, Fctn/ASE, Sw.id., Parameters"
> — [Logitech HID++ 2.0 draft spec, p.3](https://lekensteyn.nl/files/logitech/logitech_hidpp_2.0_specification_draft_2012-06-04.pdf) `[CERTAIN]`

Protocol is **big-endian** unless stated otherwise — a 16-bit value in bytes 4–5 has its MSB in byte 4 `[CERTAIN, same spec]`.

## 1.2 The software-id nibble rule

Byte 3 holds **function id in the high nibble, software id in the low nibble**:

~~~
byte3 = (function_id << 4) | sw_id
~~~

Rules `[CERTAIN]`:

1. **sw_id = 0 is reserved** — the device uses 0 to mean "this is a notification, not a reply". Never send 0 as a host.
   > "0 Do not use (allows to distinguish a notification from a response)." — spec p.3
2. **sw_id is 4 bits, 1..15.** The firmware must echo it back unchanged in the reply and in any ASE linked to your request. This is how you demultiplex replies from notifications.
3. Because HID++ 1.0 pings put `0xAA` in a parameter and HID++ 2.0 pings put sw_id in byte 3, Logitech uses `0xE<n>` rather than `0xF<n>` for the ping's high nibble so pings can't be confused with GetProtocolVersion. Spec p.3.

Real values in shipping software:
- Solaar: `SOLAAR_SOFTWARE_ID = 0x0B` (fixed, chosen to avoid 0x0F firmware sub-device self-enumeration). [Solaar base.py](https://raw.githubusercontent.com/pwr-Solaar/Solaar/master/lib/logitech_receiver/base.py)
- Linux kernel: `#define LINUX_KERNEL_SW_ID 0x01` [hid-logitech-hidpp.c](https://git.zx2c4.com/linux-rng/plain/drivers/hid/hid-logitech-hidpp.c)
- mouse_tray: `_SOFTWARE_ID = 0x01`

**Practical recommendation:** use a fixed non-zero nibble (0x01 or 0x0B). Randomizing per-request buys nothing and complicates reply matching.

## 1.3 Device index

| Value | Meaning |
|---|---|
| `0xFF` | The **receiver itself** (not a paired device) |
| `0x01`–`0x06` | Paired device slots on a Unifying/Bolt/Lightspeed receiver |
| `0x00` | Wired device enumerating directly |

> `#define HIDPP_RECEIVER_IDX 0xFF` / `#define HIDPP_WIRED_DEVICE_IDX 0x00` — [libratbag hidpp-generic.h](https://raw.githubusercontent.com/libratbag/libratbag/master/src/hidpp-generic.h) `[CERTAIN]`

### How to find which slot has a live mouse

Send a **ping** to each index 1..6 and wait for the echo:

~~~
Request  (short): 10 <idx> 00 1<sw> 00 00 <mark>
                  0x10 idx  0x00 (root feature, fctn 0 = GetProtocolVersion) ...
Reply    (short): 10 <idx> 00 1<sw> <major> <minor> <mark>
~~~

The mark byte must come back unchanged; mismatch = someone else's reply `[CERTAIN]`.

> Kernel does exactly this with `ping_data[3] = {0, 0, ping_byte}` where `ping_byte = 0x5a`, then checks `response.rap.params[2] != ping_byte`.
> — [hid-logitech-hidpp.c](https://git.zx2c4.com/linux-rng/plain/drivers/hid/hid-logitech-hidpp.c)

**Critical distinction the kernel makes:** if the ping returns error `0x01` (INVALID_SUB_ID) the device is **HID++ 1.0**, not 2.0. If it returns `0x04` (CONNECT_FAIL) or `0x05`/`0x09` (RESOURCE_ERROR/UNKNOWN_DEVICE), the slot is **empty or the device is off** — not "battery 0".

~~~c
if (ret == HIDPP_ERROR_INVALID_SUBID) { hidpp->protocol_major = 1; ... }
if (ret == HIDPP_ERROR_CONNECT_FAIL || ret == HIDPP_ERROR_RESOURCE_ERROR ||
    ret == HIDPP_ERROR_UNKNOWN_DEVICE) return -EIO;
~~~
`[CERTAIN]`

## 1.4 Feature index resolution — ROOT `0x0000` getFeature

**Feature IDs are not feature indices.** `0x1004` is an ID; the actual byte you put in the frame is a per-device, per-boot *index* (1..255), typically small (3, 5, 7…). You must resolve it at runtime and cache it for the session.

~~~
Request  (short): 10 <dev> 00 0<sw> 10 04 00
                            ^^  ^^^  ^^^^^
                            |   |    feature ID 0x1004 big-endian
                            |   function 0 = getFeature
                            root feature index 0x00
Reply    (short): 10 <dev> 00 0<sw> <featureIndex> <featureType> 00
~~~

- `featureIndex == 0` means **not supported**.
- `featureType` bit7 = obsolete, bit6 = SW-hidden, bit5 = SW-supported.
- Index 0 is forbidden/reserved for the root feature.

> "Given a desired featureID, returns its index (reference) in the feature table… featureIndex: The one based feature index in the feature table, **0 if not found**." — [spec, §0x0000 IRoot](https://lekensteyn.nl/files/logitech/logitech_hidpp_2.0_specification_draft_2012-06-04.pdf) `[CERTAIN]`

Kernel equivalent: `if (response.fap.params[0] == 0) return -ENOENT;` — same URL as above.

**Raw capture example** (from the spec's own transaction log) showing a real getFeature round-trip:

~~~
X 10 02 00 00 00 03 00        -> host asks device 2 for feature 0x0003
R 11 02 00 00 00 03 00 00 ... -> reply carries featureIndex=0x03 at byte 4
~~~
`[CERTAIN, spec p.14 "HID++ 2.0 Device transaction example"]`

## 1.5 Error and "feature not found" replies

Two completely different error formats. **This trips up almost every implementation.**

**HID++ 1.0 error** (short report only) `[CERTAIN]`:

~~~
10 <dev> 8F <echoed sub_id> <echoed address> <error code> 00
         ^^ 0x8F = error marker
~~~

**HID++ 2.0 error** (long report) `[CERTAIN]`:

~~~
11 <dev> FF <echoed fctn/sw> <error code> 00 ... 00
         ^^ 0xFF in the FEATURE INDEX position = error marker
~~~

> Kernel: `if (response->report_id == REPORT_ID_HIDPP_LONG && response->fap.feature_index == HIDPP20_ERROR) { ret = response->fap.params[1]; }`
> where `HIDPP20_ERROR` is `0xFF`. — [hid-logitech-hidpp.c](https://git.zx2c4.com/linux-rng/plain/drivers/hid/hid-logitech-hidpp.c)
>
> Solaar: `if reply_data[:1] == b"\xff" and reply_data[1:3] == request_data[:2]: ... raise FeatureCallError(...)` — [base.py](https://raw.githubusercontent.com/pwr-Solaar/Solaar/master/lib/logitech_receiver/base.py)

**HID++ 2.0 error codes** `[CERTAIN, spec p.3 + libratbag hidpp-generic.h]`:

| Code | Name | Meaning for a battery viewer |
|---|---|---|
| 0x00 | NoError | — |
| 0x01 | Unknown | |
| 0x02 | InvalidArgument | |
| 0x03 | OutOfRange | |
| 0x04 | HWError | |
| 0x05 | Logitech internal | |
| 0x06 | **INVALID_FEATURE_INDEX** | Your cached feature index is stale (device re-enumerated / slot changed). Re-run getFeature. |
| 0x07 | **INVALID_FUNCTION_ID** | Feature exists but that function doesn't. Fall back to another battery feature. |
| 0x08 | **Busy** | Retryable. Devices genuinely return this. |
| 0x09 | **Unsupported** | Feature present in table but not implemented. |

**HID++ 1.0 error codes** `[CERTAIN]`: 0x01 INVALID_SUBID, 0x02 INVALID_ADDRESS, 0x03 INVALID_VALUE, 0x04 CONNECT_FAIL, 0x05 TOO_MANY_DEVICES, 0x06 ALREADY_EXISTS, 0x07 BUSY, 0x08 UNKNOWN_DEVICE, 0x09 RESOURCE_ERROR, 0x0A REQUEST_UNAVAILABLE, 0x0B INVALID_PARAM, 0x0C WRONG_PIN.
— [libratbag hidpp-generic.h](https://raw.githubusercontent.com/libratbag/libratbag/master/src/hidpp-generic.h)

### The RESOURCE_ERROR special case

The kernel treats `HIDPP_ERROR_RESOURCE_ERROR` from battery calls as **"retry later"**, not "no battery":

~~~c
/* Ignore these intermittent errors */
if (ret == HIDPP_ERROR_RESOURCE_ERROR) return -EIO;
~~~
— [hid-logitech-hidpp.c, 0x1004 get_capabilities](https://git.zx2c4.com/linux-rng/plain/drivers/hid/hid-logitech-hidpp.c) `[CERTAIN]`

Solaar does the same in `get_adc_measurement`, with the comment *"this feature call produces an error for headsets that are connected but inactive"*.

## 1.6 Receiver with no paired / sleeping device — how it responds

This is the #1 source of wrong "0%" readings. Three distinct behaviours, all of which mean **"no reading"**, not "empty battery":

1. **Ping returns error 0x04 / 0x05 / 0x09** on that slot → nothing is there. `[CERTAIN, kernel]`
2. **No reply at all, then timeout.** The receiver silently drops requests to an empty slot. Solaar's `_RECEIVER_REQUEST_TIMEOUT` is 0.9 s even though "the receiver itself should reply very fast, within 500 ms", precisely because empty slots eat the full timeout. `[CERTAIN, base.py]`
3. **Feature calls return RESOURCE_ERROR / Busy** for a paired-but-asleep device. `[LIKELY]`

> "The dongle is reachable but the mouse behind it is not answering -- as with Logitech receivers, a sleeping mouse is indistinguishable from an absent one."
> — [mouse_tray nordic54.py](https://raw.githubusercontent.com/Fan4Metal/mouse_tray/master/mouse_tray/drivers/chipset/nordic54.py) `[CERTAIN]`

**Your UI needs three states, not two:** `Present(percent)`, `Asleep/NoLink`, `Absent`.

## 1.7 HID++ 2.0 feature `0x1004` UNIFIED BATTERY ⭐

**Use this first.** It is the modern, percentage-based feature and is what current Logitech mice/keyboards/headsets expose.

### get_capabilities — function 0x00

~~~
Request  (short): 10 <dev> <feat1004> 0<sw> 00 00 00
Reply    (short): 10 <dev> <feat1004> 0<sw> <numberOfLevels> <flags> 00

  numberOfLevels : 2..100. Number of distinct levels the device can report.
  flags          : bit0 = Disable battery OSD
                   bit1 = Enable mileage calculation
                   bit2 = Rechargeable
                   bit3..7 = must be 0
~~~

> "**GetBatteryCapability** … Returns NumberOfLevels: The number of levels the device is capable of reporting to the SW. Min: 2 - Max: 100 … Flags bit0: Disable battery OSD, bit1: Enable mileage calculation, bit2: Rechargeable, bit3-7: must be set to zero."
> — [HID++ 2.0 spec, §0x1000 (the doc labels the feature 0x1000 but the table contents are the unified-battery one; naming is inconsistent between drafts)](https://lekensteyn.nl/files/logitech/logitech_hidpp_2.0_specification_draft_2012-06-04.pdf) `[CERTAIN for the field layout]`

Kernel confirms the flag semantics with named bits:

~~~c
#define FLAG_UNIFIED_BATTERY_FLAGS_RECHARGEABLE      BIT(0)
#define FLAG_UNIFIED_BATTERY_FLAGS_STATE_OF_CHARGE   BIT(1)
...
if (params[1] & FLAG_UNIFIED_BATTERY_FLAGS_STATE_OF_CHARGE) {
    hidpp->capabilities |= HIDPP_CAPABILITY_BATTERY_PERCENTAGE;
} else {
    hidpp->capabilities |= HIDPP_CAPABILITY_BATTERY_LEVEL_STATUS;
    hidpp->battery.supported_levels_1004 = params[0];
}
~~~
— [hid-logitech-hidpp.c](https://git.zx2c4.com/linux-rng/plain/drivers/hid/hid-logitech-hidpp.c) `[CERTAIN]`

⚠️ **The kernel and the draft spec disagree on what bit1 means** (spec says "mileage calculation", kernel names it "STATE_OF_CHARGE"). Treat bit1 as *"device reports a true state-of-charge percentage"*; if it is set, params[0] is a percentage you can display directly.

### get_status — function 0x10

~~~
Request  (short): 10 <dev> <feat1004> 1<sw> 00 00 00
Reply    (short): 10 <dev> <feat1004> 1<sw> <soc> <level> <status> <extPower>
~~~

| Byte | Field | Meaning |
|---|---|---|
| 4 | `stateOfCharge` | **0–100 %**. 0 = unknown |
| 5 | `batteryLevel` | **bitmask**, not a number (see below) |
| 6 | `chargingStatus` | 0 discharging · 1 charging · 2 charging slow · 3 complete · 4 error |
| 7 | `externalPowerStatus` | 0 = not on external power, 1 = on external power `[LIKELY]` |

Kernel decoding, verbatim `[CERTAIN]`:

~~~c
*state_of_charge = params[0];
*status = hidpp20_unifiedbattery_map_status(hidpp, params[2], params[3]);
*level  = hidpp20_unifiedbattery_map_level(hidpp, params[1]);

switch (charging_status) {
  case 0: status = DISCHARGING; break;         /* discharging */
  case 1: case 2: status = CHARGING; break;    /* charging / charging slow */
  case 3: status = FULL; break;                /* complete */
  case 4: status = NOT_CHARGING; break;        /* error */
  default: status = NOT_CHARGING;
}
~~~

**The level bitmask** — never print this as a number `[CERTAIN]`:

~~~c
#define FLAG_UNIFIED_BATTERY_LEVEL_CRITICAL  BIT(0)
#define FLAG_UNIFIED_BATTERY_LEVEL_LOW       BIT(1)
#define FLAG_UNIFIED_BATTERY_LEVEL_GOOD      BIT(2)
#define FLAG_UNIFIED_BATTERY_LEVEL_FULL      BIT(3)
~~~

> "cler unsupported level bits" — the kernel ANDs `battery_level &= supported_levels_1004` before testing, because devices advertise only a subset in get_capabilities.

Solaar's independent decoder maps the *same byte* as an enum `8=FULL, 4=GOOD, 2=LOW, 1=CRITICAL, else EMPTY` — i.e. exactly these bit values. `[CERTAIN, cross-confirmed]`

### Rust crate cross-check

The `hidpp` Rust crate implements the identical layout with function ids 0 and 1 in the *high nibble*:

~~~rust
v20::MessageHeader { device_index, feature_index, function_id: U4::from_lo(1), software_id }
...
Ok(BatteryInfo {
    charging_percentage: payload[0],
    level:  BatteryLevel::try_from(payload[1])...,
    status: BatteryStatus::try_from(payload[2])...,
})
~~~
— [docs.rs hidpp unified_battery v0](https://docs.rs/hidpp/latest/src/hidpp/feature/unified_battery/v0.rs.html) `[CERTAIN]`

### Pitfall: call get_capabilities first

The kernel *refuses* to re-read capabilities and skips the call if it already has a capability flag set:

~~~c
if (hidpp->capabilities & HIDPP_CAPABILITY_BATTERY_LEVEL_STATUS ||
    hidpp->capabilities & HIDPP_CAPABILITY_BATTERY_PERCENTAGE) {
    /* we have already set the device capabilities, so let's skip */
    return 0;
}
~~~

You must still call it once per device to know whether byte 4 is a percentage or the level bitmask is authoritative.

## 1.8 Feature `0x1000` BATTERY LEVEL STATUS (legacy)

Older, but very widespread on HID++ 2.0 mice.

~~~
get_status      (short): 10 <dev> <feat1000> 0<sw> 00 00 00
reply           (short): 10 <dev> <feat1000> 0<sw> <discharge%> <next%> <status>

get_capability  (short): 10 <dev> <feat1000> 1<sw> 00 00 00
reply           (short): 10 <dev> <feat1000> 1<sw> <numLevels> <flags> 00
~~~

| Byte | Field | Meaning |
|---|---|---|
| 4 | `BatteryDischargeLevel` | % ... **but 0 means unknown**, and when *charging* the device reports 0 |
| 5 | `BatteryDischargeNextLevel` | next step %; 0 if lowest or charging |
| 6 | `BatteryStatus` | 0 discharging · 1 recharging · 2 charge final stage · 3 charge complete · 4 recharging below optimal · 5 invalid battery type · 6 thermal error · 7 other charging error · 8–255 invalid |

> "BatteryDischargeLevel: The current level expressed in %. **0 means unknown.** BatteryStatus Enum: 0=discharging (in use), 1=recharging, 2=charge in final stage, 3=charge complete, 4=recharging below optimal speed, 5=invalid battery type, 6=thermal error, 7=other charging error, 8..255: invalid"
> — [HID++ 2.0 spec §0x1000 BatteryUnifiedLevelStatus](https://lekensteyn.nl/files/logitech/logitech_hidpp_2.0_specification_draft_2012-06-04.pdf) `[CERTAIN]`

Kernel behaviour worth copying `[CERTAIN]`:
- On status 3 (complete) it **overrides capacity to 100** and sets level FULL.
- For statuses 1, 2, 4 it leaves capacity at whatever byte 4 said — which is 0 while charging. So **do not display 0% while charging**; show "charging, level unknown".
- Solaar: `if battery_discharge_level == 0: battery_discharge_level = None`.

> "the capacity is only available when discharging or full" — kernel, `hidpp->battery.online = status == DISCHARGING || status == FULL;`

## 1.9 Feature `0x1001` BATTERY VOLTAGE (millivolts)

~~~
Request  (short): 10 <dev> <feat1001> 0<sw> 00 00 00
Reply    (short): 10 <dev> <feat1001> 0<sw> <voltageHi> <voltageLo> <flags>
~~~

- Bytes 4–5: **big-endian millivolts** (`get_unaligned_be16(data)`).
- Byte 6 flags:

| Bit | Meaning |
|---|---|
| 0x80 | charging-state valid; then `flags & 0x07`: 0 charging, 1 full, 2 not charging, else unknown |
| no 0x80 | discharging |
| 0x08 | fast charge |
| 0x10 | slow / trickle charge |
| 0x20 | level = CRITICAL |

~~~c
long flags = (long) data[2];
if (flags & 0x80)
    switch (flags & 0x07) {
    case 0: status = CHARGING; break;
    case 1: status = FULL; *level = FULL; break;
    case 2: status = NOT_CHARGING; break;
    default: status = UNKNOWN; break;
    }
else status = DISCHARGING;
if (test_bit(3, &flags)) *charge_type = FAST;
if (test_bit(4, &flags)) *charge_type = TRICKLE;
if (test_bit(5, &flags)) *level = CRITICAL;
*voltage = get_unaligned_be16(data);
~~~
— [hid-logitech-hidpp.c](https://git.zx2c4.com/linux-rng/plain/drivers/hid/hid-logitech-hidpp.c) `[CERTAIN]`

**Converting mV → %:** the kernel ships a 100-entry table (`4186, 4156, 4143, …, 3579, 3537`) mapping voltage to percent, and warns loudly if voltage is outside 3500–5000:

~~~c
if (unlikely(voltage < 3500 || voltage >= 5000))
    hid_warn_once(hid_dev, "%s: possibly using the wrong voltage curve\n", __func__);
for (i = 0; i < ARRAY_SIZE(voltages); i++)
    if (voltage >= voltages[i]) return ARRAY_SIZE(voltages) - i;
~~~
`[CERTAIN]`

⚠️ **This curve is Li-ion at ~3.5–4.19 V.** Devices with different chemistry (2×AAA, LiPo at different cutoff) need their own curve. HeadsetControl ships per-model calibration tables for exactly this reason (`logitech_calibrations.hpp`).

Solaar's Python default is a compact 13-point approximation: `(4186,100) (4067,90) (3989,80) (3922,70) (3859,60) (3811,50) (3778,40) (3751,30) (3717,20) (3671,10) (3646,5) (3579,2) (3500,0)`.

## 1.10 Feature `0x1F20` ADC MEASUREMENT

Same voltage idea, different framing, and it is the feature used by several **headsets**.

~~~
Request  (short): 10 <dev> <feat1F20> 0<sw> 00 00 00
Reply    (short): 10 <dev> <feat1F20> 0<sw> <adcHi> <adcLo> <flags>
~~~

- Bytes 4–5: big-endian millivolts.
- Byte 6 flags: `0x01` discharging, `0x03` charging, `0x07` full, `0x0F`/other unknown.

~~~c
switch (flags) {
case 0x01: status = DISCHARGING; break;
case 0x03: status = CHARGING;    break;
case 0x07: status = FULL;        break;
case 0x0F: default: status = UNKNOWN;
}
*voltage = get_unaligned_be16(data);
~~~
— [hid-logitech-hidpp.c](https://git.zx2c4.com/linux-rng/plain/drivers/hid/hid-logitech-hidpp.c) `[CERTAIN]`

The kernel uses a **different** 100-point curve here (starts `4030, 4024, 4018, …` ending `3499, 3466, 3433, 3399`) adapted from HeadsetControl's G633/G933 code. Solaar's decoder only handles the discharging/charging bit (`flags & 0x01` → `RECHARGING if flags & 0x02`), which is a *different* bit convention — treat ADC flag semantics as `[UNCERTAIN]` and prefer the kernel's.

⚠️ Solaar explicitly guards this call: *"this feature call produces an error for headsets that are connected but inactive"* → catch and treat as "known but unavailable".

## 1.11 HID++ 1.0 register `0x07` — Battery Level Status

HID++ 1.0 has **no features**; it has *registers* addressed by a 16-bit command:

~~~
Get register : request_id = 0x8100 | register
Set register : request_id = 0x8000 | register
Get long reg : request_id = 0x8300 | register
~~~
`[CERTAIN — Solaar hidpp10.py read_register()/write_register()]`

### Register 0x07 wire format

~~~
Request  (short): 10 <dev> 81 07 00 00 00
                  ^^  ^^   ^^
                  |   |    register 0x07
                  |   sub_id 0x81 = GET_REGISTER
                  report id 0x10

Reply    (short): 10 <dev> 81 07 <level> <chargeState> <lowThreshold>
~~~

| Byte | Field | Encoding |
|---|---|---|
| 4 | `level` | **not a percentage** — enum: 7=FULL, 5=GOOD, 3=LOW, 1=CRITICAL, other=EMPTY |
| 5 | `chargeState` | 0x00 discharging · `&0x21==0x21` recharging · `&0x22==0x22` full |
| 6 | `lowThreshold` | threshold in **5 % steps**; values ≥7 are reserved and must be read as 0 |

Solaar `parse_battery_status()` `[CERTAIN]`:

~~~python
if status_byte_ == 7:   charge = FULL
elif status_byte_ == 5: charge = GOOD
elif status_byte_ == 3: charge = LOW
elif status_byte_ == 1: charge = CRITICAL
else:                   charge = EMPTY   # "pure charging notifications may come without a status"

def charging_byte_to_status_text(b):
    if b == 0x00: DISCHARGING
    elif b & 0x21 == 0x21: RECHARGING
    elif b & 0x22 == 0x22: FULL
~~~

libratbag is independently consistent `[CERTAIN]`:

~~~c
*level = battery.msg.parameters[0];
*charge_state = battery.msg.parameters[1];
*low_threshold_in_percent = battery.msg.parameters[2];
if (*low_threshold_in_percent >= 7) *low_threshold_in_percent = 0;
*low_threshold_in_percent *= 5; /* in 5% increments */
~~~
— [libratbag hidpp10.c](https://raw.githubusercontent.com/libratbag/libratbag/master/src/hidpp10.c)

## 1.12 HID++ 1.0 register `0x0D` — Battery Charge / "mileage" (true %)

**This is the one that gives a real percentage** — this answers your "(0x0D = battery level in %?)" question with a definite **yes**.

~~~
Request  (short): 10 <dev> 81 0D 00 00 00
Reply    (short): 10 <dev> 81 0D <percent|flags> <maxHi> <maxLo|units|state>
~~~

libratbag decode `[CERTAIN]`:

~~~c
*level_in_percent = battery.msg.parameters[0] & 0x7F;   /* <-- 7 bits! */

max  = battery.msg.parameters[1];
max |= (battery.msg.parameters[2] & 0x0F) << 8;
switch ((battery.msg.parameters[2] & 0x30) >> 4) {      /* units of "max" */
case 0x03: max *= 24;   /* days  -> fallthrough */
case 0x02: max *= 60;   /* hours -> fallthrough */
case 0x01: max *= 60;   /* min   -> fallthrough */
case 0x00: break;       /* seconds */
}
/* bits 6-7 of parameters[2] = charge state */
~~~
— [libratbag hidpp10.c](https://raw.githubusercontent.com/libratbag/libratbag/master/src/hidpp10.c)

- **Bit 7 of byte 4 is a flag, so mask with 0x7F.** This is the single most common bug when porting this register.
- Bytes 5–6 encode "remaining time at current rate" with a unit selector — usually ignorable for a battery viewer.
- Solaar reads the % as `ord(reply[:1])` raw (no mask) and derives status from `reply[2] & 0xF0`: 0x30 discharging, 0x50 recharging, 0x90 full. Slight discrepancy with libratbag on the mask — `[LIKELY]` the mask is correct; Solaar's simplicity works because devices rarely set bit 7.

### Which register does a device have?

Do **not** guess — Solaar's logic is the model to copy `[CERTAIN]`:

~~~python
for r in (Registers.BATTERY_STATUS, Registers.BATTERY_CHARGE):
    if r in device.registers:        # from the device's HID++ 1.0 descriptor
        reply = read_register(device, r)
        ...
# the descriptor does not tell us which register this device has, try them both
reply = read_register(device, Registers.BATTERY_CHARGE)   # 0x0D preferred
if reply: ... remember for next time ...
reply = read_register(device, Registers.BATTERY_STATUS)   # then 0x07
~~~

> "let's just assume HID++ 2.0 devices do not provide the battery info in a register" — Solaar short-circuits register reads entirely when `protocol >= 2.0`. **Do the same**: if the ping reports 2.0, skip registers.

## 1.13 Features you asked about that are **not** batteries

Be careful with a "try these IDs" loop — several of the IDs in your list will *never* yield a battery and some will **change device state**:

| ID | Real meaning | Battery? |
|---|---|---|
| `0x1802` | **DEVICE_RESET** | ❌ **DANGEROUS** — never blind-probe this |
| `0x1803` | (not in Solaar/kernel tables — likely a vendor/reserved slot) | ❌ unknown |
| `0x1810` | (not in Solaar/kernel tables) | ❌ unknown |
| `0x1814` | **CHANGE_HOST** — switches the device to another host slot | ❌ **DANGEROUS** — will disconnect you |
| `0x1E00` | **ENABLE_HIDDEN_FEATURES** | ❌ changes device feature visibility |
| `0x1E10` | (not in Solaar/kernel tables) | ❌ unknown |
| `0x1805` | OOBSTATE | ❌ |
| `0x1815` | HOSTS_INFO | ❌ |

Verified against [Solaar hidpp20_constants.py](https://raw.githubusercontent.com/pwr-Solaar/Solaar/master/lib/logitech_receiver/hidpp20_constants.py):

~~~python
GENERIC_TEST = 0x1800
DEVICE_RESET = 0x1802
OOBSTATE = 0x1805
CONFIG_DEVICE_PROPS = 0x1806
CHANGE_HOST = 0x1814
HOSTS_INFO = 0x1815
...
ENABLE_HIDDEN_FEATURES = 0x1E00
~~~
`[CERTAIN]`

**Recommended battery feature probe order** (cheapest and safest first):
`0x1004` → `0x1000` → `0x1001` → `0x1F20` → HID++ 1.0 registers `0x0D`/`0x07` (2.0 devices only after the first four fail).

Solaar's actual ordered dispatch table `[CERTAIN]`:

~~~python
battery_functions = {
    SupportedFeature.UNIFIED_BATTERY: Hidpp20.get_battery_unified,
    SupportedFeature.BATTERY_STATUS:  Hidpp20.get_battery_status,
    SupportedFeature.BATTERY_VOLTAGE: Hidpp20.get_battery_voltage,
    SupportedFeature.ADC_MEASUREMENT: Hidpp20.get_adc_measurement,
    ...
}
~~~

---

# 2. Razer (Razer Synapse) — 90-byte feature report

**Applies to:** Razer wireless mice (Viper/DeathAdder/Basilisk/Naga/Cobra families), VID `0x1532`.

## 2.1 Report structure `[CERTAIN]`

Exactly **90 bytes**, sent as an HID **feature report** (OpenRazer uses raw `SET_REPORT`/`GET_REPORT` control transfers with `wValue = 0x300`, `wIndex = report_index`).

~~~
offset  size  field
0       1     status          (0x00 in requests)
1       1     transaction_id
2       2     remaining_packets  (big-endian; 0 for single packet)
4       1     protocol_type      (always 0x00)
5       1     data_size          (number of argument bytes)
6       1     command_class
7       1     command_id
8       80    arguments[80]
88      1     crc
89      1     reserved (0x00)
~~~

> `struct razer_report { u8 status; union transaction_id_union transaction_id; __be16 remaining_packets; u8 protocol_type; u8 data_size; u8 command_class; union command_id_union command_id; u8 arguments[80]; u8 crc; u8 reserved; };`
> `static_assert(sizeof(struct razer_report) == 90);`
> — [razercommon.h](https://raw.githubusercontent.com/openrazer/openrazer/master/driver/razercommon.h) `[CERTAIN]`

`command_id` is a bitfield: **bit7 = direction (0 host→device, 1 device→host)**. So `0x80` is "get version of command 0x00", `0x84` is "get charging status of 0x04". `[CERTAIN, header comment]`

## 2.2 Battery query

**Command class `0x07` (Power & Battery), command id `0x80`, data size `0x02`:**

~~~c
struct razer_report razer_chroma_misc_get_battery_level(void)
{
    return get_razer_report(0x07, 0x80, 0x02);
}
struct razer_report razer_chroma_misc_get_charging_status(void)
{
    return get_razer_report(0x07, 0x84, 0x02);
}
~~~
— [razerchromacommon.c](https://raw.githubusercontent.com/openrazer/openrazer/master/driver/razerchromacommon.c) `[CERTAIN]`

### Complete request bytes (all zeros otherwise)

~~~
00 1F 00 00 00 07 80 00 | 00 * 80 | CRC 00
^^ ^^ ^^^^^ ^^ ^^ ^^ ^^
|  |  |     |  |  |  +-- command id 0x80 = GET_BATTERY_LEVEL
|  |  |     |  |  +----- command class 0x07
|  |  |     |  +-------- data_size = 0x02
|  |  |     +----------- protocol_type 0x00
|  |  +----------------- remaining_packets = 0
|  +-------------------- transaction_id (0x1F / 0x3F / 0xFF, see below)
+----------------------- status = 0
~~~

> Comment in razercommon.h: *"Data Size is the size of payload, cannot be greater than 80. 90 = header (8B) + data + CRC (1B) + Reserved (1B). Command Class is the type of command being issued."* `[CERTAIN]`

## 2.3 CRC — and the correction to your assumption ⚠️

**It is a plain XOR, not a CRC-8 with polynomial 0x07.** Your brief says "CRC8 (poly 0x07)"; every Razer implementation I checked XORs.

~~~c
unsigned char razer_calculate_crc(struct razer_report *report)
{
    /* second to last byte of report is a simple checksum */
    /* just xor all bytes up with overflow and you are done */
    unsigned char crc = 0;
    unsigned char *_report = (unsigned char*)report;
    for (i = 2; i < 88; i++)  crc ^= _report[i];
    return crc;
}
~~~
— [razercommon.c](https://raw.githubusercontent.com/openrazer/openrazer/master/driver/razercommon.c) `[CERTAIN]`

**Note the exact bounds: bytes 2..87 inclusive** (loop `i < 88`). Status (0) and transaction id (1) are excluded.

An independent reference agrees on the algorithm but states different bounds (1..86):

~~~python
def calculate_crc(report: bytes) -> int:
    crc = 0
    for i in range(1, 87):   # Bytes 1-86 inclusive
        crc ^= report[i]
    return crc
~~~
— [uChroma Razer protocol reference](https://hyperb1iss.github.io/uchroma/reference/razer-protocol-reference.html) `[UNCERTAIN — conflicts with OpenRazer]`

**Verification:** with the OpenRazer rule (2..87) and an all-zero payload the CRC is 0, so a request of `00 1F 00 00 00 07 80 00` + 80 zeros gets `crc = 0x1F ^ 0x07 ^ 0x80 = 0x98`. Test both rules against a real capture; the difference only shows when transaction_id ≠ 0 or when byte 87 is nonzero.

Also from uChroma: *"Some devices (Blade 2021+) return CRC=0x00 on success—validation should be skipped for OK responses."* `[UNCERTAIN but a useful defensive rule]`

## 2.4 Response parsing

~~~
byte 0      status: 0x00 new/pending · 0x01 BUSY · 0x02 SUCCESS · 0x03 FAILURE · 0x04 TIMEOUT · 0x05 NOT_SUPPORTED
byte 1      transaction_id echo
bytes 2-3   remaining_packets
byte 4      protocol_type
byte 5      data_size
byte 6      command_class echo (0x07)
byte 7      command_id echo (0x80)
byte 8+     arguments
~~~

**Battery percentage lives at `arguments[1]` = report byte 9**, 0–255, scale to 0–100.

~~~c
return sysfs_emit(buf, "%d\n", response.arguments[1]);
~~~
— [razermouse_driver.c, razer_attr_read_charge_level](https://raw.githubusercontent.com/openrazer/openrazer/master/driver/razermouse_driver.c) `[CERTAIN]`

**Charging status is a separate request** (`class 0x07, id 0x84`), also read from `arguments[1]`: 0 = not charging, 1 = charging.

> "Read device file "is_charging" / Returns 0 when not charging, 1 when charging" — same file `[CERTAIN]`

OpenRazer's daemon then divides by 255:

~~~python
battery_255 = float(driver_file.read().strip())
if battery_255 < 0: ...
battery_100 = (battery_255 / 255) * 100
~~~
— [dbus_methods/mamba.py](https://raw.githubusercontent.com/openrazer/openrazer/master/daemon/openrazer_daemon/dbus_services/dbus_methods/mamba.py) `[CERTAIN]`

## 2.5 hidapi specifics — the +1 offset trap

hidapi prepends the report ID, and Razer uses **feature report id 0**, which shifts every index by one:

~~~python
header = [0x00, _TRANSACTION_ID, 0x00, 0x00, 0x00, 0x02, 0x07, 0x80]
...
payload = [0x00, *payload]   # leading 0x00 = HID feature report id
res = self._transact(report, len(report), feature=True, delay=0.34)

# +1 vs the pyusb offset (9) because hidapi prepends the report id.
raw = res[10] if len(res) > 10 else 0
percent = round(raw / 255 * 100)
~~~
— [mouse_tray/drivers/vendor/razer.py](https://raw.githubusercontent.com/Fan4Metal/mouse_tray/master/mouse_tray/drivers/vendor/razer.py) `[CERTAIN]`

⚠️ Note this driver's header is `[0x00, tid, 0, 0, 0, 0x02, 0x07, 0x80]` — class and id at indices 6/7 of the *unshifted* payload, matching the struct. Do **not** copy the docstring's prose ("command class 0x02 / id 0x07") — the code is right, the prose is wrong.

**Delay: `0.34 s`** for wireless; the author notes "Wireless needs a longer settle before the reply is ready." `[CERTAIN]`

## 2.6 Transaction ID is per-model ⚠️

This is the biggest Razer pitfall. OpenRazer switches on PID:

| Transaction ID | Devices |
|---|---|
| `0x1F` | Viper V2/V3 Pro, DeathAdder V3/V4 Pro, Basilisk V3 Pro (all), Cobra Pro, Naga Pro/V2 Pro, Pro Click V2, Basilisk V3 35K |
| `0x3F` | Lancehead (W/USB), Mamba Wireless, DeathAdder V2 Pro |
| `0xFF` | Naga Epic, Mamba 2012, Ouroboros, Viper Ultimate, Basilisk X HyperSpeed |

> `case USB_DEVICE_ID_RAZER_VIPER_V2_PRO_WIRELESS: ... request.transaction_id.id = 0x1f; break;`
> — [razermouse_driver.c](https://raw.githubusercontent.com/openrazer/openrazer/master/driver/razermouse_driver.c) `[CERTAIN]`

And note the guard in the kernel driver: `if (WARN_ON(request_report->transaction_id.id == 0x00)) request_report->transaction_id.id = 0xFF;` — **0x00 is invalid**.

**Strategy for a viewer:** try `0x1F`, then `0x3F`, then `0xFF`; accept the first that returns status `0x02` with a plausible `arguments[1] <= 100`.

## 2.7 Razer pitfalls summary

| Pitfall | Detail |
|---|---|
| Sleeping mouse reports **0** | mouse_tray treats `percent == 0` as `asleep`, not empty |
| Feature reads are racy on some models | Some dongles "return the previous command's buffer" (observed on MCHOSE, same firmware class) |
| CRC rule disagreement | 2..87 (OpenRazer) vs 1..86 (uChroma) — **test on hardware** |
| Charging needs a 2nd request | Class 0x07/0x84, not in the battery reply |
| hidapi +1 | Report ID 0 shifts everything by one byte |

---

# 3. The 2.4 GHz "generic dongle" families

This is where most of your named brands live. The unifying insight: **the radio SoC vendor, not the brand, decides the protocol.** Cheap wireless mice are ~90 % built on a handful of reference designs, and rebrands (ATK/VXE/VGN/Zaopin/Scyrox/Dareu/G-Wolves…) ship identical firmware.

> "This is the shared protocol of the underlying Compx/Nordic silicon, not anything brand-specific: receivers enumerate as "Compx" and every rebrand of the chipset speaks it -- ATK, VXE, VGN, Zaopin, Scyrox, Dareu..."
> — [mouse_tray nordic52.py](https://raw.githubusercontent.com/Fan4Metal/mouse_tray/master/mouse_tray/drivers/chipset/nordic52.py) `[CERTAIN]`

## 3.1 Nordic 52840 / "Compx" — 17-byte, report ID 0x08 (ATK, VXE, VGN, Zaopin, Scyrox, Dareu, G-Wolves, Pulsar, Kysona)

**Usage page `0xFF02`, usage `0x02`.** Both directions are 17 bytes.

~~~
write   08 04 00 00 00 00 00 00 00 00 00 00 00 00 00 00 49
read    08 04 00 00 00 02 23 00 0e 94 00 00 00 00 00 00 82
        ^id ^sub          ^st ^%  ^w  ^^^^^^ voltage      ^cksum
                             0x23 = 35%     0x0E94 = 3732 mV
~~~

| Offset | Meaning |
|---|---|
| 0 | report ID `0x08` |
| 1 | sub-command `0x04` (battery) |
| 5 | status byte |
| **6** | **percent (direct)** |
| 7 | wired/charging flag: `0` on 2.4 G, `1` on cable |
| 8–9 | cell voltage, **big-endian mV** |
| 16 | checksum: **all 17 bytes sum to `0x55` mod 256** → the fixed request carries `0x49` (=73) |

> "The last byte is a checksum -- all 17 bytes sum to `0x55` mod 256, so the fixed request carries a baked-in `0x49` (= 73)."
> — [mouse_tray nordic52.py](https://raw.githubusercontent.com/Fan4Metal/mouse_tray/master/mouse_tray/drivers/chipset/nordic52.py) `[CERTAIN]`

**Charging inference:** `charging = wired and percent < 100`; `full = wired and percent >= 100`.

### Independent kernel confirmation

The in-flight `hid-pulsar` driver uses the **same 17-byte / report-0x08 / 0x55-sum** framing, with different sub-commands:

~~~c
#define USB_INTERFACE 1
#define USB_PAYLOAD_LEN 17
#define CMD_HID_REPORT_ID 0x08
#define CHECKSUM_MAGIC 0x55

enum pulsar_cmd { CMD_NONE=0, CMD_INFO=0x01, CMD_STATUS=0x03, CMD_POWER=0x04, CMD_EVENT=0x0a };

static u8 calc_checksum(const u8 *data, size_t len) {
    u8 sum = 0;
    for (size_t i = 0; i < len - 1; i++) sum += data[i];
    return (u8)CHECKSUM_MAGIC - sum;
}
~~~

Battery reply for `CMD_POWER` (0x04):

~~~c
if (response[6] > 100 || response[7] > 0x01) { ret = -EIO; goto unlock; }
battery->available = true;
battery->level     = response[6];
battery->conn      = response[7] == 1;
battery->voltage   = (response[8] << 8) | response[9];
~~~

Note `checksum = 0x55 - sum(bytes[0..15])`, which is algebraically identical to "all 17 bytes sum to 0x55". `[CERTAIN — two independent implementations]`

**Supported IDs in that driver:** Pulsar `0x3554:0xF507/0xF508/0xF509` (wired/1 kHz/4 kHz), ATK `0x373B:0xF58F`, ATK-alt `0x3554:0x1085`, plus Kysona M600 and VXE Dragonfly R1 Pro.

⚠️ The patch notes the device "listens only to control transfers" — `hid_hw_raw_request(..., HID_OUTPUT_REPORT, HID_REQ_SET_REPORT)`. With hidapi that maps to `send_feature_report`/`send_output_report` on the right collection, not a plain interrupt write.

## 3.2 Nordic 54L15 — 64-byte, report ID 0x08, command 0x72 (ATK Zero)

**Usage page `0xFF05`, usage `0x01`.**

~~~
write   08 7D 72 02 00 00 07 01 00 ...
read    08 72 00 3A 00 07 01 64 16 11 00 ...
~~~

| Byte | Meaning |
|---|---|
| 2 | command (`0x72`) |
| 3 | payload length (reply) |
| 5 | link index: `0` = cable, `1` = dongle |
| 6 | sub-index `0x07` = battery |
| reply 2 | status: `0` = mouse answered, `1` = it did not |
| reply **7** | **percent** (`0x64` = 100 %) |
| reply 8–9 | cell voltage, **little-endian mV** (`0x1116` = 4374 mV) |

> "In the reply, byte 2 is a status (`0` = the mouse answered, `1` = it did not), byte 3 the payload length, bytes 5-6 echo the request, byte 7 is the percent (0x64 = 100%) and bytes 8-9 the cell voltage in millivolts, little-endian (0x1116 = 4374 mV)."
> — [mouse_tray nordic54.py](https://raw.githubusercontent.com/Fan4Metal/mouse_tray/master/mouse_tray/drivers/chipset/nordic54.py) `[CERTAIN]`

⚠️ **Note the endianness flip vs. 52840** (LE here, BE there). Also: over the air, bytes 8–9 are junk — read percent only.

## 3.3 Realtek — **pushed** report 0x13, XOR 0xFF (MCHOSE L7 Pro)

**Usage page `0xFF01`, usage `0x01`. No request — you wait for the device.**

~~~
raw   13 1d fe fe ff a2 fd fd f9 d9 b3 c8 df af 8d 90
      ^id ^sub     ^batt                      ^^^^ "L7 Pro" XOR 0xFF
                   (0xa2 ^ 0xFF = 0x5d = 93%)
~~~

> "the battery here rides a **pushed** input report. The mouse periodically emits report id `0x13` … sub-type `0x1d` carries the battery. All payload bytes are obfuscated with **XOR 0xFF** … the battery percent sitting at offset 5"
> — [mouse_tray realtek.py](https://raw.githubusercontent.com/Fan4Metal/mouse_tray/master/mouse_tray/drivers/chipset/realtek.py) `[CERTAIN]` (reverse-engineered from USB capture)

- Percent: `res[5] ^ 0xFF`
- Wired flag: `res[4] ^ 0xFF` (decoded 1 = cable)
- Optional nudge: feature report `[0x11, 0xF9] + [0xFF]*19`
- Push interval ≈ 2 s; allow 3 s timeout.

⚠️ **"The device's feature-report *reads* are racy (they return the previous command's buffer), so the pushed report is the reliable source."** This is a recurring firmware bug class — if a feature read returns stale-looking data, prefer the push.

## 3.4 MCHOSE X9 (headset) — 64-byte report 0x55 on usage page 0xFF90

~~~
request:  55 65 01 00 ...
response: 55 65 VV 02 ...     (VV = percent, 0x02 = field id "battery")
~~~

> "Battery is queried on the vendor-defined collection with usage page 0xff90, which carries a 64-byte report 0x55 in both directions: request: 55 65 01 00 … (message type 0x65, query battery); response: 55 65 VV FF … (VV = value, FF = which field it belongs to)"
> — [HeadsetControl mchose_x9.hpp](https://raw.githubusercontent.com/Sapd/HeadsetControl/master/lib/devices/mchose_x9.hpp) `[CERTAIN]`

⚠️ Note the **two different MCHOSE protocols**: L7 Pro mouse = Realtek push (3.3); X9 headset = this request/reply. VID also differs (`0x5253` vs `0x3837`).

## 3.5 A4Tech Bloody 8K dongle (VID 0x1956)

Two channels: **command** on usage `0xFF02` (8-byte feature report ID 0), **status** on `0xFF01` (8-byte input report ID 0x06).

~~~
ping   : 00 80 00 00 00 00 00 7F
query  : 00 80 FF 00 00 00 00 80      (send ~13 ms after ping reply)

reply  : 06 56 PP CC 00 00 00 00
         ^^ ^^ ^^ ^^
         |  |  |  +-- 1 = on cable, 0 = 2.4 G
         |  |  +----- charge percent (0x55 = 85)
         |  +-------- status sub-type 0x56 = battery
         +----------- report id
~~~

> "Open the status channel *before* asking, so the reply is queued for this handle rather than dropped on the floor."
> — [mouse_tray bloody.py](https://raw.githubusercontent.com/Fan4Metal/mouse_tray/master/mouse_tray/drivers/vendor/bloody.py) `[CERTAIN — from USBPcap of vendor tool]`

⚠️ A dongle with no radio link answers `06 56 FF …` (percent 255) — **skip 255, don't display it**.

## 3.6 Ninjutso Sora V2 — 32-byte feature report ID 5

~~~
request : [5, 21, 0, 0, 1, 0...0]      (32 bytes)
reply   : percent@9, charging@10, full@11, online@12
~~~
**Usage page `0xFFA0`.** — [mouse_tray ninjutso.py](https://raw.githubusercontent.com/Fan4Metal/mouse_tray/master/mouse_tray/drivers/vendor/ninjutso.py) `[CERTAIN]`

## 3.7 Lamzu (Maya X, Inca) — 65-byte feature report ID 0

Same shape as Glorious (3.8), different VID and interface:

~~~
request : [0x00, 0, 0, 0x02, 0x02, 0, 0x83, 0, ...]   (65 bytes)
reply   : res[1] == 0xA1 (tag), res[6] == 0x83 (echo), res[7] = charging, res[8] = percent
~~~
Interface 2, usage page **0xFFFF**. — [mouse_tray lamzu.py](https://raw.githubusercontent.com/Fan4Metal/mouse_tray/master/mouse_tray/drivers/vendor/lamzu.py) `[CERTAIN]`

## 3.8 Glorious Model O/D Wireless (VID 0x258A)

Interface 2, 65-byte feature report:

~~~rust
bfr_w[3] = 0x02;  bfr_w[4] = 0x02;  bfr_w[6] = 0x83;
device.send_feature_report(&bfr_w)?;
sleep(50ms);
device.get_feature_report(&mut read_bfr)?;
battery_percentage = read_bfr[8];
~~~
— [crstian19/mow src/report/battery.rs](https://raw.githubusercontent.com/crstian19/mow/master/src/report/battery.rs) `[CERTAIN]`

Status is encoded in `read_bfr[1]` as one of `[0xA1, 0xA4, 0xA2, 0xA0, 0xA3]`; index 1 = sleeping.

⚠️ **This is byte-for-byte the same request as Lamzu (0x83 at [6], 0x02/0x02 at [3]/[4]).** Strong evidence these two share an OEM/ODM firmware (both are "send 65-byte feature report, read back"). If you implement one, implement both.

## 3.9 Attack Shark X3/X11 — pushed 5-byte report, ID 0x03

~~~
03 DD EE P1 P2
   ^^ ^^ ^^ ^^
   |  |  |  +-- P2: charge level
   |  |  +----- P1: charging state (1 complete, 2 full, 3 charging)
   |  +-------- event code 0x40 (0x41 also seen) = battery
   +----------- device id (model, not vendor)
~~~
Usage page `0x000A`, interface 2. — [mouse_tray attackshark.py](https://raw.githubusercontent.com/Fan4Metal/mouse_tray/master/mouse_tray/drivers/vendor/attackshark.py) `[CERTAIN]`

⚠️ The author explicitly documents **two unresolved ambiguities**: whether P2 is a plain percent or steps of ten, and whether P1 ever moves. Treat as `[UNCERTAIN]`.

## 3.10 Coverage summary for your named brands

| Brand | Status | Best source |
|---|---|---|
| **ATK / VXE / VGN / Zaopin / Scyrox / Dareu / G-Wolves** | ✅ Same Compx/Nordic 52840 protocol (§3.1) | mouse_tray nordic52.py |
| **ATK Zero** | ✅ Nordic 54L15 (§3.2) | mouse_tray nordic54.py |
| **Pulsar** | ✅ Same 17-byte family; kernel driver in flight | [hid-pulsar patch](https://lkml.iu.edu/2608.1/14324.html) |
| **MCHOSE** | ✅ Two variants: L7 Pro mouse (Realtek push, §3.3), X9 headset (§3.4) | mouse_tray / HeadsetControl |
| **Glorious** | ✅ §3.8 | crstian19/mow |
| **Lamzu** | ✅ §3.7 | mouse_tray / Sheroune/lamzu-battery-monitory |
| **Ninjutso** | ✅ §3.6 | mouse_tray |
| **A4Tech Bloody (8K)** | ✅ §3.5 | mouse_tray |
| **Razer** | ✅ §2 | OpenRazer |
| **Keychron** | ⚠️ No public battery protocol found. Tools exist (squeak, keychron-go) but expose DPI/lighting, not battery. | `[UNCERTAIN — not found]` |
| **VGN** | ✅ Via Compx/Nordic 52840 | same as ATK |
| **AULA (狼蛛)** | ⚠️ Reverse-engineered F108 Pro exists (Go, 64-byte feature report on interface 3) but exposes **lighting/keymap/LCD only — no battery**. | `[UNCERTAIN — not found]` |
| **Fantech** | ⚠️ No public protocol found | `[UNCERTAIN — not found]` |
| **WLmouse** | ⚠️ No public protocol found | `[UNCERTAIN — not found]` |

> **Honest statement:** for Keychron, Fantech, WLmouse, and AULA I could not find a documented or open-source battery query. Do not invent one for them — fall back to §5, and consider that they may simply not report battery over HID at all (many 2.4 G keyboards only show battery via their own Windows app talking to a proprietary dongle protocol, or not at all).

---

# 4. Corsair / SteelSeries (and other headsets)

Headsets are their own ecosystem because audio needs an isochronous path, so vendors use fat vendor reports on a dedicated interface.

## 4.1 SteelSeries Arctis 7 / Pro (legacy) — 31-byte packets

~~~
request : 06 18 00 00 ... (31 bytes)
response: byte 2 = battery percent (0-100 direct), 8 bytes read
~~~

> `std::array<uint8_t, 2> request { 0x06, 0x18 };` …
> `requestBatteryDirect(device_handle, request, 8 /*response size*/, 2 /*battery byte*/, -1 /*no status byte*/, ...)`
> — [HeadsetControl steelseries_arctis_7.hpp](https://raw.githubusercontent.com/Sapd/HeadsetControl/master/lib/devices/steelseries_arctis_7.hpp) `[CERTAIN]`

⚠️ **Requires interface 0x05.** `getCapabilityDetail() → { .interface_id = 0x05 }`. PIDs: `0x1260, 0x12ad, 0x1252, 0x1280`.

Note: SteelSeries reports **direct percentage**, not voltage. Don't apply a Li-ion curve.

## 4.2 SteelSeries Arctis Nova 7 / Nova Pro Wireless — 64/128-byte status read

Both use the same status request:

~~~
request : 00 B0
response: 64+ bytes
~~~

**Nova 7** (battery in `data[2]`, status in `data[3]`):

~~~cpp
if (data[3] == 0x00) return deviceOffline;      // headset offline
if (data[3] == 0x01 || data[3] == 0x02) status = CHARGING;
if (is_gen2_protocol) level = data[2];           // direct 0-100
else                  level = map(data[2], 0, 4, 0, 100);  // discrete 0-4
~~~
— [steelseries_arctis_nova_7.hpp](https://raw.githubusercontent.com/Sapd/HeadsetControl/master/lib/devices/steelseries_arctis_nova_7.hpp) `[CERTAIN]`

⚠️ **Same product line, two different encodings.** The header lists 12 PIDs split into `DISCRETE_BATTERY_PIDS {0x2202, 0x2206, 0x220a, 0x223a, 0x227a, 0x22a4}` (0–4 steps) and gen-2 PIDs `{0x22A1, 0x227e, 0x2258, 0x229e, 0x22ad, 0x22a9, 0x22a5, 0x22a7}` (0–100). **You must gate on PID.** Getting it wrong yields "4 %" on a full battery.

**Nova Pro Wireless** (0x12e0 / 0x12e5), 128-byte read:

~~~cpp
request { 0x06, 0xb0 }
status_byte = response[15];   // 0x01 offline, 0x02 cable charging, 0x08 online
level = map(response[6], 0, 8, 0, 100);   // discrete 0-8!
~~~
— [steelseries_arctis_nova_pro_wireless.hpp](https://raw.githubusercontent.com/Sapd/HeadsetControl/master/lib/devices/steelseries_arctis_nova_pro_wireless.hpp) `[CERTAIN]`

⚠️ Note Nova Pro uses `response[6]` with a 0–8 range while Nova 7 uses `response[2]` with 0–4 or 0–100. **Three different scales in one product family.**

## 4.3 HyperX Cloud II Wireless (VID 0x03F0, HP)

~~~
request : 06 FF BB 02 00 ... (52 bytes)
response: 06 FF BB 02 ... (20 bytes)  -> percent at index 7
charging: 06 FF BB 03 00 ...          -> state at index 4
~~~

> `#define CMD_GET_BATTERY_LEVEL 0x02` … `BATTERY_LEVEL_INDEX = 0x07` … `CMD_GET_BATTERY_CHARGING 0x03` … `BATTERY_CHARGING_INDEX = 0x04`
> — [hyperx_cloud_2_wireless.hpp](https://raw.githubusercontent.com/Sapd/HeadsetControl/master/lib/devices/hyperx_cloud_2_wireless.hpp) `[CERTAIN]`

⚠️ **Two commands required** — battery level and charging state are separate transactions. Usage page **0xFF90**, usage **0x0303**, interface 0. PIDs `0x0696`, `0x018b`.

The header validates the reply aggressively and you should copy that: check `response[0..3] == 06 FF BB cmd` and that the echoed value matches.

## 4.4 Logitech headsets

Two entirely different mechanisms:

**(a) HID++ voltage** — e.g. G535 uses `{0x05, 0x0d}` as a long HID++ message; G PRO uses `{0x06, 0x0d}`:

~~~cpp
std::array<uint8_t, 2> cmd { 0x05, 0x0d };
return requestBatteryHIDPP(device_handle, cmd, calibrations::LOGITECH_G535);
~~~
— [logitech_g535.hpp](https://raw.githubusercontent.com/Sapd/HeadsetControl/master/lib/devices/logitech_g535.hpp) `[CERTAIN]`

The generic parser:
~~~cpp
uint8_t state = response[6];
auto status = (state == 0x03) ? BATTERY_CHARGING : BATTERY_AVAILABLE;
uint16_t voltage = (response[4] << 8) | response[5];   // big-endian mV
int level = spline_battery_level(calibration.percentages, calibration.voltages, voltage);
~~~
— [protocols/hidpp_protocol.hpp](https://raw.githubusercontent.com/Sapd/HeadsetControl/master/lib/devices/protocols/hidpp_protocol.hpp) `[CERTAIN]`

⚠️ **"If voltage < calibration minimum, the headset is offline"** — a great validity check that avoids showing a bogus 0 %. Each model needs its own calibration curve.

**(b) Centurion CPL** — G PRO X 2 LIGHTSPEED (PID 0x0af7) uses a 64-byte `0x51`-prefixed vendor protocol on **usage page 0xFFA0, interface 3** rather than HID++:

> "This variant uses a vendor-specific 64-byte protocol on usage page 0xffa0 for battery data instead of HID++."
> — [logitech_gpro_x2_lightspeed.hpp](https://raw.githubusercontent.com/Sapd/HeadsetControl/master/lib/devices/logitech_gpro_x2_lightspeed.hpp) `[CERTAIN]`

Solaar's transport notes two Centurion variants:
~~~
0x51 (PRO X 2): [0x51, cpl_length, flags, feat_idx, func_sw, params..., pad]
0x50 (G522):    [0x50, device_addr, cpl_length, flags, feat_idx, func_sw, params..., pad]
~~~
— [Solaar base.py](https://raw.githubusercontent.com/pwr-Solaar/Solaar/master/lib/logitech_receiver/base.py) `[CERTAIN]`

## 4.5 Corsair Void V2 / Virtuoso Max / HS80 Max

~~~
request : 00 02 <HEADSET_ENDPOINT> 02 0F     (MSG_SIZE_WRITE bytes, interface 4)
response: battery at [4] (low) and [5] (high), 16-bit LE, range 1-1000
~~~

~~~cpp
uint16_t battery_level_vendor = (battery_response[4] | (battery_response[5] << 8));
const uint16_t battery_level_vendor_max = 1000;
// re-request up to 3 times if 0 or > 1000
~~~
— [corsair_void_v2w.hpp](https://raw.githubusercontent.com/Sapd/HeadsetControl/master/lib/devices/corsair_void_v2w.hpp) `[CERTAIN]`

⚠️ Three important notes from that file:
1. **"The receiver send packets with the paired headset ID instead of the battery level"** — hence the retry loop. This is why naive single-shot reads give nonsense.
2. **Interface 4** is required (`capability_detail { usagepage = 0, usageid = 0, interface_id = 4 }`).
3. `wakeDevice()` must succeed first, and you should `flushHIDBuffer()` before querying.

The Corsair **sidetone** command frame is 64 bytes starting `FF 0B 00 FF 04 0E FF 05 01 04 00 <level>` — a useful fingerprint if you're probing Corsair devices generically.

## 4.6 The `0x0C:0x01` "consumer control" 32-byte pattern you mentioned

I found **no** evidence that generic consumer-control (`0x0C:0x01`) devices expose battery. Usage `0x0C`/`0x01` is the standard media-key collection and Windows binds it as "Consumer Controls — Shared" `[CERTAIN, HID clients doc]`. The 32-byte vendor reports you've seen on headsets are on **vendor pages** (`0xFF43`, `0xFF90`, `0xFFA0`, `0xC`), not on `0x0C:0x01`.

Practical rule: if a headset's battery collection reports usage page `0x0C`, it is almost certainly the **media-key** interface, and the battery request goes to a *different* collection on the same device — check every interface's usage page before concluding there's no battery.

---

# 5. Standard HID battery — usage pages and what actually works

## 5.1 The usages, exactly

From the official HID Usage Tables 1.5 (I parsed the embedded JSON in the PDF):

**Usage Page `0x06` — Generic Device Controls** `[CERTAIN]`

| Usage | Name | Kind |
|---|---|---|
| `0x20` | **Battery Strength** | DV (Dynamic Value) |
| `0x21` | Wireless Channel | DV |
| `0x22` | Wireless ID | DV |
| `0x29` | RF Signal Strength | DV |

> `{"Kind": "Defined", "Id": 6, "Name": "Generic Device Controls", "UsageIds": [ ... {"Id": 32, "Name": "Battery Strength", "Kinds": ["DV"]} ... ]}`
> — [HID Usage Tables 1.5, §Generic Device Controls](https://usb.org/sites/default/files/hut1_5.pdf) `[CERTAIN]`

**Usage Page `0x85` — Battery System** `[CERTAIN]`

| Usage | Name | Kind |
|---|---|---|
| `0x64` | Relative State Of Charge | DV |
| `0x65` | Absolute State Of Charge | DV |
| `0x66` | **Remaining Capacity** | DV |
| `0x67` | **Full Charge Capacity** | DV |
| `0x6A` | **Average Time To Full** | DV |
| `0x6B` | **Cycle Count** | DV |
| `0x6C` | (unassigned — see note) | — |
| `0x44/0x45/0x46` | Charging / Discharging / Fully Charged | Sel |

> `{"Id": 133, "Name": "Battery System", "UsageIds": [{"Id": 100, "Name": "Relative State Of Charge"}, {"Id": 102, "Name": "Remaining Capacity"}, {"Id": 103, "Name": "Full Charge Capacity"}, {"Id": 106, "Name": "Average Time To Full"}, {"Id": 107, "Name": "Cycle Count"} ...]}`
> — [HID Usage Tables 1.5, §Battery System](https://usb.org/sites/default/files/hut1_5.pdf) `[CERTAIN]`

⚠️ **Correction to your brief:** `0x6C` is **not defined** in HUT 1.5 — the page jumps `0x6B Cycle Count` → `0x80 Battery Pack Model Level`. Also, "Battery Strength" is `0x20` on page **`0x06`**, and there is **no** `0x85:0x20` — don't conflate the pages.

**Usage Page `0x84` — Power Device** `[CERTAIN]`

HUT 1.5 defines page `0x84` with **no usage IDs** listed in the parsed table (`UsageIdGenerator: null`), i.e. the page is effectively reserved/undefined in the current spec. What Windows actually uses is this specific combination:

> `| 0x0084 | 0x004 | Yes | Yes | Yes | HID UPS Battery | Shared |`
> — [HID Clients Supported in Windows](https://raw.githubusercontent.com/MicrosoftDocs/windows-driver-docs/38070fe22fd5182552f89943e5a9099ddb790ac4/windows-driver-docs-pr/hid/hid-clients-supported-in-windows.md) `[CERTAIN]`

So: **page `0x84`, usage `0x04`, collection = "HID UPS Battery", access = Shared.** This is the ONLY HID battery client in Windows, and it is designed for UPSes.

## 5.2 Which wireless peripherals actually expose these?

**Short answer: almost none, and the exceptions are UPS-shaped devices.**

| Device class | Exposes `0x06:0x20`? | Exposes `0x85:*`? | Exposes `0x84:0x04`? |
|---|---|---|---|
| Logitech mice/kbd | ❌ (HID++ instead) | ❌ | ❌ |
| Razer mice | ❌ | ❌ | ❌ |
| Gaming headsets | ❌ | ❌ | ❌ |
| Bluetooth LE HID (HOGP) | ⚠️ possible via BLE Battery Service, **not** via HID | — | — |
| UPS / power banks | — | ⚠️ sometimes | ✅ |

`[LIKELY — inference from the fact that every working open-source tool implements a vendor protocol and none uses these usages]`

Even where a device *does* declare `0x06:0x20`, **Windows' own HID stack will not surface it as a battery** — there is no generic HID battery class driver that consumes page `0x06`. That means:
- `GUID_DEVCLASS_BATTERY` enumeration will **not** list it.
- You'd have to read it yourself with hidapi (`get_feature_report` or `read`) and parse the report descriptor.

**Practical guidance:** implement `0x06:0x20` and `0x85:0x64` as *generic fallbacks* in your own parser (walk the report descriptor, find the usage, read the feature/input report), but expect a near-zero hit rate on gaming peripherals. Budget your effort for the vendor table.

## 5.3 The Windows IOCTL flow (for devices that DO register a battery)

This is the `GUID_DEVCLASS_BATTERY` / `IOCTL_BATTERY_QUERY_INFORMATION` path you asked about. It works only for devices with a battery **miniport** (hidbatt.sys or a vendor miniport) — i.e. UPSes, tablets, some docks. Gaming mice are not in this set.

### Step 1 — Enumerate

~~~c
HDEVINFO hdev = SetupDiGetClassDevs(&GUID_DEVCLASS_BATTERY, 0, 0,
                                    DIGCF_PRESENT | DIGCF_DEVICEINTERFACE);
SetupDiEnumDeviceInterfaces(hdev, 0, &GUID_DEVCLASS_BATTERY, idev, &did);
SetupDiGetDeviceInterfaceDetail(hdev, &did, pdidd, cbRequired, &cbRequired, 0);
~~~
> "*ClassGuid* parameter is a pointer to **GUID_DEVCLASS_BATTERY** (defined in BatClass.h)… set the *Flags* parameter to **DIGCF_PRESENT | DIGCF_INTERFACEDEVICE**"
> — [Enumerating Battery Devices](https://raw.githubusercontent.com/MicrosoftDocs/win32/docs/desktop-src/Power/enumerating-battery-devices.md) `[CERTAIN]`

### Step 2 — Open

~~~c
HANDLE hBattery = CreateFile(pdidd->DevicePath,
                             GENERIC_READ | GENERIC_WRITE,
                             FILE_SHARE_READ | FILE_SHARE_WRITE,
                             NULL, OPEN_EXISTING, FILE_ATTRIBUTE_NORMAL, NULL);
~~~

### Step 3 — Get the tag (must be first)

~~~c
BATTERY_QUERY_INFORMATION bqi = {0};
DWORD dwWait = 0, dwOut;
DeviceIoControl(hBattery, IOCTL_BATTERY_QUERY_TAG,
                &dwWait, sizeof(dwWait),
                &bqi.BatteryTag, sizeof(bqi.BatteryTag), &dwOut, NULL);
~~~

### Step 4 — Query information

~~~c
BATTERY_INFORMATION bi = {0};
bqi.InformationLevel = BatteryInformation;      // == 0
DeviceIoControl(hBattery, IOCTL_BATTERY_QUERY_INFORMATION,
                &bqi, sizeof(bqi), &bi, sizeof(bi), &dwOut, NULL);
if (bi.Capabilities & BATTERY_SYSTEM_BATTERY) { /* not a UPS */ }
~~~

### Step 5 — Query status (this is where the % comes from)

~~~c
BATTERY_WAIT_STATUS bws = {0};
bws.BatteryTag = bqi.BatteryTag;
BATTERY_STATUS bs;
DeviceIoControl(hBattery, IOCTL_BATTERY_QUERY_STATUS,
                &bws, sizeof(bws), &bs, sizeof(bs), &dwOut, NULL);
~~~

### Structures `[CERTAIN]`

~~~c
typedef struct _BATTERY_QUERY_INFORMATION {
    ULONG BatteryTag;
    BATTERY_QUERY_INFORMATION_LEVEL InformationLevel;
    LONG AtRate;
} BATTERY_QUERY_INFORMATION;

typedef struct _BATTERY_INFORMATION {
    ULONG Capabilities;      UCHAR Technology;  UCHAR Reserved[3];
    UCHAR Chemistry[4];
    ULONG DesignedCapacity;  ULONG FullChargedCapacity;
    ULONG DefaultAlert1;     ULONG DefaultAlert2;
    ULONG CriticalBias;      ULONG CycleCount;
} BATTERY_INFORMATION;

typedef struct _BATTERY_STATUS {
    ULONG PowerState;   ULONG Capacity;   ULONG Voltage;   LONG Rate;
} BATTERY_STATUS;
~~~

**Information levels** (`BATTERY_QUERY_INFORMATION_LEVEL`) `[CERTAIN]`:

| Value | Name | Returns |
|---|---|---|
| 0 | `BatteryInformation` | `BATTERY_INFORMATION` |
| 1 | `BatteryGranularityInformation` | array of `BATTERY_REPORTING_SCALE` |
| 2 | `BatteryTemperature` | ULONG, 10ths of °K |
| 3 | `BatteryEstimatedTime` | ULONG seconds (uses `AtRate`) |
| 4 | `BatteryDeviceName` | wide string |
| 5 | `BatteryManufactureDate` | `BATTERY_MANUFACTURE_DATE` |
| 6 | `BatteryManufactureName` | wide string |
| 7 | `BatteryUniqueID` | wide string |
| 8 | `BatterySerialNumber` | wide string |

> "BatteryInformation 0 — A BATTERY_INFORMATION structure… BatteryGranularityInformation 1 — An array of BATTERY_REPORTING_SCALE structures, never more than four entries."
> — [BATTERY_QUERY_INFORMATION structure](https://learn.microsoft.com/en-us/windows/win32/power/battery-query-information-str) `[CERTAIN]`

**Computing percent:** `Capacity / FullChargedCapacity * 100`.

> "Capacity — The current battery capacity, in mWh (or relative). This value can be used to generate a "gas gauge" display by dividing it by FullChargedCapacity member of the BATTERY_INFORMATION structure."
> — [BATTERY_STATUS structure](https://learn.microsoft.com/en-us/windows/win32/power/battery-status-str) `[CERTAIN]`

**PowerState flags** `[CERTAIN]`: `BATTERY_POWER_ON_LINE 0x1`, `BATTERY_DISCHARGING 0x2`, `BATTERY_CHARGING 0x4`, `BATTERY_CRITICAL 0x8`.

**Capabilities flags** `[CERTAIN]`: `BATTERY_CAPACITY_RELATIVE 0x40000000`, `BATTERY_IS_SHORT_TERM 0x20000000`, `BATTERY_SET_CHARGE_SUPPORTED 0x1`, `BATTERY_SET_DISCHARGE_SUPPORTED 0x2`, `BATTERY_SYSTEM_BATTERY 0x80000000`.

### Pitfalls

| Pitfall | Detail |
|---|---|
| **Tag invalidation** | "Whenever this value does not match the battery's current tag, the IOCTL request will be completed with **ERROR_FILE_NOT_FOUND**." Re-get the tag and retry. |
| **`BATTERY_CAPACITY_RELATIVE`** | If set, units are undefined — you can still compute a ratio, but not mWh. |
| **`BATTERY_SYSTEM_BATTERY`** | Must be set for a "real" battery; UPS batteries clear it. |
| **`ERROR_INVALID_FUNCTION`** | Returned when a requested info level isn't supported. Not fatal — skip it. |
| **`BATTERY_UNKNOWN_*`** | Sentinel values (`BATTERY_UNKNOWN_CAPACITY`, `_VOLTAGE`, `_RATE`, `_TIME`). Check before dividing. |
| **"USB_CHARGER_STATUS"** | ⚠️ There is **no** `BatteryUSBChargerStatus` information level in the documented enum (0–8 above). You may be thinking of the `BATTERY_USB_CHARGER_STATUS` *structure* used by USB-charger battery miniports alongside `IOCTL_BATTERY_QUERY_STATUS`. Treat as `[UNCERTAIN]` — it is not part of the standard query flow. |

---

# 6. Implementation blueprint for the Rust viewer

## 6.1 Detection pipeline

~~~
hidapi::enumerate()
  ├─ vendor_id match → vendor driver
  │    ├─ Logitech 0x046D → HID++ (find usage 0x01/0x02 on the right interface)
  │    ├─ Razer    0x1532 → 90-byte feature report
  │    ├─ SteelSeries 0x1038 → 0x06 0x18 / 0x00 0xB0
  │    ├─ HyperX   0x03F0 → 0x06 0xFF 0xBB 0x02
  │    ├─ Corsair  0x1B1C → interface 4, 0x00 0x02 … 0x0F
  │    ├─ Glorious 0x258A → 65-byte feature, 0x83 at [6]
  │    ├─ Lamzu    0x373E/0x37B0 → same as Glorious
  │    └─ Compx/Nordic (0x373B/0x3554/0x260D/0x1915/0x33E4) → 17-byte report 8
  └─ else → generic: usage 0x06:0x20 or 0x85:0x64 parsed from the report descriptor
~~~

## 6.2 Non-negotiable rules

1. **Open the input handle, not just the feature handle.** Every "reply dropped on the floor" bug traces to this. Some devices only queue replies for a handle that was already open when the request landed.
2. **Never coerce "no answer" to 0 %.** Distinguish `Absent` / `Asleep` / `Present(n)` / `Charging(unknown)`.
3. **Cache feature indices per device session**; invalidate on `INVALID_FEATURE_INDEX` (0x06) or device re-enumeration.
4. **Treat percent 0, 255, and >100 as sentinels**, not data.
5. **Serialise all traffic per handle** — HID++ has no request pipelining, and Razer's transaction-id matching breaks under concurrency. Solaar uses a global `request_lock`; the kernel uses `send_mutex`.
6. **Drain stale input** before each write (`_read_input_buffer` in Solaar). Otherwise you match a reply from the *previous* poll.
7. **Don't blind-probe feature IDs.** `0x1802` (device reset) and `0x1814` (change host) will ruin the user's day.

## 6.3 Timeouts that empirically work

| Scenario | Timeout |
|---|---|
| Receiver (dev 0xFF) request | 0.9 s |
| Device behind receiver | 4.0 s |
| Razer wireless settle | 0.34 s |
| Nordic push wait | 3 s |
| Realtek push wait | 3 s |
| AttackShark push | 2.5 s |
| Generic HID++ FAP | 0.5 s (mouse_tray) |

> `_RECEIVER_REQUEST_TIMEOUT = 0.9` / `_DEVICE_REQUEST_TIMEOUT = DEFAULT_TIMEOUT = 4`
> — [Solaar base.py](https://raw.githubusercontent.com/pwr-Solaar/Solaar/master/lib/logitech_receiver/base.py) `[CERTAIN]`

## 6.4 Reference implementations, ranked

| Project | Language | Why it's useful |
|---|---|---|
| [Solaar](https://github.com/pwr-Solaar/Solaar) | Python | Most complete HID++ 1.0 + 2.0, best comments |
| [Linux kernel hid-logitech-hidpp.c](https://git.zx2c4.com/linux-rng/plain/drivers/hid/hid-logitech-hidpp.c) | C | Authoritative byte layouts, voltage curves |
| [libratbag](https://github.com/libratbag/libratbag) | C | Clean HID++1.0 register code, error enum |
| [OpenRazer](https://github.com/openrazer/openrazer) | C/Python | Only real Razer source |
| [HeadsetControl](https://github.com/Sapd/HeadsetControl) | C++ | ~30 headset protocols, per-model calibration |
| [mouse_tray](https://github.com/Fan4Metal/mouse_tray) | Python | Best cross-vendor summary + hidapi gotchas |
| [hidpp crate](https://docs.rs/hidpp/) | Rust | Ready-made HID++ 2.0 incl. unified battery |
| [crstian19/mow](https://github.com/crstian19/mow) | Rust | Glorious; small enough to read in 5 min |

---

# 7. Certainty ledger

| Claim | Status |
|---|---|
| HID++ framing (0x10/0x11, 7/20 bytes), sw_id nibble, 0 = notification | ✅ **CERTAIN** — official spec + kernel + Solaar |
| ROOT getFeature returns 0 when unsupported | ✅ **CERTAIN** — spec + kernel |
| HID++2.0 error = feature_index byte 0xFF | ✅ **CERTAIN** — kernel + Solaar |
| 0x1004 get_status: soc/level-bitmask/status/extPower at 4/5/6/7 | ✅ **CERTAIN** — kernel + hidpp crate + Solaar |
| 0x1000 byte 4 = % but 0 means unknown | ✅ **CERTAIN** — spec text + kernel |
| 0x1001 big-endian mV; 0x80 = charging-valid | ✅ **CERTAIN** — kernel |
| 0x1001 voltage→% curve is Li-ion 4186…3537 | ✅ **CERTAIN** — kernel (but model-specific in practice) |
| HID++1.0 reg 0x07 = level enum + charge state + 5 % threshold | ✅ **CERTAIN** — Solaar + libratbag agree |
| HID++1.0 reg 0x0D = true %, mask 0x7F | ✅ **CERTAIN** — libratbag (Solaar omits the mask) |
| Razer 90-byte layout | ✅ **CERTAIN** — OpenRazer struct + static_assert |
| Razer CRC is **XOR**, not CRC-8 | ✅ **CERTAIN** — OpenRazer source |
| Razer CRC byte range 2..87 | ⚠️ **CONFLICT** — OpenRazer 2..87 vs uChroma 1..86. Test on hardware. |
| Razer battery at arguments[1], 0–255 | ✅ **CERTAIN** — OpenRazer |
| Razer transaction id is per-PID (1F/3F/FF) | ✅ **CERTAIN** — OpenRazer switch table |
| 0x1802/0x1814/0x1E00 are NOT battery features | ✅ **CERTAIN** — Solaar constants |
| Nordic 52840 17-byte / 0x55 checksum | ✅ **CERTAIN** — two independent implementations |
| Nordic 54L15 64-byte, LE voltage | ⚠️ **LIKELY** — single implementation |
| Realtek push XOR 0xFF | ⚠️ **LIKELY** — single capture |
| Attack Shark P2 units (percent vs ×10) | ❓ **UNCERTAIN** — author flags it |
| SteelSeries Nova 7 discrete (0–4) vs gen2 (0–100) | ✅ **CERTAIN** — explicit PID lists |
| Corsair battery 1–1000 in [4]/[5] LE | ⚠️ **LIKELY** — single implementation, with retry workaround |
| Windows only HID battery client is 0x84:0x04 | ✅ **CERTAIN** — MS HID clients doc |
| `0x85:0x6C` exists | ❌ **FALSE** — not in HUT 1.5 |
| Keychron/Fantech/WLmouse/AULA battery protocol | ❓ **NOT FOUND** — do not guess |

---

## Appendix — quick copy-paste byte tables

~~~
# HID++ 2.0  — resolve feature index for 0x1004
10 <dev> 00 01 10 04 00

# HID++ 2.0  — 0x1004 get_capabilities  (after index resolved to <f>)
10 <dev> <f> 01 00 00 00

# HID++ 2.0  — 0x1004 get_status
10 <dev> <f> 11 00 00 00
   reply: 10 <dev> <f> 11 <soc> <lvl> <chg> <ext>

# HID++ 2.0  — 0x1000 get_status
10 <dev> <f> 01 00 00 00
# HID++ 2.0  — 0x1001 get_voltage
10 <dev> <f> 01 00 00 00
# HID++ 2.0  — 0x1F20 get_adc
10 <dev> <f> 01 00 00 00

# HID++ 1.0  — register 0x07
10 <dev> 81 07 00 00 00
# HID++ 1.0  — register 0x0D
10 <dev> 81 0D 00 00 00

# Razer  — battery (90 bytes, feature report id 0)
00 1F 00 00 00 02 07 80  <80 x 00>  <xor(2..87)>  00
# Razer  — charging status
00 1F 00 00 00 02 07 84  <80 x 00>  <xor(2..87)>  00

# Nordic 52840 / Compx  (17 bytes)
08 04 00 00 00 00 00 00 00 00 00 00 00 00 00 00 49

# Nordic 54L15  (64 bytes)
08 7D 72 02 00 00 07 01  <56 x 00>

# Glorious / Lamzu  (65 bytes)
00 00 00 02 02 00 83 00  <57 x 00>

# SteelSeries Arctis 7 / Pro  (31 bytes)
06 18  <29 x 00>
# SteelSeries Nova status
00 B0

# HyperX Cloud II Wireless  (52 bytes)
06 FF BB 02 00

# A4Tech Bloody 8K
00 80 00 00 00 00 00 7F
00 80 FF 00 00 00 00 80
~~~
