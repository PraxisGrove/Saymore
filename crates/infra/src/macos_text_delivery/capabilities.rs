use accessibility_sys::{
    AXUIElementIsAttributeSettable, kAXErrorAttributeUnsupported, kAXErrorSuccess,
    kAXRoleAttribute, kAXSecureTextFieldSubrole, kAXSelectedTextAttribute, kAXSubroleAttribute,
};
use core_foundation::base::TCFType;
use objc2_app_kit::NSRunningApplication;
use serde::Serialize;
use template_app::TextDeliveryError;

use super::{
    OwnedAxElement, TextRange, current_delivery_target, secure_event_input_enabled, system_error,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MacOsCorrectionObservationSupport {
    Observable,
    DeliveryOnly,
    Sensitive,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MacOsFocusedTextControlCapabilities {
    pub bundle_identifier: Option<String>,
    pub role: Option<String>,
    pub subrole: Option<String>,
    pub selected_text_range: bool,
    pub selected_text_settable: bool,
    pub string_for_range: bool,
    pub correction_observation: MacOsCorrectionObservationSupport,
}

pub fn focused_text_control_capabilities()
-> Result<MacOsFocusedTextControlCapabilities, TextDeliveryError> {
    let target = current_delivery_target();
    if !target.external_target {
        return Err(TextDeliveryError::NoFocusedControl);
    }
    let focused = target.focused.ok_or(TextDeliveryError::NoFocusedControl)?;
    capabilities_for_control(focused)
}

pub fn text_control_capabilities_for_process(
    process_id: i32,
) -> Result<MacOsFocusedTextControlCapabilities, TextDeliveryError> {
    let focused = OwnedAxElement::application(process_id)?.focused_control()?;
    capabilities_for_control(focused)
}

fn capabilities_for_control(
    focused: OwnedAxElement,
) -> Result<MacOsFocusedTextControlCapabilities, TextDeliveryError> {
    let process_id = focused.process_id()?;
    let role = focused.attribute_string(kAXRoleAttribute)?;
    let subrole = focused.attribute_string(kAXSubroleAttribute)?;
    let sensitive =
        secure_event_input_enabled() || subrole.as_deref() == Some(kAXSecureTextFieldSubrole);
    if sensitive {
        return Ok(MacOsFocusedTextControlCapabilities {
            bundle_identifier: bundle_identifier(process_id),
            role,
            subrole,
            selected_text_range: false,
            selected_text_settable: false,
            string_for_range: false,
            correction_observation: MacOsCorrectionObservationSupport::Sensitive,
        });
    }
    let selected_range = focused.selected_text_range()?;
    let string_for_range = match selected_range {
        Some(range) => focused
            .string_for_range(TextRange {
                location: range.location,
                length: 0,
            })?
            .is_some(),
        None => false,
    };
    let selected_text_settable = attribute_is_settable(&focused, kAXSelectedTextAttribute)?;

    Ok(MacOsFocusedTextControlCapabilities {
        bundle_identifier: bundle_identifier(process_id),
        role,
        subrole,
        selected_text_range: selected_range.is_some(),
        selected_text_settable,
        string_for_range,
        correction_observation: observation_support(
            false,
            selected_range.is_some(),
            string_for_range,
        ),
    })
}

fn bundle_identifier(process_id: i32) -> Option<String> {
    NSRunningApplication::runningApplicationWithProcessIdentifier(process_id)
        .and_then(|application| application.bundleIdentifier())
        .map(|identifier| identifier.to_string())
}

fn attribute_is_settable(
    element: &OwnedAxElement,
    attribute: &str,
) -> Result<bool, TextDeliveryError> {
    let attribute = core_foundation::string::CFString::new(attribute);
    let mut settable = 0;
    let error = unsafe {
        AXUIElementIsAttributeSettable(element.0, attribute.as_concrete_TypeRef(), &mut settable)
    };
    if error == kAXErrorSuccess {
        Ok(settable != 0)
    } else if error == kAXErrorAttributeUnsupported {
        Ok(false)
    } else {
        Err(system_error("inspect focused control attribute", error))
    }
}

fn observation_support(
    sensitive: bool,
    selected_text_range: bool,
    string_for_range: bool,
) -> MacOsCorrectionObservationSupport {
    if sensitive {
        MacOsCorrectionObservationSupport::Sensitive
    } else if selected_text_range && string_for_range {
        MacOsCorrectionObservationSupport::Observable
    } else {
        MacOsCorrectionObservationSupport::DeliveryOnly
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn observation_requires_both_range_capabilities() {
        assert_eq!(
            MacOsCorrectionObservationSupport::Observable,
            observation_support(false, true, true)
        );
        assert_eq!(
            MacOsCorrectionObservationSupport::DeliveryOnly,
            observation_support(false, true, false)
        );
        assert_eq!(
            MacOsCorrectionObservationSupport::DeliveryOnly,
            observation_support(false, false, true)
        );
    }

    #[test]
    fn sensitive_controls_are_never_observable() {
        assert_eq!(
            MacOsCorrectionObservationSupport::Sensitive,
            observation_support(true, true, true)
        );
    }
}
