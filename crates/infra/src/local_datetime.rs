use jiff::{Timestamp, civil::DateTime, tz::TimeZone};

/// Returns the system's local date and time for an instant. On Windows this
/// honors custom time zones and the user's daylight saving settings. If Windows
/// cannot convert the date, Jiff's system time zone supplies the fallback.
pub fn system_local_datetime(instant: Timestamp) -> DateTime {
    #[cfg(target_os = "windows")]
    if let Some(local) = win32::system_local(instant) {
        return local;
    }
    TimeZone::system().to_datetime(instant)
}

#[cfg(target_os = "windows")]
mod win32 {
    use super::*;
    use windows::Win32::{
        Foundation::SYSTEMTIME,
        System::Time::{
            GetTimeZoneInformationForYear, SystemTimeToTzSpecificLocalTime, TIME_ZONE_INFORMATION,
        },
    };

    pub(super) fn system_local(instant: Timestamp) -> Option<DateTime> {
        let utc = TimeZone::UTC.to_datetime(instant);
        if utc.year() < 1601 {
            return None;
        }
        let mut rules = TIME_ZONE_INFORMATION::default();
        // SAFETY: None selects the current Windows zone. The API writes to our live structure.
        unsafe { GetTimeZoneInformationForYear(utc.year() as u16, None, &mut rules) }.ok()?;
        apply_rules(utc, &rules)
    }

    fn apply_rules(utc: DateTime, rules: &TIME_ZONE_INFORMATION) -> Option<DateTime> {
        let source = SYSTEMTIME {
            wYear: utc.year() as u16,
            wMonth: utc.month() as u16,
            wDay: utc.day() as u16,
            wHour: utc.hour() as u16,
            wMinute: utc.minute() as u16,
            wSecond: utc.second() as u16,
            wMilliseconds: utc.millisecond() as u16,
            ..SYSTEMTIME::default()
        };
        let mut local = SYSTEMTIME::default();
        // SAFETY: The inputs and the distinct output stay valid throughout the call.
        unsafe { SystemTimeToTzSpecificLocalTime(Some(rules), &source, &mut local) }.ok()?;
        DateTime::new(
            local.wYear.try_into().ok()?,
            local.wMonth.try_into().ok()?,
            local.wDay.try_into().ok()?,
            local.wHour.try_into().ok()?,
            local.wMinute.try_into().ok()?,
            local.wSecond.try_into().ok()?,
            utc.subsec_nanosecond(),
        )
        .ok()
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use jiff::civil::date;

        #[test]
        fn disabled_daylight_saving_keeps_a_summer_record_on_the_previous_day() {
            // Windows reports no transition dates when daylight saving is off.
            let rules = TIME_ZONE_INFORMATION {
                Bias: 300,
                ..TIME_ZONE_INFORMATION::default()
            };
            let utc = date(2026, 7, 2).at(4, 30, 0, 123_456_789);
            let local = date(2026, 7, 1).at(23, 30, 0, 123_456_789);
            assert_eq!(Some(local), apply_rules(utc, &rules));
        }
    }
}
