---
name: deploy-retroos-kernel
description: Provision the Raspberry Pi PXE runtime and atomically deploy a RetroOS Multiboot kernel and GRUB configuration for Intel D945GSEJT bare-metal testing. Use whenever a task requires or would materially benefit from testing the current RetroOS kernel on the physical D945GSEJT, including boot debugging, hardware-driver work, PXE validation, and real-machine regression checks.
---

# Deploy RetroOS Kernel

Use the bundled script instead of issuing deployment commands manually.

## Workflow

1. Run relevant local tests.
2. Build the requested deployable ELF target, normally:

   ```bash
   bazelisk build //kernel:kernel_elf
   ```

3. Locate this project skill and inspect the PXE runtime:

   ```bash
   REPO_ROOT="${RETROOS_REPO_ROOT:-$(git rev-parse --show-toplevel)}"
   SKILL_DIR="$REPO_ROOT/.agents/skills/deploy-retroos-kernel"
   "$SKILL_DIR/scripts/deploy_kernel.sh" --status
   ```

4. Deploy the intended ELF, for example:

   ```bash
   "$SKILL_DIR/scripts/deploy_kernel.sh" bazel-bin/kernel/kernel.elf
   ```

5. Keep the bundled native VGA/BIOS GRUB configuration as the default whenever
   the current test does not need to launch a specific program. When testing a
   text diagnostic embedded in `kernel.elf` (for example
   `boot/TESTS/VBELFB.EXE`), replace only the live tmpfs GRUB configuration
   with a single timeout-zero entry whose Multiboot line appends
   `retroos.exec=<embedded-vfs-path>`. Treat this as per-execution test state:
   do not change the persistent seed, and let the next ordinary deployment
   restore the default configuration.
6. Report the local and remote checksums. Do not claim the target ran the
   kernel merely because deployment succeeded.

The script requires the configured TFTP root to be tmpfs and refuses deployment
otherwise. It reconstructs the runtime from the configured persistent seed,
validates and uploads the kernel, installs the bundled GRUB configuration,
sets TFTP-readable permissions, preserves the previous kernel, and publishes
through atomic renames.

## Expected Raspberry Pi Baseline

Expect the Pi to be provisioned according to
[references/raspberry-pi-pxe-server.md](references/raspberry-pi-pxe-server.md):

- management over Wi-Fi and an isolated, static Ethernet PXE network;
- `dnsmasq` providing DHCP and TFTP only on the dedicated Ethernet interface;
- a persistent generic GRUB seed under `RETROOS_PXE_SEED`;
- a RAM-backed live TFTP root under `RETROOS_TFTP_ROOT`;
- systemd populating the live root before `dnsmasq` starts.

Read that reference completely when provisioning a new Pi, auditing the
persistent PXE configuration, or diagnosing a failure below the deploy
script's runtime layer. Do not re-provision persistent Pi configuration during
ordinary kernel deployment.

## Configuration

All settings have defaults matching the SEJT lab:

- `RETROOS_PXE_HOST=retroos-pi`
- `RETROOS_PXE_SEED=/usr/local/share/pxe-tftp-seed`
- `RETROOS_TFTP_ROOT=/srv/tftp`
- `RETROOS_REPO_ROOT`: optional repository-root override

## Safety

- Treat a request to test on D945GSEJT as authorization to update the ephemeral
  TFTP runtime through this script.
- Do not modify the persistent seed, networking, services, GPIO, target HDD, or
  board power without explicit authorization.
- Do not deploy `kernel_bare.elf`, an HDD image, or a non-Multiboot kernel.
- Preserve unrelated worktree changes.
- Deployment alone does not reset the target; use `reboot-retroos-sejt` when
  the running kernel supports it.
