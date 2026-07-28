# RetroOS build setup on Ubuntu 24.04 WSL2

Project-local research and build notes maintained during RetroOS bring-up.

This is the short, tested path used to prepare the minimal Ubuntu image and
start building `/home/user/RetroOS`.

## 1. Refresh Ubuntu packages

```bash
sudo apt-get update
```

## 2. Install the build prerequisites

```bash
sudo apt-get install -y \
  build-essential clang lld binutils nasm e2fsprogs python3 \
  vim-common git curl ca-certificates unzip zip xz-utils
```

These provide the host C/C++ compiler, Clang/LLD, GNU linker and `objcopy`,
NASM, `mkfs.ext4`, Python, `xxd`, and download/archive utilities.

Rust and Cargo do not need to be installed separately. Bazel downloads the
Rust nightly and target toolchains pinned by the project.

## 3. Install Bazelisk

Install the official `bazelisk-linux-amd64` release as:

```text
/usr/local/bin/bazelisk
```

The tested Bazelisk version is `v1.29.0`. The repository's `.bazelversion`
selects Bazel `7.4.1`.

Verify:

```bash
cd /home/user/RetroOS
bazelisk --version
```

Expected output:

```text
bazel 7.4.1
```

## 4. First build

Start with the bootloader:

```bash
cd /home/user/RetroOS
bazelisk build //boot:bootloader_bin
```

The first invocation downloads several pinned Bazel and Rust toolchains and
can take a while. Later builds reuse the Bazel cache.

Confirmed result:

```text
bazel-bin/boot/bootloader.bin
```

The first build completed successfully. It took about 22 minutes in this
environment because downloads from `static.rust-lang.org` were unusually
slow; the actual compile and link actions took only seconds. Bazel caches the
downloaded toolchains for later builds.

## 5. Build the kernel

```bash
cd /home/user/RetroOS
bazelisk build //kernel:kernel_elf
```

Confirmed result:

```text
bazel-bin/kernel/kernel.elf
```

The first kernel build completed successfully in about 26 minutes. Most of
that time was spent downloading the pinned Rust nightly host and
`i686-unknown-linux-musl` target components from `static.rust-lang.org`.
Bazel then completed 289 build actions without errors.

## 6. Build the bootable disk image

```bash
cd /home/user/RetroOS
bazelisk build //:image
```

This creates the open-source bootable disk image at:

```text
bazel-bin/image.bin
```

The image build completed successfully in about 2 minutes 40 seconds. It
downloaded the remaining pinned Rust nightly components for the
`x86_64-unknown-linux-musl` target, then completed 131 build actions. The
linker and Rust compiler printed warnings, but there were no errors.

## 7. Install QEMU

```bash
sudo apt-get install -y qemu-system-x86
```

Verified:

```text
QEMU emulator version 8.2.2
```

Both `qemu-system-i386` and `qemu-system-x86_64` are available. The normal
graphical launcher was verified with:

```bash
cd /home/user/RetroOS
./run.sh qemu
```

The command booted successfully into DOS Navigator. The kernel initialized
storage, mounted the ext4 root and `/host`, and reached its event loop.

## Current status

- Prerequisite installation: succeeded
- Bazelisk and Bazel 7.4.1 verification: succeeded
- `//boot:bootloader_bin`: succeeded
- `//kernel:kernel_elf`: succeeded
- `//:image`: succeeded
- Bootable image: `bazel-bin/image.bin`
- QEMU 8.2.2 installation and verification: succeeded
- Normal graphical/audio boot with `./run.sh qemu`: succeeded
- Wolfenstein MIDI playback: user-verified
- Hosted executable build:
  `bazelisk build //kernel:retroos-host --platforms=@platforms//host` succeeded
- Hosted executable: `bazel-bin/kernel/retroos-host`
- Final `test/run_all.sh` result: 2 passed, 0 failed, 3 skipped
- `hosted_games` passed: DN, Digger, Doom, SB protocol/discovery, GUS, and
  PC-speaker checks all succeeded
