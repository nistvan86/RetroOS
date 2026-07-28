# Task: reliable serial HostFs root on bare metal

## Status

Planned. No implementation changes have been made for this task.

The current code already mounts HostFs as the root filesystem when HostFs is
available and no ext filesystem is found. This task makes that existing path
safe and usable over a three-wire physical serial connection, validates it in
QEMU, and adds an optional explicit boot policy for development.

## Goal

Boot `kernel.elf` on the D945GSEJT, preferably via GRUB/TFTP, and have RetroOS
automatically use a directory served by the Raspberry Pi as its root
filesystem over a serial port.

The minimum physical connection must be sufficient:

```text
D945GSEJT TX  -> adapter RX
D945GSEJT RX  <- adapter TX
D945GSEJT GND <-> adapter GND
```

RTS, CTS, DTR and DSR must not be required. HostFs availability must be
established by an application-level protocol handshake with a bounded
timeout, not by modem-control signals.

The intended two-port assignment is:

```text
COM1 (0x3F8) -> HostFs
COM2 (0x2F8) -> kernel/debug logging
```

COM2 logging is related bare-metal automation work, but is not required to
complete the HostFs protocol changes.

## Confirmed current behavior

### Root selection already exists

`kernel/src/kernel/startup.rs::mount_filesystems` currently implements:

```rust
if ext.is_empty() {
    if hostfs {
        vfs::mount(b"", host_fs());
        crate::screenln!(screen, "hostfs mounted as root");
    }
} else {
    // An ext filesystem is root.
    // If HostFs is available, it is mounted at /host.
}
```

Therefore HostFs becoming root is existing behavior, not a proposed new
filesystem architecture:

- ext found + HostFs available: ext is root, HostFs is `/host`;
- no ext + HostFs available: HostFs is root;
- neither available: only the embedded boot filesystem is available.

The embedded TAR filesystem is mounted at `C:\BOOT` after root selection, so
COMMAND.COM and DOS Navigator remain available even when HostFs supplies
`C:`.

### Current serial transport

`kernel/src/kernel/fs/hostfs.rs`:

- hard-codes COM1 at `0x3F8`;
- configures it for 115200 baud, 8 data bits, no parity, one stop bit;
- uses polling I/O;
- currently considers HostFs present when the UART exists and either CTS or
  DSR is asserted;
- performs no HostFs protocol handshake;
- has blocking receive loops with no timeout after the filesystem is mounted.

The CTS/DSR test is unsuitable for the desired three-wire connection. It can
also produce a false positive: an attached serial device is not proof that a
HostFs server is running.

### Current Pi/server transport

`hostfs.py` implements the server-side filesystem protocol, but its executable
entry point only connects as a client to a QEMU Unix-domain chardev socket.
It cannot currently open `/dev/ttyUSB0` or another physical serial device.

Existing filesystem command bytes are `0x01` through `0x07`:

```text
01 OPEN
02 READ
03 CLOSE
04 STAT
05 READDIR
06 CREATE
07 WRITE
```

The handshake must use a new, non-conflicting command.

## Scope

1. Replace the CTS/DSR availability test with a versioned HostFs handshake.
2. Make the handshake time out and fail cleanly when no server answers.
3. Preserve all existing filesystem command encodings and behavior.
4. Add a physical serial transport mode to `hostfs.py`.
5. Validate HostFs root and `/host` mounting in QEMU.
6. Validate that boot continues when no HostFs server is present.
7. Document Pi startup, serial wiring, and GRUB/QEMU usage.
8. Optionally add boot arguments for explicit root and COM-port selection.

## Non-goals

- Transferring large games efficiently over serial.
- Multiplexing HostFs and kernel logs on one UART.
- Adding a RetroOS network stack, NFS, or a remote block device.
- Making serial HostFs accesses asynchronous.
- Implementing COM2 logging as part of the core HostFs change.
- Persisting an in-memory disk image back to the Pi.

## Protocol handshake

Reserve a command value outside `0x01..0x07`, for example `0xF0`, for
`HELLO`. Do not finalize the byte until both protocol implementations and
tests have been reviewed for assumptions about unknown commands.

A compact proposed exchange is:

```text
client -> server: F0 52 4F 53 01
server -> client: F0 4F 4B 01
```

Meaning:

```text
F0       HELLO command/reply marker
52 4F 53 ASCII "ROS"
01       HostFs protocol version
4F 4B    ASCII "OK"
```

Required properties:

- a response is accepted only when the complete magic and supported version
  match;
- stale or unrelated serial bytes do not cause a successful probe;
- the client drains stale RX bytes before sending `HELLO`;
- the server may wait indefinitely for a client, but kernel probing may not;
- a version mismatch is reported as unavailable, not mounted;
- after success, the byte stream is positioned exactly at the start of the
  next filesystem request;
