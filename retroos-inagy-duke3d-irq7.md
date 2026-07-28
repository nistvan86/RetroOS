# Task: fix Duke3D Sound Blaster IRQ7 transfer-stack overflow

## Status

Open. Reproduced repeatedly in QEMU on 2026-07-27.

This is independent of the VESA LFB mapper and VBE palette fix. Those changes
allow Duke3D to run correctly in high-resolution VESA modes, but the same
audio interrupt failure was present in an earlier kernel session.

## Goal

Make Duke3D run for an extended period with Sound Blaster digital audio
enabled without DOS/4GW exhausting its interrupt transfer stack.

Preserve correct Sound Blaster DMA progress, block-completion timing, IRQ
acknowledgement, PIC priority, and audio output. Do not fix the crash merely
by dropping legitimate interrupts or lowering audio functionality.

## Confirmed failure

QEMU configuration:

```text
RetroOS platform audio: EmulatedHda
Guest Sound Blaster: emulated
BLASTER: A220 I7 D1 H5 P330
Duke3D FX: Sound Blaster, IRQ 7
```

After Duke3D runs for a while, DOS/4GW terminates it:

```text
DOS/4GW Professional error (2002): transfer stack overflow on interrupt 0Fh at 87:000074C8
TSF32: prev_tsf32 4C74
SS        2F DS        8F ES        2F FS         0 GS         0
EAX     6B0C EBX   85004F ECX        0 EDX  27F6800
ESI     6882 EDI      F7A EBP      F40 ESP     2E3E
CS:IP   87:000074C8 ID 0F COD        0 FLG     1006
```

With the BIOS/PIC vector base used by DOS, interrupt `0Fh` is IRQ7. That
matches Duke3D's configured Sound Blaster interrupt. RetroOS then reports:

```text
[mem] exit tid=2 code=210
```

The exact transfer-stack overflow also occurred before the VBE `4F09h`
palette interception. It is therefore not introduced by physical
framebuffer mapping or palette conversion.

### Non-VESA control

The same failure was subsequently reproduced quickly in Duke3D's normal
320x200 non-VESA mode:

```text
DOS/4GW Professional error (2002): transfer stack overflow on interrupt 0Fh at 87:000074C8
TSF32: prev_tsf32 4DD4
SS        2F DS        8F ES        2F FS         0 GS         0
EAX     6844 EBX      13F ECX        0 EDX   7FFF00
ESI     677A EDI     67D2 EBP     6798 ESP     2E46
CS:IP   87:000074C8 ID 0F COD        0 FLG     1002
[mem] exit tid=2 code=210
```

The failing `CS:IP`, interrupt number, DOS/4GW error, and process exit code
match the VESA failures. This experimentally rules out VBE mode setting,
palette handling, LFB mapping, framebuffer size, and high-resolution drawing
as necessary causes. The reproducer can therefore use 320x200 for faster
iterations.

After the process had exited, RetroOS printed:

```text
[gus] ULTRASND base=240 irq=5 dma=3
[mpu] MPU-401 at 330
```

Those lines reflect the restored/reconfigured DOS environment after the
crashed child. They do not identify GUS IRQ5 or MPU-401 as the interrupt that
overflowed: the reported vector remains `0Fh`, corresponding to the Sound
Blaster's IRQ7.

## Scope

This task concerns:

- the emulated Sound Blaster DSP completion clock;
- virtual DMA block boundaries;
- virtual PIC IRQ7 request, in-service, and EOI behavior;
- DPMI delivery and nesting of hardware IRQs;
- DOS/4GW's observable interrupt cadence.

It does not concern:

- DPMI `0800h/0801h` physical mappings;
- VESA mode setting or framebuffer writes;
- VBE `4F09h` palette data;
- MPU-401 detection or MIDI data;
- host HDA MSI frequency except where it influences the guest audio clock.

The MPU-401 issue is tracked separately in
`retroos-inagy-mpu401-detection.md`.

## Leading hypotheses

### Same-line IRQ relatching

Sound Blaster completion paths currently suppress a raise only while the
line remains in the PIC IRR:

```rust
if !vpic.is_requested(self.irq) {
    vpic.raise(self.irq);
}
```

`VirtualPic::is_requested()` checks IRR but not ISR. After delivery, IRQ7 is
removed from IRR and placed in ISR. A new completion can therefore relatch
IRQ7 while the previous IRQ7 handler is still in service.

That is not automatically incorrect for an edge-triggered PIC. However, if
Duke sends EOI before fully unwinding its DOS/4GW transfer frame, an already
latched next edge can be delivered immediately and nest another transfer
frame. A sustained cadence could produce the observed overflow.

Do not change this behavior blindly: real hardware may legitimately latch a
new edge during an in-service interval. First prove whether RetroOS is
generating too many completions, delivering them too aggressively after EOI,
or mishandling the DPMI continuation stack.