- `dpmi_hx` passed: the HX DPMI 0.90 probe completed without a crash
- `hosted_diff` skipped because `/dev/kvm` is unavailable
- `dpmi_smoke` and `dark_smoke` skipped because `apps-proprietary` is absent
- Required build and locally supported validation steps: complete

---

## Research notes: boot architecture

RetroOS does not boot FreeDOS or use a conventional DOS base image. Bazel
assembles a custom disk from files in the repository, and the RetroOS kernel
provides its own DOS, VM86, and DPMI compatibility environment.

### Disk and embedded filesystem layout

```text
bazel-bin/image.bin
├─ sector 0: RetroOS MBR bootloader
├─ boot-bundle area (TAR, up to 32 MiB)
│  ├─ kernel.elf
│  └─ kernel.elf.md5
└─ 1 GiB ext4 partition
   ├─ /home/retroos/CONFIG.SYS
   ├─ /home/retroos/GAMES/...
   ├─ /home/retroos/TESTS/...
   ├─ /home/retroos/TC/...
   ├─ /home/retroos/ULTRASND/...
   └─ /bin/busybox
```

`kernel.elf` also contains a small TAR linked directly into the kernel:

```text
C:\BOOT\
├─ DN\
│  ├─ DN.COM
│  └─ supporting DN files
├─ COMMAND.COM
├─ CONFIG.SYS
├─ LOADFIX.CFG
└─ SHELL.ELF
```

The boot-bundle TAR on disk and the TAR embedded in the kernel serve different
purposes. The MBR loader reads `kernel.elf` from the disk boot bundle, but the
kernel does not mount that partition. Instead, it mounts its embedded TAR as
`C:\BOOT`.

### Where the DOS content comes from

- `apps-boot/dn/`: DOS Navigator and its support files
- `apps/games/`: public/shareware DOS games
- `apps/ultrasnd/`: Gravis UltraSound instrument patches
- `apps-boot/tc/`: DOS compiler/toolchain content
- `apps/TP70/`: Turbo Pascal content when present
- `test/dos/`: DOS test programs and their sources
- `tools/command/`: the built `COMMAND.COM`
- `etc/CONFIG.SYS`: the main writable boot configuration
- `apps/busybox/busybox`: the separate Linux-style userland

Bazel gathers these files into TAR targets. The image rule extracts the extras
and uses `mkfs.ext4 -d` to populate the 1 GiB ext4 partition.

### Boot sequence

1. QEMU boots `bazel-bin/image.bin` as a raw disk. The launcher uses
   `snapshot=on`, so a normal QEMU session does not modify the source image.
2. The BIOS loads sector 0, containing RetroOS's assembly MBR.
3. The MBR begins in 16-bit real mode, installs a GDT, and switches into
   32-bit protected mode.
4. The Rust bootloader scans the boot-bundle TAR for `kernel.elf` and its MD5,
   reads the kernel with BIOS disk services, parses the ELF, and loads its
   segments at their physical addresses.
5. The bootloader creates a Multiboot-style memory-map structure and jumps to
   the kernel entry point.
6. `arch-metal` performs privileged setup: paging, memory protection,
   interrupts, PIC/PIT, input, display, storage, and audio.
7. The main kernel startup path enters ring 1, probes disks, and scans their
   partition tables.
8. The kernel ignores the boot-bundle partition and mounts the ext4 partition
   as `/`.
9. DOS `C:` maps to `/home/retroos`, so `C:\GAMES` is
   `/home/retroos/GAMES`, for example.
10. The TAR embedded in `kernel.elf` mounts over `/home/retroos/boot`, exposed
    to DOS as `C:\BOOT`.
11. The kernel loads writable `C:\CONFIG.SYS` first. If it is absent, it uses
    the embedded `C:\BOOT\CONFIG.SYS`.
12. Startup directly launches `C:\BOOT\DN\DN.COM` through RetroOS's DOS/VM86
    runtime. Protected-mode DOS programs use RetroOS's own DPMI support.
13. If DOS Navigator exits, the kernel starts it again.

```text
BIOS
 → RetroOS MBR
 → Rust bootloader
 → kernel.elf
 → arch-metal hardware setup
 → ring-1 kernel
 → ext4 mounted as /
 → /home/retroos exposed as C:
 → embedded bootfs mounted as C:\BOOT
 → CONFIG.SYS loaded
 → C:\BOOT\DN\DN.COM launched
```

