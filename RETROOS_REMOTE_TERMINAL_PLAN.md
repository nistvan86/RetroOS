# RetroOS PXE Remote Terminal Plan

## Objective

Turn the working PXE/UNDI raw-Ethernet link between the Intel D945GSEJT and
Raspberry Pi 4 into a small bidirectional hardware-control interface and a
transport suitable for the existing HostFS protocol. Preserve simultaneous VGA
logging while adding remote log following, reboot, keyboard input, managed
executable launch, and eventually Pi-backed files without IPv4 or UDP.

## Current baseline

- The PXE-netlog kernel initializes retained Intel PXE 2.1 build 082 UNDI after
  interrupt-controller initialization.
- RetroOS broadcasts timestampable RLOG payloads through EtherType `0x88B5`.
- The Pi receiver writes payload bytes unbuffered to
  `/tmp/retroos-rlog-current.log`.
- RetroOS polls UNDI from its event loop and accepts the private
  `RCTL 01 01 REBOOT` frame.
- The Pi currently uses separate Python entry points for listening and reboot.
- QEMU command execution uses boot-time `fw_cfg`; it does not simulate typing.

## Proposed architecture

```text
retroctl CLI / Codex skills
           |
           | Unix-domain socket
           v
retroos-harness daemon on Pi
  - receive and persist RLOG
  - send RCTL commands
  - serve packetized HostFS requests
  - track sessions, command IDs, and acknowledgements
           |
           | raw Ethernet, EtherType 0x88B5
           v
RetroOS PXE/UNDI transport
  - RLOG transmit
  - RCTL receive
  - RFS request/response
  - bounded command queue
  - console and startup integration
```

The daemon owns the Pi raw socket. A small `retroctl` client provides commands
such as `follow`, `status`, `reboot`, `key`, `type`, and `exec`. The same daemon
can later dispatch RFS messages to the existing HostFS server logic. Codex
skills call this client over SSH rather than starting unrelated Python
processes.

## Protocol direction

- Keep `RLOG` for target-to-Pi output.
- Generalize `RCTL` for Pi-to-target requests.
- Reserve `RFS` for packetized HostFS requests and responses.
- Give every state-changing request a Pi-generated command ID.
- Return acknowledgement, rejection, start, and completion events over RLOG.
- Reject malformed versions, unknown commands, oversized payloads, and
  duplicate command IDs.
- Retain broadcast Ethernet initially; optionally learn and use the SEJT MAC
  after the first RLOG frame.
- Treat the directly connected cable as the trust boundary. Do not imply
  cryptographic authentication.

The shared transport should be a bounded stop-and-wait request/response layer,
not a terminal-specific protocol. Only one reliable request needs to be
outstanding initially. A request ID, cached last response, timeout, and retry
make state-changing operations duplicate-safe. This is enough for both RCTL and
the existing synchronous HostFS design; no sliding window is required.

Suggested initial request types:

- `REBOOT`: warm reboot, preserving the current behavior.
- `KEY`: one named key with press/release semantics.
- `TEXT`: bounded UTF-8/ASCII text translated into console input.
- `EXEC`: bounded command line queued for managed execution.
- `PING`: request target/session status without changing state.

## Stage 1: Consolidate the Pi harness

- Extract shared RLOG/RCTL framing into one Python module.
- Create a long-running `retroos-harness` daemon that owns the AF_PACKET socket.
- Preserve timestamped stdout and unbuffered `/tmp` log output.
- Add a Unix-domain control socket under `/run` or `/tmp` with explicit
  permissions and cleanup of stale sockets.
- Add a `retroctl` client with `follow`, `status`, and `reboot`.
- Keep the existing scripts as compatibility wrappers until skills migrate.
- Parameterize interface, EtherType, log path, and socket path.
- Structure command dispatch so RCTL and future RFS handlers share the raw
  socket, session tracking, framing, and retry machinery.

Acceptance:

- Existing RLOG sessions remain gap-free.
- `retroctl reboot` produces exactly one reboot and observes a new session.
- Multiple `follow` clients cannot consume or corrupt one another's output.

