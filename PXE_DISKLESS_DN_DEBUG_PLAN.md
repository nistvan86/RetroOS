# PXE Diskless DN Debug Plan

## Current situation

Target hardware: Intel D945GSEJT, booting RetroOS through BIOS PXE and GRUB.

The PXE GRUB entry is:

```grub
menuentry "RetroOS (native VGA / BIOS)" {
    terminal_output console
    insmod multiboot

    multiboot /boot/retroos/kernel.elf
    boot
}
```

Observed behavior:

```text
Starting DN...
DN exited, restarting...
```

This repeats indefinitely.

The same machine boots from the generated RetroOS HDD image on a PATA disk,
and DN launches successfully there. The PXE test has no physical disk and no
ext partition.

## Facts already verified

- The project-root `retroos-kernel.elf` is byte-for-byte identical to
  `bazel-bin/kernel/kernel.elf`.
- Verified size: 2,096,004 bytes.
- Verified SHA-256:
  `bdc315955c758143f0b4e78eaf906e704b97027cc667267f6acd1362b10e0262`.
- This is the full kernel, not `kernel_bare.elf`.
- The kernel embeds `bootfs_tar.tar`.
- The embedded boot filesystem contains:
  - `DN/DN.COM`
  - `DN/DN.PRG`
  - the other DN support files
  - `COMMAND.COM`
  - fallback `CONFIG.SYS`
- Startup mounts the embedded filesystem at `C:\BOOT` and launches
  `C:\BOOT\DN\DN.COM`.
- If `DN.COM` were absent, the kernel would panic with a not-found error.
  Therefore the restart loop means that DN was loaded, but its process then
  exited or was terminated.
- The successful PATA boot substantially rules out general incompatibility
  with the board's CPU, VM86/VME implementation, VGA hardware, or DN itself.
- The GRUB entry is suitably minimal and does not explicitly force a graphics
  framebuffer.

## QEMU experiment and conclusion

The diskless path and the COM1 hypothesis were tested with QEMU loading the
same `kernel.elf` directly, with no disk attached.

### No serial port

QEMU option:

```text
-serial none
```

Observed result:

```text
Storage: none detected
Platform: ... hostfs=false ...
TAR: indexed 21 entries from embedded bootfs
Starting DN...
Dos Navigator Version 1.51 ...
```

DN starts successfully. This proves that the current embedded bootfs and RAM
overlay can support a generic diskless boot, at least on the QEMU hardware
path.

### Null serial backend

QEMU option:

```text
-serial null
```

Observed result:

```text
Storage: none detected
Platform: ... hostfs=true ...
hostfs mounted as root
TAR: indexed 21 entries from embedded bootfs
```

Boot then stalls before `Starting DN...`. QEMU's null serial backend makes
the UART modem-status test pass, but it does not answer the HostFS protocol.

### Interpretation

The QEMU experiment proves that CTS/DSR alone can falsely detect HostFS and
that a false detection is especially harmful without an ext4 root.

It does **not** reproduce the board's exact symptom. QEMU stalls before DN,
whereas the D945GSEJT starts DN and then repeatedly reports:

```text
DN exited, restarting...
```

Therefore false HostFS detection is a confirmed independent bug and is worth
excluding, but it is no longer the leading explanation for the restart loop.
It remains relevant only if the physical boot reports `hostfs=true` or
`hostfs mounted as root`. A physical serial peer returning garbage or echoed
bytes could fail differently from QEMU's silent null backend, but this is a
narrower possibility.

## Confirmed HostFS detection weakness

On metal, RetroOS probes COM1 and treats HostFS as connected when either CTS
or DSR is asserted:

```rust
let msr = inb(COM1 + 6);
if msr & 0x30 == 0 {
    return false;
}
true
```

See `kernel/src/kernel/fs/hostfs.rs`.

An unconnected UART, floating modem-status inputs, a serial cable, or an
attached serial adapter could make this test return true even though no
HostFS protocol server is present.

The consequence depends on whether ext4 exists:

```text
PATA boot:
  ext4 filesystem        -> root
  embedded bootfs        -> C:\BOOT
  falsely detected HostFS -> /host

Diskless PXE boot:
  falsely detected HostFS -> root
  embedded bootfs         -> C:\BOOT
```

This explains why false detection matters much more during diskless PXE boot,
but the QEMU result shows that a silent false-positive peer normally blocks
before DN rather than causing the observed restart loop.

## Current leading hypotheses

### Hidden DN termination or child `EXEC` failure

The current startup loop discards DN's termination status. `DN.COM` is loaded,
but it may then fail while loading `DN.PRG` or another support component. An
ordinary DOS exit and an unhandled exception both collapse into the same
visible restart message.

Capturing the exit status is the highest-value next step.

### Real-hardware diskless filesystem edge case

The embedded TAR is read-only. RetroOS's VFS is intended to satisfy creates
and scratch writes using its RAM-backed overlay, including files created
under the embedded boot filesystem.

DN may nevertheless expose an untested diskless edge case involving:

- creation of DN swap or temporary files;
- creation in the drive root rather than under `C:\BOOT`;
- root-directory enumeration when only the `boot` mount exists;
- opening an existing TAR file for write/truncate;
- directory existence or free-space queries without an ext4 root.

