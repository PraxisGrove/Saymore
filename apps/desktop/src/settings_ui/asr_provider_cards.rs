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
            Self::Windows => Some(AsrProviderCardKind::WindowsSpeech),
            #[cfg(any(not(any(target_os = "macos", target_os = "windows")), test))]
            Self::Other => None,
        }
    }

    fn system_card_available(self) -> bool {
        match self {
            #[cfg(any(target_os = "macos", test))]
            Self::MacOs => true,
            #[cfg(any(target_os = "windows", test))]
            Self::Windows => false,
            #[cfg(any(not(any(target_os = "macos", target_os = "windows")), test))]
            Self::Other => false,
        }
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
        unavailable(AsrProviderCardKind::Qwen3Asr),
        unavailable(AsrProviderCardKind::WhisperLargeV3Turbo),
        available(AsrProviderCardKind::Volcengine),
        unavailable(AsrProviderCardKind::Paraformer),
    ]);
    cards.push(available(AsrProviderCardKind::Custom));
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
    fn custom_provider_is_always_last() {
        for platform in [DesktopPlatform::MacOs, DesktopPlatform::Windows] {
            let cards = provider_cards(platform);
            assert_eq!(
                Some(AsrProviderCardKind::Custom),
                cards.last().map(|card| card.kind)
            );
        }
    }

    #[test]
    fn unavailable_cards_are_visible_as_non_selectable_placeholders() {
        for platform in [DesktopPlatform::MacOs, DesktopPlatform::Windows] {
            let cards = provider_cards(platform);
            assert_eq!(7, cards.len());
            assert!(cards.iter().filter(|card| !card.available).count() >= 4);
        }
    }

    #[test]
    fn development_uses_only_the_current_platform_system_card() {
        let macos = provider_cards(DesktopPlatform::MacOs);
        assert_eq!(
            vec![
                AsrProviderCardKind::SaymoreCloud,
                AsrProviderCardKind::MacosDictation,
                AsrProviderCardKind::Qwen3Asr,
                AsrProviderCardKind::WhisperLargeV3Turbo,
                AsrProviderCardKind::Volcengine,
                AsrProviderCardKind::Paraformer,
                AsrProviderCardKind::Custom,
            ],
            macos.iter().map(|card| card.kind).collect::<Vec<_>>()
        );

        let windows = provider_cards(DesktopPlatform::Windows);
        assert_eq!(
            vec![
                AsrProviderCardKind::SaymoreCloud,
                AsrProviderCardKind::WindowsSpeech,
                AsrProviderCardKind::Qwen3Asr,
                AsrProviderCardKind::WhisperLargeV3Turbo,
                AsrProviderCardKind::Volcengine,
                AsrProviderCardKind::Paraformer,
                AsrProviderCardKind::Custom,
            ],
            windows.iter().map(|card| card.kind).collect::<Vec<_>>()
        );
    }

    #[test]
    fn development_placeholders_cannot_be_selected() {
        for platform in [
            DesktopPlatform::MacOs,
            DesktopPlatform::Windows,
            DesktopPlatform::Other,
        ] {
            let development = provider_cards(platform);
            assert!(
                development
                    .iter()
                    .filter(|card| {
                        !matches!(
                            card.kind,
                            AsrProviderCardKind::MacosDictation
                                | AsrProviderCardKind::Volcengine
                                | AsrProviderCardKind::Custom
                        )
                    })
                    .all(|card| !card.available)
            );
        }
    }
}
