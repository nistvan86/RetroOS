//! Minimal ring-0 bridge to the PXE 2.1 protected-mode API.
//!
//! PXE is 16-bit protected-mode code. `EntryPointESP` means it uses a 32-bit
//! stack segment, but the far call and its return address remain 16-bit. A
//! small 16-bit code-alias trampoline provides that exact environment.

use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};

#[cfg(target_arch = "x86")]
core::arch::global_asm!(r#"
    .code32
    .global retroos_pxe_pm_call
    .type retroos_pxe_pm_call, @function
retroos_pxe_pm_call:
    push ebp
    mov ebp, esp
    push ebx
    push esi
    push edi
    pushfd
    sub esp, 16

    mov ax, ds
    mov word ptr [esp + 8], ax
    mov ax, es
    mov word ptr [esp + 10], ax
    mov ax, fs
    mov word ptr [esp + 12], ax
    mov ax, gs
    mov word ptr [esp + 14], ax

    /* Arguments carried in 16-bit halves into the .code16 trampoline. */
    mov eax, dword ptr [ebp + 8]
    mov ebx, dword ptr [ebp + 12]
    mov ecx, dword ptr [ebp + 16]
    mov edx, dword ptr [ebp + 20]
    mov esi, dword ptr [ebp + 24]

    /* Enter the helper through its temporary 16-bit code alias. The outer
     * call remains m16:32; the helper returns with lretl. */
    /* The temporary descriptor is based exactly at this helper.  Enter at
     * offset zero so no instruction can straddle a 64-KiB alias boundary. */
    mov dword ptr [esp], 0
    mov di, word ptr [ebp + 28]
    mov word ptr [esp + 4], di
    lcall [esp]

    and eax, 0xffff
    mov dx, word ptr [esp + 8]
    mov ds, dx
    mov dx, word ptr [esp + 10]
    mov es, dx
    mov dx, word ptr [esp + 12]
    mov fs, dx
    mov dx, word ptr [esp + 14]
    mov gs, dx
    add esp, 16
    popfd
    pop edi
    pop esi
    pop ebx
    pop ebp
    ret
    .size retroos_pxe_pm_call, .-retroos_pxe_pm_call

    .code16
    .global retroos_pxe_pm_call16
retroos_pxe_pm_call16:
    /* Build the three required word args.  This Intel PXE firmware's entry
     * stub addresses the parameter far pointer at [EBP+0x0e], four bytes
     * beyond the conventional layout.  Leave two words ahead of the args so
     * its saved frame resolves that slot to BX:CX rather than DX:SI (the
     * firmware entry point itself). */
    push si
    push dx
    push cx
    push bx
    push ax
    sub esp, 4
    lcall [esp + 10]
    add esp, 14
    /* The outer 32-bit caller placed EIP:CS on our ESP stack. */
    /* Operand-size override + far return: pop 32-bit EIP and 16-bit CS. */
    .byte 0x66, 0xcb
    .code32
"#);

unsafe extern "C" {
    fn retroos_pxe_pm_call(
        opcode: u32,
        param_offset: u32,
        param_selector: u32,
        entry_offset: u32,
        entry_selector: u32,
        trampoline_selector: u32,
    ) -> u16;
    static retroos_pxe_pm_call16: u8;
}

static NETLOG_READY: AtomicBool = AtomicBool::new(false);
static NETLOG_PXE: AtomicU32 = AtomicU32::new(0);
static NETLOG_SESSION: AtomicU32 = AtomicU32::new(0);
static NETLOG_SEQUENCE: AtomicU32 = AtomicU32::new(0);
static NETLOG_SLOT: AtomicU32 = AtomicU32::new(0);
static GUEST_INT1A_ORIGINAL: AtomicU32 = AtomicU32::new(0);
static mut NETLOG_MAC: [u8; 6] = [0; 6];

const RLOG_HEADER_LEN: usize = 16;
const RLOG_MAX_PAYLOAD: usize = 512;

// Intel build 082's UNDI_ISR return path is unusable after this handoff. Keep
// every accepted TBD/frame alive instead of attempting completion polling or
// recycling. The logger is deliberately bounded and disables itself after the
// pool is consumed; normal VGA/RAM logging continues.
const NETLOG_SLOT_SIZE: usize = 1024;
const NETLOG_SLOTS: usize = 64;
#[repr(align(65536))]
struct NetlogPool([u8; NETLOG_SLOT_SIZE * NETLOG_SLOTS]);
static mut NETLOG_POOL: NetlogPool = NetlogPool([0; NETLOG_SLOT_SIZE * NETLOG_SLOTS]);

/// Retain the opened UNDI entry point and station address for later ring-1
/// log flushes. Called once at ring 0 after STARTUP/INITIALIZE/OPEN succeeds.
pub unsafe fn pxe_netlog_configure(pxe: *const u8, mac: [u8; 6], session: u32) {
    // Build 082 hooks BIOS INT 1Ah and keeps the vector it chains to at
    // CS:0100 (offset,segment). Preserve that pre-PXE vector for each DOS
    // process's private IVT; the physical IVT remains hooked for UNDI.
    let ivt = crate::LOW_MEM_BASE as *const u8;
    let hook_offset = unsafe { core::ptr::read_unaligned(ivt.add(0x1a * 4) as *const u16) };
    let hook_segment = unsafe { core::ptr::read_unaligned(ivt.add(0x1a * 4 + 2) as *const u16) };
    let hook_linear = (u32::from(hook_segment) << 4).wrapping_add(u32::from(hook_offset));
    if hook_linear < 0x10_0000 {
        // The saved vector is relative to the firmware's code segment, not
        // the handler offset within that segment.
        let code_base = crate::LOW_MEM_BASE + (usize::from(hook_segment) << 4);
        let old_offset = unsafe {
            core::ptr::read_unaligned((code_base + 0x100) as *const u16)
        };
        let old_segment = unsafe {
            core::ptr::read_unaligned((code_base + 0x102) as *const u16)
        };
        if old_offset != 0 || old_segment != 0 {
            GUEST_INT1A_ORIGINAL.store(
                u32::from(old_offset) | (u32::from(old_segment) << 16),
                Ordering::Release,
            );
        }
    }
    unsafe { NETLOG_MAC = mac; }
    NETLOG_PXE.store(pxe as u32, Ordering::Release);
    NETLOG_SESSION.store(session, Ordering::Relaxed);
    NETLOG_SEQUENCE.store(0, Ordering::Relaxed);
    NETLOG_SLOT.store(0, Ordering::Relaxed);
    NETLOG_READY.store(true, Ordering::Release);
}

pub(crate) fn pxe_guest_int1a_original() -> Option<u32> {
    match GUEST_INT1A_ORIGINAL.load(Ordering::Acquire) {
        0 => None,
        vector => Some(vector),
    }
}

/// Send one bounded RLOG payload. Returns zero when UNDI accepts the persistent
/// buffer, or a compact internal/PXE status code. This runs at ring 0 through
/// an arch call.
pub(crate) fn pxe_netlog_send(payload: *const u8, len: usize) -> u32 {
    if !NETLOG_READY.load(Ordering::Acquire) || payload.is_null() || len == 0 {
        return 1;
    }
    let len = len.min(RLOG_MAX_PAYLOAD);
    let pxe = NETLOG_PXE.load(Ordering::Acquire) as *const u8;
    let mac = unsafe { NETLOG_MAC };
    let slot = NETLOG_SLOT.fetch_add(1, Ordering::Relaxed) as usize;
    if slot >= NETLOG_SLOTS { return 0x7001; }
    let bytes = unsafe {
        let ptr = core::ptr::addr_of_mut!(NETLOG_POOL.0).cast::<u8>().add(slot * NETLOG_SLOT_SIZE);
        core::slice::from_raw_parts_mut(ptr, NETLOG_SLOT_SIZE)
    };
    bytes.fill(0);
    let base = bytes.as_mut_ptr() as usize;
    const TBD: usize = 32;
    const FRAME: usize = 64;
    let frame_len = 14 + RLOG_HEADER_LEN + len;
    let wire_len = frame_len.max(60);

    // PXENV_UNDI_TRANSMIT, P_UNKNOWN + XMT_BROADCAST.
    bytes[2] = 0;
    bytes[3] = 1;
    write_u16(bytes, 8, ((base + TBD) & 0xffff) as u16);
    write_u16(bytes, TBD, wire_len as u16);
    write_u16(bytes, TBD + 2, ((base + FRAME) & 0xffff) as u16);

    let frame = &mut bytes[FRAME..FRAME + wire_len];
    frame[..6].fill(0xff);
    frame[6..12].copy_from_slice(&mac);
    frame[12..14].copy_from_slice(&0x88B5u16.to_be_bytes());
    frame[14..18].copy_from_slice(b"RLOG");
    frame[18] = 1;
    frame[19] = 0;
    frame[20..24].copy_from_slice(&NETLOG_SESSION.load(Ordering::Relaxed).to_be_bytes());
    let sequence = NETLOG_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    frame[24..28].copy_from_slice(&sequence.to_be_bytes());
    frame[28..30].copy_from_slice(&(len as u16).to_be_bytes());
    let payload_bytes = unsafe { core::slice::from_raw_parts(payload, len) };
    frame[30..30 + len].copy_from_slice(payload_bytes);

    let tx = unsafe {
        pxe_pm_call_with_param_segment(0x0008, bytes.as_mut_ptr(), pxe, &[10, TBD + 4])
    };
    let ax = match tx { Ok(ax) => ax, Err(e) => return 0x1000 | u32::from(e) };
    let status = read_u16(bytes, 0);
    if ax != 0 || status != 0 { return 0x2000 | u32::from(status); }

    0
}

/// Ring-0 entry used during early boot, before the kernel can issue the
/// ring-1 `int 0x80` arch call. The firmware bridge itself requires CPL0.
pub fn pxe_netlog_send_ring0(payload: &[u8]) -> u32 {
    pxe_netlog_send(payload.as_ptr(), payload.len())
}

#[repr(align(64))]
struct IsrParams([u8; 32]);

/// Drain a bounded UNDI interrupt batch and recognize the private raw-L2
/// control frame: broadcast Ethernet, EtherType 88B5, payload
/// `RCTL 01 01 REBOOT`. No IP stack or firmware DHCP state is involved.
pub(crate) fn pxe_netlog_poll_reboot() -> bool {
    if !NETLOG_READY.load(Ordering::Acquire) { return false; }
    let pxe = NETLOG_PXE.load(Ordering::Acquire) as *const u8;
    let mut params = IsrParams([0; 32]);
    write_u16(&mut params.0, 2, 1); // PXENV_UNDI_ISR_IN_START
    let Ok(ax) = (unsafe { pxe_pm_call(0x0014, params.0.as_mut_ptr(), pxe) }) else {
        return false;
    };
    if ax != 0 || read_u16(&params.0, 0) != 0 || read_u16(&params.0, 2) != 0 {
        return false; // failure or PXENV_UNDI_ISR_OUT_NOT_OURS
    }

    for input in core::iter::once(2u16).chain(core::iter::repeat_n(3u16, 7)) {
        params.0.fill(0);
        write_u16(&mut params.0, 2, input);
        let Ok(ax) = (unsafe { pxe_pm_call(0x0014, params.0.as_mut_ptr(), pxe) }) else {
            break;
        };
        if ax != 0 || read_u16(&params.0, 0) != 0 { break; }
        match read_u16(&params.0, 2) {
            0 => break, // PXENV_UNDI_ISR_OUT_DONE
            3 if is_reboot_frame(&params.0) => return true,
            2 | 3 | 4 => {} // transmit, receive, or busy
            _ => break,
        }
    }
    false
}

fn is_reboot_frame(params: &[u8]) -> bool {
    let available = usize::from(read_u16(params, 4));
    let frame_len = usize::from(read_u16(params, 6));
    let offset = usize::from(read_u16(params, 10));
    let selector = read_u16(params, 12);
    let Some(base) = crate::descriptors::pxe_selector_base(selector) else { return false; };
    let len = available.min(frame_len);
    const CONTROL: &[u8] = b"RCTL\x01\x01REBOOT";
    if len < 14 + CONTROL.len() || offset.checked_add(14 + CONTROL.len()).is_none() {
        return false;
    }
    let frame = unsafe {
        core::slice::from_raw_parts(
            (crate::LOW_MEM_BASE + base as usize + offset) as *const u8,
            14 + CONTROL.len(),
        )
    };
    frame[..6] == [0xff; 6]
        && frame[12..14] == 0x88B5u16.to_be_bytes()
        && &frame[14..] == CONTROL
}

fn write_u16(bytes: &mut [u8], offset: usize, value: u16) {
    bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
}

fn read_u16(bytes: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes([bytes[offset], bytes[offset + 1]])
}

#[inline(never)]
pub unsafe fn pxe_pm_call(opcode: u16, params: *mut u8, pxe: *const u8) -> Result<u16, u8> {
    unsafe { pxe_pm_call_with_param_segment(opcode, params, pxe, &[]) }
}

/// Call PXE while filling selector halves of far pointers that address data
/// in the same 64-KiB window as `params`. The caller writes each pointer's
/// 16-bit offset; this routine supplies the matching protected-mode selector
/// after installing the temporary parameter descriptor and before entering
/// firmware.
#[inline(never)]
pub unsafe fn pxe_pm_call_with_param_segment(
    opcode: u16,
    params: *mut u8,
    pxe: *const u8,
    selector_offsets: &[usize],
) -> Result<u16, u8> {
    let entry_offset = unsafe { core::ptr::read_unaligned(pxe.add(0x14) as *const u16) };
    let entry_selector = unsafe { core::ptr::read_unaligned(pxe.add(0x16) as *const u16) };
    let count = unsafe { pxe.add(0x1d).read() };
    if entry_offset == 0 || entry_selector == 0 { return Err(5); }

    let param_addr = params as u32;
    let trampoline_addr = core::ptr::addr_of!(retroos_pxe_pm_call16) as u32;
    let (param_selector, trampoline_selector) = unsafe {
        crate::descriptors::install_pxe_segments(
            pxe.add(0x20), count, param_addr, trampoline_addr,
        )?
    };
    for &offset in selector_offsets {
        if offset > 0xfffe { return Err(7); }
        unsafe { core::ptr::write_unaligned(params.add(offset) as *mut u16, param_selector); }
    }
    let saved_low_memory = crate::paging2::map_pxe_identity();
    let status = unsafe {
        retroos_pxe_pm_call(
            opcode.into(),
            param_addr & 0xffff,
            param_selector.into(),
            entry_offset.into(),
            entry_selector.into(),
            trampoline_selector.into(),
        )
    };
    crate::paging2::restore_pxe_identity(saved_low_memory);
    Ok(status)
}
