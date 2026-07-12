# Saymore Desktop

The native Saymore desktop application, implemented with Rust and Slint. It
currently supports macOS Accessibility and microphone permission flows. Hold
the right Command key to capture audio; releasing it converts the in-memory
recording to mono 16 kHz signed 16-bit PCM and shows the capture metrics.

## Development

```bash
cargo run -p saymore-desktop
```

Create the signed local macOS app bundle with Rust `xtask`:

```bash
cargo run -p xtask -- bundle-macos
```
