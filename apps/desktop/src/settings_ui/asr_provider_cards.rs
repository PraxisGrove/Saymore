use slint::{ModelRc, VecModel};

use crate::ui::{AppWindow, AsrProviderCardKind, AsrProviderCardSpec};

#[derive(Clone, Copy)]
enum DesktopPlatform {
    #[cfg(any(target_os = "macos", test))]
    MacOs,
    #[cfg(any(target_os = "windows", test))]
    Windows,
    #[cfg(any(not(any(target_os = "macos", target_os = "windows")), test))]
    Other,
}

impl DesktopPlatform {
    #[cfg(target_os = "macos")]
    const CURRENT: Self = Self::MacOs;
    #[cfg(target_os = "windows")]
    const CURRENT: Self = Self::Windows;
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    const CURRENT: Self = Self::Other;

    fn system_card(self) -> Option<AsrProviderCardKind> {
        match self {
            #[cfg(any(target_os = "macos", test))]
            Self::MacOs => Some(AsrProviderCardKind::MacosDictation),
            #[cfg(any(target_os = "windows", test))]
            Self::Windows => None,
            #[cfg(any(not(any(target_os = "macos", target_os = "windows")), test))]
            Self::Other => None,
        }
    }

    fn system_card_available(self) -> bool {
        match self {
            #[cfg(any(target_os = "macos", test))]
            Self::MacOs => cfg!(debug_assertions),
            #[cfg(any(target_os = "windows", test))]
            Self::Windows => false,
            #[cfg(any(not(any(target_os = "macos", target_os = "windows")), test))]
            Self::Other => false,
        }
    }

    fn local_models_available(self) -> bool {
        match self {
            #[cfg(any(target_os = "macos", test))]
            Self::MacOs => true,
            #[cfg(any(target_os = "windows", test))]
            Self::Windows => true,
            #[cfg(any(not(any(target_os = "macos", target_os = "windows")), test))]
            Self::Other => false,
        }
    }

    fn whisper_available(self) -> bool {
        self.local_models_available()
    }

    fn qwen3_available(self) -> bool {
        self.local_models_available()
    }

    fn sense_voice_available(self) -> bool {
        self.local_models_available()
    }
}

pub(super) fn apply(ui: &AppWindow) {
    ui.set_asr_provider_cards(ModelRc::new(VecModel::from(provider_cards(
        DesktopPlatform::CURRENT,
    ))));
}

fn provider_cards(platform: DesktopPlatform) -> Vec<AsrProviderCardSpec> {
    let mut cards = vec![unavailable(AsrProviderCardKind::SaymoreCloud)];
    if let Some(system_card) = platform.system_card() {
        cards.push(if platform.system_card_available() {
            available(system_card)
        } else {
            unavailable(system_card)
        });
    }
    cards.extend([
        if platform.local_models_available() {
            available(AsrProviderCardKind::Paraformer)
        } else {
            unavailable(AsrProviderCardKind::Paraformer)
        },
        if platform.whisper_available() {
            available(AsrProviderCardKind::WhisperLargeV3Turbo)
        } else {
            unavailable(AsrProviderCardKind::WhisperLargeV3Turbo)
        },
        if platform.qwen3_available() {
            available(AsrProviderCardKind::Qwen3Asr)
        } else {
            unavailable(AsrProviderCardKind::Qwen3Asr)
        },
        if platform.sense_voice_available() {
            available(AsrProviderCardKind::SenseVoiceSmall)
        } else {
            unavailable(AsrProviderCardKind::SenseVoiceSmall)
        },
        available(AsrProviderCardKind::Volcengine),
        available(AsrProviderCardKind::Custom),
    ]);
    cards
}

fn available(kind: AsrProviderCardKind) -> AsrProviderCardSpec {
    AsrProviderCardSpec {
        kind,
        available: true,
    }
}

