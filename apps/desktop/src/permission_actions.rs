use template_app::MicrophoneAuthorization;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PermissionAction {
    Request,
    OpenSettings,
}

pub(crate) fn microphone_permission_action(status: MicrophoneAuthorization) -> PermissionAction {
    match status {
        MicrophoneAuthorization::NotDetermined => PermissionAction::Request,
        MicrophoneAuthorization::Granted
        | MicrophoneAuthorization::Denied
        | MicrophoneAuthorization::Restricted => PermissionAction::OpenSettings,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_microphone_request_uses_the_native_prompt() {
        assert_eq!(
            PermissionAction::Request,
            microphone_permission_action(MicrophoneAuthorization::NotDetermined)
        );
    }

    #[test]
    fn denied_microphone_permission_opens_system_settings() {
        assert_eq!(
            PermissionAction::OpenSettings,
            microphone_permission_action(MicrophoneAuthorization::Denied)
        );
    }

    #[test]
    fn granted_microphone_permission_can_still_be_managed_in_system_settings() {
        assert_eq!(
            PermissionAction::OpenSettings,
            microphone_permission_action(MicrophoneAuthorization::Granted)
        );
    }
}
