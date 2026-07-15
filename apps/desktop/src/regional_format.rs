use std::str::FromStr;

use chrono::Locale as DateLocale;
use num_format::{Locale as NumberLocale, ToFormattedString};

pub fn system_locale() -> Option<String> {
    sys_locale::get_locale()
}

pub fn date_locale(locale: Option<&str>) -> DateLocale {
    locale_candidates(locale)
        .find_map(|candidate| DateLocale::from_str(&candidate).ok())
        .unwrap_or(DateLocale::POSIX)
}

pub fn format_integer(value: u64, locale: Option<&str>) -> String {
    let locale = locale_candidates(locale)
        .find_map(|candidate| NumberLocale::from_name(&candidate).ok())
        .unwrap_or(NumberLocale::en);
    value.to_formatted_string(&locale)
}

pub fn decimal_separator(locale: Option<&str>) -> &'static str {
    locale_candidates(locale)
        .find_map(|candidate| NumberLocale::from_name(&candidate).ok())
        .unwrap_or(NumberLocale::en)
        .decimal()
}

fn locale_candidates(locale: Option<&str>) -> impl Iterator<Item = String> {
    let normalized = locale
        .unwrap_or("en")
        .split(['.', '@'])
        .next()
        .unwrap_or("en")
        .replace('-', "_");
    let parts = normalized.split('_').collect::<Vec<_>>();
    let language_region =
        (parts.len() >= 3 && parts[1].len() == 4).then(|| format!("{}_{}", parts[0], parts[2]));
    let language = parts.first().map(|part| (*part).to_owned());
    [Some(normalized), language_region, language]
        .into_iter()
        .flatten()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn number_format_uses_the_device_locale() {
        assert_eq!("6,240", format_integer(6_240, Some("en-US")));
        assert_eq!("6.240", format_integer(6_240, Some("de-DE")));
        assert_eq!(",", decimal_separator(Some("de-DE")));
    }

    #[test]
    fn script_locale_falls_back_to_its_region() {
        assert_eq!(DateLocale::zh_CN, date_locale(Some("zh-Hans-CN")));
    }
}
