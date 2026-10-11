//! Replaceable licensing layer.
//!
//! The app only talks to [`LicenseProvider`]. Waffo is the preferred backend,
//! Paddle the fallback; both are stubs until merchant credentials exist, and
//! report [`LicenseError::NotConfigured`] instead of pretending to activate.
//! Trial and entitlement rules live in [`entitlement`] and are provider-free.

mod entitlement;
mod paddle;
mod waffo;

use async_trait::async_trait;
use thiserror::Error;

pub use entitlement::{
    DEVICE_LIMIT, Entitlement, Feature, LicenseRecord, TRIAL_DAYS, UPDATE_WINDOW_DAYS,
    entitlement_at,
};
pub use paddle::PaddleLicenseProvider;
pub use waffo::WaffoLicenseProvider;

/// Stable per-install device identifier sent on activation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceId(pub String);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActivatedLicense {
    pub key: String,
    pub activation_id: String,
    pub devices_used: u32,
    pub device_limit: u32,
    /// Unix seconds of purchase; drives the 12-month update window.
    pub purchased_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum LicenseError {
    #[error("license provider {0} is not configured")]
    NotConfigured(&'static str),
    #[error("license key is invalid")]
    InvalidKey,
    /// All seats are taken; UI offers "Contact support" and in-app deactivation.
    #[error("device limit reached ({used}/{limit})")]
    DeviceLimitReached { used: u32, limit: u32 },
    #[error("license was refunded or revoked")]
    Revoked,
    #[error("license provider unreachable: {0}")]
    Transport(String),
}

#[async_trait]
pub trait LicenseProvider: Send + Sync {
    fn id(&self) -> &'static str;
    fn is_configured(&self) -> bool;
    async fn activate(
        &self,
        key: &str,
        device: &DeviceId,
    ) -> Result<ActivatedLicense, LicenseError>;
    async fn validate(
        &self,
        license: &ActivatedLicense,
        device: &DeviceId,
    ) -> Result<(), LicenseError>;
    /// Frees this device's seat (self-service device moves).
    async fn deactivate(
        &self,
        license: &ActivatedLicense,
        device: &DeviceId,
    ) -> Result<(), LicenseError>;
}

/// Picks the provider: `SAYMORE_LICENSE_PROVIDER` (`waffo` | `paddle`) if set,
/// otherwise Waffo when configured, then Paddle, else Waffo (unconfigured).
pub fn select_provider(env: impl Fn(&str) -> Option<String>) -> Box<dyn LicenseProvider> {
    let waffo = WaffoLicenseProvider::from_env(&env);
    let paddle = PaddleLicenseProvider::from_env(&env);
    match env("SAYMORE_LICENSE_PROVIDER").as_deref().map(str::trim) {
        Some("paddle") => Box::new(paddle),
        Some("waffo") => Box::new(waffo),
        _ if waffo.is_configured() => Box::new(waffo),
        _ if paddle.is_configured() => Box::new(paddle),
        _ => Box::new(waffo),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn prefers_waffo_then_paddle() {
        assert_eq!(select_provider(|_| None).id(), "waffo");
        let only_paddle = |n: &str| (n == "SAYMORE_PADDLE_API_KEY").then(|| "k".to_owned());
        assert_eq!(select_provider(only_paddle).id(), "paddle");
        let both = |n: &str| {
            matches!(n, "SAYMORE_PADDLE_API_KEY" | "SAYMORE_WAFFO_API_KEY").then(|| "k".to_owned())
        };
        assert_eq!(select_provider(both).id(), "waffo");
        let forced = |n: &str| match n {
            "SAYMORE_LICENSE_PROVIDER" => Some("paddle".to_owned()),
            "SAYMORE_WAFFO_API_KEY" => Some("k".to_owned()),
            _ => None,
        };
        assert_eq!(select_provider(forced).id(), "paddle");
    }

    #[tokio::test]
    async fn unconfigured_stub_refuses_to_activate() {
        let p = select_provider(|_| None);
        let err = p.activate("KEY", &DeviceId("d".into())).await.unwrap_err();
        assert_eq!(err, LicenseError::NotConfigured("waffo"));
    }
}