Because diskless DN works in QEMU, any such issue is likely conditional on a
real-hardware path, platform result, timing difference, or a different DOS
call sequence—not a universal absence of writable storage.

### DOS/VM86/DPMI exception

The successful PATA boot makes a general CPU or DN incompatibility unlikely.
However, a different diskless startup path could still reach a fault that the
ext4-backed path does not. The missing termination status currently prevents
distinguishing a normal loader exit from `#GP`, `#PF`, or another exception.

Relevant code:

- `kernel/src/kernel/startup.rs`
- `kernel/src/kernel/vfs.rs`
- `kernel/src/kernel/fs/tarfs.rs`
- `kernel/src/kernel/dos/dfs.rs`

## Next physical tests

Perform these quick checks before building a diagnostic kernel:

1. Record or photograph the complete boot output, especially:

   ```text
   Platform: ...
   Storage: ...
   TAR: indexed ...
   Starting DN...
   ```

2. Check whether the output contains either:

   ```text
   hostfs=true
   hostfs mounted as root
   ```

3. If HostFS is reported, disconnect everything from COM1, including
   USB-to-serial adapters and serial cables, then repeat the PXE boot.

4. If HostFS remains reported, disable COM1 or all legacy serial ports
   temporarily in BIOS and repeat the PXE boot.

5. Confirm whether the restart loop is immediate or delayed:

   - immediate repetition suggests a loader, filesystem, or child `EXEC`
     failure;
   - delayed termination suggests an interrupt, exception, or runtime fault.

## Diagnostic kernel changes

This is now the primary next task. Build a temporary diagnostic kernel
containing all of the following changes so one physical boot can distinguish
the likely causes:

1. **Implemented:** `run_program()`/`event_loop()` now return and preserve the
   initial process's termination status.

2. **Implemented:** DN's termination status and runtime are printed on the
   visible console after every exit.

3. **Implemented with a loop guard:** three consecutive DN runs shorter than
   five seconds stop further restarts and leave the status visible. Longer
   runs reset the short-exit counter.

4. Surface unhandled DOS/DPMI exceptions on the visible screen, including at
   least:

   - exception number;
   - exit status;
   - CS:EIP;
   - SS:ESP;
   - CR2 for page faults.

5. Print the HostFS probe result on the visible boot console:

   ```text
   HostFS: detected
   ```

   or:

   ```text
   HostFS: disabled/not detected
   ```

6. Add a build-time or boot-time option that forcibly disables COM1 HostFS,
   allowing it to be excluded without relying on BIOS settings.

7. Optionally trace the DOS operations most likely to explain a loader exit:

   - `EXEC` paths and return codes;
   - failed opens for files beneath `C:\BOOT\DN`;
   - create/truncate failures;
   - current-directory and drive queries;
   - free-space queries.

Diagnostic artifact built on 2026-07-31:

```text
retroos-kernel.elf
size: 2,100,164 bytes
SHA-256: c295ee4d8eedc53b9cb1b567531003f2d2d59a7a4db91af21cb4506511605ab5
```

Useful status interpretations:

- `0x020D`: unhandled general-protection fault (`#GP`, exception 13).
- `0x020E`: unhandled page fault (`#PF`, exception 14).
- An ordinary low-byte exit code likely means DN or its loader deliberately
  terminated, possibly after a failed support-file or child-program load.

## Controlled comparison builds

After the termination-status build works, prepare two PXE kernels from the
same commit:

1. Diagnostic build with HostFS forcibly disabled (test this first).
2. Diagnostic build with normal HostFS probing.

Publish them under separate TFTP names rather than overwriting the known-good
file:

```text
/boot/retroos/kernel-hostfs.elf
/boot/retroos/kernel-no-hostfs.elf
```

Add separate GRUB entries so the comparison does not require republishing
files between boots.

## Further local tests

Already completed:

- Diskless QEMU with `-serial none`: DN starts.
- Diskless QEMU with `-serial null`: false `hostfs=true`, then a pre-DN stall.

Still worth doing:

1. Add a regression test for diskless boot with false-positive COM1 status.
2. Make HostFS probing use a bounded protocol handshake so the test cannot
   hang indefinitely.
3. Trace DN filesystem and `EXEC` calls in a diskless QEMU run and compare
   them with an ext4-backed run.
4. If practical, emulate a serial peer that asserts CTS/DSR and returns
   malformed or echoed bytes, to see whether that produces a fast failure
   instead of the null backend's stall.

## Likely permanent fixes

Regardless of whether HostFS causes the board's DN loop, its probe should be
fixed:

- Do not infer protocol availability solely from UART modem-status pins.
- Require an explicit HostFS handshake with a magic value and bounded timeout.
- Prefer an explicit boot option to enable serial HostFS on real hardware.
- Never mount an unverified serial HostFS as the diskless root.

If termination tracing identifies a RAM-overlay/root failure:

- Make the diskless C: root an explicit writable in-memory filesystem.
- Mount the embedded bootfs beneath that writable root at `C:\BOOT`.
- Add coverage for DN swap-file creation, root enumeration, and child `EXEC`
  with no disk present.

## Documentation note

Some project notes refer to `kernel_bios_text.elf`. That is currently a stale
build artifact; its separate Bazel target is no longer present. The current
`kernel.elf` incorporates the BIOS text/native VGA multiboot behavior and is
the correct artifact for further testing.
