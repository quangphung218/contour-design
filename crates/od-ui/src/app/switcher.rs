//! Quick switcher (Cmd/Ctrl+P, spec 1.2): fuzzy file search, recently
//! opened files first. Enter opens the highlighted file in a tab.

use super::studio::StudioCtx;
use crate::fuzzy;
use crate::i18n::{T, t};
use leptos::prelude::*;

/// Results shown at most.
const MAX_RESULTS: usize = 50;

#[component]
pub fn QuickSwitcher(ctx: StudioCtx) -> impl IntoView {
    let query = RwSignal::new(String::new());
    let selected = RwSignal::new(0usize);
    let input = NodeRef::<leptos::html::Input>::new();
    let results = Memo::new(move |_| {
        let paths: Vec<String> = ctx
            .files
            .with(|f| f.iter().map(|f| f.path.clone()).collect());
        let recent = ctx.recent.get();
        query.with(|q| {
            fuzzy::rank(q, &paths, &recent)
                .into_iter()
                .take(MAX_RESULTS)
                .map(str::to_owned)
                .collect::<Vec<_>>()
        })
    });
    Effect::new(move |_| {
        if let Some(el) = input.get() {
            let _ = el.focus();
        }
    });
    let close = move || ctx.switcher_open.set(false);
    let pick = move |path: String| {
        ctx.open_file(&path);
        close();
    };
    let on_key = move |ev: web_sys::KeyboardEvent| {
        let n = results.with_untracked(Vec::len);
        match ev.key().as_str() {
            "ArrowDown" => {
                ev.prevent_default();
                selected.update(|s| *s = if n == 0 { 0 } else { (*s + 1) % n });
            }
            "ArrowUp" => {
                ev.prevent_default();
                selected.update(|s| *s = if n == 0 { 0 } else { (*s + n - 1) % n });
            }
            "Enter" => {
                ev.prevent_default();
                let i = selected.get_untracked();
                if let Some(p) = results.with_untracked(|r| r.get(i).cloned()) {
                    pick(p);
                }
            }
            "Escape" => {
                ev.prevent_default();
                close();
            }
            _ => {}
        }
    };
    view! {
        <div class="overlay" on:click=move |_| close()>
            <div
                class="switcher"
                role="dialog"
                aria-label=t(T::QuickOpen)
                data-testid="quick-switcher"
                on:click=|ev| ev.stop_propagation()
            >
                <input
                    class="input"
                    node_ref=input
                    data-testid="switcher-input"
                    placeholder=t(T::SwitcherPlaceholder)
                    prop:value=move || query.get()
                    on:input=move |ev| {
                        query.set(event_target_value(&ev));
                        selected.set(0);
                    }
                    on:keydown=on_key
                />
                <ul class="switcher-list" role="listbox">
                    {move || {
                        let list = results.get();
                        if list.is_empty() {
                            return view! { <li class="empty small">{t(T::SwitcherEmpty)}</li> }.into_any();
                        }
                        let recent = ctx.recent.get();
                        list.into_iter()
                            .enumerate()
                            .map(|(i, path)| {
                                let is_recent = recent.contains(&path);
                                let target = path.clone();
                                view! {
                                    <li
                                        class="switcher-item"
                                        class:selected=move || selected.get() == i
                                        role="option"
                                        aria-selected=move || (selected.get() == i).to_string()
                                        data-testid="switcher-item"
                                        on:mousemove=move |_| selected.set(i)
                                        on:click=move |_| pick(target.clone())
                                    >
                                        <span class="mono">{path}</span>
                                        {is_recent.then(|| view! { <span class="badge">{t(T::RecentBadge)}</span> })}
                                    </li>
                                }
                            })
                            .collect_view()
                            .into_any()
                    }}
                </ul>
            </div>
        </div>
    }
}
