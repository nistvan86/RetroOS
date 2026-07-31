# PXE/UNDI Raw-Ethernet Kernel Logging Plan

## Goal

Send RetroOS kernel diagnostics from the diskless D945GSEJT boot to a remote
machine when no serial adapter is available.

The preferred design is a small one-way raw-Ethernet logger using the PXE/UNDI
interface left by the board firmware. A root-run Python `AF_PACKET` program on
the directly connected Raspberry Pi 4 will receive and reconstruct the log.
IPv4, UDP, ARP, DHCP, and a general network stack are unnecessary on this
closed link and are deferred unless interoperability later becomes useful.

## Current status summary — 2026-07-31

- Raw Ethernet RLOG is working end to end on the physical board while VGA,
  port `E9`, and RAM klog remain active simultaneously.
- The deployed hardware target is `//kernel:kernel_elf_pxe_netlog`; the normal
  kernel target remains separate and unchanged in behavior.
- The checked-in receiver is `tools/pxe_rlog_receiver.py`. It listens on Pi
  `eth0` as root, filters EtherType `0x88B5`, validates RLOG v1 headers, tracks
  sessions/sequences, prints live output, and appends payload bytes to a log.
- A complete boot and DN short-loop capture is preserved as
  `retroos-rlog-current.log`. Frames `0..7` arrived without gaps.
- DN's first run exits `-11` after 277 ms following a protection fault at
  address `0x9c81a`. The next two runs exit `0x0206` after 277 ms with invalid
  opcode at `CS:EIP=9b58:4a80`; that segmented address resolves exactly to
  physical `0xA0000`. The three-short-exit guard then halts as designed.
- These addresses implicate interaction between retained PXE state/calls and
  the DOS/VM86 low-memory environment. The logging channel is proven; the next
  investigation should isolate firmware identity mappings and call state from
  guest low memory rather than revisit Ethernet framing.
- User policy: never automatically roll back an experimental kernel after a
  boot loop. Leave it deployed for diagnosis unless rollback is explicitly
  requested; the user will power off the SEJT.

### Raspberry Pi raw-socket validation — 2026-07-31

- PXE-facing interface: `eth0`, MAC `e4:5f:01:66:c3:d4`, address
  `10.77.77.1/24` (the address is not needed by this design).
- Passwordless `sudo` can run Python with `CAP_NET_RAW` through root.
- A Python `AF_PACKET` sender injected a 60-byte broadcast frame on `eth0` and
  an `ETH_P_ALL` packet socket captured its outgoing copy (`PACKET_OUTGOING`):

  ```text
  sent 60 captured 60 pkttype 4 ethertype 88b5 payload RLOG...PI-RAW-SELFTEST
  ```

- A socket created with only protocol `0x88B5` did not receive the Pi's own
  outgoing copy during this self-test. The receiver should therefore open
  `ETH_P_ALL`, bind to `eth0`, and filter bytes 12–13 for EtherType `0x88B5`
  in Python. Incoming SEJT frames should still be separately recognizable by
  packet type and source MAC.

## Hardware progress — 2026-07-31

### Confirmed on the D945GSEJT

- The board boot screen identifies the option ROM as:

  ```text
  Intel PXE-2.1 build 082
  ```

  Treat the protected-mode quirks below as observations about this exact old
  Intel implementation, not as general PXE 2.1 calling requirements.
- GRUB/PXE leaves both `PXENV+` and `!PXE` structures in low memory.
- Both structures have valid checksums and nonzero entry information.
- The relevant `!PXE` fields observed on the board are:

  ```text
  EntryPointESP = 9B58:019F
  SegDescCnt    = 7
  FirstSelector = 0000
  segment words = 0000 914B 9B58 9B58 0000 0000 0000
  ```

- The zero entries correspond to an absent firmware stack/base-code recipe;
  the live UNDI data and code values are `914B` and `9B58`.
- A protected-mode far call reaches the vendor UNDI runtime and returns from
  `PXENV_UNDI_GET_INFORMATION` normally.
- The first successful call returned `AX=0001, Status=006A`, meaning
  `PXENV_STATUS_UNDI_INVALID_STATE`. The runtime is present but GRUB left UNDI
  inactive.
- `PXENV_UNDI_GET_STATE` (`0015h`) is unusable on this firmware after handoff:
  it faults internally at `9B58:00000BB8`. State recovery therefore proceeds
  directly through `PXENV_UNDI_STARTUP` (`0001h`).

### Probe implementation

The diagnostic kernel is a separate Bazel target:

```bash
bazelisk build //kernel:kernel_elf_pxe_probe
```

