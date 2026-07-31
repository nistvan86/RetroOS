//! Read-only PXE/UNDI handoff diagnostics.
//!
//! The default probe only scans the low-memory PXE structures GRUB/PXE may
//! have left behind and validates their byte checksums.  The separate
//! `pxe_call_probe` variant additionally queries and starts UNDI.  Both
//! variants halt immediately after printing, making the result photographable
//! on an 80x25 VGA screen.

use crate::{Arch, LOW_MEM_BASE};

#[cfg(pxe_call_probe)]
#[repr(align(64))]
struct InfoParams([u8; 192]);

#[cfg(pxe_call_probe)]
#[repr(align(4096))]
struct TxWorkspace([u8; 512]);

#[cfg(pxe_call_probe)]
const TX_PARAM: usize = 0;
#[cfg(pxe_call_probe)]
const TX_TBD: usize = 32;
#[cfg(pxe_call_probe)]
const TX_FRAME: usize = 64;
#[cfg(pxe_call_probe)]
const TX_FRAME_LEN: usize = 60;

const LOW_MEM_SCAN_END: usize = 0xA0000;
const MAX_STRUCT_LEN: usize = 128;

#[derive(Clone, Copy)]
pub struct Probe {
    pub pxenv: Option<(usize, bool, bool)>, // address, checksum, entry pointers
    pub pxe: Option<(usize, bool, bool)>,   // address, checksum, entry pointers
}

impl Probe {
    /// Scan paragraph-aligned low memory for PXENV+ and !PXE signatures.
    /// `machine.read` uses the kernel's low-memory mapping on metal.  Hosted
    /// callers should not invoke this routine.
    pub fn scan<A: Arch>(machine: &A) -> Self {
        let mut out = Self { pxenv: None, pxe: None };
        let mut addr = 0usize;
        while addr < LOW_MEM_SCAN_END {
            if out.pxenv.is_none() && has(machine, addr, b"PXENV+") {
                let len = machine.read::<u8>(LOW_MEM_BASE + addr + 8) as usize;
                let valid_len = (10..=MAX_STRUCT_LEN).contains(&len)
                    && addr + len <= LOW_MEM_SCAN_END;
                let checksum = valid_len && sum(machine, addr, len) == 0;
                let entries = machine.read::<u32>(LOW_MEM_BASE + addr + 10) != 0
                    || machine.read::<u32>(LOW_MEM_BASE + addr + 14) != 0;
                out.pxenv = Some((addr, checksum, entries));
            }
            if out.pxe.is_none() && has(machine, addr, b"!PXE") {
                let len = machine.read::<u8>(LOW_MEM_BASE + addr + 4) as usize;
                let valid_len = (8..=MAX_STRUCT_LEN).contains(&len)
                    && addr + len <= LOW_MEM_SCAN_END;
                let checksum = valid_len && sum(machine, addr, len) == 0;
                let entries = machine.read::<u32>(LOW_MEM_BASE + addr + 0x10) != 0
                    || machine.read::<u32>(LOW_MEM_BASE + addr + 0x14) != 0;
                out.pxe = Some((addr, checksum, entries));
            }
            if out.pxenv.is_some() && out.pxe.is_some() { break; }
            addr += 16;
        }
        out
    }
}

fn has<A: Arch>(machine: &A, addr: usize, sig: &[u8]) -> bool {
    sig.iter().enumerate().all(|(i, &b)| machine.read::<u8>(LOW_MEM_BASE + addr + i) == b)
}

fn sum<A: Arch>(machine: &A, addr: usize, len: usize) -> u8 {
    let mut total = 0u8;
    for i in 0..len {
        total = total.wrapping_add(machine.read::<u8>(LOW_MEM_BASE + addr + i));
    }
    total
}