The main design split is that DOS Navigator and `COMMAND.COM` live in the
kernel's guaranteed, read-only embedded boot environment. Games, tools,
writable configuration, and save data live on the larger ext4-backed `C:`
drive.

---

## Research notes: booting on bare metal

The initial target machine is an Intel D945GSEJT with a classic BIOS, VGA,
an Atom CPU with VME, HDA audio, SATA/IDE compatibility mode, and
UHCI/EHCI-era USB.

### Important USB distinction

GRUB can read a USB stick through BIOS services and load `kernel.elf`.
After GRUB transfers control, RetroOS uses its own storage drivers. RetroOS
does not currently have UHCI/EHCI USB mass-storage support, so it may be unable
to mount an ext4 partition on the same USB stick.

```text
GRUB can load the kernel from USB
    does not necessarily mean
RetroOS can mount that USB after boot
```

This is not fatal for the first test. `kernel.elf` contains the embedded
`C:\BOOT` filesystem, including DOS Navigator and `COMMAND.COM`, so a
diskless boot can still reach DN.

### Reusable build artifacts

```text
bazel-bin/kernel/kernel.elf       Multiboot kernel with embedded bootfs
bazel-bin/bootfs_tar.tar          Embedded DN/COMMAND.COM filesystem
bazel-bin/extras_tar.tar          Complete ext4 filesystem content
bazel-bin/dos_extras_tar.tar      DOS portion of the ext4 content
bazel-bin/games_tar.tar           Public/shareware games
bazel-bin/image.bin               Existing custom-MBR raw disk image
```

GRUB can load `kernel.elf` directly. The RetroOS MBR and boot-bundle partition
are not needed when using GRUB.

### Option 1: existing GRUB rescue ISO

The repository already has a GRUB ISO target:

```bash
sudo apt-get install -y grub-common grub-pc-bin xorriso mtools
cd /home/user/RetroOS
bazelisk build //:grub_iso
```

Expected output:

```text
bazel-bin/retroos_grub.iso
```

The ISO contains `kernel.elf` and a GRUB configuration that loads it with the
Multiboot protocol and keeps a VBE framebuffer:

```grub
set timeout=1
insmod all_video
insmod vbe
insmod vga
set gfxmode=1024x768x32
set gfxpayload=keep

menuentry "RetroOS" {
    multiboot /boot/kernel.elf
    boot
}
```

The ISO can be written to a USB stick with Etcher. Its minimum expected path
is:

```text
BIOS → GRUB → kernel.elf → embedded C:\BOOT → DOS Navigator
```

This is the recommended first hardware image because it tests GRUB,
Multiboot, framebuffer handoff, CPU/VME, interrupts, keyboard legacy
emulation, storage probing, and audio probing without requiring an installed
Linux system.

### Option 2: raw BIOS-GRUB disk with ext4

A conventional Etcher-ready raw image could be assembled as:

```text
retroos-grub.img
├─ MBR: GRUB i386-pc boot code
├─ post-MBR gap: GRUB core image
└─ ext4 partition
   ├─ /boot/grub/grub.cfg
   ├─ /boot/retroos/kernel.elf
   ├─ /home/retroos/GAMES/...
   ├─ /home/retroos/TESTS/...
   ├─ /home/retroos/TC/...
   └─ /bin/busybox
```

It can be built from `kernel.elf` and `extras_tar.tar` without installing a
Linux distribution on the target computer:

1. Create an empty raw image.
2. Write an MBR partition table.
3. Leave a post-MBR embedding gap for GRUB.
4. Create and populate an ext4 partition.
5. Copy `kernel.elf` to `/boot/retroos/`.
6. Add `boot/grub/grub.cfg`.
7. Install GRUB for the `i386-pc` target.
8. Generate a SHA-256 checksum.
9. Test in QEMU before flashing.

