//! Pure state machine for the bounded Claude↔Codex planning flow.

pub mod queue;

use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Phase {
    PlanParallelDrafts,
    PlanSynthesisPending,
    PlanCodexReviewPending,
    PlanClaudeFinalizePending,
    PlanLocked,
    PlanEscalated,
}

impl fmt::Display for Phase {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = match self {
            Self::PlanParallelDrafts => "PlanParallelDrafts",
            Self::PlanSynthesisPending => "PlanSynthesisPending",
            Self::PlanCodexReviewPending => "PlanCodexReviewPending",
            Self::PlanClaudeFinalizePending => "PlanClaudeFinalizePending",
            Self::PlanLocked => "PlanLocked",
            Self::PlanEscalated => "PlanEscalated",
        };
        f.write_str(value)
    }
}

impl TryFrom<&str> for Phase {
    type Error = String;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "PlanParallelDrafts" => Ok(Self::PlanParallelDrafts),
            "PlanSynthesisPending" => Ok(Self::PlanSynthesisPending),
            "PlanCodexReviewPending" => Ok(Self::PlanCodexReviewPending),
            "PlanClaudeFinalizePending" => Ok(Self::PlanClaudeFinalizePending),
            "PlanLocked" => Ok(Self::PlanLocked),
            "PlanEscalated" => Ok(Self::PlanEscalated),
            other => Err(format!("unknown collab phase: {other}")),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CollabSession {
    pub id: String,
    pub phase: Phase,
    pub current_owner: String,
    pub claude_draft_hash: Option<String>,
    pub codex_draft_hash: Option<String>,
    pub canonical_plan_hash: Option<String>,
    pub final_plan_hash: Option<String>,
    pub codex_review_verdict: Option<String>,
}

impl CollabSession {
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            phase: Phase::PlanParallelDrafts,
            current_owner: "claude".to_string(),
            claude_draft_hash: None,
            codex_draft_hash: None,
            canonical_plan_hash: None,
            final_plan_hash: None,
            codex_review_verdict: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CollabEvent {
    SubmitDraft {
        content_hash: String,
    },
    PublishCanonical {
        content_hash: String,
    },
    SubmitReview {
        verdict: String,
    },
    PublishFinal {
        content_hash: String,
        codex_still_objects: bool,
    },
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum CollabError {
    #[error("not your turn: expected {expected}, got {got}")]
    NotYourTurn { expected: String, got: String },

    #[error("draft already submitted by {agent}")]
    AlreadySubmittedDraft { agent: String },

    #[error("invalid verdict value: {0}")]
    InvalidVerdictValue(String),

    #[error("wrong phase: expected {expected}, got {got}")]
    WrongPhase { expected: String, got: String },

    #[error("session is locked")]
    SessionLocked,
}

fn event_name(event: &CollabEvent) -> &'static str {
    match event {
        CollabEvent::SubmitDraft { .. } => "SubmitDraft",
        CollabEvent::PublishCanonical { .. } => "PublishCanonical",
        CollabEvent::SubmitReview { .. } => "SubmitReview",
        CollabEvent::PublishFinal { .. } => "PublishFinal",
    }
}

pub fn apply_event(
    session: &CollabSession,
    actor: &str,
    event: &CollabEvent,
) -> Result<CollabSession, CollabError> {
    if matches!(session.phase, Phase::PlanLocked | Phase::PlanEscalated) {
        return Err(CollabError::SessionLocked);
    }

    let mut next = session.clone();

    match (&session.phase, event) {
        (Phase::PlanParallelDrafts, CollabEvent::SubmitDraft { content_hash }) => match actor {
            "claude" => {
                if session.claude_draft_hash.is_some() {
                    return Err(CollabError::AlreadySubmittedDraft {
                        agent: actor.to_string(),
                    });
                }
                next.claude_draft_hash = Some(content_hash.clone());
                if session.codex_draft_hash.is_some() {
                    next.phase = Phase::PlanSynthesisPending;
                    next.current_owner = "claude".to_string();
                } else {
                    next.current_owner = "codex".to_string();
                }
            }
            "codex" => {
                if session.codex_draft_hash.is_some() {
                    return Err(CollabError::AlreadySubmittedDraft {
                        agent: actor.to_string(),
                    });
                }
                next.codex_draft_hash = Some(content_hash.clone());
                if session.claude_draft_hash.is_some() {
                    next.phase = Phase::PlanSynthesisPending;
                    next.current_owner = "claude".to_string();
                } else {
                    next.current_owner = "claude".to_string();
                }
            }
            _ => {
                return Err(CollabError::NotYourTurn {
                    expected: "claude|codex".to_string(),
                    got: actor.to_string(),
                });
            }
        },
        (Phase::PlanSynthesisPending, CollabEvent::PublishCanonical { content_hash }) => {
            if actor != "claude" {
                return Err(CollabError::NotYourTurn {
                    expected: "claude".to_string(),
                    got: actor.to_string(),
                });
            }
            next.canonical_plan_hash = Some(content_hash.clone());
            next.phase = Phase::PlanCodexReviewPending;
            next.current_owner = "codex".to_string();
        }
        (Phase::PlanCodexReviewPending, CollabEvent::SubmitReview { verdict }) => {
            if actor != "codex" {
                return Err(CollabError::NotYourTurn {
                    expected: "codex".to_string(),
                    got: actor.to_string(),
                });
            }
            if !matches!(
                verdict.as_str(),
                "approve" | "approve_with_minor_edits" | "request_changes"
            ) {
                return Err(CollabError::InvalidVerdictValue(verdict.clone()));
            }
            next.codex_review_verdict = Some(verdict.clone());
            next.phase = Phase::PlanClaudeFinalizePending;
            next.current_owner = "claude".to_string();
        }
        (
            Phase::PlanClaudeFinalizePending,
            CollabEvent::PublishFinal {
                content_hash,
                codex_still_objects: _,
            },
        ) => {
            if actor != "claude" {
                return Err(CollabError::NotYourTurn {
                    expected: "claude".to_string(),
                    got: actor.to_string(),
                });
            }
            next.final_plan_hash = Some(content_hash.clone());
            let codex_still_objects = matches!(
                session.codex_review_verdict.as_deref(),
                Some("request_changes")
            );
            next.phase = if codex_still_objects {
                Phase::PlanEscalated
            } else {
                Phase::PlanLocked
            };
        }
        (Phase::PlanParallelDrafts, _) => {
            return Err(CollabError::WrongPhase {
                expected: "SubmitDraft".to_string(),
                got: event_name(event).to_string(),
            });
        }
        (Phase::PlanSynthesisPending, _) => {
            return Err(CollabError::WrongPhase {
                expected: "PublishCanonical".to_string(),
                got: event_name(event).to_string(),
            });
        }
        (Phase::PlanCodexReviewPending, _) => {
            return Err(CollabError::WrongPhase {
                expected: "SubmitReview".to_string(),
                got: event_name(event).to_string(),
            });
        }
        (Phase::PlanClaudeFinalizePending, _) => {
            return Err(CollabError::WrongPhase {
                expected: "PublishFinal".to_string(),
                got: event_name(event).to_string(),
            });
        }
        (Phase::PlanLocked | Phase::PlanEscalated, _) => return Err(CollabError::SessionLocked),
    }

    Ok(next)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session() -> CollabSession {
        CollabSession::new("test-session")
    }

    #[test]
    fn test_parallel_drafts_both_submit_advances_phase() {
        let s = session();
        let s = apply_event(
            &s,
            "claude",
            &CollabEvent::SubmitDraft {
                content_hash: "c1".to_string(),
            },
        )
        .unwrap();
        assert_eq!(s.phase, Phase::PlanParallelDrafts);
        let s = apply_event(
            &s,
            "codex",
            &CollabEvent::SubmitDraft {
                content_hash: "c2".to_string(),
            },
        )
        .unwrap();
        assert_eq!(s.phase, Phase::PlanSynthesisPending);
        assert_eq!(s.current_owner, "claude");
    }

    #[test]
    fn test_duplicate_draft_rejected() {
        let s = session();
        let s = apply_event(
            &s,
            "claude",
            &CollabEvent::SubmitDraft {
                content_hash: "c1".to_string(),
            },
        )
        .unwrap();
        let err = apply_event(
            &s,
            "claude",
            &CollabEvent::SubmitDraft {
                content_hash: "c2".to_string(),
            },
        )
        .unwrap_err();
        assert_eq!(
            err,
            CollabError::AlreadySubmittedDraft {
                agent: "claude".to_string()
            }
        );
    }

    #[test]
    fn test_non_claude_cannot_publish_canonical() {
        let s = session();
        let s = apply_event(
            &s,
            "claude",
            &CollabEvent::SubmitDraft {
                content_hash: "c1".to_string(),
            },
        )
        .unwrap();
        let s = apply_event(
            &s,
            "codex",
            &CollabEvent::SubmitDraft {
                content_hash: "c2".to_string(),
            },
        )
        .unwrap();
        let err = apply_event(
            &s,
            "codex",
            &CollabEvent::PublishCanonical {
                content_hash: "merged".to_string(),
            },
        )
        .unwrap_err();
        assert_eq!(
            err,
            CollabError::NotYourTurn {
                expected: "claude".to_string(),
                got: "codex".to_string()
            }
        );
    }

    #[test]
    fn test_codex_review_all_three_verdicts() {
        for verdict in ["approve", "approve_with_minor_edits", "request_changes"] {
            let s = session();
            let s = apply_event(
                &s,
                "claude",
                &CollabEvent::SubmitDraft {
                    content_hash: "c1".to_string(),
                },
            )
            .unwrap();
            let s = apply_event(
                &s,
                "codex",
                &CollabEvent::SubmitDraft {
                    content_hash: "c2".to_string(),
                },
            )
            .unwrap();
            let s = apply_event(
                &s,
                "claude",
                &CollabEvent::PublishCanonical {
                    content_hash: "canonical".to_string(),
                },
            )
            .unwrap();
            let s = apply_event(
                &s,
                "codex",
                &CollabEvent::SubmitReview {
                    verdict: verdict.to_string(),
                },
            )
            .unwrap();
            assert_eq!(s.phase, Phase::PlanClaudeFinalizePending);
            assert_eq!(s.codex_review_verdict.as_deref(), Some(verdict));
        }
    }

    #[test]
    fn test_finalize_locks_after_approve() {
        let s = session();
        let s = apply_event(
            &s,
            "claude",
            &CollabEvent::SubmitDraft {
                content_hash: "c1".to_string(),
            },
        )
        .unwrap();
        let s = apply_event(
            &s,
            "codex",
            &CollabEvent::SubmitDraft {
                content_hash: "c2".to_string(),
            },
        )
        .unwrap();
        let s = apply_event(
            &s,
            "claude",
            &CollabEvent::PublishCanonical {
                content_hash: "canonical".to_string(),
            },
        )
        .unwrap();
        let s = apply_event(
            &s,
            "codex",
            &CollabEvent::SubmitReview {
                verdict: "approve".to_string(),
            },
        )
        .unwrap();
        let s = apply_event(
            &s,
            "claude",
            &CollabEvent::PublishFinal {
                content_hash: "final".to_string(),
                codex_still_objects: false,
            },
        )
        .unwrap();
        assert_eq!(s.phase, Phase::PlanLocked);
        assert_eq!(s.final_plan_hash.as_deref(), Some("final"));
    }

    #[test]
    fn test_finalize_escalates_when_codex_still_objects() {
        let s = session();
        let s = apply_event(
            &s,
            "claude",
            &CollabEvent::SubmitDraft {
                content_hash: "c1".to_string(),
            },
        )
        .unwrap();
        let s = apply_event(
            &s,
            "codex",
            &CollabEvent::SubmitDraft {
                content_hash: "c2".to_string(),
            },
        )
        .unwrap();
        let s = apply_event(
            &s,
            "claude",
            &CollabEvent::PublishCanonical {
                content_hash: "canonical".to_string(),
            },
        )
        .unwrap();
        let s = apply_event(
            &s,
            "codex",
            &CollabEvent::SubmitReview {
                verdict: "request_changes".to_string(),
            },
        )
        .unwrap();
        let s = apply_event(
            &s,
            "claude",
            &CollabEvent::PublishFinal {
                content_hash: "final".to_string(),
                codex_still_objects: true,
            },
        )
        .unwrap();
        assert_eq!(s.phase, Phase::PlanEscalated);
    }

    #[test]
    fn test_wrong_phase_returns_error() {
        let s = session();
        let err = apply_event(
            &s,
            "claude",
            &CollabEvent::PublishCanonical {
                content_hash: "canonical".to_string(),
            },
        )
        .unwrap_err();
        assert_eq!(
            err,
            CollabError::WrongPhase {
                expected: "SubmitDraft".to_string(),
                got: "PublishCanonical".to_string()
            }
        );
    }
}