pub fn print<A: Arch>(machine: &A, screen: &mut crate::vga::Screen) {
    let p = Probe::scan(machine);
    match p.pxenv {
        Some((a, checksum, entries)) => crate::screenln!(screen,
            "PXENV+ {:05X} CK={} EP={}", a, yesno(checksum), yesno(entries)),
        None => crate::screenln!(screen, "PXENV+ none"),
    }
    match p.pxe {
        Some((a, checksum, entries)) => crate::screenln!(screen,
            "!PXE   {:05X} CK={} EP={}", a, yesno(checksum), yesno(entries)),
        None => crate::screenln!(screen, "!PXE   none"),
    }
    #[cfg(pxe_call_probe)]
    {
        // Use !PXE's EntryPointESP and descriptor recipes. GET_INFORMATION
        // (000Ch) is read-only: it does not reset, open, or transmit.
        match p.pxe {
            Some((a, true, true)) => {
                let ep_off = machine.read::<u16>(LOW_MEM_BASE + a + 0x14);
                let ep_sel = machine.read::<u16>(LOW_MEM_BASE + a + 0x16);
                let seg_count = machine.read::<u8>(LOW_MEM_BASE + a + 0x1d);
                let first_sel = machine.read::<u16>(LOW_MEM_BASE + a + 0x1e);
                crate::screenln!(screen, "ESP {:04X}:{:04X} N={} FS={:04X}",
                    ep_sel, ep_off, seg_count, first_sel);
                crate::screenln!(screen, "SEL {:04X} {:04X} {:04X} {:04X}",
                    machine.read::<u16>(LOW_MEM_BASE + a + 0x20),
                    machine.read::<u16>(LOW_MEM_BASE + a + 0x28),
                    machine.read::<u16>(LOW_MEM_BASE + a + 0x30),
                    machine.read::<u16>(LOW_MEM_BASE + a + 0x38));
                crate::screenln!(screen, "    {:04X} {:04X} {:04X}",
                    machine.read::<u16>(LOW_MEM_BASE + a + 0x40),
                    machine.read::<u16>(LOW_MEM_BASE + a + 0x48),
                    machine.read::<u16>(LOW_MEM_BASE + a + 0x50));
                crate::screenln!(screen, "SZ UD={:04X} UC={:04X} UW={:04X}",
                    machine.read::<u16>(LOW_MEM_BASE + a + 0x2e),
                    machine.read::<u16>(LOW_MEM_BASE + a + 0x36),
                    machine.read::<u16>(LOW_MEM_BASE + a + 0x3e));
                let code_base = machine.read::<u32>(LOW_MEM_BASE + a + 0x32) as usize;
                crate::screenln!(screen,
                    "X0={:02X}{:02X} {:02X}{:02X} {:02X}{:02X} {:02X}{:02X} {:02X}{:02X} {:02X}{:02X}",
                    machine.read::<u8>(LOW_MEM_BASE + code_base + 0x240),
                    machine.read::<u8>(LOW_MEM_BASE + code_base + 0x241),
                    machine.read::<u8>(LOW_MEM_BASE + code_base + 0x242),
                    machine.read::<u8>(LOW_MEM_BASE + code_base + 0x243),
                    machine.read::<u8>(LOW_MEM_BASE + code_base + 0x244),
                    machine.read::<u8>(LOW_MEM_BASE + code_base + 0x245),
                    machine.read::<u8>(LOW_MEM_BASE + code_base + 0x246),
                    machine.read::<u8>(LOW_MEM_BASE + code_base + 0x247),
                    machine.read::<u8>(LOW_MEM_BASE + code_base + 0x248),
                    machine.read::<u8>(LOW_MEM_BASE + code_base + 0x249),
                    machine.read::<u8>(LOW_MEM_BASE + code_base + 0x24a),
                    machine.read::<u8>(LOW_MEM_BASE + code_base + 0x24b));
                crate::screenln!(screen,
                    "X1={:02X}{:02X} {:02X}{:02X} {:02X}{:02X} {:02X}{:02X} {:02X}{:02X} {:02X}{:02X}",
                    machine.read::<u8>(LOW_MEM_BASE + code_base + 0x24c),
                    machine.read::<u8>(LOW_MEM_BASE + code_base + 0x24d),
                    machine.read::<u8>(LOW_MEM_BASE + code_base + 0x24e),
                    machine.read::<u8>(LOW_MEM_BASE + code_base + 0x24f),
                    machine.read::<u8>(LOW_MEM_BASE + code_base + 0x250),
                    machine.read::<u8>(LOW_MEM_BASE + code_base + 0x251),
                    machine.read::<u8>(LOW_MEM_BASE + code_base + 0x252),
                    machine.read::<u8>(LOW_MEM_BASE + code_base + 0x253),
                    machine.read::<u8>(LOW_MEM_BASE + code_base + 0x254),
                    machine.read::<u8>(LOW_MEM_BASE + code_base + 0x255),
                    machine.read::<u8>(LOW_MEM_BASE + code_base + 0x256),
                    machine.read::<u8>(LOW_MEM_BASE + code_base + 0x257));
                // GET_STATE faults inside this board's old Intel firmware.
                // Attempt the first standards-defined transition directly,
                // then use the already-proven GET_INFORMATION call to see
                // whether UNDI now accepts commands. STARTUP does not open the
                // NIC or transmit packets.
                let mut params = InfoParams([0u8; 192]);
                match unsafe { arch_pxe_call(0x0001, params.0.as_mut_ptr(), a) } {
                    Ok(ax) => crate::screenln!(screen, "SU  A={:04X} S={:04X}",
                        ax, u16::from_le_bytes([params.0[0], params.0[1]])),
                    Err(e) => crate::screenln!(screen, "SU  SETUP E{:02X}", e),
                }

                params.0.fill(0);
                let initialized = match unsafe { arch_pxe_call(0x0003, params.0.as_mut_ptr(), a) } {
                    Ok(ax) => {
                        let st = u16::from_le_bytes([params.0[0], params.0[1]]);
                        crate::screenln!(screen, "IN  A={:04X} S={:04X}", ax, st);
                        ax == 0 && st == 0
                    }
                    Err(e) => {
                        crate::screenln!(screen, "IN  SETUP E{:02X}", e);
                        false
                    }
                };

                if initialized {
                    params.0.fill(0);
                    print_get_info(screen, &mut params, a, "GI1");

                    // PXENV_UNDI_OPEN: OpenFlag=0 is guaranteed valid;
                    // accept directed and broadcast frames; no multicast.
                    params.0.fill(0);
                    params.0[4..6].copy_from_slice(&0x0003u16.to_le_bytes());
                    let opened = match unsafe { arch_pxe_call(0x0006, params.0.as_mut_ptr(), a) } {
                        Ok(ax) => {
                            let st = u16::from_le_bytes([params.0[0], params.0[1]]);
                            crate::screenln!(screen, "OP  A={:04X} S={:04X}", ax, st);
                            ax == 0 && st == 0
                        }
                        Err(e) => {
                            crate::screenln!(screen, "OP  SETUP E{:02X}", e);
                            false
                        }
                    };
                    if opened {
                        params.0.fill(0);
                        if let Some(mac) = print_get_info(screen, &mut params, a, "GI2") {
                            crate::screenln!(screen,
                                "M={:02X}{:02X}{:02X}{:02X}{:02X}{:02X}",
                                mac[0], mac[1], mac[2], mac[3], mac[4], mac[5]);
                            transmit_probe_frame(screen, a, mac, 1,
                                b"HELLO FROM RETROOS SEJT!");
                            let isr = undi_isr_smoke_test(screen, a);
                            let mut report = [0u8; 16];
                            report[..4].copy_from_slice(b"ISRB");
                            for (i, value) in isr.iter().enumerate() {
                                report[4 + i * 2..6 + i * 2]
                                    .copy_from_slice(&value.to_be_bytes());
                            }
                            transmit_probe_frame(screen, a, mac, 2, &report);
                        }
                    }
                }
            }
            _ => crate::screenln!(screen, "UNDI GET_INFO skipped"),
        }
    }
    #[cfg(not(pxe_call_probe))]
    crate::screenln!(screen, "UNDI calls not attempted" );
    crate::screenln!(screen, "Probe complete; system halted." );
}