This reproduces the filesystem arrangement suggested by the project creator.
It should expose the full DOS environment when installed on an internal SATA
disk in IDE compatibility mode. When written to USB, GRUB should boot it, but
RetroOS may not see its ext4 partition after taking control.

### Option 3: larger GRUB-loaded embedded filesystem

For a completely self-contained USB, a new kernel variant could embed selected
games and tools:

```text
kernel-usb.elf
└─ embedded bootfs
   ├─ DN/
   ├─ COMMAND.COM
   ├─ GAMES/
   ├─ ULTRASND/
   └─ TESTS/
```

GRUB would read the entire ELF through the BIOS before entering RetroOS.
RetroOS would then access those files from RAM and would not need a native USB
mass-storage driver.

Trade-offs:

- The kernel could grow from about 2 MiB to roughly 90–100 MiB.
- Embedded files would be read-only and nonpersistent.
- Games might appear under `C:\BOOT\GAMES` unless mount policy is adjusted.
- A new Bazel bootfs/kernel target would be required.
- GRUB loading and RAM usage would need testing on the Atom board.

This is the strongest fallback when the kernel boots successfully but cannot
mount USB storage.

### Recommended bare-metal progression

1. Build and flash the existing `//:grub_iso`.
2. Confirm that GRUB reaches the embedded DOS Navigator.
3. Record VGA, keyboard, storage, and audio detection results.
4. Build a raw GRUB/ext4 image and test whether the BIOS exposes USB storage
   in a form RetroOS can use.
5. If USB ext4 is invisible, either:
   - put the ext4 filesystem on an internal SATA disk in IDE mode, or
   - build a larger GRUB-loaded kernel with selected apps embedded.

The first GRUB ISO is a low-complexity hardware probe. Full USB-hosted games
depend on storage visibility and should be treated as a separate second goal.

### GRUB HDD image build log

Goal:

```text
retroos-grub-hdd.img
├─ BIOS GRUB in the MBR and post-MBR gap
└─ ext4 partition
   ├─ /boot/grub/grub.cfg
   ├─ /boot/retroos/kernel.elf
   ├─ /home/retroos/...
   └─ /bin/busybox
```

Installed successfully:

```bash
sudo apt-get install -y grub-common grub-pc-bin parted
```

Verified:

- GRUB version: 2.12
- `grub-mkimage`: available
- GRUB `i386-pc` modules and `boot.img`: available
- GNU Parted 3.6: available
- WSL kernel supports loop devices, but `/dev/loop*` device nodes are absent
- `grub-install` and `grub-bios-setup` are not yet present; Ubuntu packages
  these commands in `grub2-common`

Next proposed setup command:

```bash
sudo apt-get install -y grub2-common
```

Installed successfully:

```bash
sudo apt-get install -y grub2-common
```

Verified:

- `grub-install` 2.12: available
- `grub-probe` 2.12: available
- BIOS setup helper:
  `/usr/lib/grub/i386-pc/grub-bios-setup`
- Reusable inputs:
  - `bazel-bin/kernel/kernel.elf` (about 2 MiB)
  - `bazel-bin/extras_tar.tar` (about 89 MiB)

The next setup step is to create the loop-control and loop block-device nodes
that this minimal WSL root image omitted. These nodes expose loop-device
support already present in the WSL kernel; they do not attach an image by
themselves.

Loop-device verification:

```text
/dev/loop-control: present
/dev/loop0 ... /dev/loop7: present
losetup -f: /dev/loop0
```

The device nodes were already visible in the privileged environment even
though the restricted shell could not initially see them. No image is
currently attached.

#### First GRUB HDD image build

Added reusable source files:

```text
tools/build_grub_hdd_image.sh
tools/grub-hdd-grub.cfg
```

Build command:

```bash
cd /home/user/RetroOS
sudo tools/build_grub_hdd_image.sh
```

Created successfully:

```text
retroos-grub-hdd.img
retroos-grub-hdd.img.sha256
```

Image properties:

- Raw size: 1152 MiB
- Partition table: MBR/MS-DOS
- Partition 1: bootable ext4
- Partition start: LBA 2048 (1 MiB)
- BIOS GRUB installation: succeeded
- SHA-256 verification: succeeded
- Read-only `e2fsck`: clean
- Files verified inside ext4:
  - `/boot/retroos/kernel.elf`
  - `/boot/grub/grub.cfg`
  - `/home/retroos/GAMES/DOOMS/DOOM.EXE`

