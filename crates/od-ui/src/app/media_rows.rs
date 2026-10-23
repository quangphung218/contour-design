//! Media generation rows of a turn (track C2-chat-home): the daemon's
//! `media` events as the original's `ImageRow`, a cell per generation
//! that fills in as files land. Logic in [`crate::media_row`].

use super::icons::{Glyph, icon};
use super::studio::StudioCtx;
use crate::chat::AssistantTurn;
use crate::i18n::{T, t, tf};
use crate::media_row::{self, Cell, Row};
use leptos::prelude::*;

fn batch_label(surface: &str) -> &'static str {
    t(match surface {
        "audio" => T::MediaBatchAudio,
        "video" => T::MediaBatchVideo,
        _ => T::MediaBatchImage,
    })
}

fn count_label(surface: &str, n: usize) -> String {
    let key = match surface {
        "audio" => T::MediaCountAudio,
        "video" => T::MediaCountVideo,
        _ => T::MediaCountImage,
    };
    tf(key, &[&n.to_string()])
}

fn cell_glyph(surface: &str) -> Glyph {
    match surface {
        "audio" => Glyph::Audio,
        "video" => Glyph::Video,
        _ => Glyph::Image,
    }
}

#[component]
pub fn MediaRows(turn: RwSignal<AssistantTurn>) -> impl IntoView {
    let ctx = expect_context::<StudioCtx>();
    // Elapsed time ticks while a generation runs.
    let now = RwSignal::new(js_sys::Date::now() as i64);
    let ticking = StoredValue::new(false);
    Effect::new(move |_| {
        let pending = turn.with(|t| t.media.iter().any(|m| !m.status.is_terminal()));
        if !pending || ticking.get_value() {
            return;
        }
        ticking.set_value(true);
        leptos::task::spawn_local(async move {
            loop {
                gloo_timers::future::TimeoutFuture::new(1_000).await;
                now.set(js_sys::Date::now() as i64);
                if !turn.with_untracked(|t| t.media.iter().any(|m| !m.status.is_terminal())) {
                    break;
                }
            }
            ticking.set_value(false);
        });
    });
    move || {
        let (rows, live) = turn.with(|t| (media_row::rows(&t.media, now.get()), t.state.is_live()));
        rows.into_iter()
            .map(|row| media_row_view(ctx, row, live))
            .collect_view()
    }
}

fn media_row_view(ctx: StudioCtx, row: Row, live: bool) -> impl IntoView {
    let base = ctx.base();
    let surface = row.surface.clone();
    let total = row.cells.len();
    let elapsed = media_row::elapsed(row.elapsed_ms);
    let thumb = move |path: &str| {
        (surface == "image").then(|| format!("{base}/raw/{}", super::api::file_path(path)))
    };
    if row.collapsed() {
        let thumbs = row
            .cells
            .iter()
            .filter_map(|c| match c {
                Cell::Done(p) => Some(p.clone()),
                _ => None,
            })
            .map(|p| {
                let open = p.clone();
                let src = thumb(&p);
                view! {
                    <button class="media-row-thumb small" aria-label=p.clone() on:click=move |_| ctx.open_file(&open)>
                        {match src {
                            Some(s) => view! { <img src=s alt=""/> }.into_any(),
                            None => icon(cell_glyph(&row.surface)).into_any(),
                        }}
                    </button>
                }
            })
            .collect_view();
        return view! {
            <div class="media-row collapsed" data-testid="media-row" data-state="done" data-surface=row.surface.clone()>
                {icon(cell_glyph(&row.surface))}
                <span class="media-row-label">{format!("{} · {}", batch_label(&row.surface), count_label(&row.surface, total))}</span>
                <span class="media-row-thumbs">{thumbs}</span>
                <span class="muted small mono">{elapsed}</span>
            </div>
        }
        .into_any();
    }
    let state = if row.pending() { "running" } else { "failed" };
    let failed_at = row.failed_positions();
    let surface_retry = row.surface.clone();
    let cells = row
        .cells
        .iter()
        .enumerate()
        .map(|(i, cell)| match cell.clone() {
            Cell::Pending => view! {
                <span class="media-cell pending" data-testid="media-cell" data-state="pending">
                    <span class="spinner"></span>
                    <span class="visually-hidden">{batch_label(&row.surface)}</span>
                </span>
            }
            .into_any(),
            Cell::Done(path) => {
                let open = path.clone();
                let src = thumb(&path);
                view! {
                    <button class="media-cell done" data-testid="media-cell" data-state="done"
                        aria-label=tf(T::MediaOpenCell, &[&(i + 1).to_string()]) title=path.clone()
                        on:click=move |_| ctx.open_file(&open)>
                        {match src {
                            Some(s) => view! { <img src=s alt=""/> }.into_any(),
                            None => icon(cell_glyph(&row.surface)).into_any(),
                        }}
                    </button>
                }
                .into_any()
            }
            Cell::Failed(error) => view! {
                <span class="media-cell failed" data-testid="media-cell" data-state="failed" title=error>
                    {icon(Glyph::Warning)}
                </span>
            }
            .into_any(),
        })
        .collect_view();
    let retry = (!failed_at.is_empty()).then(|| {
        let positions = failed_at
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(", ");
        let text = tf(
            T::MediaRetryMessage,
            &[
                &failed_at.len().to_string(),
                &count_label(&surface_retry, total),
                &positions,
            ],
        );
        if live {
            view! { <span class="badge danger" data-testid="media-failed">{t(T::MediaFailed)}</span> }.into_any()
        } else {
            view! {
                <button class="btn ghost small" data-testid="media-retry"
                    on:click=move |_| ctx.send(text.clone(), ctx.shell)>
                    {icon(Glyph::Refresh)}{t(T::NextRetry)}
                </button>
            }
            .into_any()
        }
    });
    view! {
        <div class="media-row" data-testid="media-row" data-state=state data-surface=row.surface.clone()>
            <div class="media-row-head">
                {if row.pending() {
                    view! { <span class="spinner"></span> }.into_any()
                } else {
                    icon(Glyph::Warning).into_any()
                }}
                <span class="media-row-label">{batch_label(&row.surface)}</span>
                <span class="muted small mono" data-testid="media-count">{format!("{}/{total}", row.done())}</span>
                <span class="muted small mono">{elapsed}</span>
                <span class="spacer"></span>
                {retry}
            </div>
            {row.progress.clone().map(|p| view! { <p class="muted small media-progress" data-testid="media-progress">{p}</p> })}
            <div class="media-cells">{cells}</div>
        </div>
    }
    .into_any()
}
