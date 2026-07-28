use slint::{ModelRc, VecModel};

use crate::ui::{AppWindow, AsrProviderCardKind, AsrProviderCardSpec};

#[derive(Clone, Copy)]
enum CardListMode {
    Production,
    Development,
}

impl CardListMode {
    fn for_development_environment(development_environment: bool) -> Self {
        if development_environment {
            Self::Development
        } else {
            Self::Production
        }
    }

    fn shows_placeholders(self) -> bool {
        matches!(self, Self::Development)
    }
}

#[derive(Clone, Copy)]
enum DesktopPlatform {
    #[cfg(any(target_os = "macos", test))]
    Macos,
    #[cfg(any(target_os = "windows", test))]
    Windows,
    #[cfg(any(not(any(target_os = "macos", target_os = "windows")), test))]
    Other,
}

impl DesktopPlatform {
    #[cfg(target_os = "macos")]
    const CURRENT: Self = Self::Macos;
    #[cfg(target_os = "windows")]
    const CURRENT: Self = Self::Windows;
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    const CURRENT: Self = Self::Other;

    fn system_card(self) -> Option<AsrProviderCardKind> {
        match self {
            #[cfg(any(target_os = "macos", test))]
            Self::Macos => Some(AsrProviderCardKind::MacosDictation),
            #[cfg(any(target_os = "windows", test))]
            Self::Windows => Some(AsrProviderCardKind::WindowsSpeech),
            #[cfg(any(not(any(target_os = "macos", target_os = "windows")), test))]
            Self::Other => None,
        }
    }

    fn system_card_available(self) -> bool {
        match self {
            #[cfg(any(target_os = "macos", test))]
            Self::Macos => true,
            #[cfg(any(target_os = "windows", test))]
            Self::Windows => false,
            #[cfg(any(not(any(target_os = "macos", target_os = "windows")), test))]
            Self::Other => false,
        }
    }
}

pub(super) fn apply(ui: &AppWindow) {
    ui.set_asr_provider_cards(ModelRc::new(VecModel::from(provider_cards(
        CardListMode::for_development_environment(ui.get_development_environment()),
        DesktopPlatform::CURRENT,
    ))));
}

fn provider_cards(mode: CardListMode, platform: DesktopPlatform) -> Vec<AsrProviderCardSpec> {
    let mut cards = Vec::new();
    if mode.shows_placeholders() {
        cards.push(unavailable(AsrProviderCardKind::SaymoreCloud));
    }
    if let Some(system_card) = platform.system_card()
        && (platform.system_card_available() || mode.shows_placeholders())
    {
        cards.push(if platform.system_card_available() {
            available(system_card)
        } else {
            unavailable(system_card)
        });
    }
    cards.push(available(AsrProviderCardKind::Volcengine));
    if mode.shows_placeholders() {
        cards.extend([
            unavailable(AsrProviderCardKind::Paraformer),
            unavailable(AsrProviderCardKind::WhisperLargeV3Turbo),
            unavailable(AsrProviderCardKind::Qwen3Asr),
        ]);
    }
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
        for mode in [CardListMode::Production, CardListMode::Development] {
            for platform in [DesktopPlatform::Macos, DesktopPlatform::Windows] {
                let cards = provider_cards(mode, platform);
                assert_eq!(
                    Some(AsrProviderCardKind::Custom),
                    cards.last().map(|card| card.kind)
                );
            }
        }
    }

    #[test]
    fn unavailable_cards_are_visible_only_as_development_placeholders() {
        assert!(matches!(
            CardListMode::for_development_environment(false),
            CardListMode::Production
        ));
        assert!(matches!(
            CardListMode::for_development_environment(true),
            CardListMode::Development
        ));
        for platform in [DesktopPlatform::Macos, DesktopPlatform::Windows] {
            let production = provider_cards(CardListMode::Production, platform);
            let expected = if matches!(platform, DesktopPlatform::Macos) {
                vec![
                    AsrProviderCardKind::MacosDictation,
                    AsrProviderCardKind::Volcengine,
                    AsrProviderCardKind::Custom,
                ]
            } else {
                vec![AsrProviderCardKind::Volcengine, AsrProviderCardKind::Custom]
            };
            assert_eq!(
                expected,
                production.iter().map(|card| card.kind).collect::<Vec<_>>()
            );
            assert!(production.iter().all(|card| card.available));
        }
    }

    #[test]
    fn development_uses_only_the_current_platform_system_card() {
        let macos = provider_cards(CardListMode::Development, DesktopPlatform::Macos);
        assert_eq!(
            vec![
                AsrProviderCardKind::SaymoreCloud,
                AsrProviderCardKind::MacosDictation,
                AsrProviderCardKind::Volcengine,
                AsrProviderCardKind::Paraformer,
                AsrProviderCardKind::WhisperLargeV3Turbo,
                AsrProviderCardKind::Qwen3Asr,
                AsrProviderCardKind::Custom,
            ],
            macos.iter().map(|card| card.kind).collect::<Vec<_>>()
        );

        let windows = provider_cards(CardListMode::Development, DesktopPlatform::Windows);
        assert_eq!(
            vec![
                AsrProviderCardKind::SaymoreCloud,
                AsrProviderCardKind::WindowsSpeech,
                AsrProviderCardKind::Volcengine,
                AsrProviderCardKind::Paraformer,
                AsrProviderCardKind::WhisperLargeV3Turbo,
                AsrProviderCardKind::Qwen3Asr,
                AsrProviderCardKind::Custom,
            ],
            windows.iter().map(|card| card.kind).collect::<Vec<_>>()
        );
    }

    #[test]
    fn development_placeholders_cannot_be_selected() {
        for platform in [
            DesktopPlatform::Macos,
            DesktopPlatform::Windows,
            DesktopPlatform::Other,
        ] {
            let development = provider_cards(CardListMode::Development, platform);
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
