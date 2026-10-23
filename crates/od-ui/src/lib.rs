//! Nét UI (spec 5.3): a Leptos CSR app compiled to WASM by trunk and
//! served by the daemon. Pure state lives in the top-level modules and is
//! tested natively; the browser-only parts live under [`app`].

pub mod byok;
pub mod chat;
pub mod comment_board;
pub mod events;
pub mod file_tree;
pub mod fuzzy;
pub mod guide;
pub mod handoff;
pub mod highlight;
pub mod i18n;
pub mod intent;
pub mod manual_model;
pub mod markdown;
pub mod mention;
pub mod models;
pub mod preview_bridge;
pub mod project_panel;
pub mod question_form;
pub mod schedule;
pub mod settings;
pub mod sketch_model;
pub mod speaker_notes;
pub mod toolbox;
pub mod workspace;

pub mod app;
