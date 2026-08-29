//! What the agents have used of their plans, asked of each CLI by a command
//! the settings name — every agent reports differently, and a command line
//! the user writes is the one shape that fits them all. The command prints
//! JSON: `{"plan": "Max", "percent": 62, "detail": "…", "reset": "…"}`.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::Instant;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Usage {
    pub agent: String,
    pub plan: String,
    pub percent: u8,
    pub detail: String,
    pub reset: String,
}

/// Runs every command and reads what it printed; an agent whose command
/// fails or prints nonsense is reported with the error in its detail.
pub fn poll(commands: &BTreeMap<String, String>) -> Vec<Usage> {
    commands
        .iter()
        .map(|(agent, command)| {
            let out = std::process::Command::new(if cfg!(windows) { "cmd" } else { "sh" })
                .args([if cfg!(windows) { "/C" } else { "-c" }, command])
                .output();
            let text = match out {
                Ok(out) if out.status.success() => {
                    String::from_utf8_lossy(&out.stdout).into_owned()
                }
                Ok(out) => {
                    let err = String::from_utf8_lossy(&out.stderr);
                    return failed(agent, err.lines().next().unwrap_or("the command failed"));
                }
                Err(e) => return failed(agent, &e.to_string()),
            };
            match serde_json::from_str::<serde_json::Value>(&text) {
                Ok(value) => Usage {
                    agent: agent.clone(),
                    plan: value["plan"].as_str().unwrap_or("").to_string(),
                    percent: value["percent"].as_u64().unwrap_or(0).min(100) as u8,
                    detail: value["detail"].as_str().unwrap_or("").to_string(),
                    reset: value["reset"].as_str().unwrap_or("").to_string(),
                },
                Err(_) => failed(agent, "did not print usage JSON"),
            }
        })
        .collect()
}

fn failed(agent: &str, why: &str) -> Usage {
    Usage {
        agent: agent.to_string(),
        plan: String::new(),
        percent: 0,
        detail: why.to_string(),
        reset: String::new(),
    }
}

/// The figures, and when they landed.
type Fetched = Option<(Vec<Usage>, Instant)>;

/// The poll as a frontend runs it: on a thread, kept with when it landed,
/// so the panel can say how fresh its figures are.
#[derive(Default)]
pub struct Poller {
    state: Arc<Mutex<Fetched>>,
    running: bool,
}

impl Poller {
    pub fn start(
        &mut self,
        commands: BTreeMap<String, String>,
        notify: impl Fn() + Send + 'static,
    ) {
        if self.running {
            return;
        }
        self.running = true;
        let state = self.state.clone();
        std::thread::spawn(move || {
            let usage = poll(&commands);
            *state.lock().unwrap() = Some((usage, Instant::now()));
            notify();
        });
    }

    /// The latest figures and how long ago they were fetched.
    pub fn latest(&mut self) -> Option<(Vec<Usage>, u64)> {
        let state = self.state.lock().unwrap();
        let (usage, at) = state.as_ref()?;
        self.running = false;
        Some((usage.clone(), at.elapsed().as_secs()))
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn each_agents_command_is_run_and_its_json_read() {
        let commands = BTreeMap::from([
            (
                "claude".to_string(),
                r#"echo '{"plan":"Max","percent":62,"detail":"1.2M tokens","reset":"in 3h"}'"#
                    .to_string(),
            ),
            ("codex".to_string(), "echo not json".to_string()),
            ("cursor".to_string(), "exit 3".to_string()),
        ]);
        let usage = poll(&commands);
        assert_eq!(usage[0].agent, "claude");
        assert_eq!((usage[0].plan.as_str(), usage[0].percent), ("Max", 62));
        assert_eq!(usage[0].detail, "1.2M tokens");
        assert_eq!(usage[1].detail, "did not print usage JSON");
        assert_eq!(usage[2].detail, "the command failed");
        let mut poller = Poller::default();
        poller.start(commands, || {});
        let start = Instant::now();
        while poller.latest().is_none() && start.elapsed().as_secs() < 5 {
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert_eq!(poller.latest().unwrap().0.len(), 3);
    }
}
