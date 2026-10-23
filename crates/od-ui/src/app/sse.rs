//! `EventSource` with named events. The browser reconnects on its own and
//! sends `Last-Event-ID`, which the daemon uses to replay (spec 2.2).

use wasm_bindgen::JsCast;
use wasm_bindgen::closure::Closure;
use web_sys::{Event, EventSource, MessageEvent};

type Handler = Closure<dyn FnMut(Event)>;

/// An open stream. Dropping it closes the connection.
pub struct Sse {
    source: EventSource,
    _handlers: Vec<Handler>,
}

impl Sse {
    /// Opens `url` and calls `on_event(name, data)` for each listed event.
    /// `on_closed` fires when the browser gives up (non-200, CORS…), not on
    /// transient drops it will retry.
    pub fn open(
        url: &str,
        names: &[&'static str],
        on_event: impl Fn(&str, String) + 'static,
        on_closed: impl Fn() + 'static,
    ) -> Option<Self> {
        let source = EventSource::new(url).ok()?;
        let on_event = std::rc::Rc::new(on_event);
        let mut handlers = Vec::with_capacity(names.len() + 1);
        for &name in names {
            let cb = std::rc::Rc::clone(&on_event);
            let handler: Handler = Closure::new(move |ev: Event| {
                // A run may emit an `error` event of its own; connection
                // errors arrive as plain `Event`s on the same name.
                if let Some(data) = ev
                    .dyn_ref::<MessageEvent>()
                    .and_then(|m| m.data().as_string())
                {
                    cb(name, data);
                }
            });
            source
                .add_event_listener_with_callback(name, handler.as_ref().unchecked_ref())
                .ok()?;
            handlers.push(handler);
        }
        let watched = source.clone();
        let on_error: Handler = Closure::new(move |ev: Event| {
            if ev.dyn_ref::<MessageEvent>().is_none()
                && watched.ready_state() == EventSource::CLOSED
            {
                on_closed();
            }
        });
        source.set_onerror(Some(on_error.as_ref().unchecked_ref()));
        handlers.push(on_error);
        Some(Self {
            source,
            _handlers: handlers,
        })
    }

    /// Also calls `on_link(true)` when the connection opens (again) and
    /// `on_link(false)` when it drops, retried or not.
    #[must_use]
    pub fn on_connection(mut self, on_link: impl Fn(bool) + 'static) -> Self {
        let on_link = std::rc::Rc::new(on_link);
        for (name, up) in [("open", true), ("error", false)] {
            let cb = std::rc::Rc::clone(&on_link);
            let handler: Handler = Closure::new(move |ev: Event| {
                // A run's own `error` event is a message, not a drop.
                if ev.dyn_ref::<MessageEvent>().is_none() {
                    cb(up);
                }
            });
            if self
                .source
                .add_event_listener_with_callback(name, handler.as_ref().unchecked_ref())
                .is_ok()
            {
                self._handlers.push(handler);
            }
        }
        self
    }

    pub fn close(&self) {
        self.source.close();
    }
}

impl Drop for Sse {
    fn drop(&mut self) {
        self.source.set_onerror(None);
        self.source.close();
    }
}
