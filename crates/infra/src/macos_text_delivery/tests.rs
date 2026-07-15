use super::{
    AccessibilityAuthorization, DeliveryTargetAction, DeliveryTargetState, FocusResolutionAction,
    FocusSnapshot, InsertionVerification, SecureSubrole, TextRange, authorization_from,
    delivery_target_action, delivery_target_privacy, focus_resolution_action,
    insertion_range_matches, text_between_anchors, verify_observed_insertion,
};
use template_app::DeliveryTargetPrivacy;

#[test]
fn selects_a_safe_delivery_path_for_the_current_focus_state() {
    assert_eq!(
        [
            DeliveryTargetAction::UseFocusedControl,
            DeliveryTargetAction::PasteWithoutVerification,
            DeliveryTargetAction::PasteSecurely,
            DeliveryTargetAction::RejectNoTarget,
        ],
        [
            delivery_target_action(DeliveryTargetState {
                external_target: true,
                secure_input: false,
                focused_control: true,
            }),
            delivery_target_action(DeliveryTargetState {
                external_target: true,
                secure_input: false,
                focused_control: false,
            }),
            delivery_target_action(DeliveryTargetState {
                external_target: true,
                secure_input: true,
                focused_control: false,
            }),
            delivery_target_action(DeliveryTargetState {
                external_target: false,
                secure_input: false,
                focused_control: false,
            }),
        ]
    );
}

#[test]
fn secure_or_unknown_targets_are_sensitive_before_delivery() {
    assert_eq!(
        [
            DeliveryTargetPrivacy::Sensitive,
            DeliveryTargetPrivacy::Sensitive,
            DeliveryTargetPrivacy::Sensitive,
            DeliveryTargetPrivacy::Standard,
            DeliveryTargetPrivacy::Standard,
        ],
        [
            delivery_target_privacy(
                DeliveryTargetState {
                    external_target: true,
                    secure_input: true,
                    focused_control: false,
                },
                None,
            ),
            delivery_target_privacy(
                DeliveryTargetState {
                    external_target: true,
                    secure_input: false,
                    focused_control: true,
                },
                Some(SecureSubrole::Secure),
            ),
            delivery_target_privacy(
                DeliveryTargetState {
                    external_target: true,
                    secure_input: false,
                    focused_control: true,
                },
                Some(SecureSubrole::Unknown),
            ),
            delivery_target_privacy(
                DeliveryTargetState {
                    external_target: true,
                    secure_input: false,
                    focused_control: true,
                },
                Some(SecureSubrole::Standard),
            ),
            delivery_target_privacy(
                DeliveryTargetState {
                    external_target: false,
                    secure_input: false,
                    focused_control: false,
                },
                None,
            ),
        ]
    );
}

#[test]
fn unresolved_external_focus_is_sensitive() {
    assert_eq!(
        DeliveryTargetPrivacy::Sensitive,
        delivery_target_privacy(
            DeliveryTargetState {
                external_target: true,
                secure_input: false,
                focused_control: false,
            },
            None,
        )
    );
}

#[test]
fn prefers_external_focus_and_recovers_from_stale_saymore_focus() {
    assert_eq!(
        FocusResolutionAction::UseSystemFocus,
        focus_resolution_action(FocusSnapshot {
            current_process: 10,
            system_focused_process: Some(20),
            frontmost_external_process: Some(30),
        })
    );
    assert_eq!(
        FocusResolutionAction::QueryFrontmostApplication,
        focus_resolution_action(FocusSnapshot {
            current_process: 10,
            system_focused_process: Some(10),
            frontmost_external_process: Some(30),
        })
    );
    assert_eq!(
        FocusResolutionAction::RejectNoTarget,
        focus_resolution_action(FocusSnapshot {
            current_process: 10,
            system_focused_process: Some(10),
            frontmost_external_process: None,
        })
    );
}

#[test]
fn maps_accessibility_trust_to_authorization() {
    assert_eq!(
        [
            AccessibilityAuthorization::Granted,
            AccessibilityAuthorization::Denied,
        ],
        [authorization_from(true), authorization_from(false)]
    );
}

#[test]
fn verifies_collapsed_cursor_after_unicode_insertion() {
    assert!(insertion_range_matches(
        TextRange {
            location: 3,
            length: 0,
        },
        TextRange {
            location: 6,
            length: 0,
        },
        "A😀"
    ));
}

#[test]
fn verifies_inserted_text_when_control_keeps_it_selected() {
    assert!(insertion_range_matches(
        TextRange {
            location: 3,
            length: 5,
        },
        TextRange {
            location: 3,
            length: 3,
        },
        "A😀"
    ));
}

#[test]
fn rejects_a_matching_cursor_when_the_inserted_text_differs() {
    let initial = TextRange {
        location: 3,
        length: 0,
    };
    let current = TextRange {
        location: 5,
        length: 0,
    };

    assert_eq!(
        InsertionVerification::Verified,
        verify_observed_insertion(initial, current, "测试", Some("测试"))
    );
    assert_eq!(
        InsertionVerification::Unverified,
        verify_observed_insertion(initial, current, "测试", Some("侧式"))
    );
    assert_eq!(
        InsertionVerification::Verified,
        verify_observed_insertion(initial, current, "测试", None)
    );
}

#[test]
fn rejects_unchanged_or_unrelated_cursor_ranges() {
    let initial = TextRange {
        location: 3,
        length: 0,
    };

    assert!(!insertion_range_matches(initial, initial, "测试"));
    assert!(!insertion_range_matches(
        initial,
        TextRange {
            location: 4,
            length: 0,
        },
        "测试"
    ));
}

#[test]
fn extracts_only_text_between_delivery_anchors() {
    assert_eq!(
        Some("我们使用 Saymore".to_owned()),
        text_between_anchors("前文我们使用 Saymore后文", "前文", "后文")
    );
    assert_eq!(
        Some("Saymore".to_owned()),
        text_between_anchors("前文Saymore", "前文", "")
    );
}

#[test]
fn rejects_windows_that_no_longer_contain_the_original_anchors() {
    assert_eq!(
        None,
        text_between_anchors("其他Saymore后文", "前文", "后文")
    );
    assert_eq!(
        None,
        text_between_anchors("前文Saymore其他", "前文", "后文")
    );
}
