#![no_std]
#![no_main]

use core::arch::asm;

const SYS_EXIT: i32 = 1;
const SYS_FORK: i32 = 2;
const SYS_EXECVE: i32 = 11;
const SYS_WAIT4: i32 = 114;
const SYS_REBOOT: i32 = 88;

const REBOOT_MAGIC1: i32 = 0xFEE1_DEADu32 as i32;
const REBOOT_MAGIC2: i32 = 672_274_793;
const REBOOT_CMD_RESTART: i32 = 0x0123_4567;

#[unsafe(no_mangle)]
pub extern "C" fn main(argc: i32, argv: *const *const u8, _envp: *const *const u8) -> i32 {
    let args = unsafe { core::slice::from_raw_parts(argv, argc.max(0) as usize) };
    if argc >= 2 && !cstr_eq(args[1], b"--restart") {
        return spawn_and_wait(args[1], args.as_ptr().wrapping_add(1));
    }
    if argc >= 3 && cstr_eq(args[1], b"--restart") {
        let status = spawn_and_wait(args[2], args.as_ptr().wrapping_add(2));
        let _ = status;
        reboot();
    }
    spawn_and_wait(b"/bin/sh\0".as_ptr(), [b"/bin/sh\0".as_ptr(), core::ptr::null()].as_ptr())
}

fn spawn_and_wait(path: *const u8, child_argv: *const *const u8) -> i32 {
    let child = syscall0(SYS_FORK);
    if child == 0 {
        let result = syscall3(SYS_EXECVE, path as i32, child_argv as i32, 0);
        syscall1(SYS_EXIT, 127);
        return result;
    }
    if child < 0 {
        return 127;
    }
    let mut status = 0i32;
    let waited = syscall4(SYS_WAIT4, child, &mut status as *mut i32 as i32, 0, 0);
    if waited < 0 {
        return 127;
    }
    if status & 0x7f == 0 { (status >> 8) & 0xff } else { 128 + (status & 0x7f) }
}

fn reboot() -> ! {
    syscall3(SYS_REBOOT, REBOOT_MAGIC1, REBOOT_MAGIC2, REBOOT_CMD_RESTART);
    loop { core::hint::spin_loop(); }
}

fn cstr_eq(ptr: *const u8, expected: &[u8]) -> bool {
    for (index, &byte) in expected.iter().enumerate() {
        if unsafe { *ptr.add(index) } != byte { return false; }
    }
    unsafe { *ptr.add(expected.len()) == 0 }
}

fn syscall0(number: i32) -> i32 { unsafe { let mut out = number; asm!("int 0x80", inout("eax") out, options(nostack)); out } }
fn syscall1(number: i32, a0: i32) -> i32 { unsafe { let mut out = number; asm!("int 0x80", inout("eax") out, in("ebx") a0, options(nostack)); out } }
fn syscall3(number: i32, a0: i32, a1: i32, a2: i32) -> i32 { unsafe { let mut out = number; asm!("int 0x80", inout("eax") out, in("ebx") a0, in("ecx") a1, in("edx") a2, options(nostack)); out } }
fn syscall4(number: i32, a0: i32, a1: i32, a2: i32, a3: i32) -> i32 { unsafe { let mut out = number; asm!("int 0x80", inout("eax") out, in("ebx") a0, in("ecx") a1, in("edx") a2, in("edi") a3, options(nostack)); out } }

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! { loop {} }