Its output is `bazel-bin/kernel/kernel_pxe_probe.elf`. It:

- scans and validates `PXENV+` and `!PXE`;
- runs before RetroOS drops from ring 0 to ring 1;
- temporarily identity-maps physical memory below 1 MiB;
- reconstructs the firmware's GDT descriptors;
- currently calls `PXENV_UNDI_STARTUP` followed by
  `PXENV_UNDI_GET_INFORMATION`;
- provides a probe-only compatibility layer for the legacy PCI BIOS calls made
  by the Intel UNDI runtime;
- emulates the observed protected-mode UNDI self-patch while retaining normal
  executable-segment protection;
- halts after one screen of output;
- uses a probe-only compact exception handler so a fault does not clear the
  useful screen or replace it with a stack trace.

Relevant implementation files:

- `kernel/src/kernel/pxe.rs`: discovery, compact screen report, and call request.
- `kernel/src/arch/boot.rs`: invokes the probe at ring 0 and halts.
- `arch-metal/src/pxe_call.rs`: 32-bit-to-16-bit protected-mode call adapter.
- `arch-metal/src/descriptors.rs`: temporary PXE GDT reconstruction.
- `arch-metal/src/paging2.rs`: temporary low-memory identity mapping.
- `arch-metal/src/traps.rs`: compact probe-only ring-0 fault report.

### Fault sequence and conclusions

Each hardware fault removed one ambiguity:

1. The original `PXENV+` protected-mode call faulted in the ordinary ring-1
   trap path. Its selector belonged to the old PXE/GRUB environment and the
   call used the wrong ABI.
2. The first `!PXE` call produced:

   ```text
   #GP err=9148 at 00A0:000001B5
   ```

   This proved the firmware entry executed, then tried to load its original
   UNDI data selector `914B`. The runtime uses those high selector values
   internally, so RetroOS must install them exactly; assigning replacement
   consecutive selectors is not sufficient on this Intel implementation.
3. After installing the exact selectors, the next result was:

   ```text
   #GP err=C0B0 at 9B58:000001FB
   ```

   `C0B0` is the high word of RetroOS's kernel return address. This showed that
   a 32-bit far-call return frame was being interpreted as the 16-bit frame
   expected by UNDI.
4. A small 16-bit protected-mode trampoline was added. RetroOS enters the
   trampoline from 32-bit code and pushes the three required 16-bit PXE
   parameters. The first layout placed the parameter pointer four bytes before
   the location used by this firmware. The screen probe showed:

   ```text
   [EBP+0A] parameter pointer
   [EBP+0E] firmware entry point 9B58:019F
   ```

   Four bytes of experimental call-frame padding moved the parameter pointer
   to `[EBP+0E]`. This is not a general PXE requirement; the final bridge must
   replace the experiment with a standards-correct far-call/return frame.
5. With the corrected observed frame, `GET_INFORMATION` returned normally:

   ```text
   UNDI GET_INFO AX=0001 ST=006A
   ```

   `006A` is `PXENV_STATUS_UNDI_INVALID_STATE`, proving the firmware is alive
   and callable after GRUB but its UNDI state was closed or shut down.
6. `GET_STATE` then faulted inside vendor firmware at offset `0BB8`, so it is
   skipped on this board.
7. Direct `UNDI_STARTUP` invoked software interrupt `INT 1Ah`. The captured
   register state identified the exact BIOS operation:

   ```text
   AX=B109 BX=0100 DI=0010
   ```

   This is PCI BIOS "read configuration word" for bus 1, device 0, function 0,
   offset `10h` (BAR0). RetroOS owns the protected-mode IDT, so the old BIOS
   handler was no longer reachable.
8. A probe-only PCI BIOS compatibility layer now handles:

   - `B101`: installation check;
   - `B102`/`B103`: find device/class;
   - `B108`–`B10A`: configuration byte/word/dword reads;
   - `B10B`–`B10D`: configuration byte/word/dword writes;
   - BIOS status in `AH` and carry-flag success/failure semantics.

   The observed `B109` call then succeeded and returned `CX=E000`.
9. Firmware resumed and faulted at `9B58:0249`. The captured bytes decode as:

   ```asm
   mov  ah, 0xB1
   mov  al, 0x09
   mov  di, 0x0010
   int  0x1A
   and  cl, 0xFE
   mov  cs:[0x0106], cx    ; #GP at 0249
   ```

   The Intel runtime self-patches its code image through a `CS:` override.
   Protected-mode code descriptors are not writable, and the advertised UNDI
   code/write recipes use the same internal segment value on this board. The
   probe now narrowly emulates `mov cs:[disp16], r16` only for a ring-0 PXE
   segment below `A0000h`, writes through RetroOS's flat mapping, advances EIP,
   and resumes the firmware. Normal kernel faults are not bypassed.
