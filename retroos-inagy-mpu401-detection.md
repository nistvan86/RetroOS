# Task: make MPU-401 detection deterministic

## Status

Open. Intermittently reproduced with Duke3D in QEMU on 2026-07-27.

One direct Duke3D launch reported:

```text
Could not detect MPU-401.
```

A later setup-launched Duke3D session detected the device and proceeded with
audio. The guest declares MPU-401 port `330h`.

## Goal

Make the emulated MPU-401 at `330h/331h` respond deterministically to standard
DOS reset and UART-mode detection sequences across repeated program launches.

Preserve correct active-low status bits, reset/UART acknowledgements, MIDI
byte queuing, program-exit reset, and absence behavior when `BLASTER` lacks a
`P` token.

## Confirmed environment

The launcher uses:

```text
QEMU intel-hda + hda-duplex
RetroOS platform audio: EmulatedHda
Guest Sound Blaster/MPU devices: RetroOS emulation
BLASTER: A220 I7 D1 H5 P330
```

Relevant implementation:

```text
kernel/src/kernel/dos/machine/vmpu.rs
lib/sound/src/mpu401.rs
```

`Mpu401` currently implements UART mode only:

- command port `331h`;
- data port `330h`;
- reset command `FFh`;
- UART command `3Fh`;
- acknowledgement byte `FEh`;
- active-low input-empty/output-busy status.

`Mpu::configure_from_env()` makes the device present when the newly populated
program environment contains `BLASTER ... P330`. `Mpu::reset()` clears the
wire state and marks the device absent on program exit; the next program load
must configure it again from its environment.

## Scope

This task concerns:

- program environment propagation of `BLASTER P330`;
- I/O ownership and trapping for ports `330h/331h`;
- status and acknowledgement behavior during detection;
- state reset across normal exit and nested `EXEC`;
- differences between direct DN launch and setup-launched Duke3D.

It does not concern:

- VESA or DPMI physical framebuffer mapping;
- VBE palette handling;
- Sound Blaster IRQ7 delivery;
- MIDI synthesis quality or instrument-bank availability after detection.

The IRQ7 crash is tracked separately in
`retroos-inagy-duke3d-irq7.md`.

## Leading hypotheses

### Environment or launch-chain difference

Compare the environment created for:

```text
DN -> DUKE3D.EXE
DN -> SETUP.EXE -> DUKE3D.EXE
```

Confirm that both contain the exact `BLASTER` variable with `P330`, and that
`configure_from_env()` runs before I/O policy is applied. A stale
`present=false` state would make both ports read as an unpopulated bus.

### Reset/ACK queue semantics

The current model increments `ack_pending` for every reset, UART, or unknown
command. Some probes send reset twice or abandon an earlier acknowledgement.
A stale queued `FEh` may change subsequent status reads or make the next
handshake consume an acknowledgement for the wrong command.

Capture the exact Duke sequence before changing this. Determine whether real
UART MPU hardware queues multiple ACKs, replaces a pending reset ACK, or
flushes the input queue on reset.

### Poll timing and active-low status

The status register is active-low:

```text
bit 7 clear: input byte waiting
bit 6 clear: output ready
```

Record every status/data read and command write during the short probe. Check
whether the guest times out because a status transition is missing, reversed,
or delayed by RetroOS scheduling.

### I/O policy transition

`Mpu::owns()` depends on both `present` and the platform audio mode. Verify
that ports `330h/331h` are trapped to the emulated device throughout the
probe, and do not temporarily fall through to the host or floating-bus path
during an `EXEC`, focus switch, or DPMI transition.

## Instrumentation plan

Add a narrow MPU probe trace containing:

```text
program/PSP
BLASTER value and configured base
present/owns state
IN/OUT port and value
UART state
ack_pending before and after each access
timestamp or tick
```

Keep the trace enabled only during detection or behind an MPU-specific
constant. A failed launch and a successful launch must be captured back to
back for comparison.

Also record program-exit reset and the next program's reconfiguration so
state leakage can be ruled in or out.

## Likely files

Primary:

```text
kernel/src/kernel/dos/machine/vmpu.rs
lib/sound/src/mpu401.rs
kernel/src/kernel/dos/machine/mod.rs
```

Environment and lifecycle:

```text
kernel/src/kernel/dos/dos.rs
kernel/src/kernel/dos/mod.rs
kernel/src/kernel/io_policy.rs
```

Tests:

```text
lib/sound/src/mpu401.rs
test/dos/
```

Reproducer:

```text
apps/games/DUKE3D/DUKE3D.CFG
apps/games/DUKE3D/DUKE3D.EXE
run_grub_hdd.sh
retroos-grub-hdd.img
```

## Tests

Existing `Mpu401` unit coverage already verifies a basic reset-then-UART
handshake. Extend it with:

1. Two reset commands with the exact acknowledgement pattern used by Duke.
2. Reset while an acknowledgement is pending.
3. UART command while a reset acknowledgement is pending.
4. Status polling before and after every command/data read.
5. Repeated complete detection cycles separated by `reset()`.
6. Aborted detection followed by a fresh program's detection.
7. Unknown commands not poisoning the next standard handshake.
8. `set_base()` and `owns()` behavior at a non-default port.
9. Missing `BLASTER P` leaving the device absent.
10. Direct launch and nested-`EXEC` environment propagation.

If the observed Duke sequence differs from documented/common MPU behavior,
add it as a named regression test rather than weakening every command rule.

## Manual validation

Start from a fresh snapshot each time:

```bash
./run_grub_hdd.sh
```

Repeat at least 20 times:

1. Launch `DUKE3D.EXE` directly from DOS Navigator's command prompt.
2. Record whether it prints `Could not detect MPU-401`.
3. Exit normally.
4. Launch through `SETUP.EXE`.
5. Record detection and music behavior.

Also test:

- Wolfenstein 3D or another known General MIDI program;
- a small focused MPU reset/UART probe under `test/dos/`;
- repeated probes without rebooting;
- repeated probes from a fresh VM.

Detection success must not depend on program order, prior VBE tests, current
directory, DOS Navigator state, or whether setup launches the game.

## Definition of done

- The failed and successful Duke probe sequences are captured and compared.
- The cause is identified as environment, I/O routing, status semantics, ACK
  lifecycle, or another proven mechanism.
- Direct and setup-launched Duke3D detect MPU-401 on every run.
- Twenty repeated detection cycles pass without rebooting.
- A second General MIDI program detects and plays correctly.
- Program exit leaves no stale UART bytes or acknowledgements.
- Missing `BLASTER P` still presents no MPU device.
- No Sound Blaster IRQ/PIC behavior is changed as part of this task.
