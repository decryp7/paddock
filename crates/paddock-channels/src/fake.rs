//! In-process fakes of the two ports, for the apps' own tests (no network, nothing to clean up).

use crate::{HttpPoster, MailRequest, MailSender, PostRequest, PostResponse, SendError};
use paddock_download::TransportError;
use std::collections::VecDeque;
use std::sync::{Mutex, MutexGuard, PoisonError};

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

enum Step<T, E> {
    Answer(Result<T, E>),
    Panic,
}

/// An [`HttpPoster`] that answers from a script (200 once the script is used up) and records every request.
pub struct FakePoster {
    script: Mutex<VecDeque<Step<PostResponse, TransportError>>>,
    panic_on: Mutex<Vec<String>>,
    seen: Mutex<Vec<PostRequest>>,
}

impl Default for FakePoster {
    fn default() -> Self {
        FakePoster {
            script: Mutex::new(VecDeque::new()),
            panic_on: Mutex::new(Vec::new()),
            seen: Mutex::new(Vec::new()),
        }
    }
}

impl FakePoster {
    /// A poster that answers 200 to everything.
    pub fn new() -> FakePoster {
        FakePoster::default()
    }

    /// The next request is answered with `answer`.
    pub fn then(self, answer: Result<PostResponse, TransportError>) -> FakePoster {
        lock(&self.script).push_back(Step::Answer(answer));
        self
    }

    /// The next request is answered with this status.
    pub fn then_status(self, status: u16) -> FakePoster {
        self.then(Ok(PostResponse {
            status,
            retry_after: None,
        }))
    }

    /// Every request whose URL contains `url_part` panics, whatever the order the channels post in.
    pub fn panic_on(self, url_part: &str) -> FakePoster {
        lock(&self.panic_on).push(url_part.to_owned());
        self
    }

    /// The requests received so far.
    pub fn requests(&self) -> Vec<PostRequest> {
        lock(&self.seen).clone()
    }
}

impl HttpPoster for FakePoster {
    fn post(&self, request: &PostRequest) -> Result<PostResponse, TransportError> {
        lock(&self.seen).push(request.clone());
        if lock(&self.panic_on)
            .iter()
            .any(|p| request.url.contains(p.as_str()))
        {
            panic!("fake poster panic");
        }
        let step = lock(&self.script).pop_front();
        match step {
            Some(Step::Answer(a)) => a,
            _ => Ok(PostResponse {
                status: 200,
                retry_after: None,
            }),
        }
    }
}

/// A [`MailSender`] that answers from a script (delivers once the script is used up) and records every mail.
pub struct FakeMail {
    script: Mutex<VecDeque<Step<(), SendError>>>,
    seen: Mutex<Vec<MailRequest>>,
}

impl Default for FakeMail {
    fn default() -> Self {
        FakeMail {
            script: Mutex::new(VecDeque::new()),
            seen: Mutex::new(Vec::new()),
        }
    }
}

impl FakeMail {
    /// A sender that delivers everything.
    pub fn new() -> FakeMail {
        FakeMail::default()
    }

    /// The next mail fails with `error`.
    pub fn then_fail(self, error: SendError) -> FakeMail {
        lock(&self.script).push_back(Step::Answer(Err(error)));
        self
    }

    /// The next mail panics.
    pub fn then_panic(self) -> FakeMail {
        lock(&self.script).push_back(Step::Panic);
        self
    }

    /// The mails received so far.
    pub fn mails(&self) -> Vec<MailRequest> {
        lock(&self.seen).clone()
    }
}

impl MailSender for FakeMail {
    fn send(&self, mail: &MailRequest) -> Result<(), SendError> {
        lock(&self.seen).push(mail.clone());
        let step = lock(&self.script).pop_front();
        match step {
            Some(Step::Answer(a)) => a,
            Some(Step::Panic) => panic!("fake mail panic"),
            None => Ok(()),
        }
    }
}