fn unavailable(kind: AsrProviderCardKind) -> AsrProviderCardSpec {
    AsrProviderCardSpec {
        kind,
        available: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unavailable_cards_are_visible_as_non_selectable_placeholders() {
        for platform in [DesktopPlatform::MacOs, DesktopPlatform::Windows] {
            let cards = provider_cards(platform);
            let expected_count = if matches!(platform, DesktopPlatform::MacOs) {
                8
            } else {
                7
            };
            assert_eq!(expected_count, cards.len());
            assert_eq!(1, cards.iter().filter(|card| !card.available).count());
        }
    }

    #[test]
    fn providers_follow_the_requested_order_for_each_platform() {
        let macos = provider_cards(DesktopPlatform::MacOs);
        assert_eq!(
            vec![
                AsrProviderCardKind::SaymoreCloud,
                AsrProviderCardKind::MacosDictation,
                AsrProviderCardKind::Paraformer,
                AsrProviderCardKind::WhisperLargeV3Turbo,
                AsrProviderCardKind::Qwen3Asr,
                AsrProviderCardKind::SenseVoiceSmall,
                AsrProviderCardKind::Volcengine,
                AsrProviderCardKind::Custom,
            ],
            macos.iter().map(|card| card.kind).collect::<Vec<_>>()
        );

        let windows = provider_cards(DesktopPlatform::Windows);
        assert_eq!(
            vec![
                AsrProviderCardKind::SaymoreCloud,
                AsrProviderCardKind::Paraformer,
                AsrProviderCardKind::WhisperLargeV3Turbo,
                AsrProviderCardKind::Qwen3Asr,
                AsrProviderCardKind::SenseVoiceSmall,
                AsrProviderCardKind::Volcengine,
                AsrProviderCardKind::Custom,
            ],
            windows.iter().map(|card| card.kind).collect::<Vec<_>>()
        );
    }

    #[test]
    fn sense_voice_is_the_last_local_model_before_cloud_apis() {
        for platform in [DesktopPlatform::MacOs, DesktopPlatform::Windows] {
            let cards = provider_cards(platform);
            let sense_voice_index = cards
                .iter()
                .position(|card| card.kind == AsrProviderCardKind::SenseVoiceSmall);
            let volcengine_index = cards
                .iter()
                .position(|card| card.kind == AsrProviderCardKind::Volcengine);
            assert_eq!(
                sense_voice_index.and_then(|index| index.checked_add(1)),
                volcengine_index
            );
            assert!(
                sense_voice_index.is_some_and(|index| {
                    cards[index].available == platform.sense_voice_available()
                }),
                "SenseVoice availability must match the local sherpa runtime"
            );
        }
    }

    #[test]
    fn paraformer_is_available_on_supported_desktop_platforms() {
        for (platform, expected) in [
            (DesktopPlatform::MacOs, true),
            (DesktopPlatform::Windows, true),
            (DesktopPlatform::Other, false),
        ] {
            let paraformer = provider_cards(platform)
                .into_iter()
                .find(|card| card.kind == AsrProviderCardKind::Paraformer);
            assert_eq!(expected, paraformer.is_some_and(|card| card.available));
        }
    }

    #[test]
    fn whisper_is_available_on_supported_desktop_platforms() {
        for (platform, expected) in [
            (DesktopPlatform::MacOs, true),
            (DesktopPlatform::Windows, true),
            (DesktopPlatform::Other, false),
        ] {
            let whisper = provider_cards(platform)
                .into_iter()
                .find(|card| card.kind == AsrProviderCardKind::WhisperLargeV3Turbo);
            assert_eq!(expected, whisper.is_some_and(|card| card.available));
        }
    }

    #[test]
    fn qwen3_is_available_on_supported_desktop_platforms() {
        for (platform, expected) in [
            (DesktopPlatform::MacOs, true),
            (DesktopPlatform::Windows, true),
            (DesktopPlatform::Other, false),
        ] {
            let qwen3 = provider_cards(platform)
                .into_iter()
                .find(|card| card.kind == AsrProviderCardKind::Qwen3Asr);
            assert_eq!(expected, qwen3.is_some_and(|card| card.available));
        }
    }
}
