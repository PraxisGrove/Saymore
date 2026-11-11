use std::sync::{Arc, OnceLock};

use template_app::SpeechRecognitionError;

use super::super::protocol;

static INITIALIZED: OnceLock<Result<(), SpeechRecognitionError>> = OnceLock::new();

pub(super) fn initialize() -> Result<(), SpeechRecognitionError> {
    INITIALIZED.get_or_init(initialize_linked_runtime).clone()
}

fn initialize_linked_runtime() -> Result<(), SpeechRecognitionError> {
    // sherpa-onnx-sys links the CPU ONNX Runtime archive on every supported
    // desktop target, including the static-MT Windows build. Reuse its C ABI
    // rather than downloading or dynamically loading a second runtime.
    // SAFETY: OrtGetApiBase is the linked runtime's stable entry point. Both
    // pointers are checked before dereferencing. GetApi guarantees a table
    // with the exact ABI requested, or null for an incompatible runtime.
    let api = unsafe {
        let base = ort::sys::OrtGetApiBase();
        let Some(base) = base.as_ref() else {
            return Err(protocol("native ONNX Runtime entry point is unavailable"));
        };
        let api = (base.GetApi)(ort::sys::ORT_API_VERSION);
        let Some(api) = api.as_ref() else {
            return Err(protocol(
                "linked ONNX Runtime does not support the required API",
            ));
        };
        api.clone()
    };
    ort::set_api(api);
    // Suppress native diagnostics because they may contain model paths or
    // vocabulary. All app errors cross this boundary as content-free categories.
    ort::init()
        .with_name("saymore-seaco")
        .with_logger(Arc::new(|_, _, _, _, _| {}))
        .commit();
    Ok(())
}
