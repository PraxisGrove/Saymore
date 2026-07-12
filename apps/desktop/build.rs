fn main() {
    if let Err(error) = slint_build::compile("ui/app-window.slint") {
        eprintln!("failed to compile Saymore UI: {error}");
        std::process::exit(1);
    }
}
