//! Plugin pipelines (spec 1.4 "Atoms và pipeline", 3.6): `od.pipeline` is
//! a list of stages, each naming catalogue atoms; a stage with
//! `repeat: true` loops until its `until` expression holds or the devloop
//! cap is hit. Port of the original `plugins/{pipeline,until}.ts`.
//!
//! The scheduler is headless: the caller supplies the stage worker and an
//! event sink, so the same walk drives a run, a dry run and the tests.

use crate::atoms::is_known_atom;
use serde::Serialize;
use serde_json::Value;
use std::collections::HashSet;

/// Default `OD_MAX_DEVLOOP_ITERATIONS`: a buggy plugin cannot loop forever.
pub const DEFAULT_MAX_ITERATIONS: u32 = 10;
pub const MAX_ITERATIONS_ENV: &str = "OD_MAX_DEVLOOP_ITERATIONS";

/// The devloop cap from the environment, else the default.
#[must_use]
pub fn max_iterations_from_env() -> u32 {
    std::env::var(MAX_ITERATIONS_ENV)
        .ok()
        .and_then(|v| v.trim().parse::<u32>().ok())
        .filter(|n| *n > 0)
        .unwrap_or(DEFAULT_MAX_ITERATIONS)
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PipelineError {
    #[error("pipeline must be an object with a non-empty stages array")]
    Shape,
    #[error("stage {0} has no id")]
    MissingId(usize),
    #[error("duplicate stage id {0:?}")]
    DuplicateId(String),
    #[error("stage {stage:?} names no atoms")]
    NoAtoms { stage: String },
    #[error("stage {stage:?} uses unknown atom {atom:?}")]
    UnknownAtom { stage: String, atom: String },
    #[error("stage {0:?} sets repeat:true but no until expression")]
    RepeatWithoutUntil(String),
    #[error("stage {stage:?} until expression rejected: {error}")]
    Until { stage: String, error: UntilError },
}

// ---------------------------------------------------------------------------
// until

/// A signal the closed `until` vocabulary can read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum Signal {
    #[serde(rename = "critique.score")]
    CritiqueScore,
    #[serde(rename = "iterations")]
    Iterations,
    #[serde(rename = "user.confirmed")]
    UserConfirmed,
    #[serde(rename = "preview.ok")]
    PreviewOk,
    #[serde(rename = "build.passing")]
    BuildPassing,
    #[serde(rename = "tests.passing")]
    TestsPassing,
}

impl Signal {
    const ALL: [(&'static str, Self); 6] = [
        ("critique.score", Self::CritiqueScore),
        ("iterations", Self::Iterations),
        ("user.confirmed", Self::UserConfirmed),
        ("preview.ok", Self::PreviewOk),
        ("build.passing", Self::BuildPassing),
        ("tests.passing", Self::TestsPassing),
    ];

    fn parse(name: &str) -> Option<Self> {
        Self::ALL.iter().find(|(n, _)| *n == name).map(|(_, s)| *s)
    }

    fn is_number(self) -> bool {
        matches!(self, Self::CritiqueScore | Self::Iterations)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum Op {
    #[serde(rename = "==")]
    Eq,
    #[serde(rename = "!=")]
    Ne,
    #[serde(rename = ">=")]
    Ge,
    #[serde(rename = "<=")]
    Le,
    #[serde(rename = ">")]
    Gt,
    #[serde(rename = "<")]
    Lt,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(untagged)]
pub enum Literal {
    Number(f64),
    Bool(bool),
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Comparison {
    pub signal: Signal,
    pub op: Op,
    pub value: Literal,
}

/// Disjunction of conjunctions: `a && b || c`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Until {
    pub any: Vec<Vec<Comparison>>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum UntilError {
    #[error("empty until expression")]
    Empty,
    #[error("expected a comparison operator in {0:?}")]
    NoOperator(String),
    #[error(
        "unknown signal {0:?}; supported: critique.score, iterations, user.confirmed, preview.ok, build.passing, tests.passing"
    )]
    UnknownSignal(String),
    #[error("signal {signal:?} expects true/false, got {got:?}")]
    ExpectedBool { signal: String, got: String },
    #[error("boolean signal {0:?} only supports == and !=")]
    BoolOperator(String),
    #[error("signal {signal:?} expects a number, got {got:?}")]
    ExpectedNumber { signal: String, got: String },
}

/// Splits on `sep` outside parentheses; empty parts are dropped.
fn split_top_level<'a>(input: &'a str, sep: &str) -> Vec<&'a str> {
    let bytes = input.as_bytes();
    let sep = sep.as_bytes();
    let mut parts = Vec::new();
    let (mut depth, mut cursor, mut i) = (0i32, 0usize, 0usize);
    while i + 1 < bytes.len() {
        match bytes[i] {
            b'(' => depth += 1,
            b')' => depth -= 1,
            _ if depth == 0 && bytes[i..].starts_with(sep) => {
                parts.push(&input[cursor..i]);
                cursor = i + sep.len();
                i += sep.len();
                continue;
            }
            _ => {}
        }
        i += 1;
    }
    parts.push(&input[cursor..]);
    parts
        .into_iter()
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .collect()
}

