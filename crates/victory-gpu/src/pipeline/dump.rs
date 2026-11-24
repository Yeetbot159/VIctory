//! `VICTORY_PIPELINE_DUMP=<file>`: every host pipeline the title asks for,
//! once each, so a library can be built without a title run.
//!
//! A pipeline leaves no record anywhere else: its name is a hash of the root
//! signature, the translated shader code and the graphics state, and those
//! exist only while the title is up. `victory precompile` reads this file and
//! the shader containers beside it (`VICTORY_SHADER_SAVE`) and makes the same
//! pipelines offline, on the card's own adapter, so that a first session
//! carries a warm library instead of compiling for fourteen minutes.
//!
//! One line per distinct pipeline, in the order they were first asked for:
//!
//! ```text
//! kind=graphics stages=vertex:<hash>,pixel:<hash> state=<fields>
//! kind=compute stages=compute:<hash> state=-
//! ```
//!
//! `state` is [`super::host::GraphicsState::fields`], not `describe()`: the
//! name a pipeline gets is `library::name(kind, root, code, describe())`, and
//! the reader rebuilds the state from these integers and takes `describe()` of
//! the rebuild, so the two are the same string by construction instead of by
//! parsing Rust's `{:?}` of a D3D12 struct and hoping.

use std::io::Write;
use std::sync::{Mutex, OnceLock};

use super::host::GraphicsState;

/// The pipelines written so far, so a pipeline asked for on every frame is
/// written once. The key is the line without its `state` (kind and stages),
/// plus the state's own fields: two descriptions of one shader pair are two
/// different pipelines.
fn seen() -> &'static Mutex<std::collections::HashSet<String>> {
    static SEEN: OnceLock<Mutex<std::collections::HashSet<String>>> = OnceLock::new();
    SEEN.get_or_init(|| Mutex::new(std::collections::HashSet::new()))
}

/// The file the dump goes to, if `VICTORY_PIPELINE_DUMP` names one. Read
/// once: a title run's environment does not change.
fn file() -> Option<&'static std::path::PathBuf> {
    static FILE: OnceLock<Option<std::path::PathBuf>> = OnceLock::new();
    FILE.get_or_init(|| {
        let path = std::path::PathBuf::from(std::env::var_os("VICTORY_PIPELINE_DUMP")?);
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        tracing::info!(path = %path.display(), "host pipeline dump");
        Some(path)
    })
    .as_ref()
}

/// Writes `shaders` and `state` to the dump, once per distinct pipeline.
pub(crate) fn record(shaders: &[(String, String)], state: Option<&GraphicsState>) {
    let Some(path) = file() else { return };
    let layout = if shaders.len() == 1 && shaders[0].0 == "compute" {
        "compute"
    } else if shaders.iter().any(|(stage, _)| stage == "mesh") {
        "mesh"
    } else {
        "graphics"
    };
    let stages: String = shaders
        .iter()
        .map(|(stage, hash)| format!("{stage}:{hash}"))
        .collect::<Vec<_>>()
        .join(",");
    let fields = state.map(GraphicsState::fields).unwrap_or_else(|| "-".into());
    let line = format!("kind={layout} stages={stages} state={fields}");
    // Under the lock, and in one write: a second process on the same dump (the
    // title relaunches itself, and the audio capture child has the environment
    // too) appends to the same file with its own set, so a write that is not one
    // call can land inside another process's line. A torn line is a dump line
    // `precompile` refuses, and a refused line is a pipeline nobody compiles.
    let mut seen = seen().lock().unwrap_or_else(|e| e.into_inner());
    if !seen.insert(line.clone()) {
        return;
    }
    // Appended, not rewritten: a run that dies mid-frame still leaves every
    // pipeline it had reached, and two runs accumulate.
    let mut bytes = line.into_bytes();
    bytes.push(0x0a);
    if let Ok(mut out) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        if let Err(e) = out.write_all(&bytes) {
            tracing::warn!(error = %e, "host pipeline dump not written");
        }
    }
}
