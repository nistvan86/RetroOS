---
name: test-retroos-sejt
description: Build, deploy, and observe RetroOS changes on the Intel D945GSEJT through the Raspberry Pi PXE server and raw-Ethernet RLOG receiver. Use for any SEJT bare-metal test, hardware-driver validation, boot regression, crash diagnosis, or request to deploy/run a RetroOS kernel on the board; expect kernel evidence through Pi RLOG by default, not only for PXE-specific work.
---

# Test RetroOS on SEJT

Use `deploy-retroos-kernel` for atomic publication and
`reboot-retroos-sejt` when the running kernel supports remote reset.

## Workflow

1. Preserve unrelated worktree changes. Run tests appropriate to the change.
2. Build the ordinary kernel as a regression check and the RLOG-enabled kernel:

   ```bash
   bazelisk build //kernel:kernel_elf //kernel:kernel_elf_pxe_netlog
   ```

3. Inspect the Pi runtime, then deploy only
   `bazel-bin/kernel/kernel_pxe_netlog.elf`. Never deploy `kernel_bare.elf`.
   Report local and Pi checksums; publication alone does not prove execution.
4. Start the bundled listener in a long-running exec session:

   ```bash
   REPO_ROOT="${RETROOS_REPO_ROOT:-$(git rev-parse --show-toplevel)}"
   "$REPO_ROOT/.agents/skills/test-retroos-sejt/scripts/listen_rlog.sh"
   ```

5. Wait for `RLOG listener ready`. If the running kernel supports RCTL, use
   `reboot-retroos-sejt` and verify a new session automatically. Ask for a
   manual restart only when the board is off, halted before receive polling,
   or running an older kernel. Wait on the listener proactively.
6. Interpret session/sequence continuity, payload, and VGA together. Prefer the
   live SSH stream or read-only remote commands. Put copied captures under
   local `/tmp`, never the repository, unless explicitly requested otherwise.
7. Stop the listener cleanly unless continued monitoring was requested.

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