The builder refuses to overwrite an existing output image, uses a temporary
loop device and mount, and detaches them on success or failure.

Next proposed step: boot this exact raw image in QEMU using an IDE disk
controller and verify the full path:

```text
SeaBIOS → GRUB → kernel.elf → ext4 root → DOS Navigator
```

#### Display experiments and finding

The first GRUB images successfully booted the kernel, mounted ext4, and
started DN, but requested a linear VBE framebuffer:

```text
GRUB/VBE → RetroOS software VGA renderer → linear framebuffer
```

The first attempt produced an unsupported 800x600 24-bpp mode. An explicit
1024x768x32 Multiboot variant fixed the blank screen, but DOS scaling was
distorted and Doom's planar VGA output was garbled. The storage and DOS runtime
were healthy; the problem was the software VGA-to-framebuffer presentation
path.

For a classic-BIOS machine with real VGA, the correct architecture is:

```text
GRUB console → RetroOS direct legacy VGA → hardware/QEMU VGA scanout
```

The framebuffer-specific target and GRUB settings were therefore removed.

#### Final known-good BIOS-text/VGA image

A dedicated legacy-BIOS kernel entry now omits Multiboot's video-request flag:

```text
//kernel:kernel_elf_bios_text
→ bazel-bin/kernel/kernel_bios_text.elf
```

It reuses the normal kernel and embedded bootfs. The default
`//kernel:kernel_elf` remains unchanged for modern framebuffer machines.

The GRUB configuration uses the console rather than VBE/GFXTERM:

```grub
insmod part_msdos
insmod ext2
insmod multiboot
terminal_output console

menuentry "RetroOS" {
    search --no-floppy --file /boot/retroos/kernel.elf --set=root
    multiboot /boot/retroos/kernel.elf
    boot
}
```

Build the dedicated kernel:

```bash
cd /home/user/RetroOS
bazelisk build //kernel:kernel_elf_bios_text
```

Build the single HDD image:

```bash
sudo tools/build_grub_hdd_image.sh \
  /home/user/RetroOS/retroos-grub-hdd.img \
  /home/user/RetroOS/bazel-bin/kernel/kernel_bios_text.elf
```

Only one large HDD `.img` artifact is retained. Superseded framebuffer test
images and their checksums were removed.

Final artifacts:

```text
retroos-grub-hdd.img
retroos-grub-hdd.img.sha256
```

Known-good QEMU test:

```bash
cd /home/user/RetroOS
./run_grub_hdd.sh
```

The launcher uses `retroos-grub-hdd.img`, starts QEMU with the known-good
BIOS/IDE/VGA settings, and enables QEMU snapshot mode so experiments do not
modify the image. It explicitly selects QEMU's standard VGA adapter with
`-vga std`, keeping the test focused on portable VGA/VESA behavior rather
than a paravirtualized graphics device. Extra QEMU options can be appended to
the command. Run `./run_grub_hdd.sh --help` to see the image, memory, and
display overrides.

QEMU cannot emulate the D945GSEJT's Intel GMA 950 graphics hardware exactly.
The standard VGA model is nevertheless the most relevant approximation for
RetroOS's current direct legacy-VGA path. Real hardware testing is still
needed for the Intel VGA BIOS, chipset-specific behavior, display timings,
and physical output.

It also configures QEMU's `intel-hda` controller with an `hda-duplex` codec.
QEMU identifies this controller as ICH6; it has no exact ICH7M model, so this
is the closest available option and exercises the same RetroOS HDA driver
needed by the D945GSEJT. The audio backend can be overridden, for example:

```bash
RETROOS_AUDIO_BACKEND=none ./run_grub_hdd.sh
```

Verified result:

```text
SeaBIOS
 → BIOS GRUB console
 → Multiboot kernel_bios_text.elf
 → direct VgaCard path
 → ATA/IDE disk detected (1152 MiB)
 → ext4 root mounted (1151 MiB)
 → DOS Navigator launched
 → Doom rendered correctly
 → Intel HDA audio detected
 → Doom MIDI music working
```