/// `(a)` → `a`, only when the outer pair encloses everything.
fn strip_outer_parens(mut s: &str) -> &str {
    while s.starts_with('(') && s.ends_with(')') {
        let mut depth = 0i32;
        for (i, b) in s.bytes().enumerate() {
            match b {
                b'(' => depth += 1,
                b')' => depth -= 1,
                _ => {}
            }
            if depth == 0 && i < s.len() - 1 {
                return s;
            }
        }
        s = s[1..s.len() - 1].trim();
    }
    s
}

fn parse_comparison(raw: &str) -> Result<Comparison, UntilError> {
    let expr = strip_outer_parens(raw).trim();
    // Two-character operators first so `>=` is not read as `>`.
    let found = ["==", "!=", ">=", "<=", ">", "<"]
        .iter()
        .filter_map(|op| expr.find(op).map(|at| (at, *op)))
        .min_by_key(|(at, op)| (*at, std::cmp::Reverse(op.len())));
    let Some((at, op_str)) = found else {
        // A bare boolean signal reads as `== true`, `!signal` as `== false`
        // (the bundled code-migration scenario writes
        // `(build.passing && tests.passing)`).
        let (name, value) = match expr.strip_prefix('!') {
            Some(rest) => (rest.trim(), false),
            None => (expr, true),
        };
        return match Signal::parse(name) {
            Some(signal) if !signal.is_number() => Ok(Comparison {
                signal,
                op: Op::Eq,
                value: Literal::Bool(value),
            }),
            _ => Err(UntilError::NoOperator(raw.to_owned())),
        };
    };
    let op = match op_str {
        "==" => Op::Eq,
        "!=" => Op::Ne,
        ">=" => Op::Ge,
        "<=" => Op::Le,
        ">" => Op::Gt,
        _ => Op::Lt,
    };
    let lhs = expr[..at].trim();
    let rhs = expr[at + op_str.len()..].trim();
    let signal = Signal::parse(lhs).ok_or_else(|| UntilError::UnknownSignal(lhs.to_owned()))?;
    let value = if signal.is_number() {
        let n: f64 = rhs.parse().map_err(|_| UntilError::ExpectedNumber {
            signal: lhs.to_owned(),
            got: rhs.to_owned(),
        })?;
        if !n.is_finite() {
            return Err(UntilError::ExpectedNumber {
                signal: lhs.to_owned(),
                got: rhs.to_owned(),
            });
        }
        Literal::Number(n)
    } else {
        let b = match rhs {
            "true" => true,
            "false" => false,
            _ => {
                return Err(UntilError::ExpectedBool {
                    signal: lhs.to_owned(),
                    got: rhs.to_owned(),
                });
            }
        };
        if !matches!(op, Op::Eq | Op::Ne) {
            return Err(UntilError::BoolOperator(lhs.to_owned()));
        }
        Literal::Bool(b)
    };
    Ok(Comparison { signal, op, value })
}

/// Parses `critique.score>=4 || iterations>=3`.
///
/// # Errors
/// [`UntilError`] for anything outside the closed grammar.
pub fn parse_until(source: &str) -> Result<Until, UntilError> {
    let trimmed = source.trim();
    if trimmed.is_empty() {
        return Err(UntilError::Empty);
    }
    let any = split_top_level(strip_outer_parens(trimmed), "||")
        .into_iter()
        .map(|or| {
            split_top_level(strip_outer_parens(or), "&&")
                .into_iter()
                .map(parse_comparison)
                .collect::<Result<Vec<_>, _>>()
        })
        .collect::<Result<Vec<_>, _>>()?;
    if any.iter().all(Vec::is_empty) {
        return Err(UntilError::Empty);
    }
    Ok(Until { any })
}

/// What a stage reported, plus the iteration count the scheduler adds.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Signals {
    pub critique_score: Option<f64>,
    pub iterations: u32,
    pub user_confirmed: Option<bool>,
    pub preview_ok: Option<bool>,
    pub build_passing: Option<bool>,
    pub tests_passing: Option<bool>,
}

