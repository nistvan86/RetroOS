# Raspberry Pi RAM-Backed Legacy BIOS PXE Server

This is the expected persistent host configuration for the
`deploy-retroos-kernel` skill. Its paths and interface names match that skill's
defaults; use `RETROOS_PXE_HOST`, `RETROOS_PXE_SEED`, `RETROOS_TFTP_ROOT`, and
`RETROOS_PXE_INTERFACE` when a lab uses different values.

Ordinary kernel deployment must not repeat these provisioning steps or modify
the persistent configuration. Use this guide for initial setup, audit, repair,
or migration only.

This guide configures a Raspberry Pi as a dedicated legacy BIOS PXE server.
The Pi uses Wi-Fi for management and reserves Ethernet for a directly
connected PXE client.

The live TFTP root is a 256 MiB `tmpfs` mounted at `/srv/tftp`. A persistent
seed under `/usr/local/share/pxe-tftp-seed` contains the GRUB PXE binary,
i386-pc modules, and a minimal Hello menu. A systemd service copies that seed
into RAM at every boot before `dnsmasq` starts.

The baseline deliberately contains no operating-system kernel or
machine-specific boot entries.

## Network topology

```text
Home Wi-Fi
    |
    | SSH
    v
Raspberry Pi
    wlan0: home network
    eth0: 10.77.77.1/24
    |
    | direct Ethernet cable
    v
Legacy BIOS PXE client
    DHCP: 10.77.77.100-150
```

## 1. Install Raspberry Pi OS Lite

Use Raspberry Pi Imager to configure the hostname, user, Wi-Fi, and SSH
access. Boot the Pi, connect over SSH, and update it:

```bash
sudo apt update
sudo apt full-upgrade -y
sudo reboot
```

Reconnect after the reboot.

## 2. Configure the isolated Ethernet interface

Confirm NetworkManager is active:

```bash
systemctl is-active NetworkManager
nmcli device status
```

Configure NetworkManager to bring up `eth0` even without carrier:

```bash
sudo tee /etc/NetworkManager/conf.d/10-pxe-ignore-carrier.conf >/dev/null <<'EOF'
[device-eth0-ignore-carrier]
match-device=interface-name:eth0
ignore-carrier=yes
EOF
```

Restart NetworkManager:

```bash
sudo systemctl restart NetworkManager
```

Create the dedicated connection:

```bash
sudo nmcli connection add \
    type ethernet \
    ifname eth0 \
    con-name pxe \
    ipv4.method manual \
    ipv4.addresses 10.77.77.1/24 \
    ipv4.never-default yes \
    ipv4.may-fail no \
    ipv6.method disabled \
    connection.autoconnect yes \
    connection.autoconnect-priority 100
```

If the `pxe` connection already exists, inspect it instead:

```bash
nmcli connection show pxe
```

Activate and verify it:

```bash
sudo nmcli connection up pxe
ip addr show eth0
ip route
```

Expected addressing:

```text
inet 10.77.77.1/24
10.77.77.0/24 dev eth0
```

The Wi-Fi interface must retain the default route. Reboot once and confirm
that `eth0` receives `10.77.77.1/24` without manual intervention.

## 3. Install dnsmasq and GRUB tools

```bash
sudo apt install -y dnsmasq grub-common
```

The Pi runs ARM software, but a legacy x86 client needs GRUB's `i386-pc`
modules. Download those modules without installing the x86 package:

```bash
sudo dpkg --add-architecture i386
sudo apt update
mkdir -p ~/grub-pxe-build/extracted
cd ~/grub-pxe-build
apt download grub-pc-bin:i386
dpkg-deb -x grub-pc-bin_*_i386.deb extracted
```

Verify the extracted module set:

```bash
ls extracted/usr/lib/grub/i386-pc
```

It should include at least:

```text
pxe.mod
tftp.mod
net.mod
normal.mod
configfile.mod
moddep.lst
```

## 4. Create the persistent PXE seed

The seed is read during boot but is not used as the live TFTP root:

```bash
sudo mkdir -p /usr/local/share/pxe-tftp-seed/grub/i386-pc
sudo cp -a \
    ~/grub-pxe-build/extracted/usr/lib/grub/i386-pc/. \
    /usr/local/share/pxe-tftp-seed/grub/i386-pc/
```