#[cfg(pxe_call_probe)]
unsafe fn arch_pxe_call(opcode: u16, params: *mut u8, pxe_phys: usize) -> Result<u16, u8> {
    let pxe = (LOW_MEM_BASE + pxe_phys) as *const u8;
    unsafe { crate::arch::pxe_pm_call(opcode, params, pxe) }
}

#[cfg(pxe_call_probe)]
fn print_get_info(
    screen: &mut crate::vga::Screen,
    params: &mut InfoParams,
    pxe_phys: usize,
    label: &str,
) -> Option<[u8; 6]> {
    match unsafe { arch_pxe_call(0x000C, params.0.as_mut_ptr(), pxe_phys) } {
        Ok(ax) => {
            let status = u16::from_le_bytes([params.0[0], params.0[1]]);
            crate::screenln!(screen, "{} A={:04X} S={:04X}", label, ax, status);
            if ax == 0 && status == 0
                && u16::from_le_bytes([params.0[10], params.0[11]]) >= 6
            {
                let mut mac = [0u8; 6];
                mac.copy_from_slice(&params.0[12..18]);
                Some(mac)
            } else {
                None
            }
        }
        Err(e) => {
            crate::screenln!(screen, "{} SETUP E{:02X}", label, e);
            None
        }
    }
}

