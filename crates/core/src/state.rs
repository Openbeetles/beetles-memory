use std::sync::atomic::{AtomicBool, Ordering};

static VOICE_EXCLUSIVE_ACTIVE: AtomicBool = AtomicBool::new(false);
static BACKGROUND_MAINTENANCE_ACTIVE: AtomicBool = AtomicBool::new(false);

pub fn wifi_sta_connected() -> bool {
    true
}

pub fn set_voice_exclusive_active(active: bool) {
    VOICE_EXCLUSIVE_ACTIVE.store(active, Ordering::Relaxed);
}

pub fn voice_exclusive_active() -> bool {
    VOICE_EXCLUSIVE_ACTIVE.load(Ordering::Relaxed)
}

pub fn set_background_maintenance_active(active: bool) {
    BACKGROUND_MAINTENANCE_ACTIVE.store(active, Ordering::Relaxed);
}

pub fn background_maintenance_active() -> bool {
    BACKGROUND_MAINTENANCE_ACTIVE.load(Ordering::Relaxed)
}
