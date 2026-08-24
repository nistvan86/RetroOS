//! Console input routing — drained host/hardware events to their owners.
//!
//! ONE place decides where an input event goes:
//! - The F12 host monitor is intercepted first: F12 opens an on-screen menu
//!   (kill / switch / volume / trace / profile / dump), and while it is open every key
//!   drives the menu instead of the guest. See [`crate::kernel::osd`].
//! - Everything else is addressed to the console owner — `focus::focused()`,
//!   which today is also the running thread (focus implies execution until
//!   the scheduler decouples them; this router is written against the
//!   owner, so that change won't touch the routing).
//! - Keys to a *blocked* DOS owner feed the console stdin pipe (the Linux
//!   shell the DOS thread is wait4-blocked on reads them from fd 0).
//! - Keys to a running DOS owner go through its BIOS keyboard path; other
//!   IRQs (mouse packets) queue into its per-thread virtual devices.
//! - Linux owners get keys cooked into their fds; they have no virtual
//!   device bus for other IRQs.

pub mod coordinator;
pub mod dos;
pub mod kernel;
pub mod polling;
pub mod protocol;
pub mod serial;
pub mod serial_log;
pub mod session;
pub mod stream;

use crate::Regs;
use crate::kernel::thread;

/// F12 scancode (press) — open the host monitor.
pub const F12_PRESS: u8 = 0x58;

/// Route already-drained input/guest events into the console owner. Kernel
/// device IRQs were consumed earlier by `irq_dispatch`.
pub fn dispatch<A: crate::Arch>(
    machine: &mut A,
    bios_workspace: &mut crate::kernel::bios_display::BiosDisplayWorkspace<A>,
    regs: &mut Regs,
    kt: &mut thread::KernelThread<A>,
    personality: &mut thread::Personality<A>,
    display: &mut Option<crate::kernel::display::Display>,
    coordinator: &mut coordinator::ConsoleCoordinator,
    events: alloc::vec::Vec<crate::Irq>,
) {
    let blocked = kt.state == thread::ThreadState::Blocked;
    for event in events {
        match event {
            crate::Irq::Key(scancode) => {
                if monitor_key(machine, &mut *bios_workspace, regs, scancode, personality, display) {
                    continue;
                }
                let _ = coordinator.deliver_personality(
                    machine,
                    regs,
                    kt,
                    personality,
                    crate::kernel::console_session::InputEvent::Scancode(scancode),
                );
            }
            event if matches!(personality, thread::Personality::Dos(_)) && !blocked => {
                if let thread::Personality::Dos(dos) = personality {
                    crate::kernel::dos::queue_irq(machine, dos, regs, event);
                }
            }
            _ => {}
        }
    }
}

/// The single host-monitor gate, tried before any owner sees the key. Returns
/// true when the key was consumed (never reaches the guest).
///
/// One door, not one chord per action: while the monitor is open it eats every
/// key; when closed, only F12 (opening it) is special.
fn monitor_key<A: crate::Arch>(
    machine: &mut A,
    bios_workspace: &mut crate::kernel::bios_display::BiosDisplayWorkspace<A>,
    regs: &mut Regs,
    sc: u8,
    personality: &mut thread::Personality<A>,
    display: &mut Option<crate::kernel::display::Display>,
) -> bool {
    if crate::kernel::osd::is_open() {
        let dos = match &*personality {
            thread::Personality::Dos(dos) => Some(&**dos),
            thread::Personality::Linux(_) | thread::Personality::Os2(_) | thread::Personality::Windows(_) => None,
        };
        crate::kernel::osd::key(machine, regs, sc, dos);
        if !crate::kernel::osd::is_open() {
            restore_from_monitor(machine, &mut *bios_workspace, personality, display);
        } else {
            personality.repaint_osd();
        }
        return true;
    }
    if sc == F12_PRESS {
        if display.is_none() {
            let handoff = personality.release_display(machine, bios_workspace);
            *display = Some(handoff.into_surface(machine, bios_workspace));
        }
        crate::kernel::osd::open();
        personality.repaint_osd();
        return true;
    }
    false
}

/// Return the monitor-held display to its focused personality. This is shared
/// by an ordinary Esc/F12 close and forced dismissal before owner teardown.
pub fn restore_from_monitor<A: crate::Arch>(
    machine: &mut A,
    bios_workspace: &mut crate::kernel::bios_display::BiosDisplayWorkspace<A>,
    personality: &mut thread::Personality<A>,
    display: &mut Option<crate::kernel::display::Display>,
) {
    if matches!(personality, thread::Personality::Dos(_)) {
        let display = display.take().expect("closing OSD without compositor display");
        let handoff = crate::kernel::display::DisplayHandoff::from_surface(display, machine);
        personality.acquire_display_restore(
            machine, bios_workspace, handoff,
        );
    }
}


// =============================================================================
// The console role: holding the display
// =============================================================================

/// The console: the machine's own display, held.
///
/// Holding this value *is* the licence to draw kernel text — not by a rule
/// written down somewhere, but because the display token is in here and there
/// is no other way to reach it. There used to be a separate `Screen` token
/// alongside the display for exactly this purpose; it enforced nothing,
/// because the terminal it claimed to guard was reachable through a public
/// global, and its own first user was the bootloader issuing itself a licence
/// while alone on the machine.
///
/// There is deliberately no type for a console that is not holding the
/// display. A console that gave up its display is not a suspended console, it
/// is a destructed one — so [`Console::release`] consumes it and hands the
/// pieces back, and the absence of the value is the suspended state.
pub struct Console {
    display: crate::kernel::display::Display,
}

/// What a released console leaves behind: the *card's* state, not the
/// console's. A real VGA is handed to the next owner mid-mode, so its register
/// file and planes are snapshotted for whoever takes the display back. The
/// text itself needs none of this — the terminal owns its grid and can simply
/// be redrawn.
pub struct SuspendedCard;

impl Console {
    pub fn new(display: crate::kernel::display::Display) -> Self {
        Self { display }
    }

    /// Publish pending terminal cells immediately. Startup has no event loop
    /// yet, so leaving the console merely dirty can keep every boot message in
    /// shadow RAM until the display is handed to the first DOS program.
    pub fn present<A: crate::Arch>(
        &mut self,
        machine: &mut A,
        bios: &mut crate::kernel::bios_display::BiosDisplayWorkspace<A>,
    ) {
        crate::kernel::term::present(machine, bios, &mut self.display);
    }

    pub fn bios_display(&self) -> Option<&crate::kernel::platform::VgaCap> {
        self.display.vga_capability()
    }

    /// Give up the display. The console ceases to exist; whoever takes the
    /// token can now drive the screen, and nothing left in the kernel can
    /// print over them.
    pub fn release<A: crate::Arch>(
        mut self,
        machine: &mut A,
        bios: &mut crate::kernel::bios_display::BiosDisplayWorkspace<A>,
    ) -> (SuspendedCard, crate::kernel::display::Display) {
        self.present(machine, bios);
        (SuspendedCard, self.display)
    }

    /// Take the display back and become a console again, restoring the card to
    /// the state the previous owner found it in.
    pub fn acquire<A: crate::Arch>(
        _machine: &mut A,
        _card: SuspendedCard,
        display: crate::kernel::display::Display,
    ) -> Self {
        crate::kernel::term::mark_dirty();
        Self { display }
    }
}

impl core::fmt::Write for Console {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        core::fmt::Write::write_str(lib::term::term(), s)?;
        for byte in s.bytes() {
            crate::kernel::serial_log::write_session_byte(byte);
        }
        crate::kernel::term::mark_dirty();
        Ok(())
    }
}