#[cfg(pxe_call_probe)]
fn transmit_probe_frame(
    screen: &mut crate::vga::Screen,
    pxe_phys: usize,
    source_mac: [u8; 6],
    flags: u8,
    payload: &[u8],
) {
    // One 4-KiB-aligned object cannot straddle a 64-KiB boundary, so the PXE
    // parameter block, TBD, and immediate frame share one temporary selector.
    let mut work = TxWorkspace([0u8; 512]);
    let base = work.0.as_mut_ptr() as usize;
    let payload = &payload[..payload.len().min(30)];
    let frame_len = (14 + 16 + payload.len()).max(TX_FRAME_LEN);

    // PXENV_UNDI_TRANSMIT: P_UNKNOWN means the complete Ethernet header is
    // already present. XMT_BROADCAST lets the driver know its destination.
    work.0[TX_PARAM + 2] = 0;
    work.0[TX_PARAM + 3] = 1;
    put_u16(&mut work.0, TX_PARAM + 8, ((base + TX_TBD) & 0xffff) as u16);

    // One immediate buffer and no additional data blocks.
    put_u16(&mut work.0, TX_TBD, frame_len as u16);
    put_u16(&mut work.0, TX_TBD + 2, ((base + TX_FRAME) & 0xffff) as u16);
    put_u16(&mut work.0, TX_TBD + 6, 0);

    let frame = &mut work.0[TX_FRAME..TX_FRAME + frame_len];
    frame[..6].fill(0xff);
    frame[6..12].copy_from_slice(&source_mac);
    frame[12..14].copy_from_slice(&0x88B5u16.to_be_bytes());
    frame[14..18].copy_from_slice(b"RLOG");
    frame[18] = 1; // format version
    frame[19] = flags;
    frame[20..28].fill(0); // diagnostic session and sequence
    frame[28..30].copy_from_slice(&(payload.len() as u16).to_be_bytes());
    frame[30..30 + payload.len()].copy_from_slice(payload);

    let pxe = (LOW_MEM_BASE + pxe_phys) as *const u8;
    let result = unsafe {
        crate::arch::pxe_pm_call_with_param_segment(
            0x0008,
            work.0.as_mut_ptr().add(TX_PARAM),
            pxe,
            &[10, TX_TBD + 4],
        )
    };
    match result {
        Ok(ax) => crate::screenln!(screen, "TX  A={:04X} S={:04X}", ax,
            u16::from_le_bytes([work.0[0], work.0[1]])),
        Err(e) => crate::screenln!(screen, "TX  SETUP E{:02X}", e),
    }
}

