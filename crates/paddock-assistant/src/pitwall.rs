//! The Pitwall adapter: [`LlmProvider`] over `paddock_pitwall::Link` (`chat_with`, streamed and cancellable), with the
//! prompted tool protocol on top of Pitwall's chat API so every engine behaves the same. Typed Pitwall errors become the
//! needs-AI states the UI shows (never a raw error for a missing Pitwall).

use crate::protocol::{Finish, Generated, prompted_chat};
use crate::provider::{LlmProvider, ProviderError};
use crate::types::{ChatRequest, ChatResponse};
use paddock_pitwall::{AiState, CancelToken, Caps, Link, Message, ModelSel, PitwallError, Role};
use std::sync::Arc;

/// The state a Pitwall error means for the UI, or `None` when it is a failure of this one request (shown as an error).
pub fn needs_ai(link: &Link, error: &PitwallError) -> Option<AiState> {
    let app = link.app_name();
    Some(match error {
        PitwallError::NotRunning | PitwallError::Stopped => AiState::NotRunning,
        PitwallError::NeedsPairing
        | PitwallError::KeyRevoked
        | PitwallError::PairingDenied
        | PitwallError::PairingExpired => AiState::NeedsPairing,
        PitwallError::Incompatible { found, .. } => AiState::Incompatible {
            message: error.user_message(app),
            update_app: *found > paddock_pitwall::API_VERSION,
        },
        PitwallError::NoModelFits(_) => AiState::NoSuitableModel {
            needs: link.needs(),
        },
        PitwallError::NotReady(message) => {
            // The link knows more than the error: paused, no model, incompatible.
            let state = AiState::from_view(&link.view(), link.needs(), false);
            if state.is_ready() {
                AiState::Unavailable {
                    message: message.clone(),
                }
            } else {
                state
            }
        }
        PitwallError::Forbidden(_)
        | PitwallError::Memory(_)
        | PitwallError::Overloaded { .. }
        | PitwallError::RateLimited { .. } => AiState::Unavailable {
            message: error.user_message(app),
        },
        PitwallError::Cancelled
        | PitwallError::Unsupported { .. }
        | PitwallError::Invalid(_)
        | PitwallError::Timeout(_)
        | PitwallError::Protocol(_)
        | PitwallError::Server { .. } => return None,
    })
}

/// [`LlmProvider`] over one app's Pitwall link.
pub struct PitwallProvider {
    link: Arc<Link>,
}

impl PitwallProvider {
    /// A provider over `link` (the app's one link; see `paddock_pitwall::Link`).
    pub fn new(link: Arc<Link>) -> PitwallProvider {
        PitwallProvider { link }
    }

    fn error(&self, e: PitwallError) -> ProviderError {
        match needs_ai(&self.link, &e) {
            Some(state) => ProviderError::Unavailable(state),
            None => ProviderError::Failed(e.user_message(self.link.app_name())),
        }
    }
}

fn finish_of(reason: Option<&str>) -> Finish {
    match reason {
        Some("length") => Finish::Length,
        _ => Finish::Stop,
    }
}

impl LlmProvider for PitwallProvider {
    fn id(&self) -> &str {
        "pitwall"
    }

    fn display_name(&self) -> String {
        match self.link.view().model {
            Some(model) => format!("Pitwall ({model})"),
            None => "Pitwall".into(),
        }
    }

    fn context_length(&self) -> usize {
        self.link.view().context_tokens
    }

    fn chat(
        &self,
        req: &ChatRequest,
        on_delta: &mut dyn FnMut(&str),
    ) -> Result<ChatResponse, ProviderError> {
        prompted_chat(req, on_delta, &mut |prepared, sink| {
            let mut messages = vec![Message::text(Role::System, prepared.system.clone())];
            messages.extend(prepared.turns.iter().map(|t| {
                Message::text(
                    if t.role == "assistant" {
                        Role::Assistant
                    } else {
                        Role::User
                    },
                    t.text.clone(),
                )
            }));
            let mut request =
                paddock_pitwall::ChatRequest::new(ModelSel::Auto(Caps::chat(8192)), messages);
            request.max_tokens = u32::try_from(prepared.max_tokens).ok();
            request.temperature = Some(prepared.temperature);
            request.json_schema = prepared.json_schema.clone();
            request.options.cancel = CancelToken::from_flag(req.cancel.flag());
            let mut text = String::new();
            let result = self.link.chat_with(request, &mut |d| {
                text.push_str(d);
                sink(d);
            });
            match result {
                Ok(reply) => Ok(Generated {
                    text: reply.text,
                    prompt_tokens: reply.usage.prompt_tokens,
                    completion_tokens: reply.usage.completion_tokens,
                    finish: finish_of(reply.finish_reason.as_deref()),
                }),
                // A stop keeps what was generated until then.
                Err(PitwallError::Cancelled) => Ok(Generated {
                    text,
                    prompt_tokens: 0,
                    completion_tokens: 0,
                    finish: Finish::Cancelled,
                }),
                Err(e) => Err(self.error(e)),
            }
        })
    }
}
