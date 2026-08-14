//! Synchronous notifications for operations which may stall the machine.

#[derive(Clone, Copy)]
pub struct BlockingOperationHook {
    callback: Option<fn(*mut ())>,
    context: *mut (),
}

impl BlockingOperationHook {
    pub const fn new(callback: fn(*mut ()), context: *mut ()) -> Self {
        Self { callback: Some(callback), context }
    }

    pub const fn none() -> Self {
        Self { callback: None, context: core::ptr::null_mut() }
    }
}

static mut HOOK: BlockingOperationHook = BlockingOperationHook::none();

pub fn install(hook: BlockingOperationHook) {
    // SAFETY: installed before the event loop starts and read synchronously.
    unsafe { HOOK = hook; }
}

pub fn before_blocking_operation() {
    // SAFETY: the registration is immutable while the event loop runs.
    let hook = unsafe { HOOK };
    if let Some(callback) = hook.callback {
        callback(hook.context);
    }
}