impl Signals {
    fn get(&self, s: Signal) -> Option<Literal> {
        match s {
            Signal::CritiqueScore => self.critique_score.map(Literal::Number),
            Signal::Iterations => Some(Literal::Number(f64::from(self.iterations))),
            Signal::UserConfirmed => self.user_confirmed.map(Literal::Bool),
            Signal::PreviewOk => self.preview_ok.map(Literal::Bool),
            Signal::BuildPassing => self.build_passing.map(Literal::Bool),
            Signal::TestsPassing => self.tests_passing.map(Literal::Bool),
        }
    }
}

impl Comparison {
    /// A missing signal never satisfies a term.
    #[must_use]
    pub fn holds(&self, signals: &Signals) -> bool {
        match (signals.get(self.signal), self.value) {
            (Some(Literal::Bool(l)), Literal::Bool(r)) => match self.op {
                Op::Eq => l == r,
                Op::Ne => l != r,
                _ => false,
            },
            (Some(Literal::Number(l)), Literal::Number(r)) => match self.op {
                Op::Eq => (l - r).abs() < f64::EPSILON,
                Op::Ne => (l - r).abs() >= f64::EPSILON,
                Op::Ge => l >= r,
                Op::Le => l <= r,
                Op::Gt => l > r,
                Op::Lt => l < r,
            },
            _ => false,
        }
    }
}

impl Until {
    #[must_use]
    pub fn satisfied(&self, signals: &Signals) -> bool {
        self.any
            .iter()
            .any(|all| !all.is_empty() && all.iter().all(|c| c.holds(signals)))
    }
}

// ---------------------------------------------------------------------------
// Pipeline

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Stage {
    pub id: String,
    pub atoms: Vec<String>,
    pub repeat: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub until: Option<String>,
    #[serde(skip)]
    parsed_until: Option<Until>,
}

impl Stage {
    #[must_use]
    pub fn until_expr(&self) -> Option<&Until> {
        self.parsed_until.as_ref()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Pipeline {
    pub stages: Vec<Stage>,
}

fn str_list(v: Option<&Value>) -> Vec<String> {
    v.and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect()
}

impl Pipeline {
    /// Validates `od.pipeline`: unique stage ids, catalogue atoms, a
    /// parseable `until`, and no `repeat` without `until`.
    ///
    /// # Errors
    /// The first [`PipelineError`] found, in stage order.
    pub fn parse(raw: &Value) -> Result<Self, PipelineError> {
        let stages = raw
            .get("stages")
            .and_then(Value::as_array)
            .filter(|s| !s.is_empty())
            .ok_or(PipelineError::Shape)?;
        let mut seen = HashSet::new();
        let mut out = Vec::with_capacity(stages.len());
        for (i, s) in stages.iter().enumerate() {
            let id = s
                .get("id")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|id| !id.is_empty())
                .ok_or(PipelineError::MissingId(i + 1))?
                .to_owned();
            if !seen.insert(id.clone()) {
                return Err(PipelineError::DuplicateId(id));
            }
            let atoms = str_list(s.get("atoms"));
            if atoms.is_empty() {
                return Err(PipelineError::NoAtoms { stage: id });
            }
            if let Some(atom) = atoms.iter().find(|a| !is_known_atom(a)) {
                return Err(PipelineError::UnknownAtom {
                    stage: id,
                    atom: atom.clone(),
                });
            }
            let repeat = s.get("repeat").and_then(Value::as_bool) == Some(true);
            let until = s
                .get("until")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|u| !u.is_empty())
                .map(str::to_owned);
            if repeat && until.is_none() {
                return Err(PipelineError::RepeatWithoutUntil(id));
            }
            let parsed_until = until
                .as_deref()
                .map(parse_until)
                .transpose()
                .map_err(|error| PipelineError::Until {
                    stage: id.clone(),
                    error,
                })?;
            out.push(Stage {
                id,
                atoms,
                repeat,
                until,
                parsed_until,
            });
        }
        Ok(Self { stages: out })
    }
}

/// What a stage worker returns for one iteration.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct StageRun {
    pub signals: Signals,
    pub diff_summary: Option<String>,
}