Relevant kernel output:

```text
Platform: host=Qemu display=VgaCard firmware=NativeBios audio=EmulatedHda
Storage: ata0 (1152 MB)
ext4 root (1151 MB)
Audio: EmulatedHda (SB_AUDIO=native)
Starting DN...
```

The user visually verified that DOS Navigator and Doom render correctly
without distortion or garbling. After changing the launcher from a null audio
backend to QEMU's `intel-hda` plus `hda-duplex`, the user also verified that
Doom gained working MIDI music and that overall audio quality improved. This
confirms the HDA output path is working end-to-end and makes the test more
representative of the D945GSEJT's ICH7-family HDA hardware.

This raw image is the candidate for writing to the D945GSEJT HDD with Etcher.
Configure the target for legacy BIOS boot and IDE compatibility mode.

### Banked VESA diagnostic

The existing HDD image includes `C:\TESTS\VBETEST.EXE`, compiled from
`test/dos/vbetest/vbetest.c` with the bundled Turbo C compiler. It directly
tests VBE controller information, mode information, mode setting, and the
64 KiB banked window at `A000:0000`.

Boot the image, open `C:\TESTS` in DOS Navigator, and first run:

```dos
VBETEST
```

This lists the modes advertised by the active VGA BIOS. Then test a mode from
that list, beginning with the usual 640x480x8 mode:

```dos
VBETEST 101
```

The program should paint a colour pattern; press any key to restore text mode.
If this banked test works while a protected-mode game still crashes in a VESA
mode, the likely remaining fault is the game's VESA linear-framebuffer mapping
through DPMI rather than basic VBE mode setting or bank switching.

User-verified banked VESA results under SeaBIOS/QEMU standard VGA:

```text
VBE 3.0, 16 MiB video memory
0x101  640x480x8   passed
0x110  640x480x15  passed
0x111  640x480x16  passed
0x112  640x480x24  passed
0x142  640x480x32  passed
0x103  800x600x8   passed
```

Every mode painted the expected RGB/indexed gradient and returned cleanly to
DOS. Kernel traces showed normal process exit and no fault. This rules out
basic VBE discovery, mode setting, bank switching, resolution above 640x480,
and the tested packed/direct colour layouts as the general cause of games
crashing in high-resolution modes. The leading suspect is now the separate
VBE linear-framebuffer path used by protected-mode games, especially DPMI
physical-address mapping (`INT 31h AX=0800h`).

For complete diagnostic output, `VBETEST` can write
`C:\HOST\VBEMODES.TXT` when the launcher is given an isolated hostfs folder:

```bash
install -d -m 700 /tmp/retroos-vbe-results
RETROOS_HOSTFS_DIR=/tmp/retroos-vbe-results ./run_grub_hdd.sh
```

The launcher temporarily exposes the guest VFS root as DOS `C:` so the share
is reachable at `C:\HOST`; only the specified host directory is served.
During this work, two stale wire-protocol mismatches in `hostfs.py` were fixed:
READDIR now sends the 32-bit modification time expected by the kernel, and
CLOSE no longer sends an unsolicited reply to the kernel's fire-and-forget
clunk operation. These fixes stopped DN directory browsing and repeated F3
file viewing from desynchronizing and freezing the serial protocol.

### Confirmed VESA LFB failure

`C:\TESTS\VBELFB.EXE` is a 32-bit `dosrt` diagnostic with two stages:

```dos
VBELFB INFO
VBELFB DRAW
```

`INFO` queries SeaBIOS mode 111h (640x480x16), calls DPMI physical-address
mapping (`INT 31h AX=0800h`), logs the result, and deliberately does not touch
the framebuffer. It produced:

```text
attributes=000000BB
phys_base=FD000000
map_size=00096000
map_cf=00000000
map_linear=FD000000
```

This shows that SeaBIOS places QEMU standard VGA's LFB at physical
`0xFD000000`. RetroOS incorrectly reports DPMI mapping success while merely
returning that physical address unchanged. The address is above RetroOS's
`0xC0000000` user-space ceiling and is not mapped into the client.