Build a generic legacy BIOS PXE image into the seed:

```bash
sudo grub-mkimage \
    -d /usr/local/share/pxe-tftp-seed/grub/i386-pc \
    -O i386-pc-pxe \
    -o /usr/local/share/pxe-tftp-seed/bootx86.pxe \
    -p /grub \
    pxe \
    tftp \
    net \
    normal \
    configfile \
    echo \
    read
```

Create the stable baseline configuration:

```bash
sudo tee /usr/local/share/pxe-tftp-seed/grub/grub.cfg >/dev/null <<'EOF'
set timeout=30
set default=0

menuentry "Hello PXE" {
    clear

    echo
    echo "================================"
    echo " Raspberry Pi PXE Server Works "
    echo "================================"
    echo
    echo "GRUB loaded successfully."
    echo
    echo "Press any key..."

    read
}
EOF
```

Set predictable seed permissions:

```bash
sudo find /usr/local/share/pxe-tftp-seed -type d -exec chmod 0755 {} +
sudo find /usr/local/share/pxe-tftp-seed -type f -exec chmod 0644 {} +
```

Verify the seed:

```bash
find /usr/local/share/pxe-tftp-seed -maxdepth 3 -type f -ls
```

The persistent seed should have this shape:

```text
/usr/local/share/pxe-tftp-seed/
|-- bootx86.pxe
`-- grub/
    |-- grub.cfg
    `-- i386-pc/
        |-- normal.mod
        |-- pxe.mod
        |-- tftp.mod
        `-- ...
```

## 5. Mount the live TFTP root in RAM

Create the mount point:

```bash
sudo mkdir -p /srv/tftp
```

Add this line to `/etc/fstab`:

```fstab
tmpfs /srv/tftp tmpfs rw,nosuid,nodev,noatime,mode=0755,size=256M 0 0
```

Mount it:

```bash
sudo mount /srv/tftp
```

Verify that the live root is RAM-backed:

```bash
findmnt -T /srv/tftp -o TARGET,SOURCE,FSTYPE,OPTIONS
df -hT /srv/tftp
```

Expected filesystem type:

```text
tmpfs
```

The `size=256M` value is a ceiling, not reserved RAM. Memory is consumed only
as files are copied or uploaded.

## 6. Populate the tmpfs during boot

Create `/etc/systemd/system/pxe-tftp-populate.service`:

```ini
[Unit]
Description=Populate the RAM-backed PXE TFTP root
Requires=srv-tftp.mount
After=srv-tftp.mount
Before=dnsmasq.service

[Service]
Type=oneshot
ExecStart=/usr/bin/cp -a /usr/local/share/pxe-tftp-seed/. /srv/tftp/
ExecStart=/usr/bin/find /srv/tftp -type d -exec chmod 0755 {} +
ExecStart=/usr/bin/find /srv/tftp -type f -exec chmod 0644 {} +
RemainAfterExit=yes

[Install]
WantedBy=multi-user.target
```

The `srv-tftp.mount` unit is generated automatically from `/etc/fstab`.

Reload systemd and enable the population service:

```bash
sudo systemctl daemon-reload
sudo systemctl enable pxe-tftp-populate.service
sudo systemctl start pxe-tftp-populate.service
```

Verify the live tree:

```bash
findmnt -T /srv/tftp
find /srv/tftp -maxdepth 3 -type f -ls
cmp /usr/local/share/pxe-tftp-seed/grub/grub.cfg \
    /srv/tftp/grub/grub.cfg
```

Any runtime changes under `/srv/tftp` disappear at reboot. The next boot
restores the stable Hello-only seed.

## 7. Configure dnsmasq

Back up an existing configuration before replacing it:

```bash
sudo cp -a /etc/dnsmasq.conf /etc/dnsmasq.conf.orig
```

Create `/etc/dnsmasq.conf`:

```ini
interface=eth0
bind-interfaces
except-interface=wlan0

dhcp-range=10.77.77.100,10.77.77.150,255.255.255.0,12h

enable-tftp
tftp-root=/srv/tftp
dhcp-boot=bootx86.pxe

