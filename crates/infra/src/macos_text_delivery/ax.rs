use super::*;

pub(super) struct OwnedAxElement(pub(super) AXUIElementRef);

// SAFETY: AXUIElementRef is an immutable Core Foundation handle. Its documented operations
// message the owning process and may be called from a worker thread; ownership remains unique.
unsafe impl Send for OwnedAxElement {}

impl OwnedAxElement {
    pub(super) fn system_wide() -> Result<Self, TextDeliveryError> {
        let element = unsafe { AXUIElementCreateSystemWide() };
        if element.is_null() {
            Err(TextDeliveryError::System(
                "AXUIElementCreateSystemWide returned null".to_owned(),
            ))
        } else {
            Ok(Self(element))
        }
    }

    pub(super) fn application(process_id: i32) -> Result<Self, TextDeliveryError> {
        let element = unsafe { AXUIElementCreateApplication(process_id) };
        if element.is_null() {
            Err(TextDeliveryError::System(
                "AXUIElementCreateApplication returned null".to_owned(),
            ))
        } else {
            Ok(Self(element))
        }
    }

    pub(super) fn focused_control(&self) -> Result<Self, TextDeliveryError> {
        let attribute = CFString::new(kAXFocusedUIElementAttribute);
        let mut value: CFTypeRef = ptr::null();
        let error = unsafe {
            AXUIElementCopyAttributeValue(self.0, attribute.as_concrete_TypeRef(), &mut value)
        };

        if error == kAXErrorNoValue || value.is_null() {
            return Err(TextDeliveryError::NoFocusedControl);
        }
        if error != kAXErrorSuccess {
            return Err(system_error("read focused control", error));
        }

        Ok(Self(value as AXUIElementRef))
    }

    pub(super) fn process_id(&self) -> Result<i32, TextDeliveryError> {
        let mut process_id = 0;
        let error = unsafe { AXUIElementGetPid(self.0, &mut process_id) };
        if error == kAXErrorSuccess {
            Ok(process_id)
        } else {
            Err(system_error("read focused control process", error))
        }
    }

    pub(super) fn attribute_string(&self, name: &str) -> Result<Option<String>, TextDeliveryError> {
        let attribute = CFString::new(name);
        let mut value: CFTypeRef = ptr::null();
        let error = unsafe {
            AXUIElementCopyAttributeValue(self.0, attribute.as_concrete_TypeRef(), &mut value)
        };

        if error == kAXErrorNoValue || value.is_null() {
            return Ok(None);
        }
        if error != kAXErrorSuccess {
            return Err(system_error("read control attribute", error));
        }

        if unsafe { CFGetTypeID(value) } != unsafe { CFStringGetTypeID() } {
            unsafe { CFRelease(value) };
            return Ok(None);
        }

        let value = unsafe { CFString::wrap_under_create_rule(value.cast::<_>() as CFStringRef) };
        Ok(Some(value.to_string()))
    }

    pub(super) fn attribute_bool(&self, name: &str) -> Result<Option<bool>, TextDeliveryError> {
        let value = self.copy_attribute(name)?;
        let Some(value) = value else {
            return Ok(None);
        };
        if unsafe { CFGetTypeID(value) } != unsafe { CFBooleanGetTypeID() } {
            unsafe { CFRelease(value) };
            return Ok(None);
        }
        let result = unsafe { CFBooleanGetValue(value.cast::<_>() as CFBooleanRef) };
        unsafe { CFRelease(value) };
        Ok(Some(result))
    }

    pub(super) fn attribute_usize(&self, name: &str) -> Result<Option<usize>, TextDeliveryError> {
        let value = self.copy_attribute(name)?;
        let Some(value) = value else {
            return Ok(None);
        };
        if unsafe { CFGetTypeID(value) } != unsafe { CFNumberGetTypeID() } {
            unsafe { CFRelease(value) };
            return Ok(None);
        }
        let mut number: i64 = 0;
        let read = unsafe {
            CFNumberGetValue(
                value.cast::<_>() as CFNumberRef,
                kCFNumberSInt64Type,
                ptr::from_mut(&mut number).cast::<c_void>(),
            )
        };
        unsafe { CFRelease(value) };
        if read && number >= 0 {
            Ok(usize::try_from(number).ok())
        } else {
            Ok(None)
        }
    }

