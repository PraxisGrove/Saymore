use anyhow::Result;
use template_app::build_greeting;
use template_infra::EnvRecipient;

fn main() -> Result<()> {
    let provider = EnvRecipient::from_args(std::env::args());
    println!("{}", build_greeting(&provider));

    Ok(())
}
