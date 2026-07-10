#![cfg_attr(test, allow(clippy::panic))]

use template_domain::Greeting;

pub trait RecipientProvider {
    fn recipient(&self) -> &str;
}

pub fn build_greeting(provider: &impl RecipientProvider) -> String {
    Greeting::new(provider.recipient()).message()
}

#[cfg(test)]
mod tests {
    use super::*;

    struct StaticRecipient;

    impl RecipientProvider for StaticRecipient {
        fn recipient(&self) -> &str {
            "workspace"
        }
    }

    #[test]
    fn builds_greeting_from_provider() {
        let message = build_greeting(&StaticRecipient);

        assert_eq!("Hello, workspace!", message);
    }
}
