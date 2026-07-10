#![cfg_attr(test, allow(clippy::panic))]

use template_app::RecipientProvider;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnvRecipient {
    recipient: String,
}

impl EnvRecipient {
    pub fn from_args(mut args: impl Iterator<Item = String>) -> Self {
        let recipient = args.nth(1).unwrap_or_else(|| "world".to_owned());

        Self { recipient }
    }
}

impl RecipientProvider for EnvRecipient {
    fn recipient(&self) -> &str {
        &self.recipient
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_to_world_without_argument() {
        let provider = EnvRecipient::from_args(["template-cli".to_owned()].into_iter());

        assert_eq!("world", provider.recipient());
    }

    #[test]
    fn uses_first_user_argument_as_recipient() {
        let provider =
            EnvRecipient::from_args(["template-cli".to_owned(), "Rust".to_owned()].into_iter());

        assert_eq!("Rust", provider.recipient());
    }
}
