#![no_std]
#![no_main]

use core::arch::asm;

const SYS_REBOOT: i32 = 88;
const MAGIC1: i32 = 0xFEE1_DEADu32 as i32;
const MAGIC2: i32 = 672_274_793;
const CMD_RESTART: i32 = 0x0123_4567;

#[unsafe(no_mangle)]
pub extern "C" fn main(_argc: i32, _argv: *const *const u8, _envp: *const *const u8) -> i32 {
    let mut result = SYS_REBOOT;
    unsafe {
        asm!(
            "int 0x80",
            inout("eax") result,
            in("ebx") MAGIC1,
            in("ecx") MAGIC2,
            in("edx") CMD_RESTART,
            options(nostack),
        );
    }
    if result < 0 { 1 } else { 0 }
}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! { loop {} }