log-dhcp
log-queries
```

Validate it:

```bash
sudo dnsmasq --test
```

Add an explicit dependency on the population service:

```bash
sudo systemctl edit dnsmasq.service
```

Enter:

```ini
[Unit]
Requires=pxe-tftp-populate.service
After=pxe-tftp-populate.service
```

Enable and restart dnsmasq:

```bash
sudo systemctl daemon-reload
sudo systemctl enable dnsmasq.service
sudo systemctl restart dnsmasq.service
systemctl status dnsmasq.service
```

## 8. Verify the complete boot-time setup

Reboot the Pi:

```bash
sudo reboot
```

Reconnect and verify:

```bash
findmnt -T /srv/tftp -o TARGET,SOURCE,FSTYPE,OPTIONS
systemctl status pxe-tftp-populate.service
systemctl status dnsmasq.service
cmp /usr/local/share/pxe-tftp-seed/grub/grub.cfg \
    /srv/tftp/grub/grub.cfg
test -r /srv/tftp/bootx86.pxe
test -r /srv/tftp/grub/i386-pc/normal.mod
```

The live tree should initially contain only the generic PXE boot files:

```text
/srv/tftp/
|-- bootx86.pxe
`-- grub/
    |-- grub.cfg
    `-- i386-pc/
        `-- ...
```

There should be no OS-specific kernel tree in the baseline.

## 9. Test a PXE client

Connect the legacy BIOS client directly to the Pi's Ethernet port. Enable its
onboard LAN boot agent and legacy PXE boot, then boot from the network.

Watch dnsmasq:

```bash
sudo journalctl -fu dnsmasq
```

A successful exchange includes DHCP followed by TFTP requests for:

```text
bootx86.pxe
grub/grub.cfg
```

The client should reach GRUB and display only:

```text
Hello PXE
```

Selecting it should print:

```text
================================
 Raspberry Pi PXE Server Works
================================

GRUB loaded successfully.

Press any key...
```

## Troubleshooting

### `/srv/tftp` is not tmpfs

Check the fstab entry and mount unit:

```bash
grep '/srv/tftp' /etc/fstab
systemctl status srv-tftp.mount
findmnt -T /srv/tftp
```

Do not start dnsmasq until `/srv/tftp` reports `FSTYPE=tmpfs`.

### The live TFTP tree is empty

Check the seed and population service:

```bash
find /usr/local/share/pxe-tftp-seed -maxdepth 3 -type f
systemctl status pxe-tftp-populate.service
journalctl -u pxe-tftp-populate.service -b
```

Repopulate it manually for diagnosis:

```bash
sudo systemctl restart pxe-tftp-populate.service
```

### dnsmasq starts before files are populated

Inspect unit ordering:

```bash
systemctl cat dnsmasq.service
systemctl list-dependencies dnsmasq.service
```

Confirm the override requires and starts after
`pxe-tftp-populate.service`.

### `eth0` has no address

```bash
nmcli connection show pxe
ip addr show eth0
journalctl -u NetworkManager -b
```

Confirm the ignore-carrier configuration and the static
`10.77.77.1/24` profile.

### The client receives no DHCP response

```bash
ip link show eth0
sudo ss -lunp | grep ':67'
sudo journalctl -fu dnsmasq
```

Check cable carrier, BIOS PXE enablement, the boot agent, and the isolated
Ethernet interface.

### GRUB downloads but no menu appears

```bash
test -r /srv/tftp/grub/grub.cfg
test -r /srv/tftp/grub/i386-pc/normal.mod
sudo journalctl -u dnsmasq -b
```

The PXE image was built with `-p /grub`, so it expects
`/srv/tftp/grub/grub.cfg`.

## Persistent files worth backing up

Back up the persistent configuration, not the tmpfs contents:

```text
/etc/NetworkManager/conf.d/10-pxe-ignore-carrier.conf
/etc/NetworkManager/system-connections/pxe.nmconnection
/etc/dnsmasq.conf
/etc/fstab
/etc/systemd/system/pxe-tftp-populate.service
/etc/systemd/system/dnsmasq.service.d/override.conf
/usr/local/share/pxe-tftp-seed/
```

Do not back up `/srv/tftp` as authoritative state; it is an ephemeral runtime
copy.
