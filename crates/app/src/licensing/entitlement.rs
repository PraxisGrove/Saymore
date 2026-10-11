//! Provider-independent trial and entitlement rules (approved pricing design):
//! - 14-day full trial from first launch, no card.
//! - After the trial, dictation stays free; refinement and the personal
//!   dictionary pause, but dictionary data is kept.
//! - A license unlocks everything on up to 3 devices and includes 12 months of
//!   updates; after that the installed version keeps working.
//! - A deactivated / over-limit device keeps free dictation.

pub const TRIAL_DAYS: i64 = 14;
pub const DEVICE_LIMIT: u32 = 3;
pub const UPDATE_WINDOW_DAYS: i64 = 365;
const DAY: i64 = 86_400;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Feature {
    Dictation,
    Refinement,
    PersonalDictionary,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LicenseRecord {
    pub purchased_at: i64,
    pub revoked: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Entitlement {
    Trial { days_left: i64 },
    TrialEnded,
    Licensed { updates_until: i64 },
}

impl Entitlement {
    pub fn allows(self, feature: Feature) -> bool {
        match (self, feature) {
            (_, Feature::Dictation) => true,
            (Self::TrialEnded, _) => false,
            _ => true,
        }
    }

    /// Whether a build released at `build_released_at` may be used with full
    /// features. Licensed users keep their version forever; newer builds past
    /// the update window fall back to free features ("updates ended" screen).
    pub fn covers_build(self, build_released_at: i64) -> bool {
        match self {
            Self::Licensed { updates_until } => build_released_at <= updates_until,
            _ => true,
        }
    }
}

/// `first_launch` and `now` are Unix seconds.
pub fn entitlement_at(first_launch: i64, now: i64, license: Option<&LicenseRecord>) -> Entitlement {
    if let Some(l) = license.filter(|l| !l.revoked) {
        return Entitlement::Licensed {
            updates_until: l.purchased_at + UPDATE_WINDOW_DAYS * DAY,
        };
    }
    let elapsed = (now - first_launch).max(0);
    let used_days = elapsed / DAY;
    if used_days < TRIAL_DAYS {
        Entitlement::Trial {
            days_left: TRIAL_DAYS - used_days,
        }
    } else {
        Entitlement::TrialEnded
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn trial_runs_fourteen_days_then_dictation_only() {
        assert_eq!(
            entitlement_at(0, 0, None),
            Entitlement::Trial { days_left: 14 }
        );
        assert_eq!(
            entitlement_at(0, 13 * DAY + 1, None),
            Entitlement::Trial { days_left: 1 }
        );
        let ended = entitlement_at(0, 14 * DAY, None);
        assert_eq!(ended, Entitlement::TrialEnded);
        assert!(ended.allows(Feature::Dictation));
        assert!(!ended.allows(Feature::Refinement));
        assert!(!ended.allows(Feature::PersonalDictionary));
    }

    #[test]
    fn license_unlocks_and_bounds_updates() {
        let rec = LicenseRecord {
            purchased_at: 100,
            revoked: false,
        };
        let e = entitlement_at(0, 999 * DAY, Some(&rec));
        assert!(e.allows(Feature::Refinement));
        assert!(e.covers_build(100 + 365 * DAY));
        assert!(!e.covers_build(100 + 366 * DAY));
        let revoked = LicenseRecord {
            revoked: true,
            ..rec
        };
        assert_eq!(
            entitlement_at(0, 999 * DAY, Some(&revoked)),
            Entitlement::TrialEnded
        );
    }
}
