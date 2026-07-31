# RetroOS D945GSEJT Hardware Harness Plan

## Purpose

Build a reliable development harness around the Intel D945GSEJT so Codex and
human developers can iteratively:

1. Build a RetroOS kernel.
2. Publish it to the Raspberry Pi PXE server.
3. Reset or power-control the target.
4. Observe boot progress over a dedicated serial connection.
5. Capture the DVI output when serial output is insufficient.
6. Preserve enough evidence to diagnose failures and compare runs.

The Raspberry Pi is the hardware proxy. Details of SSH, TFTP, GRUB, GPIO,
serial devices, and video capture should remain behind a stable harness
interface rather than being reproduced in prompts or issued as ad-hoc
commands.

See `retroos-inagy-baremetal-automation.md` for the initial hardware research
and proposed physical topology.

## Recommended architecture

```text
Codex or developer
        |
        | repository CLI initially; MCP tools later
        v
tools/lab on development machine
        |
        | SSH with connection multiplexing
        v
retroos-labd on Raspberry Pi
        |
        +-- atomic PXE artifact publication
        +-- GRUB next-boot selection
        +-- serial capture
        +-- isolated reset/power control
        +-- DVI/USB video capture
        +-- run locking and evidence storage
        |
        v
Intel D945GSEJT target
```

The first implementation need not have a resident `retroos-labd`. A
non-interactive Pi-side `labctl` command can provide the same interface while
the protocol and workflows stabilize.

## Implemented lightweight PXE path

An alternative to the proposed `tools/lab`/`retroos-labd` stack is now working
for the current single-board development loop. It uses repository-level Codex
skills, SSH, the Pi's existing PXE services, and the PXE firmware retained by
RetroOS:

```text
Codex or developer
        |
        | .agents/skills
        v
deploy-retroos-kernel
        |  atomic SSH publication into RAM-backed TFTP
        v
Raspberry Pi 4  <------ timestamped RLOG raw-Ethernet frames
        |                                         ^
        | DHCP/TFTP/GRUB                           |
        v                                         |
Intel D945GSEJT ---- RCTL raw-Ethernet reboot ----+
```

The implemented pieces are:

- `.agents/skills/deploy-retroos-kernel`: validates a Multiboot i386 ELF,
  reconstructs the ephemeral TFTP runtime from the persistent seed, publishes
  the kernel and GRUB configuration atomically, verifies checksums, and retains
  the previous kernel;
- `.agents/skills/test-retroos-sejt`: builds and deploys the RLOG-enabled
  kernel, starts the Pi-side raw-packet listener, and treats the timestamped
  session/sequence stream as the default hardware evidence channel;
- `.agents/skills/reboot-retroos-sejt`: sends one dedicated broadcast RCTL
  frame from the Pi and verifies reset by waiting for a new RLOG session;
- `tools/pxe_rlog_receiver.py`: receives EtherType `88B5` frames without an IP
  stack and records timestamped kernel output;
- `tools/pxe_rlog_reboot.py`: emits the link-local reboot request used by the
  skill.

The D945GSEJT's Intel PXE 2.1 build 082 UNDI runtime is reinitialized by
RetroOS. VGA logging remains active in parallel. UNDI completion/receive
polling, the protected-mode firmware bridge, guest low-memory restoration, and
PXE BIOS-vector isolation are implemented sufficiently for DN to run while
RLOG and remote reboot remain available.

This path intentionally does not implement the full harness architecture. It
has no serial adapter, GPIO reset/power control, DVI capture, Pi-side exclusive
run lock, structured evidence directory, known-good boot selector, resident
daemon, or MCP server. The raw reboot command is unauthenticated and suitable
only for the closed Pi-to-SEJT Ethernet link. A kernel that halts before UNDI
receive polling still requires a manual reset or future external reset
hardware.

The lightweight path is the current default for kernel iteration. The larger
harness below remains the plan when stronger recovery, concurrency, and visual
evidence are required.

## Deferred SEJT native-VGA OSD investigation

