#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ThemeId {
    #[default]
    LimePulse,
    WarmClay,
    BerryGraphite,
    IrisMist,
    ClearSky,
}

impl ThemeId {
    pub const fn storage_value(self) -> &'static str {
        match self {
            Self::WarmClay => "warm-clay",
            Self::LimePulse => "lime-pulse",
            Self::BerryGraphite => "berry-graphite",
            Self::IrisMist => "iris-mist",
            Self::ClearSky => "clear-sky",
        }
    }

    pub fn from_storage_value(value: &str) -> Option<Self> {
        match value {
            "warm-clay" => Some(Self::WarmClay),
            "lime-pulse" => Some(Self::LimePulse),
            "berry-graphite" => Some(Self::BerryGraphite),
            "iris-mist" => Some(Self::IrisMist),
            "clear-sky" => Some(Self::ClearSky),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ColorSchemePreference {
    #[default]
    System,
    Light,
    Dark,
}

impl ColorSchemePreference {
    pub const fn storage_value(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::Light => "light",
            Self::Dark => "dark",
        }
    }

    pub fn from_storage_value(value: &str) -> Option<Self> {
        match value {
            "system" => Some(Self::System),
            "light" => Some(Self::Light),
            "dark" => Some(Self::Dark),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{ColorSchemePreference, ThemeId};

    #[test]
    fn theme_storage_values_round_trip() {
        for theme in [
            ThemeId::LimePulse,
            ThemeId::WarmClay,
            ThemeId::BerryGraphite,
            ThemeId::IrisMist,
            ThemeId::ClearSky,
        ] {
            assert_eq!(
                Some(theme),
                ThemeId::from_storage_value(theme.storage_value())
            );
        }
        assert_eq!(None, ThemeId::from_storage_value("unknown"));
    }

    #[test]
    fn color_scheme_storage_values_round_trip() {
        for scheme in [
            ColorSchemePreference::System,
            ColorSchemePreference::Light,
            ColorSchemePreference::Dark,
        ] {
            assert_eq!(
                Some(scheme),
                ColorSchemePreference::from_storage_value(scheme.storage_value())
            );
        }
        assert_eq!(None, ColorSchemePreference::from_storage_value("automatic"));
    }
}
