//! Previews for the app's own process (ADR 0044 §2, §6): a drag asks for the result of a gesture many times a second,
//! and the answer comes from the same pure engine, in-process, never through the agent that stores the layout.

use std::any::Any;
use std::collections::HashMap;
use std::sync::{Mutex, PoisonError};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::catalogue::Catalogue;
use crate::error::WidgetError;
use crate::grid::GridGesture;
use crate::read::Stored;
use crate::view::ViewOf;

ts! {
/// The webview's request for a preview. Parsed at the edge: the layout is never taken from the webview.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewRequest {
    /// The surface, as its id serialises ("dashboard").
    pub surface: String,
    /// The revision the webview's view was computed on.
    #[cfg_attr(any(test, feature = "ts"), ts(type = "number"))]
    pub revision: u64,
    /// The gesture.
    pub gesture: GridGesture,
    /// The webview's counter; replies older than the latest are dropped there.
    #[cfg_attr(any(test, feature = "ts"), ts(type = "number"))]
    pub seq: u64,
}
}

ts! {
/// The reply: the request's `seq` and the view the gesture would give (its `outcome` says where the placement would land).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewReply<V> {
    /// The request's counter.
    #[cfg_attr(any(test, feature = "ts"), ts(type = "number"))]
    pub seq: u64,
    /// The previewed [`crate::LayoutView`].
    pub view: V,
}
}

trait Slot: Send {
    fn as_any_mut(&mut self) -> &mut dyn Any;
    fn preview(&self, revision: u64, gesture: GridGesture) -> Result<Value, WidgetError>;
}

struct Typed<C: Catalogue>(Option<Stored<C>>);

impl<C: Catalogue> Slot for Typed<C>
where
    C::Widget: Send,
    C::Place: Send,
{
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn preview(&self, revision: u64, gesture: GridGesture) -> Result<Value, WidgetError> {
        let stored = self.0.as_ref().ok_or(WidgetError::Stale)?;
        let view = stored.preview(revision, gesture)?;
        serde_json::to_value(view)
            .map_err(|_| WidgetError::Invalid("The preview could not be made.".into()))
    }
}

/// The last view the app relayed for each registered surface, and the preview command's logic over them.
#[derive(Default)]
pub struct Previews {
    surfaces: Mutex<HashMap<String, Box<dyn Slot>>>,
}

fn key<C: Catalogue>() -> String {
    serde_json::to_value(C::SURFACE_ID)
        .ok()
        .and_then(|v| v.as_str().map(str::to_string))
        .unwrap_or_default()
}

impl Previews {
    /// No surfaces yet.
    pub fn new() -> Self {
        Previews::default()
    }

    /// Registers a grid surface by its serialised id (one line per surface).
    pub fn grid<C: Catalogue>(self) -> Self
    where
        C::Widget: Send,
        C::Place: Send,
    {
        self.surfaces
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(key::<C>(), Box::new(Typed::<C>(None)));
        self
    }

    /// Records the view the app just relayed from the store owner (with its revision); later previews are computed on it.
    /// `false` for a surface that is not registered or a view that is not valid.
    pub fn record<C: Catalogue>(&self, view: &ViewOf<C>) -> bool
    where
        C::Widget: Send,
        C::Place: Send,
    {
        let Ok(stored) = Stored::<C>::at(view.layout.clone(), view.revision) else {
            return false;
        };
        let mut map = self.surfaces.lock().unwrap_or_else(PoisonError::into_inner);
        match map
            .get_mut(&key::<C>())
            .and_then(|s| s.as_any_mut().downcast_mut::<Typed<C>>())
        {
            Some(slot) => {
                slot.0 = Some(stored);
                true
            }
            None => false,
        }
    }

    /// Answers a preview request from the recorded view. [`WidgetError::Stale`] when the revision is not the recorded
    /// one or nothing is recorded yet; an unknown surface is refused.
    pub fn preview(&self, request: &PreviewRequest) -> Result<PreviewReply<Value>, WidgetError> {
        let map = self.surfaces.lock().unwrap_or_else(PoisonError::into_inner);
        let slot = map.get(&request.surface).ok_or_else(|| {
            WidgetError::Invalid(format!(
                "Unknown surface \"{}\".",
                request.surface.chars().take(32).collect::<String>()
            ))
        })?;
        let view = slot.preview(request.revision, request.gesture.clone())?;
        Ok(PreviewReply {
            seq: request.seq,
            view,
        })
    }
}
