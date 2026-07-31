# SEJT HDA Audio DMA Plan

## Problem

The Intel D945GSEJT detects its ICH7M HDA controller and ALC662 codec, but no
audio reaches either the rear or front-panel analog jack when booting the
Duke-derived PXE kernel.

Hardware session `520240e8` reports:

```text
Platform: host=Metal display=VgaCard firmware=NativeBios audio=EmulatedHda
hda: 00:1b.0 INTx line 11
hda: 00:1b.0 failed: no DMA buffer
```

## Current diagnosis

The HDA driver borrows the permanent 128 KiB ISA DMA channel-5 buffer through
`Arch::dma_channel_buf(5)`. The Duke-derived kernel occupies physical memory
from 1 MiB to nearly 16 MiB, leaving no suitable contiguous region in the
allocator's below-16-MiB ISA reserve. HDA is a PCI bus-master device and does
not need this ISA addressing restriction.

## Work plan

1. Add an architecture interface for allocating a permanent, physically
   contiguous general-purpose DMA region, backed by
   `arch-metal::phys_mm::alloc_contig` on metal.
2. Change HDA to request `DMA_PAGES` from that general pool instead of channel
   5's ISA buffer.
3. Preserve the existing 32-bit/64-bit controller address capability checks;
   fail clearly if an allocated address is outside what the controller can
   DMA to.
4. Keep SB16 and real ISA DMA users on the below-16-MiB pools.
5. Verify codec routing and amplifier setup independently for the rear and
   front-panel ALC662 pins after DMA starts.

## Acceptance criteria

- SEJT RLOG reaches successful HDA codec and output-stream initialization with
  no `no DMA buffer` message.
- PCM audio is audible from the rear jack.
- Front-panel behavior is tested and its pin/jack-detect result documented.
- The ordinary and Duke-derived kernels still boot in QEMU and on the SEJT.
- SB16/GUS emulation and ISA DMA regressions remain passing.