Hardware session `0e7189f0` confirmed that F12 can suspend DN, switch the
physical VGA card to the fixed 320x200x8 Mode 13h OSD surface, display a
readable and functional menu, close it, and restore DN. The initial completely
blank OSD was caused by `prepare_native_osd` omitting the Mode 13h Attribute
Controller state; setting the identity palette, graphics/256-colour mode, and
colour-plane enable made the OSD itself work.

The guest preview behind the OSD remains completely corrupted on the
D945GSEJT: colours are wrong and screen elements repeat. This is not expected
scaling loss. The menu drawn over the preview is correct, and the guest is
restored correctly afterward, which isolates the defect to capture or
reconstruction of the suspended native text framebuffer before it is rendered
into the OSD background.

Defer this work. When resumed:

1. Instrument the F12 save path with the detected VGA mode, AC palette and
   control registers, CRTC start/stride state, and checksums/samples of saved
   planes 0 and 1.
2. Verify the Intel VGA's odd/even text-memory layout when temporarily read
   through the flat A0000 aperture; do not assume the resulting per-plane
   offsets match the emulated canonical layout.
3. Compare captured character/attribute cells with direct B8000 samples before
   switching to the OSD mode.
4. Correct the native-text capture conversion and verify text, planar graphics,
   Mode X, and Mode 13h backgrounds independently.
5. If accurate capture cannot be made safe for a mode, use a plain OSD
   background for that mode rather than displaying corrupted guest content.

## Codex integration strategy

### Repository rules

Add an `AGENTS.md` after the harness commands and safety rules are sufficiently
stable. It should document:

- the canonical build and hardware-test commands;
- the expected build artifacts;
- safe operations Codex may perform without confirmation;
- operations that require explicit confirmation;
- boot success markers and standard timeouts;
- evidence locations and retention expectations;
- the rule that hardware test runs must be serialized;
- recovery procedures for a hung or unreachable target;
- the prohibition on ad-hoc Pi configuration changes during normal testing.

`AGENTS.md` should explain policy and workflow. It should not contain
credentials, private keys, fragile shell pipelines, or low-level GPIO
sequences.

### Command-line interface first

Implement a small repository-owned CLI before creating an MCP server. The CLI
is easy to run manually, easy to test, and establishes the eventual MCP
server's underlying domain interface.

Tentative commands:

```text
tools/lab status
tools/lab deploy <kernel-elf>
tools/lab boot --target current|known-good|maintenance
tools/lab reset
tools/lab power on|off|cycle
tools/lab serial capture --timeout <seconds>
tools/lab screenshot --output <path>
tools/lab test boot-smoke [--kernel <kernel-elf>]
```

Normal commands should:

- be non-interactive;
- have explicit, finite timeouts;
- use meaningful exit statuses;
- emit a stable JSON result, with optional concise human-readable output;
- be safe to retry where practical;
- identify the run ID on every state-changing operation;
- never expose secrets in command output or stored evidence.

### MCP server later

Add a local MCP server when the CLI and hardware protocol are stable, or
earlier if typed image results and long-lived serial operations become a
development bottleneck.

Likely MCP tools:

```text
get_lab_status()
deploy_kernel(artifact, checksum)
select_boot_target(target)
reset_target()
power_target(action)
capture_serial(timeout_seconds, stop_patterns)
capture_frame()
run_boot_test(test_case, artifact)
get_run_evidence(run_id)
```

The MCP server should call the same library or Pi-side API as `tools/lab`.
It must not become a second, behaviorally different implementation.

MCP is useful here for typed arguments and results, direct screenshot return,
central authorization, and structured run state. It must still tolerate
server restarts, dropped SSH connections, and disconnected hardware.

## SSH connection handling

Do not depend on Codex retaining an interactive SSH process between commands
or turns. Each harness operation must be independently invocable and capable
of reconnecting.

Use OpenSSH multiplexing to avoid repeated connection setup:

```sshconfig
Host retroos-lab
    HostName <pi-address>
    User <lab-user>
    BatchMode yes
    ControlMaster auto
    ControlPersist 15m
    ControlPath /tmp/retroos-ssh-%C
    ServerAliveInterval 15
    ServerAliveCountMax 3
```