10. The exact motherboard BIOS archives were obtained from
    [The Retro Web D945GSEJT page](https://theretroweb.com/motherboards/s/intel-d945gsejt-johnstown).
    Intel BIOS releases 0025 through 0040 contain `Intel UNDI, PXE-2.1 (build
    082)`. Releases 0031 through 0040 share this identical extracted CSM/PXE
    module SHA-256:

    ```text
    af7d422a809decbe729cdfe5cc2be89b8cdc5361730f8cf40ada6195902feabc
    ```

    The captured bytes at runtime offsets `0240` and `20BE` match that module
    exactly. Runtime offset `20BE` is another self-write, not interrupt `BD`:

    ```asm
    mov word cs:[0x20B9], 0
    ```

    Static disassembly found four write encodings used by build 082. The probe
    handler now supports register word stores, immediate word stores, word
    increments, and EAX/AX absolute stores. The module's software interrupts
    are PCI BIOS `INT 1Ah` plus `INT 10h`/`INT 16h` in console/error paths.
11. With all build-082 self-write forms emulated, `UNDI_STARTUP` completed:

    ```text
    SU A=0000 S=0000
    GI A=0001 S=006A
    ```

    This is the expected state transition: startup succeeded, while
    `GET_INFORMATION` remains invalid until `UNDI_INITIALIZE`. The current
    probe conditionally attempts `STARTUP -> INITIALIZE -> GET_INFO -> OPEN ->
    GET_INFO`. `INITIALIZE` uses a null `ProtocolIni`; `OPEN` uses `OpenFlag=0`,
    directed+broadcast filtering, and an empty multicast list. It does not
    transmit a packet.
12. The Intel D945GSEJT hardware then completed the entire conditional UNDI
    bring-up sequence successfully (phone OCR normalized below):

    ```text
    SU  A=0000 S=0000
    IN  A=0000 S=0000
    GI1 A=0000 S=0000
    OP  A=0000 S=0000
    GI2 A=0000 S=0000
    ```

    This confirms that Intel PXE 2.1 build 082 remains usable after GRUB and
    RetroOS enter protected mode, provided the probe's PCI-BIOS and code-write
    compatibility handling is active. UNDI is now initialized and open; the
    next unknown is transmit operation, not firmware reinitialization.
13. Raw Ethernet transmit is now confirmed end to end. The probe built a
    60-byte broadcast frame with EtherType `88B5h` and returned:

    ```text
    M=00270E04CF7C
    TX A=0000 S=0000
    ```

    The root-run Pi `AF_PACKET` listener on `eth0` captured the exact frame:

    ```text
    src=00:27:0e:04:cf:7c len=60
    RLOG 01 01 "HELLO FROM RETROOS SEJT!"
    ```

    This proves the complete path: RetroOS parameter/TBD construction,
    protected-mode Intel UNDI call, NIC transmission, physical Ethernet link,
    and Pi raw-socket reception. No ARP, IPv4, UDP, DHCP, or native NIC driver
    was involved.
14. The first non-halting logger attempted to recycle each buffer only after
    polling `PXENV_UNDI_ISR`. Intel build 082 returned from that ISR call to
    the correct temporary trampoline selector but invalid offset `FFFF`,
    producing `#GP`; repairing only the offset left inconsistent call state
    and caused a reboot/triple-fault loop. That repair was removed.
15. The revised logger does not call `UNDI_ISR` and never reuses an accepted
    transmit buffer. Its first pool layout accidentally consumed 1 MiB because
    every 1 KiB element had 4 KiB alignment; the enlarged early ELF memory
    segment caused a pre-console reboot and was rolled back. The corrected
    layout is one 64-KiB-aligned allocation containing 64 tightly packed 1-KiB
    slots. After an immediate first-line health marker, the sink batches up to
    512 bytes per frame, providing roughly 32 KiB of remote payload. Pool
    exhaustion or any transmit failure disables only remote logging; VGA, port
    `E9`, RAM klog, and normal boot continue.

### Current deployed experiment

The current image is the non-halting `kernel_pxe_netlog.elf`. It initializes
and opens UNDI at ring 0, enters the ordinary RetroOS ring-1 boot path, and
mirrors bounded kernel-log chunks into sequenced RLOG Ethernet frames. It does
not call the broken `PXENV_UNDI_ISR`; accepted transmit buffers are never
reused during the boot.

Current deployed SHA-256:

```text
852d8651244ee10365ccd5cd744f9e468a20a4841dbe314f6f928ac42d906177
```

This checksum was verified identical locally and at
`/srv/tftp/boot/retroos/kernel.elf`. Deployment does not reboot the board.

Immediate next action: determine which PXE call side effect makes DN's VM86
environment fault in the `0x9Bxxx..0xA0000` region. Audit `map_pxe_identity()`
and the temporary GDT/call stack lifecycle first. Preserve the now-working
RLOG transport and receiver as the observation channel for every hardware
iteration.

### Important limitation for a possible future UDP logger

PXE 2.1 UNDI calls support the `EntryPointESP` environment used by this probe.
The firmware Base-Code UDP/TFTP services generally require a 16-bit stack
entry and are explicitly unavailable through the 32-bit-stack form. Therefore
any future UDP logger would use:

```text
UNDI Ethernet transmit + RetroOS ARP/IPv4/UDP
```

Using the firmware's higher-level `PXENV_UDP_WRITE` would require an additional
16-bit-stack bridge and retaining a usable Base-Code runtime; it should be
treated as a separate alternative, not assumed to follow automatically from
the current UNDI probe.

## Why raw Ethernet is preferred

The alternatives are:

- PXE/UNDI + TFTP upload: standardized file transfer, but requires TFTP
  `WRQ`/`ACK` state handling, block numbering, transfer ports, a writable TFTP
  server, and filename management.
- PXE/UNDI + raw-Ethernet logging: only needs packet transmission, a small
  frame header, and a privileged receiver. It avoids TFTP, ARP, IP, UDP, and
  checksum machinery and is the shortest route to one-way crash diagnostics.
- PXE/UNDI + custom UDP logging remains an optional compatibility layer if
  logs later need to cross a routed network.

TFTP remains a possible later feature. The [`ttftp`](https://docs.rs/ttftp)
Rust crate is `no_std` and
`no_alloc`, but it supplies only the TFTP protocol state machine, not PXE,
UNDI, Ethernet, ARP, IP, or UDP.

## Layered architecture

```text
RetroOS kernel log buffer
        |
        v
custom raw-Ethernet log framing
        |
        v
PXE/UNDI firmware calls
        |
        v
physical NIC
        |
        v
Python AF_PACKET receiver on the Raspberry Pi
```

UNDI already provides the only transport layer required for this design:
low-level Ethernet packet access.

## Firmware assertions to validate

Before sending a real log, the kernel should test and display each result:

- PXE discovery:
  - `PXENV+` or `!PXE` signature found in low memory.
  - Structure checksum valid.
  - Structure length and version sane.
  - PXE API entry pointer nonzero and inside valid low memory.
- UNDI:
  - `PXENV_UNDI_GET_INFORMATION` succeeds.
  - Valid hardware type and MAC address reported.
  - Interface is already started/initialized, or can be safely initialized.
  - UNDI open succeeds when required.
  - A harmless test transmit is accepted.
  - Transmit completion normally uses the UNDI ISR path; Intel build 082's ISR
    return is unusable here, so the bounded logger retains accepted buffers.
- Link configuration:
  - UNDI reports the board's source MAC address.
  - A broadcast frame with EtherType `88B5h` is accepted for transmission.
  - The Pi's raw socket observes the frame on the PXE Ethernet interface.
- Firmware lifetime:
  - GRUB did not unload the PXE stack.
  - The kernel has not overwritten PXE structures or firmware buffers.
  - The firmware entry point remains callable after protected mode and paging.
  - Firmware calls use the real-mode/protected-mode convention they require.

The first visible diagnostic milestone should look like:

```text
PXE: PXENV+/!PXE found
PXE: checksum valid
UNDI: information OK
UNDI: MAC xx:xx:xx:xx:xx:xx
PXE: DHCP cache valid
UNDI: test transmit OK
ETH: test frame sent
```

## Raw-Ethernet log frame format

Use a deliberately small, stateless packet format. A proposed packet is:

```text
Ethernet destination = FF:FF:FF:FF:FF:FF initially
Ethernet source      = UNDI station MAC
EtherType            = 88B5h (local experimental use)
magic[4]             = "RLOG"
version[1]
flags[1]
session_id[4]
sequence[4]
payload_len[2]
payload[payload_len]
zero padding to the Ethernet minimum frame size
```

Recommended behavior:

- Send a boot/session identifier with every packet.
- Increment a sequence number for every packet.
- Keep payloads below the Ethernet MTU, initially 512 or 1024 bytes.
- Let the NIC append the Ethernet frame-check sequence; it is not included in
  the UNDI transmit buffer.
- Send an explicit final packet when the kernel halts after the DN short-loop
  guard.
- Start with best-effort logging; add ACKs only if packet loss is observed.
- Never block kernel startup indefinitely waiting for the receiver.
- Rate-limit repeated log lines and bound the RAM buffer.

## Python receiver

Create a root-run Python program on the Pi that:

- Opens an `AF_PACKET`/`SOCK_RAW` socket on the PXE-facing interface.
- Filters EtherType `0x88B5` and validates the Ethernet and `RLOG` headers.
- Validates the `RLOG` magic and version.
- Groups packets by session ID.
- Detects missing or duplicate sequence numbers.
- Writes payloads to a timestamped `.log` file.
- Prints packets live to the terminal.
- Marks the final packet and reports packet loss.
- Documents that root or `CAP_NET_RAW` is required.

The receiver should be usable both with QEMU and with the physical PXE test.

## QEMU validation strategy

QEMU supports BIOS PXE boot through virtual NIC option ROMs. Its user-mode
network backend can provide DHCP and a read-only built-in TFTP server:

```bash
qemu-system-i386 \
  -machine pc \
  -m 128M \
  -boot n \
  -device e1000,netdev=net0 \
  -netdev user,id=net0,tftp=/tmp/pxe,bootfile=/bootx86.pxe
```

This can validate:

- BIOS PXE loading `bootx86.pxe`.
- GRUB loading its modules and configuration through PXE/TFTP.
- GRUB loading `kernel.elf` through PXE/TFTP.
- Whether `PXENV+`/`!PXE` structures remain discoverable by the kernel.
- Whether an UNDI test transmit succeeds.
- Whether a raw Ethernet frame reaches a TAP-backed host receiver.

QEMU's built-in TFTP server is read-only, so it cannot validate TFTP upload
(`WRQ`). That is not a limitation for custom UDP logging.

For more faithful Ethernet behavior, use a TAP backend and an external DHCP/
TFTP server. A TAP backend is preferred for the raw-socket receiver test;
user-mode networking does not expose equivalent host Ethernet frames and does
not reproduce the D945GSEJT's exact NIC PXE firmware.

QEMU validation cannot prove that the physical board's vendor PXE stack remains
callable after GRUB. It can prove the layering and catch packet-format,
framing, timeout, and receiver bugs before hardware testing.

## Suggested implementation phases

1. Add read-only PXE/UNDI structure discovery and visible assertions.
2. Add a harmless UNDI transmit test.
3. Add the raw-Ethernet `RLOG` frame format.
4. Add the Python `AF_PACKET` receiver and verify it locally on the Pi, then
   optionally through QEMU with TAP networking.
5. Send a short kernel startup marker from QEMU through the full PXE path.
6. Add bounded kernel-log buffering.
7. Flush logs on the DN short-restart halt, including the `0x0206` invalid
   opcode diagnosis currently observed on the D945GSEJT.
8. Test the same kernel through the Raspberry Pi PXE deployment path.
9. Add optional packet acknowledgments only if best-effort delivery is
   insufficient.

## Reuse and estimated effort

- [`ttftp`](https://docs.rs/ttftp): reusable only for a future TFTP
  implementation.
- [`smoltcp`](https://docs.rs/crate/smoltcp/latest): possible reusable
  Ethernet/ARP/IPv4/UDP stack, but requires a
  custom UNDI-backed device adapter and integration into the Bazel/no-std
  build.
- GRUB PXE source: useful as a reference for PXE structure layouts and
  firmware-call conventions, not a drop-in Rust dependency.
- Custom raw-Ethernet logger: approximately 1–3 engineering days from the
  now-confirmed UNDI open state. Most remaining uncertainty is in the transmit
  descriptor and completion path, not protocol implementation.
- Custom minimal UDP logger: approximately 2–5 additional engineering days if
  later required, depending on whether fixed addressing or ARP/DHCP is wanted.
- TFTP upload: approximately 6–12 engineering days because of the additional
  TFTP state machine, writable server, transfer semantics, and retries.

## Safety and failure behavior

- Logging must be optional and disabled if PXE/UNDI assertions fail.
- No network operation may prevent normal boot when the receiver is absent.
- Use bounded retries and a short total timeout.
- The Pi receiver should choose its own output path and reject malformed or
  oversized frames.
- Preserve the existing on-screen diagnostic output even when UDP logging is
  active.
