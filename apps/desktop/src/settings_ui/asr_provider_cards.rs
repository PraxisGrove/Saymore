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
        local_model(platform, AsrProviderCardKind::WhisperLargeV3Turbo),
        local_model(platform, AsrProviderCardKind::Qwen3Asr),
        local_model(platform, AsrProviderCardKind::SeacoParaformer),
        local_model(platform, AsrProviderCardKind::Zipformer),
        available(AsrProviderCardKind::Volcengine),
        available(AsrProviderCardKind::Openai),
        available(AsrProviderCardKind::Groq),
        available(AsrProviderCardKind::Mistral),
        available(AsrProviderCardKind::Custom),
    ]);
    cards
}

fn local_model(platform: DesktopPlatform, kind: AsrProviderCardKind) -> AsrProviderCardSpec {
    if platform.local_models_available() {
        available(kind)
    } else {
        unavailable(kind)
    }
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
    fn providers_follow_the_requested_order_for_each_platform() {
        let macos = provider_cards(DesktopPlatform::MacOs);
        assert_eq!(
            vec![
                AsrProviderCardKind::SaymoreCloud,
                AsrProviderCardKind::MacosDictation,
                AsrProviderCardKind::WhisperLargeV3Turbo,
                AsrProviderCardKind::Qwen3Asr,
                AsrProviderCardKind::SeacoParaformer,
                AsrProviderCardKind::Zipformer,
                AsrProviderCardKind::Volcengine,
                AsrProviderCardKind::Openai,
                AsrProviderCardKind::Groq,
                AsrProviderCardKind::Mistral,
                AsrProviderCardKind::Custom,
            ],
            macos.iter().map(|card| card.kind).collect::<Vec<_>>()
        );

        let windows = provider_cards(DesktopPlatform::Windows);
        assert_eq!(
            vec![
                AsrProviderCardKind::SaymoreCloud,
                AsrProviderCardKind::WhisperLargeV3Turbo,
                AsrProviderCardKind::Qwen3Asr,
                AsrProviderCardKind::SeacoParaformer,
                AsrProviderCardKind::Zipformer,
                AsrProviderCardKind::Volcengine,
                AsrProviderCardKind::Openai,
                AsrProviderCardKind::Groq,
                AsrProviderCardKind::Mistral,
                AsrProviderCardKind::Custom,
            ],
            windows.iter().map(|card| card.kind).collect::<Vec<_>>()
        );
    }

    #[test]
    fn local_models_are_available_on_supported_platforms() {
        for platform in [
            DesktopPlatform::MacOs,
            DesktopPlatform::Windows,
            DesktopPlatform::Other,
        ] {
            let cards = provider_cards(platform);
            for kind in [
                AsrProviderCardKind::WhisperLargeV3Turbo,
                AsrProviderCardKind::Qwen3Asr,
                AsrProviderCardKind::SeacoParaformer,
                AsrProviderCardKind::Zipformer,
            ] {
                assert_eq!(
                    platform.local_models_available(),
                    cards.iter().any(|card| card.kind == kind && card.available)
                );
            }
        }
    }
}
