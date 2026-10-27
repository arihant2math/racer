//! Gate results and their Markdown/JSON rendering.

use serde::Serialize;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Pass,
    Fail,
    /// Informational: reported, never fails a gate.
    Info,
    /// The gate did not run (tool missing or disabled).
    Skip,
    /// Produced material that needs a (multimodal) reviewer's verdict.
    Review,
}

impl Status {
    pub fn label(self) -> &'static str {
        match self {
            Status::Pass => "PASS",
            Status::Fail => "FAIL",
            Status::Info => "info",
            Status::Skip => "SKIP",
            Status::Review => "REVIEW",
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct Check {
    pub id: String,
    pub what: String,
    pub value: String,
    pub threshold: String,
    pub status: Status,
}

#[derive(Clone, Debug, Serialize)]
pub struct Gate {
    pub number: u8,
    pub name: String,
    pub status: Status,
    pub checks: Vec<Check>,
    pub notes: Vec<String>,
    /// Images written for this gate, relative to the report directory.
    pub images: Vec<String>,
}

impl Gate {
    pub fn new(number: u8, name: &str) -> Gate {
        Gate {
            number,
            name: name.to_string(),
            status: Status::Pass,
            checks: Vec::new(),
            notes: Vec::new(),
            images: Vec::new(),
        }
    }

    pub fn skipped(number: u8, name: &str, why: impl Into<String>) -> Gate {
        let mut g = Gate::new(number, name);
        g.status = Status::Skip;
        g.notes.push(why.into());
        g
    }

    /// Records a check; a failing one fails the gate.
    pub fn check(
        &mut self,
        id: &str,
        what: impl Into<String>,
        value: impl Into<String>,
        threshold: impl Into<String>,
        pass: bool,
    ) {
        let status = if pass { Status::Pass } else { Status::Fail };
        if !pass {
            self.status = Status::Fail;
        }
        self.checks.push(Check {
            id: id.to_string(),
            what: what.into(),
            value: value.into(),
            threshold: threshold.into(),
            status,
        });
    }

    pub fn info(&mut self, id: &str, what: impl Into<String>, value: impl Into<String>) {
        self.checks.push(Check {
            id: id.to_string(),
            what: what.into(),
            value: value.into(),
            threshold: String::new(),
            status: Status::Info,
        });
    }

    pub fn note(&mut self, note: impl Into<String>) {
        self.notes.push(note.into());
    }

    pub fn failed(&self) -> Vec<&Check> {
        self.checks
            .iter()
            .filter(|c| c.status == Status::Fail)
            .collect()
    }

    pub fn passed(&self) -> bool {
        self.status != Status::Fail
    }

    /// Status of a check by id (tests use this).
    pub fn status_of(&self, id: &str) -> Option<Status> {
        self.checks.iter().find(|c| c.id == id).map(|c| c.status)
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct Report {
    pub car_id: String,
    pub car_name: String,
    pub glb: String,
    pub glb_bytes: u64,
    pub generator: Option<String>,
    pub verdict: Status,
    pub gates: Vec<Gate>,
    pub load_warnings: Vec<String>,
}

impl Report {
    /// FAIL if any gate failed, REVIEW if the automatic gates passed but a
    /// review pack awaits its reviewer, else PASS.
    pub fn verdict_of(gates: &[Gate]) -> Status {
        if gates.iter().any(|g| g.status == Status::Fail) {
            Status::Fail
        } else if gates.iter().any(|g| g.status == Status::Review) {
            Status::Review
        } else {
            Status::Pass
        }
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("report serialises")
    }

    pub fn to_markdown(&self) -> String {
        let mut s = String::new();
        let p = |s: &mut String, line: &str| {
            s.push_str(line);
            s.push('\n');
        };
        p(&mut s, &format!("# Asset QA: {} (`{}`)", self.car_name, self.car_id));
        p(&mut s, "");
        p(
            &mut s,
            &format!(
                "Asset `{}` ({:.1} MB{}). **Verdict: {}**",
                self.glb,
                self.glb_bytes as f64 / 1e6,
                self.generator
                    .as_deref()
                    .map(|g| format!(", generator {g}"))
                    .unwrap_or_default(),
                self.verdict.label()
            ),
        );
        p(&mut s, "");
        p(&mut s, "| Gate | Result | Failed checks |");
        p(&mut s, "|---|---|---|");
        for g in &self.gates {
            let failed: Vec<&str> = g.failed().iter().map(|c| c.id.as_str()).collect();
            p(
                &mut s,
                &format!(
                    "| {}. {} | {} | {} |",
                    g.number,
                    g.name,
                    g.status.label(),
                    failed.join(", ")
                ),
            );
        }
        if !self.load_warnings.is_empty() {
            p(&mut s, "");
            p(&mut s, "Load warnings:");
            p(&mut s, "");
            for w in &self.load_warnings {
                p(&mut s, &format!("- {w}"));
            }
        }
        for g in &self.gates {
            p(&mut s, "");
            p(
                &mut s,
                &format!("## {}. {}: {}", g.number, g.name, g.status.label()),
            );
            p(&mut s, "");
            if !g.checks.is_empty() {
                p(&mut s, "| Check | What | Value | Threshold | Result |");
                p(&mut s, "|---|---|---|---|---|");
                for c in &g.checks {
                    p(
                        &mut s,
                        &format!(
                            "| `{}` | {} | {} | {} | {} |",
                            c.id,
                            escape(&c.what),
                            escape(&c.value),
                            escape(&c.threshold),
                            c.status.label()
                        ),
                    );
                }
                p(&mut s, "");
            }
            for n in &g.notes {
                p(&mut s, n);
                p(&mut s, "");
            }
            for img in &g.images {
                p(&mut s, &format!("![{img}]({img})"));
            }
        }
        s
    }
}

fn escape(s: &str) -> String {
    s.replace('|', "\\|").replace('\n', " ")
}
