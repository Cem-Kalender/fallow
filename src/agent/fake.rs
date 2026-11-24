//! The `fake` runtime (007 research S-13, contracts/runtime.md): `fallow-fake-agent` playing a
//! TOML scenario. It speaks `claude -p`'s flags and stream-json, so it runs through the Claude
//! runtime's code; tests drive the loop through the same interface as Claude Code.

use std::path::Path;
use std::sync::atomic::AtomicBool;

use super::claude::Claude;
use super::{Availability, Call, Capabilities, Event, Outcome, Runtime};

#[derive(Debug, Clone)]
pub struct Fake {
    inner: Claude,
    caps: Capabilities,
}

impl Fake {
    /// `exe` is `fallow-fake-agent`, `scenario` the TOML file it plays.
    pub fn new(exe: impl Into<String>, scenario: &Path) -> Fake {
        Fake::from_profile(
            exe,
            vec!["--scenario".into(), scenario.display().to_string()],
        )
    }
}

impl Fake {
    /// The runtime of an `[agents.fake]` profile: the program and its leading arguments (the
    /// scenario). 012: the profile declares the fake's capabilities among its arguments
    /// (`--images`, `--web-off`, `--control-file <path>`), so tests can play both paths; the
    /// fake agent ignores them.
    pub fn from_profile(command: impl Into<String>, args: Vec<String>) -> Fake {
        let mut caps = Capabilities::default();
        let mut i = 0;
        while i < args.len() {
            match args[i].as_str() {
                "--images" => caps.images = true,
                "--web-off" => caps.web_off = true,
                "--control-file" => {
                    if let Some(p) = args.get(i + 1) {
                        caps.control_files.push(p.clone());
                    }
                    i += 1;
                }
                _ => {}
            }
            i += 1;
        }
        Fake {
            inner: Claude::new(command, args),
            caps,
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

    fn capabilities(&self) -> Capabilities {
        self.caps.clone()
    }

    fn start(&self, call: &Call, sink: &mut dyn FnMut(Event), stop: &AtomicBool) -> Outcome {
        self.inner.start(call, sink, stop)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_profile_declares_the_fakes_capabilities() {
        let f = Fake::from_profile(
            "fallow-fake-agent",
            vec![
                "--scenario".into(),
                "s.toml".into(),
                "--images".into(),
                "--control-file".into(),
                ".claude/settings.json".into(),
            ],
        );
        let c = f.capabilities();
        assert!(c.images && !c.web_off);
        assert_eq!(c.control_files, vec![".claude/settings.json".to_string()]);
        assert_eq!(
            Fake::new("x", Path::new("s.toml")).capabilities(),
            Capabilities::default()
        );
    }
}
