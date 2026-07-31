# SEJT Native VBE Exit Restore Plan

## Problem

Duke runs correctly on the D945GSEJT in native VESA modes, but after the game
exits the monitor retains Duke's final graphics frame with an incorrect
palette. DN continues running and the screen changes, so the machine and DOS
process are alive; physical VGA has not returned to character mode.

Hardware session `520240e8` confirms normal process completion:

```text
[mem] exit tid=2 code=0
exec_return: parent ...
```

The adjacent VGA snapshots show the child entering extended graphics state
and the parent state being restored in software, but the visible card remains
in the VBE scanout mode.

## Current diagnosis

The native VGA ownership handoff saves and restores standard VGA registers,
planes, and DAC state. A native VBE BIOS mode also programs Cirrus extended
SVGA registers that are outside that snapshot. Restoring only standard VGA
state can therefore restore the parent's palette without disabling the VBE
timing/framebuffer, producing the observed stale frame and wrong colours.

## Work plan

1. Track whether the foreground DOS owner entered a native BIOS VBE mode.
2. On native-VGA owner teardown, call the VGA BIOS outside DPMI to set mode
   03h before restoring the suspended parent's standard VGA/text snapshot.
3. Perform the BIOS transition while ring-0 RLOG remains available; log entry,
   BIOS result, and the final hardware mode/readback.
4. Do not apply the BIOS reset to emulated/substitute VBE or GOP-only paths.
5. Audit DAC mask/index and Attribute Controller flip-flop state after the BIOS
   call so palette restoration does not introduce a second artifact.
6. Test clean exit from several VBE modes, Mode 13h, Mode X, and ordinary text
   programs, including F12 OSD suspend/resume.

## Acceptance criteria

- Exiting Duke from every supported tested VESA mode visibly returns to DN's
  80x25 character display with the correct palette.
- RLOG confirms a successful mode-03h BIOS transition and normal child exit.
- No screen corruption or hang occurs when the game exits abnormally.
- Non-VBE VGA programs and QEMU substitute-VBE behavior remain unchanged.