## Stage 2: Shared reliable message envelope

- Define a compact, endian-explicit envelope with protocol family, version,
  message type, request ID, payload length, fragment metadata, and flags.
- Support `RCTL` immediately while reserving stable family/type space for RFS.
- Add strict bounded parsing in the ring-0 UNDI receive path.
- Add duplicate suppression with a small fixed-size recent-command table.
- Cache the last completed response so retrying a state-changing request never
  repeats its side effect.
- Replace the current 64-slot never-reused transmit pool with bounded reusable
  buffers reclaimed through UNDI transmit completion.
- Define a stop-and-wait timeout/retry policy and a maximum payload that fits
  safely inside one Ethernet frame.
- Emit RLOG control events for accepted, rejected, and duplicate requests.
- Add `PING` so the daemon can distinguish a live target from a healthy Pi
  listener with no target traffic.
- Document behavior across reboot/session changes and 32-bit ID wraparound.

Acceptance:

- Re-sending a command ID never performs the action twice.
- Bad length, version, and command values are rejected without affecting boot.
- Pi output shows request ID and target acknowledgement together.
- Logging and control continue beyond 64 transmitted frames.
- Lost request or response tests recover through bounded retry.

## Stage 3: Remote keyboard and text

- Add a bounded kernel input queue owned above the UNDI transport layer.
- Translate `TEXT` into the existing console input representation.
- Define stable named keys for Enter, Escape, arrows, function keys, modifiers,
  Backspace, Tab, and common control combinations.
- Feed remote keys through the same console/focus path as physical PS/2 input,
  rather than writing directly into a guest keyboard buffer.
- Specify queue-full behavior and report dropped/rejected input.
- Add `retroctl key ...` and `retroctl type ...`.

Acceptance:

- Typed text reaches whichever application owns console focus.
- DN and COMMAND.COM respond to named keys like their physical equivalents.
- Physical and remote keyboards continue to work together.
- QEMU tests validate translation and queue behavior without PXE firmware;
  the SEJT validates the complete UNDI receive path.

## Stage 4: Managed executable launch

- Reuse the existing `run_program()` parser and loader used by the QEMU
  `fw_cfg` command path.
- Copy each accepted command into a bounded kernel-owned queue; never retain a
  pointer into the UNDI receive buffer.
- Add an explicit startup-loop control action that asks the active DN session
  to exit at a safe scheduling boundary.
- Run the requested executable with defined CWD and command-tail semantics.
- Emit `EXEC_ACCEPTED`, `EXEC_STARTED`, and `EXEC_EXITED(status)` events.
- Restart DN after completion unless the request explicitly selects another
  documented policy.
- Initially allow only one queued/running EXEC request.

Acceptance:

- `retroctl exec "C:\\BOOT\\TEST.EXE arg"` interrupts DN cleanly, runs once,
  reports the exit status, and returns to DN.
- Missing executables and loader failures are reported without reboot loops.
- Duplicate packets cannot launch duplicate processes.

## Stage 5: Automation and skill migration

- Update the deploy/test/reboot project skills to use `retroctl` over SSH.
- Add helpers to wait for a session, text pattern, command acknowledgement, or
  executable exit status with explicit timeouts.
- Keep Pi-side temporary logs and copied artifacts outside the repository.
- Add a single provisioning path for daemon installation/configuration.
- Add a status command reporting Pi interface, daemon uptime, last target MAC,
  current RLOG session, last sequence, and pending command.

Acceptance:

- A hardware test can deploy, reboot, wait for RLOG, run a command, collect its
  result, and return to DN without manual keyboard or reset interaction.
- Existing manual VGA and physical-keyboard workflows remain unchanged.

## Stage 6: PXE HostFS transport

The existing HostFS protocol is intentionally synchronous and simple: one
OPEN, READ, CLOSE, STAT, READDIR, CREATE, or WRITE request is completed before
the next begins. File reads and writes already carry explicit offsets. Preserve
those semantics rather than redesigning the filesystem protocol.

