//! The `fake` runtime (007 research S-13, contracts/runtime.md): `fallow-fake-agent` playing a
//! TOML scenario. It speaks `claude -p`'s flags and stream-json, so it runs through the Claude
//! runtime's code; tests drive the loop through the same interface as Claude Code.

use std::path::Path;
use std::sync::atomic::AtomicBool;

use super::claude::Claude;
use super::{Availability, Call, Event, Outcome, Runtime};

#[derive(Debug, Clone)]
pub struct Fake {
    inner: Claude,
}

impl Fake {
    /// `exe` is `fallow-fake-agent`, `scenario` the TOML file it plays.
    pub fn new(exe: impl Into<String>, scenario: &Path) -> Fake {
        Fake {
            inner: Claude::new(
                exe,
                vec!["--scenario".into(), scenario.display().to_string()],
            ),
        }
    }
}

impl Fake {
    /// The runtime of an `[agents.fake]` profile: the program and its leading arguments (the
    /// scenario).
    pub fn from_profile(command: impl Into<String>, args: Vec<String>) -> Fake {
        Fake {
            inner: Claude::new(command, args),
        }
    }
}

impl Runtime for Fake {
    fn name(&self) -> &str {
        "fake"
    }

    fn available(&self) -> Availability {
        Availability {
            installed: crate::platform::find_program(&self.inner.agent.command).is_some(),
            version: Some("fake".into()),
        }
    }

    fn start(&self, call: &Call, sink: &mut dyn FnMut(Event), stop: &AtomicBool) -> Outcome {
        self.inner.start(call, sink, stop)
    }
}
