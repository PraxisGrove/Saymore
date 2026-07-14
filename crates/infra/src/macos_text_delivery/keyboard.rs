use std::{ffi::c_void, os::raw::c_ulong};

use core_foundation_sys::{
    base::CFRelease,
    data::{CFDataGetBytePtr, CFDataRef},
    string::CFStringRef,
};
use core_graphics::{
    event::{CGEvent, CGEventFlags, CGEventTapLocation},
    event_source::{CGEventSource, CGEventSourceStateID},
};
use template_app::TextDeliveryError;

const COMMAND_KEY_CODE: u16 = 55;
const FALLBACK_V_KEY_CODE: u16 = 9;
const MAX_KEY_CODE: u16 = 127;
const KEY_ACTION_DISPLAY: u16 = 3;
const NO_DEAD_KEYS_MASK: u32 = 1;

#[repr(transparent)]
struct TisInputSource(c_void);

type TisInputSourceRef = *mut TisInputSource;

#[repr(transparent)]
struct UcKeyboardLayout(c_void);

#[link(name = "Carbon", kind = "framework")]
unsafe extern "C" {
    static kTISPropertyUnicodeKeyLayoutData: CFStringRef;

    fn TISCopyCurrentKeyboardLayoutInputSource() -> TisInputSourceRef;

    fn TISGetInputSourceProperty(
        input_source: TisInputSourceRef,
        property_key: CFStringRef,
    ) -> *mut c_void;

    fn LMGetKbdType() -> u8;

    fn UCKeyTranslate(
        keyboard_layout: *const UcKeyboardLayout,
        virtual_key_code: u16,
        key_action: u16,
        modifier_key_state: u32,
        keyboard_type: u32,
        key_translate_options: u32,
        dead_key_state: *mut u32,
        max_string_length: c_ulong,
        actual_string_length: *mut c_ulong,
        unicode_string: *mut u16,
    ) -> i32;
}

pub(super) fn post_paste_shortcut() -> Result<(), TextDeliveryError> {
    let source = CGEventSource::new(CGEventSourceStateID::HIDSystemState)
        .map_err(|()| event_error("create paste event source"))?;
    let paste_key_code = current_v_key_code().unwrap_or(FALLBACK_V_KEY_CODE);
    let command_down = keyboard_event(source.clone(), COMMAND_KEY_CODE, KeyTransition::Down)?;
    let paste_down = keyboard_event(source.clone(), paste_key_code, KeyTransition::Down)?;
    let paste_up = keyboard_event(source.clone(), paste_key_code, KeyTransition::Up)?;
    let command_up = keyboard_event(source, COMMAND_KEY_CODE, KeyTransition::Up)?;

    command_down.set_flags(CGEventFlags::CGEventFlagCommand);
    paste_down.set_flags(CGEventFlags::CGEventFlagCommand);
    paste_up.set_flags(CGEventFlags::CGEventFlagCommand);

    for event in [command_down, paste_down, paste_up, command_up] {
        event.post(CGEventTapLocation::HID);
    }
    Ok(())
}

fn find_v_key_code(mut translated_text: impl FnMut(u16) -> Option<String>) -> Option<u16> {
    (0..=MAX_KEY_CODE).find(|key_code| {
        translated_text(*key_code).is_some_and(|text| text.eq_ignore_ascii_case("v"))
    })
}

fn current_v_key_code() -> Option<u16> {
    let input_source = OwnedInputSource::current()?;
    let layout = input_source.unicode_layout()?;
    // SAFETY: Carbon returns the keyboard type for the current process without preconditions.
    let keyboard_type = unsafe { LMGetKbdType() };
    find_v_key_code(|key_code| translate_key(layout, keyboard_type, key_code))
}

fn translate_key(
    layout: *const UcKeyboardLayout,
    keyboard_type: u8,
    key_code: u16,
) -> Option<String> {
    let mut buffer = [0_u16; 8];
    let mut actual_length = 0;
    let mut dead_key_state = 0;
    // SAFETY: `layout` points into the retained input source, and all output buffers are valid.
    let status = unsafe {
        UCKeyTranslate(
            layout,
            key_code,
            KEY_ACTION_DISPLAY,
            0,
            u32::from(keyboard_type),
            NO_DEAD_KEYS_MASK,
            &mut dead_key_state,
            buffer.len() as c_ulong,
            &mut actual_length,
            buffer.as_mut_ptr(),
        )
    };
    let actual_length = usize::try_from(actual_length).ok()?;
    if status != 0 || actual_length == 0 || actual_length > buffer.len() {
        return None;
    }
    String::from_utf16(&buffer[..actual_length]).ok()
}

struct OwnedInputSource(TisInputSourceRef);

impl OwnedInputSource {
    fn current() -> Option<Self> {
        // SAFETY: Carbon returns a retained input source which is released by `Drop`.
        let input_source = unsafe { TISCopyCurrentKeyboardLayoutInputSource() };
        (!input_source.is_null()).then_some(Self(input_source))
    }

    fn unicode_layout(&self) -> Option<*const UcKeyboardLayout> {
        // SAFETY: The property key is a Carbon constant and `self` retains the input source.
        let layout_data =
            unsafe { TISGetInputSourceProperty(self.0, kTISPropertyUnicodeKeyLayoutData) };
        if layout_data.is_null() {
            return None;
        }
        // SAFETY: This property is documented as CFData containing a UCKeyboardLayout.
        let bytes = unsafe { CFDataGetBytePtr(layout_data.cast::<_>() as CFDataRef) };
        (!bytes.is_null()).then_some(bytes.cast::<UcKeyboardLayout>())
    }
}

impl Drop for OwnedInputSource {
    fn drop(&mut self) {
        // SAFETY: `self.0` is a retained Core Foundation object owned by this wrapper.
        unsafe { CFRelease(self.0.cast()) };
    }
}

fn keyboard_event(
    source: CGEventSource,
    key_code: u16,
    transition: KeyTransition,
) -> Result<CGEvent, TextDeliveryError> {
    CGEvent::new_keyboard_event(source, key_code, transition == KeyTransition::Down)
        .map_err(|()| event_error("create paste keyboard event"))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum KeyTransition {
    Down,
    Up,
}

fn event_error(operation: &str) -> TextDeliveryError {
    TextDeliveryError::System(format!("{operation} failed"))
}

#[cfg(test)]
mod tests {
    use super::find_v_key_code;

    #[test]
    fn selects_the_key_code_that_produces_v_in_the_current_layout() {
        assert_eq!(
            Some(42),
            find_v_key_code(|key_code| (key_code == 42).then(|| "v".to_owned()))
        );
        assert_eq!(None, find_v_key_code(|_| None));
    }
}