/// `pipeline_stage_started` / `pipeline_stage_completed`, in the shape the
/// run's SSE stream carries.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum StageEvent {
    PipelineStageStarted {
        #[serde(rename = "stageId")]
        stage_id: String,
        iteration: u32,
    },
    PipelineStageCompleted {
        #[serde(rename = "stageId")]
        stage_id: String,
        iteration: u32,
        converged: bool,
        #[serde(rename = "diffSummary", skip_serializing_if = "Option::is_none")]
        diff_summary: Option<String>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Termination {
    /// Single-shot stage.
    NoLoop,
    UntilSatisfied,
    IterationCap,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StageOutcome {
    pub stage_id: String,
    pub iterations: u32,
    pub converged: bool,
    pub termination: Termination,
}

/// Walks every stage in order. A repeating stage runs until its `until`
/// holds (signals plus the completed iteration count) or `max_iterations`
/// iterations ran; hitting the cap is reported, not an error.
pub async fn run_pipeline<R, E>(
    pipeline: &Pipeline,
    max_iterations: u32,
    mut run_stage: R,
    mut emit: E,
) -> Vec<StageOutcome>
where
    R: AsyncFnMut(&Stage, u32) -> StageRun,
    E: FnMut(StageEvent),
{
    let mut out = Vec::with_capacity(pipeline.stages.len());
    for stage in &pipeline.stages {
        let max = if stage.repeat {
            max_iterations.max(1)
        } else {
            1
        };
        let mut outcome = StageOutcome {
            stage_id: stage.id.clone(),
            iterations: 0,
            converged: false,
            termination: Termination::IterationCap,
        };
        while outcome.iterations < max {
            let iteration = outcome.iterations;
            emit(StageEvent::PipelineStageStarted {
                stage_id: stage.id.clone(),
                iteration,
            });
            let mut run = run_stage(stage, iteration).await;
            outcome.iterations += 1;
            run.signals.iterations = outcome.iterations;
            let converged = match stage.until_expr() {
                Some(until) if stage.repeat => until.satisfied(&run.signals),
                _ => true,
            };
            emit(StageEvent::PipelineStageCompleted {
                stage_id: stage.id.clone(),
                iteration,
                converged,
                diff_summary: run.diff_summary,
            });
            if converged {
                outcome.converged = true;
                outcome.termination = if stage.repeat {
                    Termination::UntilSatisfied
                } else {
                    Termination::NoLoop
                };
                break;
            }
        }
        out.push(outcome);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn block_on<F: std::future::Future>(f: F) -> F::Output {
        // The scheduler never awaits real I/O in these tests: one poll
        // with a no-op waker completes it.
        let mut f = std::pin::pin!(f);
        let mut cx = std::task::Context::from_waker(std::task::Waker::noop());
        match f.as_mut().poll(&mut cx) {
            std::task::Poll::Ready(v) => v,
            std::task::Poll::Pending => panic!("scheduler future did not complete"),
        }
    }

    #[test]
    fn until_grammar() {
        let u = parse_until("critique.score>=4 || iterations>=3").unwrap();
        assert_eq!(u.any.len(), 2);
        let mut s = Signals {
            iterations: 1,
            critique_score: Some(3.5),
            ..Signals::default()
        };
        assert!(!u.satisfied(&s));
        s.critique_score = Some(4.0);
        assert!(u.satisfied(&s));
        s.critique_score = None;
        s.iterations = 3;
        assert!(u.satisfied(&s));

        let both = parse_until("(build.passing == true) && tests.passing==true").unwrap();
        let mut s = Signals {
            build_passing: Some(true),
            ..Signals::default()
        };
        assert!(!both.satisfied(&s), "a missing signal never holds");
        s.tests_passing = Some(true);
        assert!(both.satisfied(&s));
        let bare = parse_until("(build.passing && !preview.ok) || (critique.score > 4)").unwrap();
        assert!(bare.satisfied(&Signals {
            build_passing: Some(true),
            preview_ok: Some(false),
            ..Signals::default()
        }));
        assert!(matches!(
            parse_until("critique.score"),
            Err(UntilError::NoOperator(_))
        ));
        assert!(parse_until("iterations<2").unwrap().satisfied(&Signals {
            iterations: 1,
            ..Signals::default()
        }));
    }

    #[test]
    fn until_rejects_outside_the_vocabulary() {
        assert_eq!(parse_until("  "), Err(UntilError::Empty));
        assert!(matches!(
            parse_until("score>=4"),
            Err(UntilError::UnknownSignal(_))
        ));
        assert!(matches!(
            parse_until("iterations"),
            Err(UntilError::NoOperator(_))
        ));
        assert!(matches!(
            parse_until("preview.ok>=true"),
            Err(UntilError::BoolOperator(_))
        ));
        assert!(matches!(
            parse_until("preview.ok==yes"),
            Err(UntilError::ExpectedBool { .. })
        ));
        assert!(matches!(
            parse_until("iterations>=three"),
            Err(UntilError::ExpectedNumber { .. })
        ));
    }

    #[test]
    fn pipeline_validation() {
        let ok = json!({"stages": [
            {"id": "plan", "atoms": ["todo-write"]},
            {"id": "critique", "atoms": ["critique-theater"], "repeat": true, "until": "critique.score>=4"}
        ]});
        let p = Pipeline::parse(&ok).unwrap();
        assert_eq!(p.stages.len(), 2);
        assert!(p.stages[1].until_expr().is_some());

        let cases = [
            (json!({}), PipelineError::Shape),
            (
                json!({"stages": [{"atoms": ["todo-write"]}]}),
                PipelineError::MissingId(1),
            ),
            (
                json!({"stages": [{"id": "a", "atoms": ["todo-write"]}, {"id": "a", "atoms": ["file-read"]}]}),
                PipelineError::DuplicateId("a".into()),
            ),
            (
                json!({"stages": [{"id": "a", "atoms": []}]}),
                PipelineError::NoAtoms { stage: "a".into() },
            ),
            (
                json!({"stages": [{"id": "a", "atoms": ["shell-anything"]}]}),
                PipelineError::UnknownAtom {
                    stage: "a".into(),
                    atom: "shell-anything".into(),
                },
            ),
            (
                json!({"stages": [{"id": "a", "atoms": ["todo-write"], "repeat": true}]}),
                PipelineError::RepeatWithoutUntil("a".into()),
            ),
        ];
        for (raw, want) in cases {
            assert_eq!(Pipeline::parse(&raw).unwrap_err(), want, "{raw}");
        }
        assert!(matches!(
            Pipeline::parse(
                &json!({"stages": [{"id": "a", "atoms": ["todo-write"], "repeat": true, "until": "nope"}]})
            ),
            Err(PipelineError::Until { .. })
        ));
    }

    #[test]
    fn scheduler_loops_until_converged_and_stops_at_the_cap() {
        let p = Pipeline::parse(&json!({"stages": [
            {"id": "generate", "atoms": ["file-write"]},
            {"id": "critique", "atoms": ["critique-theater"], "repeat": true, "until": "critique.score>=4"},
            {"id": "polish", "atoms": ["file-edit"], "repeat": true, "until": "user.confirmed==true"}
        ]}))
        .unwrap();
        let scores = [2.0, 3.0, 4.5];
        let mut events = Vec::new();
        let mut calls = Vec::new();
        let outcomes = block_on(run_pipeline(
            &p,
            3,
            async |stage: &Stage, i: u32| {
                calls.push((stage.id.clone(), i));
                let critique_score = (stage.id == "critique").then(|| scores[i as usize]);
                StageRun {
                    signals: Signals {
                        critique_score,
                        ..Signals::default()
                    },
                    diff_summary: None,
                }
            },
            |e| events.push(e),
        ));
        assert_eq!(
            outcomes,
            vec![
                StageOutcome {
                    stage_id: "generate".into(),
                    iterations: 1,
                    converged: true,
                    termination: Termination::NoLoop
                },
                StageOutcome {
                    stage_id: "critique".into(),
                    iterations: 3,
                    converged: true,
                    termination: Termination::UntilSatisfied
                },
                StageOutcome {
                    stage_id: "polish".into(),
                    iterations: 3,
                    converged: false,
                    termination: Termination::IterationCap
                },
            ]
        );
        assert_eq!(calls.len(), 7);
        assert_eq!(events.len(), 14, "started + completed per iteration");
        assert_eq!(
            serde_json::to_value(&events[0]).unwrap(),
            json!({"type": "pipeline_stage_started", "stageId": "generate", "iteration": 0})
        );
        assert_eq!(
            serde_json::to_value(&events[5]).unwrap(),
            json!({"type": "pipeline_stage_completed", "stageId": "critique", "iteration": 1, "converged": false})
        );
    }

    #[test]
    fn bundled_scenarios_parse() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../content/plugins/_official/scenarios");
        let mut parsed = 0;
        for entry in std::fs::read_dir(&root).unwrap() {
            let path = entry.unwrap().path().join("open-design.json");
            let Ok(text) = std::fs::read_to_string(&path) else {
                continue;
            };
            let v: Value = serde_json::from_str(&text).unwrap();
            if let Some(p) = v.get("od").and_then(|od| od.get("pipeline")) {
                Pipeline::parse(p).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
                parsed += 1;
            }
        }
        assert!(parsed >= 3, "only {parsed} bundled pipelines");
    }
}
