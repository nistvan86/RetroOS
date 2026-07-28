//! Protected-mode VBE linear-framebuffer diagnostic.
//!
//! `VBELFB INFO` queries mode 111h and asks DPMI to map PhysBasePtr, but never
//! touches the mapping. `VBELFB DRAW` additionally selects 4111h (LFB bit)
//! and writes a gradient. `VBELFB PAL` selects 640x480x8 and loads four known
//! colours through VBE 4F09h to diagnose BGR/RGB palette ordering.

#![no_std]

use dosrt::{conv_flat_ptr, dos, dpmi, putc, puts};

const MODE: u16 = 0x111; // 640x480x16
const PAL_MODE: u16 = 0x101; // 640x480x8
const INFO_BYTES: u16 = 256 + 4 * 4;
const REPORT: &[u8] = b"C:\\HOST\\VBELFB.TXT\0";

fn emit(handle: u16, s: &[u8]) {
    for &b in s { putc(b); }
    let _ = dos::write(handle, s);
}

fn emit_hex(handle: u16, label: &[u8], value: u32) {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut line = [0u8; 80];
    let mut n = 0usize;
    for &b in label {
        line[n] = b;
        n += 1;
    }
    for shift in (0..8).rev() {
        line[n] = HEX[((value >> (shift * 4)) & 0xF) as usize];
        n += 1;
    }
    line[n] = b'\r'; line[n + 1] = b'\n'; n += 2;
    emit(handle, &line[..n]);
}

fn rd16(p: *const u8, off: usize) -> u16 {
    unsafe { core::ptr::read_unaligned(p.add(off) as *const u16) }
}

fn rd32(p: *const u8, off: usize) -> u32 {
    unsafe { core::ptr::read_unaligned(p.add(off) as *const u32) }
}

fn vbe_mode_info(seg: u16, mode: u16) -> bool {
    let mut r = dpmi::Rmcs::default();
    r.eax = 0x4F01;
    r.ecx = mode as u32;
    r.es = seg;
    r.edi = 0;
    dpmi::sim_int(0x10, &mut r);
    r.eax as u16 == 0x004F
}

fn vbe_set_palette(seg: u16) -> bool {
    let p = unsafe { conv_flat_ptr(seg).add(256) };
    // VBE 4F09 entries are Blue, Green, Red, reserved.
    let entries: [u8; 16] = [
        0, 0, 0, 0,       // index 0: black
        0, 0, 63, 0,      // index 1: red
        0, 63, 0, 0,      // index 2: green
        63, 0, 0, 0,      // index 3: blue
    ];
    unsafe { core::ptr::copy_nonoverlapping(entries.as_ptr(), p, entries.len()); }
    let mut r = dpmi::Rmcs::default();
    r.eax = 0x4F09;
    r.ebx = 0; // set palette immediately
    r.ecx = 4;
    r.edx = 0; // first index
    r.es = seg;
    r.edi = 256;
    dpmi::sim_int(0x10, &mut r);
    r.eax as u16 == 0x004F
}

fn vbe_set(mode: u16) -> bool {
    let mut r = dpmi::Rmcs::default();
    r.eax = 0x4F02;
    r.ebx = mode as u32;
    dpmi::sim_int(0x10, &mut r);
    r.eax as u16 == 0x004F
}

/// DPMI 0.9 AX=0800h. Returns (CF, AX, mapped linear address).
fn map_physical(phys: u32, size: u32) -> (bool, u16, u32) {
    let ax: u16;
    let bx: u16;
    let cx: u16;
    let eflags: u32;
    unsafe {
        core::arch::asm!(
            "push esi",
            "push edi",
            "mov si, {size_hi:x}",
            "mov di, {size_lo:x}",
            "int 0x31",
            "pushfd",
            "pop edx",
            "pop edi",
            "pop esi",
            size_hi = in(reg) (size >> 16) as u16,
            size_lo = in(reg) size as u16,
            inlateout("ax") 0x0800u16 => ax,
            inlateout("bx") (phys >> 16) as u16 => bx,
            inlateout("cx") phys as u16 => cx,
            lateout("edx") eflags,
            clobber_abi("C"),
        );
    }
    (eflags & 1 != 0, ax, ((bx as u32) << 16) | cx as u32)
}

fn draw_gradient(mapped: u32, pitch: u16, width: u16, height: u16) {
    let ds_base = dpmi::seg_base(dpmi::ds_sel());
    let fb = mapped.wrapping_sub(ds_base) as *mut u8;
    for y in 0..height as usize {
        for x in 0..width as usize {
            let r = (x * 31 / width as usize) as u16;
            let g = (y * 63 / height as usize) as u16;
            let b = 16u16;
            let px = (r << 11) | (g << 5) | b;
            let off = y * pitch as usize + x * 2;
            unsafe {
                core::ptr::write_volatile(fb.add(off), px as u8);
                core::ptr::write_volatile(fb.add(off + 1), (px >> 8) as u8);
            }
        }
    }
}

