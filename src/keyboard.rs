/// macOS does not reliably deliver repeat/key-up events for Command shortcuts.
/// Read the physical key state while our own short-lived repeat timer is active.
#[cfg(target_os = "macos")]
pub fn file_shortcut_pressed(direction: isize) -> bool {
    #[link(name = "CoreGraphics", kind = "framework")]
    unsafe extern "C" {
        fn CGEventSourceKeyState(state: i32, key: u16) -> bool;
    }
    // Constants from CGEventTypes.h and HIToolbox/Events.h in the macOS SDK.
    unsafe {
        (CGEventSourceKeyState(0, 0x37) || CGEventSourceKeyState(0, 0x36))
            && CGEventSourceKeyState(0, if direction < 0 { 0x7e } else { 0x7d })
    }
}
#[cfg(not(target_os = "macos"))]
pub fn file_shortcut_pressed(_: isize) -> bool {
    false
}