#[cfg(pxe_call_probe)]
fn undi_isr_smoke_test(screen: &mut crate::vga::Screen, pxe_phys: usize) -> [u16; 6] {
    let mut params = InfoParams([0u8; 192]);
    let mut report = [0xffff; 6];

    // START asks whether the pending interrupt belongs to UNDI.  PROCESS is
    // valid only after START returns OUT_OURS (zero).  A transmit immediately
    // precedes this test, so completion will commonly make it ours; NOT_OURS
    // is also a valid smoke-test result when no interrupt is pending.
    params.0[2..4].copy_from_slice(&1u16.to_le_bytes());
    let start = unsafe { arch_pxe_call(0x0014, params.0.as_mut_ptr(), pxe_phys) };
    let status = u16::from_le_bytes([params.0[0], params.0[1]]);
    let flag = u16::from_le_bytes([params.0[2], params.0[3]]);
    match start {
        Ok(ax) => {
            report[..3].copy_from_slice(&[ax, status, flag]);
            crate::screenln!(screen, "ISR1 A{:04X} S{:04X} F{:04X}", ax, status, flag);
        }
        Err(e) => {
            report[0] = 0xe000 | u16::from(e);
            crate::screenln!(screen, "ISR1 E{:02X}", e);
            return report;
        }
    }
    if status != 0 || flag != 0 { return report; }

    params.0.fill(0);
    params.0[2..4].copy_from_slice(&2u16.to_le_bytes());
    match unsafe { arch_pxe_call(0x0014, params.0.as_mut_ptr(), pxe_phys) } {
        Ok(ax) => {
            report[3..].copy_from_slice(&[ax,
                u16::from_le_bytes([params.0[0], params.0[1]]),
                u16::from_le_bytes([params.0[2], params.0[3]])]);
            crate::screenln!(screen, "ISR2 A{:04X} S{:04X} F{:04X}",
                report[3], report[4], report[5]);
        }
        Err(e) => {
            report[3] = 0xe000 | u16::from(e);
            crate::screenln!(screen, "ISR2 E{:02X}", e);
        }
    }
    report
}

#[cfg(pxe_call_probe)]
fn put_u16(bytes: &mut [u8], offset: usize, value: u16) {
    bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
}

fn yesno(v: bool) -> &'static str { if v { "Y" } else { "N" } }

/// Bring up the retained PXE UNDI interface for the non-halting netlog build.
/// This must run at ring 0 before the ordinary boot path enters ring 1.
#[cfg(pxe_netlog)]
pub fn netlog_init<A: Arch>(machine: &A, screen: &mut crate::vga::Screen) -> Option<[u8; 6]> {
    let probe = Probe::scan(machine);
    let pxe_phys = match probe.pxe {
        Some((addr, true, true)) => addr,
        _ => {
            crate::screenln!(screen, "PXE netlog: no usable !PXE");
            return None;
        }
    };
    let mut params = InfoParams([0u8; 192]);
    for (opcode, name) in [(0x0001, "startup"), (0x0003, "initialize")] {
        params.0.fill(0);
        let result = unsafe { arch_pxe_call(opcode, params.0.as_mut_ptr(), pxe_phys) };
        let status = u16::from_le_bytes([params.0[0], params.0[1]]);
        if !matches!(result, Ok(0)) || status != 0 {
            crate::screenln!(screen, "PXE netlog: {} failed S={:04X}", name, status);
            return None;
        }
    }
    params.0.fill(0);
    let info = unsafe { arch_pxe_call(0x000C, params.0.as_mut_ptr(), pxe_phys) };
    let status = u16::from_le_bytes([params.0[0], params.0[1]]);
    let addr_len = u16::from_le_bytes([params.0[10], params.0[11]]);
    if !matches!(info, Ok(0)) || status != 0 || addr_len < 6 {
        crate::screenln!(screen, "PXE netlog: get-info failed S={:04X}", status);
        return None;
    }
    let mut mac = [0u8; 6];
    mac.copy_from_slice(&params.0[12..18]);

    params.0.fill(0);
    params.0[4..6].copy_from_slice(&0x0003u16.to_le_bytes());
    let open = unsafe { arch_pxe_call(0x0006, params.0.as_mut_ptr(), pxe_phys) };
    let status = u16::from_le_bytes([params.0[0], params.0[1]]);
    if !matches!(open, Ok(0)) || status != 0 {
        crate::screenln!(screen, "PXE netlog: open failed S={:04X}", status);
        return None;
    }

    let pxe = (LOW_MEM_BASE + pxe_phys) as *const u8;
    let session = machine.rdtsc() as u32;
    unsafe { crate::arch::pxe_netlog_configure(pxe, mac, session); }
    Some(mac)
}