fn draw_palette_bands(mapped: u32, pitch: u16, width: u16, height: u16) {
    let ds_base = dpmi::seg_base(dpmi::ds_sel());
    let fb = mapped.wrapping_sub(ds_base) as *mut u8;
    for y in 0..height as usize {
        for x in 0..width as usize {
            let index = if x < width as usize / 3 {
                1 // expected red
            } else if x < width as usize * 2 / 3 {
                2 // expected green
            } else {
                3 // expected blue
            };
            unsafe { core::ptr::write_volatile(fb.add(y * pitch as usize + x), index); }
        }
    }
}

#[unsafe(no_mangle)]
pub fn app_main(argc: usize, argv: &[&[u8]]) {
    let draw = (1..argc).any(|i| argv[i].eq_ignore_ascii_case(b"DRAW"));
    let pal = (1..argc).any(|i| argv[i].eq_ignore_ascii_case(b"PAL"));
    let mode = if pal { PAL_MODE } else { MODE };
    let handle = match dos::create(REPORT) {
        Some(h) => h,
        None => {
            puts("VBELFB: cannot create C:\\HOST\\VBELFB.TXT\r\n");
            dos::exit(1);
        }
    };

    emit(handle, b"VBELFB protected-mode LFB diagnostic\r\n");
    emit(handle, if pal {
        b"stage=PAL\r\n"
    } else if draw {
        b"stage=DRAW\r\n"
    } else {
        b"stage=INFO\r\n"
    });

    let (seg, _sel) = match dpmi::alloc_dos_mem(INFO_BYTES.div_ceil(16)) {
        Some(v) => v,
        None => {
            emit(handle, b"DOS buffer allocation failed\r\n");
            dos::close(handle);
            dos::exit(1);
        }
    };
    let info = conv_flat_ptr(seg);
    unsafe { core::ptr::write_bytes(info, 0, INFO_BYTES as usize); }

    if !vbe_mode_info(seg, mode) {
        emit(handle, b"VBE 4F01 failed\r\n");
        dos::close(handle);
        dos::exit(1);
    }

    let attributes = rd16(info, 0);
    let pitch = rd16(info, 0x10);
    let width = rd16(info, 0x12);
    let height = rd16(info, 0x14);
    let bpp = unsafe { *info.add(0x19) };
    let phys = rd32(info, 0x28);
    let size = pitch as u32 * height as u32;

    emit_hex(handle, b"mode=", mode as u32);
    emit_hex(handle, b"attributes=", attributes as u32);
    emit_hex(handle, b"width=", width as u32);
    emit_hex(handle, b"height=", height as u32);
    emit_hex(handle, b"bpp=", bpp as u32);
    emit_hex(handle, b"pitch=", pitch as u32);
    emit_hex(handle, b"phys_base=", phys);
    emit_hex(handle, b"map_size=", size);

    let (cf, map_ax, mapped) = map_physical(phys, size);
    emit_hex(handle, b"map_cf=", cf as u32);
    emit_hex(handle, b"map_ax=", map_ax as u32);
    emit_hex(handle, b"map_linear=", mapped);

    if !draw && !pal {
        emit(handle, b"INFO complete; framebuffer not touched\r\n");
        dos::close(handle);
        dos::exit(if cf { 2 } else { 0 });
    }
    if cf {
        emit(handle, b"DRAW refused: DPMI mapping failed\r\n");
        dos::close(handle);
        dos::exit(2);
    }

    emit(handle, if pal {
        b"Selecting mode 4101h\r\n"
    } else {
        b"Selecting mode 4111h\r\n"
    });
    if !vbe_set(mode | 0x4000) {
        emit(handle, b"VBE 4F02 LFB mode failed\r\n");
        dos::close(handle);
        dos::exit(3);
    }
    if pal {
        let ok = vbe_set_palette(seg);
        emit_hex(handle, b"palette_4f09_ok=", ok as u32);
        emit(handle, b"Expected bands: RED | GREEN | BLUE\r\n");
    }
    emit(handle, b"About to write mapped framebuffer\r\n");
    dos::close(handle); // persist the evidence before the potentially fatal access

    if pal {
        draw_palette_bands(mapped, pitch, width, height);
    } else {
        draw_gradient(mapped, pitch, width, height);
    }
    unsafe {
        core::arch::asm!("int 0x21", in("ax") 0x0800u16, lateout("ax") _,
                         clobber_abi("C"));
    }
    let _ = vbe_set(0x0003);
    dos::exit(0);
}
