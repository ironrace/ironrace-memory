use provbench_labeler::ast::spans::Span;
use provbench_labeler::facts::Fact;
use provbench_labeler::label::{classify, Label, MockState};

fn fn_fact() -> Fact {
    Fact::FunctionSignature {
        qualified_name: "f".into(),
        source_path: "a.rs".into(),
        span: Span {
            byte_range: 0..16,
            line_start: 1,
            line_end: 1,
        },
        content_hash: "deadbeef".to_string(),
    }
}

#[test]
fn missing_file_yields_stale_source_deleted() {
    let st = MockState {
        file_exists: false,
        ..MockState::default()
    };
    assert_eq!(classify(&fn_fact(), &st), Label::StaleSourceDeleted);
}

#[test]
fn unresolved_symbol_with_rename_yields_renamed() {
    let st = MockState {
        file_exists: true,
        symbol_resolves: false,
        rename_candidate: Some("g".into()),
        ..MockState::default()
    };
    assert_eq!(
        classify(&fn_fact(), &st),
        Label::StaleSymbolRenamed {
            new_name: "g".into()
        }
    );
}

#[test]
fn matching_hash_yields_valid() {
    let st = MockState {
        file_exists: true,
        symbol_resolves: true,
        post_span_hash: Some("deadbeef".into()),
        ..MockState::default()
    };
    assert_eq!(classify(&fn_fact(), &st), Label::Valid);
}

#[test]
fn whitespace_only_diff_yields_valid() {
    let st = MockState {
        file_exists: true,
        symbol_resolves: true,
        post_span_hash: Some("different".into()),
        whitespace_or_comment_only: true,
        ..MockState::default()
    };
    assert_eq!(classify(&fn_fact(), &st), Label::Valid);
}

#[test]
fn structural_change_yields_stale_source_changed() {
    let st = MockState {
        file_exists: true,
        symbol_resolves: true,
        post_span_hash: Some("different".into()),
        structurally_classifiable: true,
        ..MockState::default()
    };
    assert_eq!(classify(&fn_fact(), &st), Label::StaleSourceChanged);
}

#[test]
fn unclassifiable_change_yields_needs_revalidation() {
    let st = MockState {
        file_exists: true,
        symbol_resolves: true,
        post_span_hash: Some("different".into()),
        ..MockState::default()
    };
    assert_eq!(classify(&fn_fact(), &st), Label::NeedsRevalidation);
}
