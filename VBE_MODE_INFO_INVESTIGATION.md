# SEJT VBE `4F01h` Investigation

## Objective

Determine why the Intel D945GSEJT VGA BIOS reports success for VBE
`INT 10h AX=4F01h` mode-information queries while returning an empty mode
information block to a protected-mode DOS client.

## Test environment

- Board: Intel D945GSEJT
- VGA BIOS access: DPMI simulated real-mode interrupt (`INT 31h AX=0300h`)
- Diagnostic: `C:\BOOT\TESTS\VBELFB.EXE`
- Report file: `C:\VBELFB.TXT`
- Kernel branch: `work/pxe-undi-rlog`
- Baseline fix commit: `ed277be` (`fix(metal): preserve diagnostics on diskless boots`)
- Expanded diagnostic deployment SHA-256:
  `3327988907c64bca9d50b54ed1a57f358aa67a170e3b8d667cbb7b92692345d1`
- RLOG session: `6e7f2a6c`
- Capture time: 2026-07-31 16:13 Europe/Budapest

The diskless kernel mounts a writable RAM overlay as `C:\`, mounts the
embedded TAR at `C:\BOOT`, and includes both `VBELFB.EXE` and its required
`RLOADER.BIN`.

## Relevant code

- Diagnostic: `test/dos/vbelfb/src/main.rs`
- DPMI runtime and real-mode call wrapper: `tools/dosrt/src/lib.rs`
- DPMI real-mode call implementation: `kernel/src/kernel/dos/dpmi/rm_calls.rs`
- Mode-transition machinery: `kernel/src/kernel/dos/mode_transitions.rs`
- RetroOS synthetic VBE implementation: `kernel/src/kernel/dos/bios.rs`

The controller data below does not match the synthetic implementation. It is
therefore evidence from the retained local VGA BIOS rather than RetroOS's
synthetic VBE table.

## Reproduction

Boot the RLOG-enabled kernel on the SEJT, wait for DN, then run:

```dos
C:\BOOT\TESTS\VBELFB.EXE
```

The program runs in `INFO` mode, creates `C:\VBELFB.TXT`, queries VBE
controller and mode information, and does not touch the framebuffer.

## Pre-fix controller information (`AX=4F00h`)

The call succeeds:

```text
controller_ax=0000004F
controller_signature=41534556
controller_version=00000300
controller_caps=00000001
controller_modes_ptr=17540040
controller_memory_64k=00000FFF
```

`VideoModePtr` resolves to `1754:0040`, linear address `0x17580`. The first
64 bytes of the mode list are:

```text
60016101620163016401650166016701
680169016A016B016C016D016E016F01
700171013C014D015C013A014B015A01
07011A011B0105011701180112011401
```

Decoded entries captured before the 32-entry diagnostic limit:

```text
0160 0161 0162 0163 0164 0165 0166 0167
0168 0169 016A 016B 016C 016D 016E 016F
0170 0171 013C 014D 015C 013A 014B 015A
0107 011A 011B 0105 0117 0118 0112 0114
```

The list continues beyond the captured 32 entries. In particular, modes
`0105h` and `0112h` are explicitly advertised.

## Mode information (`AX=4F01h`)

Before every query the diagnostic fills its 512-byte conventional-memory
buffer with `0xCC`. It passes `ES:DI=1754:0000`.

Unsupported examples behave coherently:

```text
query_mode=00000100
query_ax=0000014F
raw00..raw70=CC...

query_mode=00000110
query_ax=0000014F
raw00..raw70=CC...
```

Advertised modes behave incorrectly:

```text
query_mode=00000105
query_ax=0000004F
raw00..raw70=00...