- retries must not leave partial handshake bytes that confuse the server.

One or two bounded retries are sufficient. The protocol should stay simple:
the two dedicated serial ports remove the need for log/filesystem framing or
multiplexing.

## Timeout design

The current `hostfs::init()` has no `Arch` argument and cannot directly use
the architecture clock. `platform::probe(machine, boot)` does have access to
`machine.get_ticks()`.

Preferred implementation:

1. Refactor the serial probe so it receives the machine/time source, either by
   making `hostfs::init` generic over `Arch` or by passing a small callback.
2. Poll the UART line-status register for RX data while comparing
   `machine.get_ticks()` against a deadline.
3. Use a short boot-time timeout, initially around 500-1000 ms.
4. Avoid an uncalibrated CPU spin-count timeout; it would vary greatly between
   QEMU, the Atom board, and modern hardware.
5. Confirm that the hardware timer is running before `platform::probe`.
   If it is not, move the HostFs probe later in startup or expose an
   appropriate architecture delay/deadline primitive.

Only the discovery handshake needs to be non-blocking for this task. Once
HostFs has been positively identified, normal filesystem operations may keep
their existing blocking semantics. Per-operation timeouts and reconnect
support can be a later reliability task.

## Kernel-side implementation

Likely files:

```text
kernel/src/kernel/fs/hostfs.rs
kernel/src/kernel/platform.rs
kernel/src/kernel/startup.rs
kernel/src/kernel/boot_config.rs or the current boot-argument parser
```

Required changes:

1. Keep the UART scratch-register existence check.
2. Configure the selected UART as 115200 8N1 with FIFO enabled.
3. Do not gate availability on modem-status-register CTS/DSR bits.
4. Drain any pending RX bytes with a bounded loop.
5. Send the versioned `HELLO`.
6. Read and validate the response under a deadline.
7. Return `true` only after a valid response.
8. Log a concise reason for absence, timeout, or version mismatch.
9. Ensure the failure path leaves boot usable and does not mount HostFs.

The first implementation may keep COM1 fixed. A subsequent small cleanup can
replace the constant with a UART descriptor selected from a boot option:

```text
hostfs=com1
hostfs=com2
hostfs=off
```

Do not silently move HostFs to COM2 because the planned logging assignment
uses COM2.

## Root-selection boot policy

No policy change is required for diskless operation: the current automatic
selection already makes HostFs root when no ext partition exists.

For repeatable development, consider adding:

```text
root=auto
root=hostfs
root=ext
```

Suggested semantics:

- `root=auto`: preserve current behavior;
- `root=hostfs`: require a successful HostFs handshake and mount it as root
  even if an ext filesystem exists;
- `root=ext`: never select HostFs as root, but it may still mount at `/host`.

If `root=hostfs` is explicitly requested but the handshake fails, display a
clear error and fall back to the embedded boot filesystem rather than hanging.
Whether this should be a fatal boot error can be revisited after bare-metal
experience.

## Server-side implementation

Extend `hostfs.py` without duplicating its filesystem dispatcher. Both
transports should supply the same blocking byte-stream interface.

Suggested CLI:

```text
hostfs.py --socket /tmp/retroos-hostfs.sock DIRECTORY
hostfs.py --serial /dev/ttyUSB0 --baud 115200 DIRECTORY
```

Compatibility with the current positional socket invocation should be kept
temporarily so existing scripts do not break.

The serial mode must configure:

```text
115200 baud
8 data bits
no parity
1 stop bit
no RTS/CTS flow control
no DSR/DTR flow control
no XON/XOFF
raw binary mode
```

Implementation choices:

- Python `pyserial` is convenient and portable but adds a Pi dependency.
- Linux `termios` avoids an external package but is Linux-specific and needs
  careful raw-mode setup.

Prefer `pyserial` for the first physical implementation unless keeping the Pi
installation dependency-free is considered more important. Document the
chosen package and exact installation command, but do not install it as part
of this task without confirmation.

The server must recognize `HELLO`, return the versioned reply, and then
continue dispatching the existing commands. Unknown commands should terminate
or resynchronize explicitly rather than leaving an ambiguous stream.

## QEMU test topology

QEMU already emulates a 16550-compatible ISA serial device and can connect it
to the existing Unix socket:

```text
RetroOS COM1 <-> QEMU isa-serial <-> Unix socket <-> hostfs.py
```

This validates the actual kernel byte protocol and root-mount behavior. It
does not validate RS-232 voltage levels or the USB adapter.

