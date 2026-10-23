//! Open Design UI (spec 5.3): a Leptos CSR app compiled to WASM by trunk and
//! served by the daemon. Pure state lives in the top-level modules and is
//! tested natively; the browser-only parts live under [`app`].

pub mod chat;
pub mod events;
pub mod fuzzy;
pub mod i18n;
pub mod intent;
pub mod markdown;
pub mod preview_bridge;
pub mod question_form;
pub mod settings;
pub mod speaker_notes;
pub mod workspace;

pub mod app;