`DRAW` then selected mode 4111h (mode 111h plus the VBE linear-framebuffer
bit) and attempted the first pixel write. The mode switch succeeded and the
screen went blank, but the write failed exactly as predicted:

```text
fault rip=0x501303 addr=0xfd000000 err=0x6
SEGV in thread 2 at 0xfd000000
[mem] exit tid=2 code=-11
```

Error code `0x6` is a user-mode write to a non-present page. This is direct
confirmation—not merely inference—that protected-mode games using a native
VBE linear framebuffer crash because DPMI AX=0800h does not create a usable
user mapping. Banked VESA remains working. A real fix must map the requested
physical framebuffer pages at an available user linear address below 3 GiB,
return that address from AX=0800h, track it for AX=0801h, and apply suitable
cache policy (normally write-combining for video memory).

### How UniVBE relates to RetroOS

UniVBE, later sold as SciTech Display Doctor, is an optional replacement or
enhancement for a video card's VBE BIOS implementation. It contains
card-specific knowledge and was historically used to add missing VESA modes
or work around incomplete, buggy, or slow video BIOS implementations. It is
not a framebuffer emulator.

Without UniVBE, a game calls the video card's built-in VBE implementation:

```text
DOS game -> VBE calls -> video BIOS -> VGA hardware
```

With UniVBE loaded, it supplies or improves the VBE-provider layer:

```text
DOS game -> VBE calls -> UniVBE -> VGA hardware
```

This is separate from RetroOS's DPMI responsibility. After either the native
VBE BIOS or UniVBE reports an LFB physical address, a protected-mode program
still needs `INT 31h AX=0800h` to turn that physical device address into a
usable client linear address. UniVBE therefore cannot fix the confirmed
`0xFD000000` page fault: RetroOS must implement the mapping.

The recommended order is:

1. Implement and validate DPMI physical mapping with QEMU's native VBE BIOS.
2. Test the D945GSEJT's native GMA 950 VBE modes on bare metal.
3. Try UniVBE only if the real video BIOS lacks required modes or exhibits
   mode-setting, banking, or other VBE-specific bugs.

UniVBE is a useful optional compatibility tool, not a required image
component. It may itself depend on conventional DOS behavior such as direct
I/O, interrupt-vector handling, hardware probing, and compatible DPMI
services, so testing it may reveal additional RetroOS compatibility gaps.

### VESA LFB mapper implemented and verified

On 2026-07-27, RetroOS's DPMI `0800h` identity-address placeholder was
replaced with a real physical-device mapper. It uses a dedicated
downward-growing virtual window at `0xA0000000..0xB0000000`, keeps per-client
mapping records, implements `0801h`, and cleans mappings up on process exit
and nested `EXEC` state replacement. Device pages are mapped uncached and
marked externally owned; programs that never request `0800h` take no new
mapping path.

The distinction from RetroOS's existing synthetic SVGA path is important.
Synthetic SVGA already creates backing at its reported `0x40000000` address.
External VBE providers such as SeaBIOS or a real video BIOS instead report a
physical device address that the DPMI host must map for the protected-mode
client.

The new BIOS-text kernel was installed into the existing single
`retroos-grub-hdd.img` using:

```bash
sudo tools/update_grub_hdd_kernel.sh \
  /home/user/RetroOS/retroos-grub-hdd.img \
  /home/user/RetroOS/bazel-bin/kernel/kernel_bios_text.elf
```

No additional image was created. In QEMU, `VBELFB INFO` mapped SeaBIOS's
physical `0xFD000000` framebuffer to client address `0xAFF6A000`, below the
3 GiB ceiling:

```text
map_cf=00000000
map_linear=AFF6A000
[mem] exit tid=2 code=0
```

`VBELFB DRAW` selected VBE mode `0x4111`, displayed the expected gradient,
restored text mode, and exited with code 0 without a page fault. The protected
mode LFB path is therefore confirmed working in QEMU. A formerly crashing
high-resolution game and explicit `0801h` success/error cases remain the next
regression checks.