The exact host and user belong in developer-local SSH configuration, not in
tracked secrets. The harness should allow the SSH host alias to be selected
through a non-secret configuration value.

A future resident Pi daemon may keep serial and video devices open for
efficiency, but its public operations must remain reconnectable and
bounded by timeouts.

## Safety model

Classify operations before allowing autonomous use.

### Read-only

- Query Pi, target, serial, video, PXE, and lock status.
- Read retained logs and screenshots.
- Capture a screenshot.
- Observe serial output without changing boot state.

### Routine development mutations

- Atomically publish a kernel to the designated `current` PXE slot.
- Select an approved GRUB entry.
- Pulse reset.
- Run a defined test that combines deployment, reset, and capture.

These may eventually be allowed without confirmation when the electrical
interface, locking, recovery, and known-good boot path have been validated.

### Confirmation required

- Change DHCP, networking, GRUB, TFTP, SSH, device, or GPIO configuration.
- Overwrite or remove the known-good kernel.
- Write the D945GSEJT HDD outside an explicit maintenance workflow.
- Hold the power switch or force a power cycle.
- Delete retained evidence.
- Operate hardware other than the designated RetroOS target.

All reset and power outputs must use appropriate electrical isolation. The Pi
must not drive motherboard switch contacts directly.

## Run and evidence model

Every combined hardware test gets a unique run ID. Suggested layout:

```text
artifacts/hardware/<run-id>/
|-- request.json
|-- result.json
|-- build.json
|-- kernel.elf.sha256
|-- serial.log
|-- boot.png
|-- result.png
`-- capture.mp4
```

`request.json` records the requested operation and non-secret parameters.
`build.json` records the Git commit, dirty-worktree state, Bazel target,
artifact path, size, and checksum. `result.json` records timestamps, state
transitions, matched serial markers, timeout or failure reason, and paths to
captured evidence.

Large kernel binaries need not be copied into every run directory if their
checksum and immutable published location are recorded.

## Concurrency and state

There is one physical target, so all state-changing workflows require an
exclusive Pi-side lock. The lock should cover the complete operation, not
individual steps:

```text
acquire lock
publish artifact
start capture
reset target
wait for result
save evidence
stop capture
release lock
```

Status and evidence reads may occur while a run is active. A lock record
should contain the run ID, owner, operation, start time, and expiry or lease
information. Recovery from an abandoned lock must be explicit and audited.

Model the target using observable states rather than assuming a reset worked:

```text
unknown -> powered-off -> firmware -> PXE/GRUB -> kernel -> ready
                                      \-> maintenance
                         \-> timeout/failure
