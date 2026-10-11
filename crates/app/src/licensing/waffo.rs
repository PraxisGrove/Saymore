//! Waffo license backend. Stub: real HTTP calls are added once merchant
//! credentials and API docs are confirmed. Credentials come only from
//! configuration / environment (`SAYMORE_WAFFO_API_KEY`), never source.

use async_trait::async_trait;

use super::{ActivatedLicense, DeviceId, LicenseError, LicenseProvider};

#[derive(Debug, Clone, Default)]
pub struct WaffoLicenseProvider {
    api_key: Option<String>,
}

impl WaffoLicenseProvider {
    pub fn new(api_key: Option<String>) -> Self {
        Self {
            api_key: api_key.filter(|k| !k.trim().is_empty()),
        }
    }

    pub fn from_env(env: impl Fn(&str) -> Option<String>) -> Self {
        Self::new(env("SAYMORE_WAFFO_API_KEY"))
    }

    fn require(&self) -> Result<&str, LicenseError> {
        self.api_key
            .as_deref()
            .ok_or(LicenseError::NotConfigured("waffo"))
    }
}

#[async_trait]
impl LicenseProvider for WaffoLicenseProvider {
    fn id(&self) -> &'static str {
        "waffo"
    }

    fn is_configured(&self) -> bool {
        self.api_key.is_some()
    }

    async fn activate(
        &self,
        _key: &str,
        _device: &DeviceId,
    ) -> Result<ActivatedLicense, LicenseError> {
        self.require()?;
        Err(LicenseError::Transport(
            "waffo activation not implemented yet".into(),
        ))
    }

    async fn validate(
        &self,
        _license: &ActivatedLicense,
        _device: &DeviceId,
    ) -> Result<(), LicenseError> {
        self.require()?;
        Err(LicenseError::Transport(
            "waffo validation not implemented yet".into(),
        ))
    }

    async fn deactivate(
        &self,
        _license: &ActivatedLicense,
        _device: &DeviceId,
    ) -> Result<(), LicenseError> {
        self.require()?;
        Err(LicenseError::Transport(
            "waffo deactivation not implemented yet".into(),
        ))
    }
}
