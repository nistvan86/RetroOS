# Task: implement DPMI physical mappings for VESA LFB

## Status

Implemented and validated in QEMU on 2026-07-27.

The BIOS-text kernel now maps externally owned physical device pages into a
dedicated downward-growing client window at `0xA0000000..0xB0000000`. It
preserves unaligned physical offsets, rejects zero/overflowing/exhausted
ranges, tracks up to 32 mappings per DPMI client, implements exact-address
`0801h` unmapping, and releases live mappings on client exit and nested
`EXEC` state replacement. Mappings use named cache-disabled and foreign-page
flags so framebuffer pages are never returned to the RAM allocator.

The allocator cursor is shared across nested DOS `EXEC` clients because a
suspended parent's mappings remain present while its child runs; ownership
and cleanup remain per-client.

## Goal

Implement DPMI `INT 31h AX=0800h` and `AX=0801h` so protected-mode DOS
programs can map a physical VESA linear framebuffer into RetroOS user space.

The returned linear address must be below RetroOS's `0xC0000000` user/kernel
boundary. Do not return a high physical address unchanged.

## Confirmed failure

SeaBIOS/QEMU standard VGA reports the following for VBE mode `0x111`
(640x480x16):

```text
attributes=000000BB
pitch=00000500
phys_base=FD000000
map_size=00096000
```

The current DPMI implementation reports success but returns the physical
address unchanged:

```text
map_cf=00000000
map_linear=FD000000
```

`VBELFB DRAW` selects mode `0x4111` successfully and faults on its first
framebuffer write:

```text
fault rip=0x501303 addr=0xfd000000 err=0x6
SEGV in thread 2 at 0xfd000000
[mem] exit tid=2 code=-11
```

Page-fault error `0x6` is a user-mode write to a non-present page.

Banked VESA is separately verified working at 640x480 in 8/15/16/24/32-bit
and at 800x600x8. This task concerns only protected-mode physical/LFB
mappings.

## Scope and UniVBE

UniVBE/SciTech Display Doctor operates at the VBE-provider layer: it can
replace or improve a card's video BIOS interface, add modes, and work around
card-specific VBE defects. It does not replace the DPMI host and cannot make
a high physical LFB address accessible to a protected-mode client by itself.

Consequently, UniVBE is neither a fix nor a prerequisite for this task.
`AX=0800h` and `AX=0801h` must work regardless of whether `PhysBasePtr` came
from SeaBIOS, a motherboard/video-card BIOS, or UniVBE. The implementation
must use the address and mapping size supplied at runtime and must not contain
QEMU-, GMA 950-, or fixed-address special cases.

Testing UniVBE is deferred until after the native VBE plus DPMI LFB path works
in QEMU and has been tested on the D945GSEJT. If the real video BIOS then has
missing or defective VBE modes, UniVBE can be evaluated as an optional
compatibility workaround. Any direct-I/O, interrupt, or hardware-probing
failures encountered while loading UniVBE should be tracked separately from
this physical-mapping task.

## Current incorrect implementation

The handlers in `kernel/src/kernel/dos/dpmi/mod.rs` currently treat physical
and linear addresses as identical:

```rust
0x0800 => {
    clear_carry(regs);
}
0x0801 => {
    clear_carry(regs);
}
```

RetroOS already has the required architecture primitive:

```rust
machine.map_phys_range(vpage_start, num_pages, ppage_start, flags);
machine.unmap_range(vpage_start, num_pages);
```

No paging redesign should be necessary.

## Required behavior

### AX=0800h — Map Physical Address

Input:

```text
BX:CX = physical address
SI:DI = mapping size in bytes
```

Required steps:

1. Reject a zero size.
2. Align the physical address down to 4096 bytes.
3. Preserve the original offset within the first physical page.
4. Include that offset when calculating the number of pages.
5. Allocate a page-aligned, non-overlapping linear range below
   `0xC0000000`.
6. Ensure the range cannot collide with:
   - DPMI `0501h` allocations;
   - the DOS/XMS area;
   - the synthetic SVGA framebuffer;
   - process image, stack, or other fixed user mappings.
7. Map the physical pages into the selected linear range.
8. Record the mapping in the current process's `DpmiState`.
9. Return `linear_page_base + original_physical_offset` in `BX:CX`.
10. Clear carry only after the complete mapping succeeds.

On failure:

1. Do not leave a partial mapping.
2. Set carry.
3. Return a suitable DPMI error code in `AX`.

### AX=0801h — Free Physical Address Mapping

Input:

```text
BX:CX = linear address previously returned by AX=0800h
```

Required steps:

1. Find the exact tracked mapping, including an unaligned returned address.
2. Reject unknown or already-freed addresses.
3. Unmap every virtual page in the mapping.
4. Remove its bookkeeping entry.
5. Clear carry on success; set carry and return an error on failure.

Mappings must also disappear safely when the process exits.

## Bookkeeping

Add a bounded per-process table to
`kernel/src/kernel/dos/dpmi/state.rs`. A suggested entry is:

```rust
#[derive(Clone, Copy)]
struct PhysicalMapping {
    returned_linear: u32,
    virtual_page_base: u32,
    physical_page_base: u32,
    page_count: u32,
}
```

The exact representation may differ, but it must support:

- lookup by the address returned from `0800h`;
- complete unmapping for `0801h`;
- collision checks;
- deterministic slot-exhaustion failure.

Start with a small fixed capacity, such as 16 or 32 mappings per DPMI client.

## Linear-address allocation

Prefer a dedicated downward-growing physical-mapping window below 3 GiB over
blindly sharing the upward-growing `dpmi.mem_next` bump pointer.

Whichever allocator is chosen must explicitly check both ends of every range
and reject arithmetic overflow. It must not silently cross `0xC0000000`.

A minimal first version may allocate monotonically and not reuse holes until
process exit, provided `0801h` still unmaps pages and removes the active
mapping. Hole reuse can be added later.

## Page ownership and cache policy

These pages belong to a device, not RetroOS's physical-page allocator.
Address-space teardown and `unmap_range` must never free the framebuffer's
physical pages.

For the first correctness implementation, use an uncached/device mapping
consistent with existing hardware BAR mappings. This is slower but safe.

After correctness is established, consider write-combining:

- use the PAT write-combining entry when available;
- retain an uncached fallback;
- be aware that QEMU TCG display dirty tracking may require strong-uncached
  mappings even when real hardware benefits from WC.

If new mapping flags are needed across the architecture boundary, define
named flags in `arch-abi` rather than duplicating raw PTE bits in another
driver.

## Likely files

Primary:

```text
kernel/src/kernel/dos/dpmi/state.rs
kernel/src/kernel/dos/dpmi/mod.rs
```

Possible supporting changes:

```text
arch-abi/src/arch.rs
arch-metal/src/paging2.rs
arch-interp/src/
```

Diagnostic already added:

```text
test/dos/vbelfb/
```

## Tests

Add automated unit or focused integration coverage for:

1. Page-aligned physical address and page-aligned size.
2. Unaligned physical address.
3. Size crossing a page boundary.
4. Zero size.
5. Arithmetic overflow.
6. Mapping that would cross `0xC0000000`.
7. Multiple simultaneous mappings.
8. Mapping-table exhaustion.
9. Successful `0801h`.
10. Unknown address passed to `0801h`.
11. Double unmap.
12. Process exit with live mappings.

## Manual validation

Boot the GRUB HDD image with the isolated hostfs directory:

```bash
install -d -m 700 /tmp/retroos-vbe-results
RETROOS_HOSTFS_DIR=/tmp/retroos-vbe-results ./run_grub_hdd.sh
```

Run the non-destructive stage:

```dos
C:\HOME\RETROOS\TESTS\VBELFB INFO
```

Expected after the fix:

```text
phys_base=FD000000
map_cf=00000000
map_linear=<valid address below C0000000>
```

The returned address must not be `FD000000`.

Then run:

```dos
C:\HOME\RETROOS\TESTS\VBELFB DRAW
```

Expected:

- VBE mode `0x4111` is selected;
- a correct 640x480x16 gradient appears;
- no page fault occurs;
- pressing a key restores text mode;
- the process exits with code 0.

Finally retest games that previously crashed in 640x480 or higher VESA modes.

Optional follow-up, after native VBE validation:

- record the D945GSEJT video BIOS's supported modes and LFB addresses;
- test UniVBE only if native VBE support is incomplete or faulty;
- verify that the same generic DPMI mapper works with any changed
  `PhysBasePtr`;
- document UniVBE-specific RetroOS compatibility failures as separate tasks.

## Definition of done

- [x] `0800h` returns a usable user-space mapping rather than the physical BAR.
- [x] `0801h` unmaps and validates mappings in the implementation.
- [x] Foreign physical pages are never freed as normal RAM.
- [x] Failure paths do not leave partial mappings.
- [x] Existing banked VGA/VESA behavior remains unchanged by the demand-driven
  mapper.
- [x] `VBELFB INFO` returns a linear address below 3 GiB.
- [x] `VBELFB DRAW` renders and restores text mode without a fault.
- At least one previously crashing protected-mode VESA game runs in its
  high-resolution mode.

### QEMU validation result

SeaBIOS continued to report physical LFB address `0xFD000000`. `VBELFB INFO`
returned:

```text
map_size=00096000
map_cf=00000000
map_ax=00000800
map_linear=AFF6A000
INFO complete; framebuffer not touched
[mem] exit tid=2 code=0
```

`VBELFB DRAW` then selected mode `0x4111`, wrote through `0xAFF6A000`, showed
the expected gradient, restored DOS text mode, and exited with code 0. There
was no page fault. This proves that the new user mapping aliases QEMU's
physical framebuffer correctly.

Direct runtime exercise of successful and invalid `0801h` calls, plus a
previously failing high-resolution game, remain useful final regression
tests.