```

Serial markers, video presence, power LED input, and elapsed time may all
contribute evidence. They should not silently contradict one another.

## Phased implementation

The checklists below describe the full harness, not the lightweight PXE path
above. Items satisfied only by that alternative remain documented separately
so its limitations are not mistaken for completion of the daemon/CLI design.

### Phase 0: Record the actual lab configuration

- [ ] Record the Pi OS and management connection.
- [ ] Record the isolated PXE network layout and addresses.
- [ ] Verify the GRUB/TFTP paths and current manual deployment procedure.
- [ ] Confirm D945GSEJT reset and power header pinout.
- [ ] Document the isolation circuit before enabling GPIO control.
- [ ] Identify the COM header, adapter, voltage levels, device path, and baud
      rate.
- [ ] Identify the video capture device, supported modes, and stable format.
- [ ] Establish a separately named, protected known-good kernel.

Deliverable: a local lab configuration template with secrets and machine-local
values excluded from Git.

### Phase 1: PXE deployment CLI

- [ ] Create `tools/lab` with configuration discovery and `status`.
- [ ] Upload kernels to a staging filename.
- [ ] Validate ELF format, architecture, size, and checksum on the Pi.
- [ ] Atomically promote staging to the current PXE slot.
- [ ] Record build metadata and retain the prior current image.
- [ ] Implement boot-target selection without risking the known-good entry.
- [ ] Enable SSH multiplexing and reconnection.

Deliverable: repeatable deployment with no GPIO, serial, or video dependency.

### Phase 2: Reset and serial observation

- [ ] Implement a Pi-side exclusive hardware lock.
- [ ] Add isolated reset pulse control with conservative timing.
- [ ] Add power-state observation if hardware permits.
- [ ] Add timestamped COM capture.
- [ ] Add configurable boot markers such as kernel entry and RetroOS ready.
- [ ] Return a structured timeout result while preserving partial output.
- [ ] Implement the atomic `test boot-smoke` workflow.

Deliverable: one command deploys, resets, captures output, and reports boot
success or a diagnosable failure.

### Phase 3: Video evidence

- [ ] Lock the capture format and resolution.
- [ ] Capture still frames without disturbing continuous capture.
- [ ] Detect no-signal, blank-frame, and video-mode-change conditions.
- [ ] Attach screenshots to the same run as serial evidence.
- [ ] Define when OCR or visual comparison is appropriate.
- [ ] Avoid brittle exact-pixel assertions unless the signal path is stable.

Deliverable: a boot test can return both structured serial results and DVI
screenshots.

### Phase 4: Recovery and maintenance

- [ ] Add guarded power-button and power-cycle operations.
- [ ] Define escalation from reset to forced power recovery.
- [ ] Add maintenance-Linux boot selection for controlled HDD updates.
- [ ] Ensure RetroOS and maintenance Linux never mount the HDD writable at the
      same time.
- [ ] Add retention and cleanup policy for evidence and published artifacts.

Deliverable: unattended tests recover from common hangs without risking the
known-good boot path or HDD contents.

### Phase 5: Codex-facing MCP server

- [ ] Extract shared harness logic from the CLI if necessary.
- [ ] Define narrow, typed tools with explicit timeouts and result schemas.
- [ ] Return screenshots as image content as well as retained evidence.
- [ ] Separate read-only, routine mutation, and privileged configuration
      permissions.
- [ ] Test server restart and SSH/device reconnection during failed runs.
- [ ] Keep `tools/lab` as a human-operable diagnostic and fallback interface.
- [ ] Add the stable workflow and safety rules to `AGENTS.md`.

Deliverable: Codex can execute an evidence-producing kernel iteration without
needing knowledge of SSH, GPIO, serial, or capture-device details.

## Initial smoke-test contract

The first automated test should remain deliberately small:

```text
Input:
  kernel ELF
  expected build identifier or checksum
  overall timeout

Actions:
  validate and publish kernel
  start serial capture
  pulse reset
  observe PXE/GRUB and kernel boot
  wait for a unique RetroOS-ready marker
  take a screenshot
  persist evidence

Success:
  published checksum matches the input artifact
  ready marker belongs to the requested build
  marker arrives within the timeout

Failure:
  validation error, lock conflict, reset failure, serial/device error,
  unexpected build identifier, boot panic marker, or timeout
```

The ready marker should contain a machine-readable build identifier so a
stale PXE artifact cannot be mistaken for a successful deployment.

## Design questions to resolve

- Where should the authoritative lab configuration live, and which fields may
  be committed?
- Should `tools/lab` be shell, Python, Rust, or another language already
  supported by the repository?
- Should the Pi initially expose SSH commands only, or a small authenticated
  local service?
- Which serial port remains available for HostFS, and which carries kernel
  diagnostics?
- What exact marker format should identify boot phase, build, test, and
  pass/fail state?
- How should a test request pass kernel command-line arguments through GRUB?
- What is the safe maximum reset pulse, power-button pulse, and boot timeout?
- Which evidence should be retained by default, and for how long?
- Should video recording be continuous per run or started only after a serial
  trigger?
- Where will the MCP server run: on the development host or on the Pi?

## Completion criteria

The harness is ready for routine Codex-driven iteration when:

- a single documented command builds or accepts a kernel artifact, publishes
  it, boots the target, and returns bounded structured results;
- the reported build identity proves that the requested kernel ran;
- every failure retains serial output and, when available, a screenshot;
- simultaneous state-changing runs are prevented;
- dropped SSH sessions and restarted client processes do not corrupt state;
- the known-good PXE target cannot be overwritten through the routine API;
- privileged configuration and destructive recovery remain separately gated;
- the same underlying behavior is usable manually through the CLI and
  programmatically through MCP.
