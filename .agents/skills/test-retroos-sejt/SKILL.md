---
name: test-retroos-sejt
description: Build, deploy, and observe RetroOS changes on the Intel D945GSEJT through the Raspberry Pi PXE server and raw-Ethernet RLOG receiver. Use for any SEJT bare-metal test, hardware-driver validation, boot regression, crash diagnosis, or request to deploy/run a RetroOS kernel on the board; expect kernel evidence through Pi RLOG by default, not only for PXE-specific work.
---

# Test RetroOS on SEJT

Use `deploy-retroos-kernel` for atomic publication and
`reboot-retroos-sejt` when the running kernel supports remote reset.

## Workflow

Use the bundled iteration script by default. With no argument it installs the
normal native VGA/BIOS GRUB entry. Pass `--exec <embedded-vfs-path>` only when
the current test must start a specific embedded diagnostic:

```bash
REPO_ROOT="${RETROOS_REPO_ROOT:-$(git rev-parse --show-toplevel)}"
"$REPO_ROOT/.agents/skills/test-retroos-sejt/scripts/iterate.sh"
"$REPO_ROOT/.agents/skills/test-retroos-sejt/scripts/iterate.sh" \
    --exec boot/TESTS/VBELFB.EXE
```

For a derived kernel, override both its Bazel label and output artifact while
retaining the same deploy/reboot/listen transaction:

```bash
"$REPO_ROOT/.agents/skills/test-retroos-sejt/scripts/iterate.sh" \
    --target //kernel:kernel_elf_pxe_netlog_duke \
    --artifact bazel-bin/kernel/kernel_pxe_netlog_duke.elf
```

The script runs the ordinary and PXE/RLOG builds, inspects the Pi runtime,
atomically deploys `kernel_pxe_netlog.elf`, configures the selected live GRUB
entry, starts RLOG, waits for `RLOG listener ready`, sends exactly one RCTL
reboot, and remains attached to the new boot log. Run it as one long-running
agent command, wait on it proactively, and report the conclusion from each
hardware iteration. Stop it with Ctrl-C after capturing the needed evidence.

The `--exec` configuration changes only the live tmpfs GRUB file. It never
changes the persistent seed, and the next invocation without `--exec` restores
the normal entry during deployment.

Use the individual build, deploy, listener, and reboot helpers only to isolate
a failure in one stage, to continue an already-running listener, or when the
board is off, halted before receive polling, or running a kernel without RCTL.
In those cases, start RLOG before requesting one manual reboot.

Interpret session/sequence continuity, payload, and VGA together. Prefer the
live SSH stream or read-only remote commands. Put copied captures under local
`/tmp`, never the repository, unless explicitly requested otherwise.
Stop the listener cleanly unless continued monitoring was requested.

## Configuration

- `RETROOS_PXE_HOST=retroos-pi`
- `RETROOS_PXE_INTERFACE=eth0`
- `RETROOS_REPO_ROOT`: optional repository-root override

## Interpretation

- RLOG begins during early ring-0 boot immediately after UNDI setup; VGA
  remains simultaneous. The expected first line is `PXE netlog ready: ...`.
- No screen indicates failure before the observable kernel path.
- VGA without RLOG can indicate PXE initialization or transmit failure.
- Sequence gaps indicate missing frames; distinct sessions indicate reboots.
- RLOG is bounded and best-effort; never let it obscure the hardware result.

## Safety

- Do not automatically roll back an experimental kernel after a boot loop.
- Deployment does not authorize changing the Pi's persistent seed, networking,
  services, GPIO, the SEJT disk, or board power.
- Intel PXE 2.1 build 082 and EtherType `88B5` are board-specific test details,
  not a portability claim.