- Add an RFS family over the Stage 2 stop-and-wait envelope.
- Reuse the path resolution, handle table, and operation implementations from
  `hostfs.py`; separate them from its Unix-stream parsing where necessary.
- Add a RetroOS HostFS transport backend beside the existing COM1 backend.
- Map each existing HostFS operation to one reliable RFS request/response.
- Keep individual wire messages within the selected Ethernet payload bound.
- Split larger VFS reads and writes into sequential offset-based chunks; avoid
  general multi-frame reassembly where ordinary repeated READ/WRITE operations
  suffice.
- Use cached request responses so retrying CREATE or WRITE cannot apply the
  operation twice.
- Define mount discovery/configuration without probing a nonexistent serial
  CTS/DSR peer.
- Restrict the Pi server to a configured root and retain case-insensitive guest
  path resolution and escape prevention.
- Keep one outstanding filesystem request initially; do not add pipelining or
  a sliding window unless measurements later justify it.

Acceptance:

- The SEJT can mount a Pi directory and perform OPEN, READ, STAT, READDIR,
  CREATE, WRITE, and CLOSE through retained UNDI.
- Executables can be loaded from the remote mount and test output can be saved
  back to it.
- A dropped request or response does not corrupt offsets or duplicate writes.
- Transfers remain functional after substantially more than 64 Ethernet sends.
- Existing COM1 and hosted injected HostFS backends continue to work.

## Validation strategy

### Hosted/unit tests

- Encode/decode every RCTL message and malformed boundary case.
- Test command-ID duplicate suppression and wraparound.
- Test stop-and-wait retry, cached responses, buffer reuse, and timeout limits.
- Test text-to-key translation, modifiers, and queue-full behavior.
- Test EXEC state transitions independently of UNDI.
- Run the existing HostFS operation suite against an in-memory RFS transport,
  including chunk boundaries and duplicate CREATE/WRITE requests.

### QEMU

- Exercise the kernel command queue and console injection through a test-only
  transport or direct harness hook.
- Compare remote-key behavior with QEMU monitor/QMP keyboard injection.
- Verify managed EXEC uses the same loader semantics as `fw_cfg opt/cmdline`.
- Exercise the packetized HostFS backend through a test transport independently
  of real PXE firmware.
- QEMU cannot by itself prove compatibility with the retained Intel UNDI ROM.

### D945GSEJT hardware

- Validate every stage with simultaneous VGA and timestamped Pi RLOG.
- Confirm sequence-zero session markers and command acknowledgements.
- Test physical and remote keyboard input concurrently.
- Test repeated EXEC, missing program, crash, queue-full, and reboot recovery.
- Test remote mount reads/writes, executable loading, retry injection, and long
  transfers after the terminal stages are stable.
- Preserve one-command-at-a-time testing until acknowledgement is observed.

## Safety and design constraints

- UNDI calls stay serialized and non-reentrant.
- Firmware calls remain at ring 0; parsing and policy should be kept outside
  the lowest-level transport where practical.
- No dynamic allocation in the ring-0 receive parser.
- All packet lengths, text lengths, and queue counts are bounded before copy.
- Reliable messages initially use one outstanding request and bounded retries;
  no unbounded queues or retransmission loops.
- Reusable UNDI transmit buffers must not be recycled until firmware reports
  transmit completion.
- Panic-time RLOG remains best effort; never recursively enter PXE after a
  failure inside a firmware call.
- Remote control must not remove simultaneous VGA, physical keyboard, RAM klog,
  or debugcon output.
- Do not add IPv4, UDP, DHCP, or a general network stack for this feature.

## Recommended next action

Implement Stage 1 without changing the on-wire reboot payload yet. This gives
one stable Pi process and CLI while preserving the already validated kernel
behavior. Design the Stage 2 envelope and reusable transmit-buffer lifecycle for
both RCTL and RFS before adding new state-changing commands; this avoids a later
HostFS-driven transport refactor.
