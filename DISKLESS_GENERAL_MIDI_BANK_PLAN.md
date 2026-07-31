# Diskless General MIDI Bank Plan

## Problem

General MIDI is silent in the self-contained Duke PXE kernel. Hardware session
`520240e8` reports:

```text
midi: no ULTRASND under the C: root — General MIDI silent
```

## Current diagnosis

The software General MIDI synth burns its instrument ROM once during startup
from `C:\ULTRASND\MIDI\*.PAT`. The full ext4/disk images include
`//:ultrasnd_tar` under the DOS C: root, but the Duke-specific embedded bootfs
currently includes Duke and the small standard bootfs only. Copying patches
into the volatile C: drive after startup is too late for the current one-shot
loader.

This is a Gravis `.PAT` instrument bank, not a SoundFont (`.SF2`). The same
bank is shared by the software MPU-401/General MIDI synth and GUS-oriented
guest configuration.

## Work plan

1. Include `ULTRASND/MIDI` and its redistribution notice in the Duke-derived
   embedded bootfs, while keeping the ordinary small PXE kernel unchanged.
2. Ensure the path is visible as `C:\ULTRASND\MIDI` at the point
   `midi_bank::load_from_c_root` runs.
3. Confirm that `CONFIG.SYS`/the master environment exposes the intended
   `BLASTER` MPU port and `ULTRADIR` values to Duke.
4. Log loaded instrument count and pool size, and retain the missing-program
   report for incomplete banks.
5. Longer term, consider loading optional large content as a Multiboot module
   rather than growing the linked kernel ELF.

## Acceptance criteria

- Boot RLOG reports a non-zero `midi: GM bank ROM` instrument count.
- Duke detects/configures General MIDI through the emulated MPU-401 port.
- Music is audible through the selected host audio sink on the SEJT.
- Missing-patch diagnostics contain no unexpected melodic holes.
- Licensing/redistribution notice remains packaged with the patches.

