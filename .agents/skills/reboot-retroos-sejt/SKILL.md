---
name: reboot-retroos-sejt
description: Remotely reboot the Intel D945GSEJT running an RCTL-capable RetroOS kernel by sending its private raw-Ethernet command from the Raspberry Pi, then verify a fresh PXE boot through a new timestamped RLOG session. Use when a SEJT hardware test needs a restart after deployment or while iterating without asking the user to press reset.
---

# Reboot RetroOS SEJT

Use this only for the dedicated SEJT test board. The user has authorized remote
reboots during RetroOS hardware iteration.

## Workflow

1. Ensure the Pi RLOG listener is active and has printed `RLOG listener ready`.
2. Confirm the running kernel is RCTL-capable. A live RLOG session from the
   receive-enabled `kernel_pxe_netlog.elf` is sufficient. If the board is off,
   halted in an older probe, or failed before RLOG, request one manual reboot
   after deploying a capable kernel.
3. Record the current RLOG session ID and timestamp.
4. Send exactly one reboot frame:

   ```bash
   REPO_ROOT="${RETROOS_REPO_ROOT:-$(git rev-parse --show-toplevel)}"
   "$REPO_ROOT/.agents/skills/reboot-retroos-sejt/scripts/reboot_sejt.sh"
   ```

5. Wait up to 90 seconds for a different RLOG session ID at sequence zero.
6. Report send time, old/new sessions, and the first meaningful boot result.

## Configuration

- `RETROOS_PXE_HOST=retroos-pi`
- `RETROOS_PXE_INTERFACE=eth0`
- `RETROOS_REPO_ROOT`: optional repository-root override

## Safety

- Do not reboot while the user is interactively preserving state.
- Never retry automatically after a timeout; inspect VGA/PXE state or ask.
- Distinguish preceding manual boots using the exact send timestamp and session.
- The unauthenticated raw `88B5` frame is safe only on the closed Pi-SEJT link.