To emulate the three-wire/no-modem-signals requirement, the kernel test must
not use CTS/DSR as an availability condition. Where QEMU happens to report
those bits, add a focused unit or interpreter test that forces the modem
status bits low while allowing TX/RX bytes.

Continue using one QEMU process and one temporary image at a time, consistent
with the project image-work policy.

## Validation matrix

### 1. Successful handshake

- Start the HostFs server before boot.
- Boot QEMU with COM1 connected to it.
- Confirm the kernel reports a successful versioned handshake.
- Confirm ordinary open, read, directory, create, write and close operations.

### 2. HostFs as root

- Boot without an ext disk.
- Confirm the screen reports `hostfs mounted as root`.
- Confirm DOS `C:` resolves into the served Pi/host directory.
- Confirm `C:\BOOT` still contains the embedded tools.
- Start DOS Navigator and read/write a small test file.

### 3. HostFs beside an ext root

- Boot with the existing ext HDD image.
- Confirm ext remains root under `root=auto`.
- Confirm HostFs appears at `C:\HOST`.
- Copy and run a small test utility from the share.

### 4. No server

- Boot with a UART attached but no server.
- Confirm the handshake times out within the documented bound.
- Confirm the kernel does not hang.
- Confirm ext or embedded boot fallback remains usable.

### 5. Wrong or silent peer

- Attach a byte stream that gives no response, a truncated response, incorrect
  magic, and an unsupported version.
- Confirm none are mounted as HostFs.
- Confirm every case terminates within a bounded time.

### 6. No modem-control signals

- Force CTS, DSR, DCD and RI low in an interpreter/focused UART test.
- Permit TX/RX data.
- Confirm the handshake and filesystem operations still succeed.

### 7. Server restart

- For the initial implementation, document that server loss after mount may
  block an active filesystem request.
- Verify that restarting the server before reboot allows the next boot to
  connect.
- Treat live reconnect as follow-up work rather than silently claiming it.

## Pi deployment

Expected Pi-side layout:

```text
/srv/retroos-root/       directory exported through HostFs
/dev/ttyUSB0             HostFs serial adapter
/dev/ttyUSB1             COM2 logging adapter
```

After manual validation, create a systemd service which:

1. starts before the SEJT is reset;
2. waits for `/dev/ttyUSB0`;
3. opens it exclusively at 115200 8N1;
4. serves `/srv/retroos-root`;
5. restarts after a disconnect or process failure;
6. records protocol errors in the journal.

Use stable `/dev/serial/by-id/...` paths rather than assuming USB enumeration
always assigns the same `ttyUSB` numbers.

## Hardware cautions

- Verify the D945GSEJT serial-header pinout before connecting anything.
- Cross TX and RX and join signal ground.
- Do not connect Raspberry Pi GPIO UART pins directly to true RS-232 levels.
- Use two proper USB-to-RS-232 adapters or suitable level converters.
- Do not connect hardware-flow-control pins for the three-wire test.
- A motherboard header-to-DB9 cable must match the board's header pinout;
  these cables are not universally wired.

## Performance expectations

At 115200 baud the theoretical one-direction payload ceiling with 8N1 is
about 11.25 KiB/s. Protocol round trips, directory operations and UART polling
reduce effective throughput.

Appropriate HostFs-root content:

- small test executables;
- configuration files;
- scripts;
- captured diagnostic output;
- recently rebuilt utilities.

Keep games and large assets on a local ext disk. In that normal development
configuration HostFs should mount at `C:\HOST`, while the local disk remains
`C:`. HostFs-root mode is primarily for diskless bring-up, recovery and small
tests.

## Completion criteria

This task is complete when:

1. a three-wire-capable handshake replaces modem-line detection;
2. an absent or invalid server cannot hang boot-time probing;
3. `hostfs.py` serves both QEMU sockets and a physical serial device;
4. QEMU validates HostFs root without an ext disk;
5. QEMU validates `/host` with an ext root;
6. low modem-status bits do not prevent a valid connection;
7. Pi commands, systemd setup, wiring and limitations are documented;
8. existing HostFs filesystem operations and hosted/interpreter backends still
   pass their relevant tests.

## Recommended implementation order

1. Add protocol constants and server-side `HELLO` handling.
2. Establish a timer-backed bounded UART receive primitive.
3. Replace the CTS/DSR probe with drain, request, response and validation.
4. Add focused handshake tests, including modem-status bits forced low.
5. Add `hostfs.py --serial` while retaining socket compatibility.
6. Run the successful and failed QEMU boot matrix.
7. Validate root and `/host` mount policies.
8. Add optional `root=` and `hostfs=` boot arguments if desired.
9. Deploy to the Pi manually, then add its systemd service.
10. Test on the D945GSEJT with COM1 HostFs and COM2 logging.
