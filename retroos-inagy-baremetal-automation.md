# Research: automated D945GSEJT bare-metal development

## Purpose

Use a Raspberry Pi 4 beside the D945GSEJT to automate RetroOS deployment,
rebooting, observation, and regression testing.

The preferred architecture avoids rewriting the HDD for ordinary kernel
iterations. The Pi network-boots the current kernel while the D945GSEJT keeps
using its local ext4 HDD as RetroOS's root filesystem.

The D945GSEJT documentation lists PXE/network boot, front-panel power/reset
connections, and two serial headers:

- [Intel D945GSEJT specifications](https://resources.mini-box.com/online/MBD-I-D945GSEJT/MBD-I-D945GSEJT-specs.pdf)

## Recommended topology

```text
WSL build machine
  │  scp/rsync kernel + test metadata
  ▼
Raspberry Pi 4
  ├─ PXE/DHCP/TFTP server ───────────► D945GSEJT Ethernet
  ├─ GPIO → optocouplers ────────────► power/reset headers
  ├─ USB capture card ◄────────────── D945GSEJT DVI output
  ├─ USB serial adapter ◄──────────── D945GSEJT COM2 header
  └─ optional USB HID controller ────► D945GSEJT keyboard input

D945GSEJT
  ├─ PXE loads the current kernel.elf
  └─ local HDD supplies ext4 and DOS files
```

The Pi can use a direct Ethernet connection to the D945GSEJT and run an
isolated `dnsmasq` DHCP/TFTP network. The Pi's Wi-Fi can remain its management
connection to the development machine.

## Fast kernel deployment with PXE

Instead of replacing the kernel inside the HDD image for every iteration:

1. Build `kernel_bios_text.elf` in WSL.
2. Upload it to a staging filename on the Pi.
3. Verify it and atomically publish it in the TFTP tree.
4. Pulse the D945GSEJT reset input.
5. PXE loads BIOS GRUB from the Pi.
6. GRUB downloads the new kernel.
7. RetroOS mounts the regular ext4 filesystem from the local HDD.

GRUB supports BIOS network boot and files on its `(tftp)` device. The
`grub-mknetdir` command prepares an `i386-pc` network-boot tree:

- [GNU GRUB network-boot documentation](https://www.gnu.org/software/grub/manual/grub/grub.html)

Conceptual GRUB entries:

```grub
menuentry "RetroOS current build" {
    insmod multiboot
    multiboot (tftp,192.168.44.1)/retroos/kernel.elf
    boot
}

menuentry "RetroOS known good" {
    insmod multiboot
    multiboot (tftp,192.168.44.1)/retroos/kernel-good.elf
    boot
}
```

Conceptual workstation deployment:

```bash
bazelisk build //kernel:kernel_elf_bios_text

scp bazel-bin/kernel/kernel_bios_text.elf \
    pi@retroos-lab:/srv/tftp/retroos/kernel.elf.new

ssh pi@retroos-lab \
    sudo /usr/local/sbin/retroos-publish-and-reset
```

The Pi-side publisher should:

1. Check the ELF type and expected architecture.
2. Calculate and record its checksum.
3. Rename `kernel.elf.new` atomically to `kernel.elf`.
4. Record the Git commit/build identifier.
5. Start log and video capture.
6. Pulse reset.

Always retain a separately named known-good kernel.

## Updating the root filesystem and DOS files

PXE handles kernel replacement, but DOS applications still live on the HDD.
Use a second PXE target containing a small maintenance Linux:

```text
PXE GRUB
  ├─ RetroOS current kernel
  ├─ RetroOS known-good kernel
  └─ maintenance Linux initramfs
```

The maintenance environment should:

1. Mount the HDD ext4 partition.
2. Download a staging TAR or manifest from the Pi.
3. Replace only selected files under `/home/retroos` or `/boot/retroos`.
4. Run `sync`.
5. Unmount the filesystem.
6. Reboot into RetroOS.

This avoids transferring and flashing the complete 1.2 GiB raw image for a
small diagnostic or game update.

The Pi can publish a small next-boot selection:

```text
next-boot = retroos
next-boot = maintenance
next-boot = known-good
```

Only maintenance Linux should write the HDD during automated deployment.
Never allow RetroOS and maintenance Linux to mount it writable at the same
time.

## Reset and power control

The Pi can bridge the motherboard's front-panel switch contacts using
optocouplers or suitable isolated open-drain outputs:

```text
Pi GPIO → resistor → optocoupler LED
                         │
                  isolated transistor
                         │
            motherboard RESET_SW contacts
```

Useful controls:

- short reset pulse for normal development reboot;
- short power-switch pulse for normal power toggle;
- long power-switch pulse for recovery from a hard hang;
- isolated power-LED input to determine whether the board is on.

Do not connect Pi GPIO pins directly across motherboard switch contacts.
Verify the exact motherboard header pinout and use isolation or a suitable
transistor interface.

## Video capture

Connect the D945GSEJT DVI output through the known-working adapter to the USB
capture card attached to the Pi.

The Pi can:

- stream the display to the development machine;
- save screenshots at defined boot/test stages;
- record complete sessions;
- detect a blank screen or video-mode change;
- compare graphical diagnostic results with reference images;
- apply OCR to BIOS and DOS text where useful.

Example screenshot command:

```bash
ffmpeg -f v4l2 -i /dev/video0 \
    -frames:v 1 /var/lib/retroos-runs/current/screen.png
```

Lock the capture format and resolution. Capture-card autoscaling otherwise
makes pixel comparisons unstable. Prefer image hashes or bounded visual
comparisons for graphics tests and OCR for text status.

## Bare-metal debug logging

QEMU captures RetroOS debug output written to port `E9h`; actual hardware
does not. RetroOS retains an in-memory kernel log, but that is difficult to
recover after a fatal hang.

The D945GSEJT has two serial headers. A useful kernel development feature is:

```text
debug sink = E9 + RAM klog + COM2
```

Recommended allocation:

- keep COM1 available for RetroOS hostfs;
- mirror kernel/debug output to COM2;
- connect COM2 to a Pi USB serial adapter;
- save one serial log per build and boot.

Use the correct serial-header cable and electrical-level conversion. Do not
connect RS-232-level signals directly to Pi GPIO UART pins.

## Automated keyboard input

### Raspberry Pi USB HID gadget

The Pi can emulate a USB keyboard and send scripted keystrokes. On a Pi 4,
USB peripheral mode uses the USB-C connector, which complicates simultaneous
power and USB-host use. Plan power and cabling accordingly:

- [Raspberry Pi USB gadget documentation](https://www.raspberrypi.com/news/usb-gadget-mode-in-raspberry-pi-os-ssh-over-usb/?pubDate=20260121)

Powering the Pi through PoE or an appropriate GPIO supply while reserving
USB-C for OTG may be necessary.

### Dedicated HID microcontroller

A Raspberry Pi Pico or another small USB HID-capable controller commanded by
the Pi is often simpler and more robust than using the Pi 4's power/OTG port.

### RetroOS test autorun

For repeatable kernel tests, avoid keyboard automation where possible. Add a
test selector through the Multiboot command line:

```grub
multiboot (tftp)/retroos/kernel.elf test=vbelfb-pal
```

RetroOS could automatically launch the selected test and write a
machine-readable result to COM2. USB HID remains useful for games, BIOS
setup, and exploratory interaction.

## Automated development cycle

```text
1. Build kernel in WSL
2. Upload kernel and checksum to Pi
3. Pi validates and atomically publishes it
4. Pi starts serial and video capture
5. Pi pulses motherboard reset
6. D945GSEJT PXE-loads GRUB and kernel
7. Pi waits for a serial "RetroOS ready" marker
8. Autorun or HID starts the selected test
9. Pi records result, logs, and screenshots
10. On timeout, Pi saves evidence and power-cycles the board
```

Suggested result directory:

```text
timestamp/
├── commit.txt
├── kernel.elf.sha256
├── serial.log
├── boot.png
├── result.png
├── capture.mp4
└── result.json
```

Every run should be associated with the exact kernel checksum, source commit,
boot profile, and deployed filesystem manifest.

## Suggested implementation order

1. Connect the capture card and obtain stable screenshots.
2. Configure isolated Pi PXE/DHCP/TFTP and boot the existing RetroOS kernel.
3. Add optoisolated reset control.
4. Add a COM2 kernel log sink and Pi serial capture.
5. Add atomic `deploy-and-reset` scripts.
6. Add and verify the known-good fallback kernel.
7. Add maintenance-Linux filesystem deployment.
8. Add RetroOS test autorun or USB HID control.
9. Add timeouts, result archiving, and automatic power recovery.

The first useful milestone is:

```text
bazel build → scp kernel → reset → PXE boot → serial log
```

That reduces kernel iteration to one small network transfer and one hardware
reboot while leaving the HDD filesystem stable.

## Safety and recovery rules

- Electrically isolate motherboard power/reset signals from Pi GPIO.
- Use correct level conversion for serial connections.
- Keep only one writable owner of the HDD filesystem.
- Publish kernels atomically; never let TFTP serve a partially copied ELF.
- Retain a known-good kernel and a maintenance boot entry.
- Record checksums and commits for every run.
- Treat video timeout and missing serial-ready markers as recoverable test
  failures, not permission to overwrite or reformat the disk.
- Prefer a controlled reset before escalating to a forced power cycle.