query_mode=00000112
query_ax=0000004F
raw00..raw70=00...
```

Other recognized legacy modes (`0101h`, `0103h`, and `0111h`) show the same
success-plus-zero-block behavior. Consequently all required fields are zero:

```text
attributes=00000000
pitch=00000000
width=00000000
height=00000000
planes=00000000
bpp=00000000
memory_model=00000000
image_pages=00000000
phys_base=00000000
```

`VBELFB.EXE` exits with code 2 because selected mode `0111h` has no usable
dimensions or physical framebuffer address.

## Pre-fix conclusions

1. `VBELFB.EXE` and its report file are working. The diskless RAM-root issue
   is not involved in the VBE failure.
2. The `4F00h` output buffer and returned far pointers are readable and
   contain plausible local-BIOS data.
3. The `4F01h` destination is not merely wrong or missing a copy-back. On a
   recognized mode the BIOS modifies exactly the requested block, replacing
   at least bytes `0x00..0x7F` with zero.
4. The mode number reaches the BIOS: unrecognized modes preserve the `0xCC`
   buffer and return failure, while recognized/advertised modes clear it and
   return success.
5. The immediate defect is therefore: the VGA BIOS recognizes the mode and
   reports `AX=004Fh`, but its mode-table population path produces only zeros.

## Resolution (2026-07-31 hardware iterations)

The BIOS was correct; RetroOS's VM86 I/O mediation was not. The Intel VGA BIOS
uses wide-decoded indexed registers at ports `F140h` and `F144h`. RetroOS
previously applied `port & 03FFh` to every VM86 I/O access and decomposed every
16/32-bit operation into byte operations. Consequently these accesses landed
at `0140h/0144h`, returned floating-bus `FFh`, and mode-setting writes triggered
side effects with partial values.

The production fix scopes native wide-port access to an explicit DPMI VBE
`INT 10h` call and:

- passes `F140h..F147h` through without ISA folding;
- preserves atomic `IN/OUT` operations at `F140h` and `F144h`; and
- also preserves PCI configuration mechanism-1 ports `CF8h/CFCh` in that
  same narrow native-BIOS scope.

Hardware results after the fix:

```text
controller_memory_64k=0000007B
attributes=0000009B
pitch=00000500
width=00000280
height=000001E0
bpp=00000010
memory_model=00000006
phys_base=D0000000
map_cf=00000000
map_linear=AFF6A000
```

Ordinary mode `4111h` (mode `111h` plus the LFB bit) then displayed the
expected RGB565 gradient on the D-sub monitor: blue/red across the top and
green/yellow across the bottom. A custom CRTC timing was tested and proved
unnecessary. VBE DDC/EDID `4F15h` returned `AX=014Fh` on this analog path.

This also explains the earlier `TotalMemory=0FFFh`: byte-split/folded reads
returned all ones. Once atomic access was restored, the DPMI controller value
matched GRUB's pre-kernel snapshot (`007Bh`).

## Complete hardware mode inventory

The fixed diagnostic copied the complete `4F00h` mode list before reusing its
buffer, then queried all 36 advertised entries. Seventeen modes returned a
usable mode-information block:

| Mode | Resolution | Bpp | Pitch | Memory model |
| ---: | ---: | ---: | ---: | --- |
| `013C` | 1920x1440 | 8 | 1920 | Packed pixel |
| `014D` | 1920x1440 | 16 | 3840 | Direct color |
| `013A` | 1600x1200 | 8 | 1600 | Packed pixel |
| `014B` | 1600x1200 | 16 | 3200 | Direct color |
| `015A` | 1600x1200 | 32 | 6400 | Direct color |
| `0107` | 1280x1024 | 8 | 1280 | Packed pixel |
| `011A` | 1280x1024 | 16 | 2560 | Direct color |
| `011B` | 1280x1024 | 32 | 5120 | Direct color |
| `0105` | 1024x768 | 8 | 1024 | Packed pixel |
| `0117` | 1024x768 | 16 | 2048 | Direct color |
| `0118` | 1024x768 | 32 | 4096 | Direct color |
| `0103` | 800x600 | 8 | 832 | Packed pixel |
| `0114` | 800x600 | 16 | 1600 | Direct color |
| `0115` | 800x600 | 32 | 3200 | Direct color |
| `0101` | 640x480 | 8 | 640 | Packed pixel |
| `0111` | 640x480 | 16 | 1280 | Direct color |
| `0112` | 640x480 | 32 | 2560 | Direct color |

Every usable entry reports attributes `009Bh` and linear framebuffer
`D0000000h`. The following 19 entries are present in `VideoModePtr` but return
`AX=004Fh` with an all-zero mode-information block and must be filtered out:

```text
0160 0161 0162 0163 0164 0165 0166 0167 0168
0169 016A 016B 016C 016D 016E 016F 0170 0171 015C
```

Final enumeration was captured in RLOG session `ed84c878`. The cleaned
per-process implementation was hardware-verified in session `a7fd5008`, and
ordinary mode `4111h` was visually confirmed without a custom CRTC block.