    fn copy_attribute(&self, name: &str) -> Result<Option<CFTypeRef>, TextDeliveryError> {
        let attribute = CFString::new(name);
        let mut value: CFTypeRef = ptr::null();
        let error = unsafe {
            AXUIElementCopyAttributeValue(self.0, attribute.as_concrete_TypeRef(), &mut value)
        };
        if error == kAXErrorAttributeUnsupported || error == kAXErrorNoValue || value.is_null() {
            Ok(None)
        } else if error == kAXErrorSuccess {
            Ok(Some(value))
        } else {
            Err(system_error("read control attribute", error))
        }
    }

    pub(super) fn replace_selection(&self, text: &str) -> Result<(), TextDeliveryError> {
        let attribute = CFString::new(kAXSelectedTextAttribute);
        let mut settable = 0;
        let error = unsafe {
            AXUIElementIsAttributeSettable(self.0, attribute.as_concrete_TypeRef(), &mut settable)
        };

        if error == kAXErrorAttributeUnsupported {
            return Err(TextDeliveryError::UnsupportedControl);
        }
        if error != kAXErrorSuccess {
            return Err(system_error("inspect selected text", error));
        }
        if settable == 0 {
            return Err(TextDeliveryError::UnsupportedControl);
        }

        let text = CFString::new(text);
        let error = unsafe {
            AXUIElementSetAttributeValue(
                self.0,
                attribute.as_concrete_TypeRef(),
                text.as_CFTypeRef(),
            )
        };

        if error == kAXErrorSuccess {
            Ok(())
        } else {
            Err(system_error("replace selected text", error))
        }
    }

    pub(super) fn selected_text_range(&self) -> Result<Option<TextRange>, TextDeliveryError> {
        let attribute = CFString::new(kAXSelectedTextRangeAttribute);
        let mut value: CFTypeRef = ptr::null();
        let error = unsafe {
            AXUIElementCopyAttributeValue(self.0, attribute.as_concrete_TypeRef(), &mut value)
        };

        if error == kAXErrorAttributeUnsupported || error == kAXErrorNoValue || value.is_null() {
            return Ok(None);
        }
        if error != kAXErrorSuccess {
            return Err(system_error("read selected text range", error));
        }

        let range = read_cf_range(value);
        unsafe { CFRelease(value) };
        Ok(range)
    }

    pub(super) fn string_for_range(
        &self,
        range: TextRange,
    ) -> Result<Option<String>, TextDeliveryError> {
        let Some(range) = range.to_cf_range() else {
            return Err(TextDeliveryError::System(
                "text verification range exceeds macOS limits".to_owned(),
            ));
        };
        let parameter =
            unsafe { AXValueCreate(kAXValueTypeCFRange, ptr::from_ref(&range).cast::<c_void>()) };
        if parameter.is_null() {
            return Err(TextDeliveryError::System(
                "AXValueCreate returned null for text verification range".to_owned(),
            ));
        }

        let attribute = CFString::new(kAXStringForRangeParameterizedAttribute);
        let mut value: CFTypeRef = ptr::null();
        let error = unsafe {
            AXUIElementCopyParameterizedAttributeValue(
                self.0,
                attribute.as_concrete_TypeRef(),
                parameter.cast(),
                &mut value,
            )
        };
        unsafe { CFRelease(parameter.cast()) };

        if error == kAXErrorParameterizedAttributeUnsupported
            || error == kAXErrorAttributeUnsupported
            || error == kAXErrorNoValue
            || value.is_null()
        {
            return Ok(None);
        }
        if error != kAXErrorSuccess {
            return Err(system_error("read inserted text", error));
        }
        if unsafe { CFGetTypeID(value) } != unsafe { CFStringGetTypeID() } {
            unsafe { CFRelease(value) };
            return Ok(None);
        }

        let value = unsafe { CFString::wrap_under_create_rule(value.cast::<_>() as CFStringRef) };
        Ok(Some(value.to_string()))
    }
}