### Completion cadence or duplicate sources

IRQ7 may be raised from several emulated-SB paths:

- DSP drain-clock block completion;
- explicit DSP `F2h/F3h` trigger IRQ;
- short probe transfer completion.

Verify that a normal Duke playback session has exactly one active production
source and one interrupt per programmed block. Check for duplicate terminal
count events, stale triggers, or a completion being regenerated before the
guest acknowledges the DSP.

### DSP acknowledgement mismatch

Duke acknowledges an 8-bit DSP interrupt by reading the appropriate DSP
status/ack port. Verify that this clears the emulated DSP's interrupt cause,
not merely the PIC ISR. If the cause remains asserted or is regenerated from
an unchanged cursor, each EOI could immediately produce another edge.

### DPMI interrupt-frame lifecycle

If the guest interrupt cadence is valid, inspect whether RetroOS fails to
fully pop a DPMI host continuation or DOS/4GW transfer frame on an IRQ7
return. Record nesting depth at delivery, EOI, and continuation return.

## Instrumentation plan

Add focused, rate-limited diagnostics rather than enabling every port trace.
For each IRQ7 event, record:

```text
sequence
timestamp / mixer frame
source: dsp-clock | F2/F3 | probe
DSP block/cursor state
PIC IRR/ISR/IMR before raise
PIC IRR/ISR/IMR at delivery
DPMI continuation depth
EOI and DSP-ack observations
```

Print immediately on suspicious conditions:

- IRQ7 raised while IRQ7 is already in service;
- more than one IRQ for the same DSP block boundary;
- EOI without a corresponding DSP acknowledgement;
- continuation depth increasing across completed IRQ handlers;
- IRQ cadence substantially higher than the programmed block rate.

Keep diagnostics behind a narrow constant or feature so normal audio logs do
not become unusable.

## Likely files

Primary:

```text
kernel/src/kernel/dos/machine/vsb.rs
kernel/src/kernel/dos/machine/vpic.rs
kernel/src/kernel/dos/machine/mod.rs
lib/sound/src/sb.rs
```

Possible DPMI supporting investigation:

```text
kernel/src/kernel/dos/mode_transitions.rs
kernel/src/kernel/dos/dpmi/
kernel/src/kernel/dos/dos.rs
```

Configuration and reproducer:

```text
apps/games/DUKE3D/DUKE3D.CFG
apps/games/DUKE3D/DUKE3D.EXE
run_grub_hdd.sh
retroos-grub-hdd.img
```

## Tests

Add focused automated coverage for:

1. An IRQ request moving from IRR to ISR at delivery.
2. A second same-line edge arriving while the line is in service.
3. EOI making a legitimately latched second edge deliverable.
4. One DSP block boundary producing exactly one interrupt cause.
5. DSP acknowledgement clearing the correct 8-/16-bit cause.
6. Auto-init playback across many blocks without duplicate completions.
7. Probe-trigger IRQs not leaking into normal playback.
8. DPMI IRQ delivery/return restoring continuation depth.
9. Long synthetic playback with bounded IRQ nesting.

The PIC tests must preserve correct 8259 edge-trigger behavior rather than
encoding a Duke-specific suppression rule.

## Manual validation

Use the known launcher and existing single image:

```bash
./run_grub_hdd.sh
```

In Duke setup, retain the current Sound Blaster wiring:

```text
address 220h
IRQ 7
DMA 1
16-bit DMA 5
```

Test matrix:

1. Duke3D 320x200 with digital effects and music enabled.
2. Duke3D 320x200 with music disabled but digital effects enabled.
3. Duke3D 320x200 with digital effects disabled but music enabled.
4. Duke3D 320x200 with both digital effects and music disabled.
5. Duke3D VESA 640x480 with the same audio controls.
6. Duke3D VESA 800x600 with the same audio controls.
7. A second Sound Blaster game or the in-tree SB diagnostic.

Run Duke substantially longer than the previous time-to-failure and exercise
frequent sound effects. Capture the debug stream throughout.

The 320x200 failure has already isolated the crash from VESA. The
digital-effects-disabled case is now the critical control: if it runs
indefinitely while digital-effects-enabled 320x200 fails, the emulated
Sound Blaster IRQ7 path is isolated from both VESA and MIDI.

## Definition of done

- The exact source and lifecycle of the repeated IRQ7 events are proven.
- Duke3D no longer reports DOS/4GW error 2002 on interrupt `0Fh`.
- IRQ7 delivery remains faithful to PIC edge semantics.
- Sound Blaster playback and DMA progress remain correct.
- DSP acknowledgement prevents duplicate delivery of one completion.
- DPMI continuation depth returns to baseline after every IRQ.
- Duke3D runs for at least 30 minutes in both 640x480 and 800x600 with
  digital effects enabled.
- At least one other Sound Blaster program still detects and plays correctly.
- MPU-401 behavior is not changed as part of this task.
