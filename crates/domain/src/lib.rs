#![cfg_attr(test, allow(clippy::panic))]

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Greeting {
    recipient: String,
}

impl Greeting {
    pub fn new(recipient: impl Into<String>) -> Self {
        Self {
            recipient: recipient.into(),
        }
    }

    pub fn message(&self) -> String {
        format!("Hello, {}!", self.recipient)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    #[rstest]
    #[case("world", "Hello, world!")]
    #[case("Rust", "Hello, Rust!")]
    fn formats_greeting(#[case] recipient: &str, #[case] expected: &str) {
        let greeting = Greeting::new(recipient);

        assert_eq!(expected, greeting.message());
    }
}
