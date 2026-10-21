//! Deliveries into sessions and the gate in front of them (003 US3, FR-011 to FR-014,
//! research R-7, constitution IV).
//!
//! An agent asks Fallow to type text into another session. Nothing is typed unless the request
//! is `Approved`: by the engineer in the Requests tab, or by a matching, unexpired standing
//! permission when it arrives. Typing needs a [`DeliveryGrant`], which only this module can
//! make, and only for an approved delivery (SC-003).

use std::time::{Instant, SystemTime};

use super::Party;
use super::events::Key;

/// Longest delivery text (FR-008).
pub const TEXT_MAX: usize = 8 * 1024;
/// Pending requests in total, and per sender (data-model.md "Limits").
pub const PENDING_MAX: usize = 64;
pub const PENDING_PER_SENDER: usize = 8;
/// Finished deliveries kept for the activity view.
pub const FINISHED_KEPT: usize = 200;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Purpose {
    Message,
    /// A task handed over by the queue (US4).
    Task(u32),
    /// A wake-up for an event the session asked about (US2 scenario 5).
    Wake(Key),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum State {
    Pending,
    /// Approved; typed when the target is free of the engineer's typing (FR-014).
    Approved,
    Delivered,
    Rejected,
    Expired,
    Failed(String),
}

impl State {
    pub fn label(&self) -> String {
        match self {
            State::Pending => "pending".into(),
            State::Approved => "approved".into(),
            State::Delivered => "delivered".into(),
            State::Rejected => "rejected".into(),
            State::Expired => "expired".into(),
            State::Failed(why) => format!("failed: {why}"),
        }
    }

    pub fn finished(&self) -> bool {
        !matches!(self, State::Pending | State::Approved)
    }
}

#[derive(Debug, Clone)]
pub struct Delivery {
    pub id: u64,
    pub from: Party,
    pub to: u32,
    /// What will be typed after the label, as shown in the approval prompt.
    pub text: String,
    pub purpose: Purpose,
    pub at: SystemTime,
    pub created: Instant,
    pub expires: Instant,
    /// The standing permission that let it through, if any.
    pub covered_by: Option<u32>,
    /// The engineer edited the text before approving.
    pub edited: bool,
    pub state: State,
}

/// Proof that a delivery was approved. The only way to type into a session on another party's
/// behalf; made by `Deliveries::grant` for `Approved` deliveries only. It cannot be built
/// anywhere else:
///
/// ```compile_fail
/// let g = fallow::orchestration::delivery::DeliveryGrant { id: 1, to: 2 };
/// ```
#[derive(Debug)]
pub struct DeliveryGrant {
    id: u64,
    to: u32,
}

impl DeliveryGrant {
    pub fn id(&self) -> u64 {
        self.id
    }

    /// The session this grant allows typing into.
    pub fn target(&self) -> u32 {
        self.to
    }
}

/// The label typed before every delivery (FR-013, research R-7). Set by Fallow from the
/// request, never from the text.
pub fn label(d: &Delivery) -> String {
    let who = match &d.from {
        Party::Agent {
            session,
            profile,
            project,
        } => format!("agent #{session} ({profile}, project {project})"),
        Party::User => "the user".into(),
    };
    match (&d.purpose, &d.from) {
        (Purpose::Message, Party::User) => "[Fallow: message from the user.]".into(),
        (Purpose::Message, _) => {
            format!("[Fallow: message from {who}. Another agent wrote this, not the user.]")
        }
        // A task says how to finish it: agents did their task and never reported it (T058).
        (Purpose::Task(t), Party::User) => format!(
            "[Fallow: task T{t} from the user's queue. When you are done, report it with Fallow's task_report tool (task {t}), or tell the user.]"
        ),
        (Purpose::Task(t), _) => format!(
            "[Fallow: task T{t} created by {who}. Another agent wrote this, not the user. When you are done, report it with Fallow's task_report tool (task {t}).]"
        ),
        (Purpose::Wake(k), _) => format!(
            "[Fallow: event {} was signalled by {who}. You asked to be woken for it.]",
            k.1
        ),
    }
}

/// The bytes typed into the target: label and text as one paste, then Enter. Agents that did
/// not ask for bracketed paste get one line, because each line break would be Enter.
pub fn bytes(d: &Delivery, bracketed: bool) -> Vec<u8> {
    let mut v = if bracketed {
        crate::session::keys::paste(&format!("{}\n{}", label(d), d.text), true)
    } else {
        let flat = d.text.replace("\r\n", "\n").replace(['\n', '\r'], " / ");
        crate::session::keys::paste(&format!("{} {flat}", label(d)), false)
    };
    v.push(b'\r');
    v
}

#[derive(Debug, Default)]
pub struct Deliveries {
    pub list: Vec<Delivery>,
    next: u64,
}

impl Deliveries {
    pub fn pending(&self) -> impl Iterator<Item = &Delivery> {
        self.list.iter().filter(|d| d.state == State::Pending)
    }

    /// Whether `from` may add another pending request.
    pub fn room_for(&self, from: &Party) -> Result<(), String> {
        let pending: Vec<&Delivery> = self.pending().collect();
        if pending.len() >= PENDING_MAX {
            return Err(format!("{PENDING_MAX} requests are waiting for the user"));
        }
        if pending.iter().filter(|d| &d.from == from).count() >= PENDING_PER_SENDER {
            return Err(format!(
                "{PENDING_PER_SENDER} of your requests are waiting for the user"
            ));
        }
        Ok(())
    }

    /// Adds a request; `covered_by` approves it at once.
    #[allow(clippy::too_many_arguments)]
    pub fn add(
        &mut self,
        from: Party,
        to: u32,
        text: String,
        purpose: Purpose,
        now: Instant,
        expires: Instant,
        covered_by: Option<u32>,
    ) -> u64 {
        self.next += 1;
        let id = self.next;
        self.list.push(Delivery {
            id,
            from,
            to,
            text,
            purpose,
            at: SystemTime::now(),
            created: now,
            expires,
            covered_by,
            edited: false,
            state: if covered_by.is_some() {
                State::Approved
            } else {
                State::Pending
            },
        });
        self.trim();
        id
    }

    pub fn get(&self, id: u64) -> Option<&Delivery> {
        self.list.iter().find(|d| d.id == id)
    }

    fn get_mut(&mut self, id: u64) -> Option<&mut Delivery> {
        self.list.iter_mut().find(|d| d.id == id)
    }

    /// The engineer approves a pending request, optionally with edited text.
    pub fn approve(&mut self, id: u64, text: Option<String>) -> Result<(), String> {
        let d = self.get_mut(id).ok_or("no such request")?;
        if d.state != State::Pending {
            return Err(format!("the request is {}", d.state.label()));
        }
        if let Some(t) = text {
            d.edited = t != d.text;
            d.text = t;
        }
        d.state = State::Approved;
        Ok(())
    }

    /// Sets a finished state (rejected, expired, failed, delivered).
    pub fn finish(&mut self, id: u64, state: State) -> Option<&Delivery> {
        let d = self.get_mut(id)?;
        if d.state.finished() {
            return None;
        }
        d.state = state;
        self.get(id)
    }

    /// A grant for an approved delivery: the only way to type it.
    pub fn grant(&self, id: u64) -> Option<DeliveryGrant> {
        let d = self.get(id)?;
        (d.state == State::Approved).then_some(DeliveryGrant { id, to: d.to })
    }

    /// Keeps the newest `FINISHED_KEPT` finished deliveries.
    fn trim(&mut self) {
        let finished = self.list.iter().filter(|d| d.state.finished()).count();
        if finished > FINISHED_KEPT {
            let mut drop = finished - FINISHED_KEPT;
            self.list.retain(|d| {
                if drop > 0 && d.state.finished() {
                    drop -= 1;
                    false
                } else {
                    true
                }
            });
        }
    }
}
