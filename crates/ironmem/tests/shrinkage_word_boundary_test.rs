//! Integration tests for the word-boundary matcher in `shrinkage_rerank`.
//!
//! Cases cover both branches of the `IRONMEM_SHRINKAGE_WORD_BOUNDARY`
//! tunable so the bench's A/B comparison is locked in by tests.

use std::sync::Mutex;

use ironmem::db::{Drawer, ScoredDrawer};
use ironmem::search::rerank::{extract_signals, shrinkage_rerank};

/// Serializes tests that mutate `IRONMEM_SHRINKAGE_WORD_BOUNDARY` because
/// the tunable reads the env var on every call but other tests in the
/// binary may read it concurrently.
static ENV_LOCK: Mutex<()> = Mutex::new(());

fn make(content: &str, score: f32) -> ScoredDrawer {
    ScoredDrawer {
        drawer: Drawer {
            id: "x".into(),
            content: content.into(),
            wing: "w".into(),
            room: "r".into(),
            source_file: "".into(),
            added_by: "".into(),
            filed_at: "".into(),
            date: "".into(),
        },
        score,
    }
}

#[test]
fn boundary_match_exact_form_lifts_correct_drawer() {
    let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    std::env::set_var("IRONMEM_SHRINKAGE_WORD_BOUNDARY", "1");

    // Two drawers with the SAME initial RRF score. Only one mentions
    // "photography" exactly. Boundary mode lifts the relevant drawer.
    let mut cs = vec![
        make("photography accessories for my Sony camera", 0.50),
        make("a photographer once said something else entirely", 0.50),
    ];
    let signals = extract_signals("recommend photography gear");
    shrinkage_rerank(&mut cs, &signals);

    cs.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap());
    assert!(
        cs[0].drawer.content.contains("photography accessories"),
        "exact 'photography' match should rank above 'photographer'"
    );

    std::env::remove_var("IRONMEM_SHRINKAGE_WORD_BOUNDARY");
}

#[test]
fn boundary_match_inflected_forms_still_lift() {
    let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    std::env::set_var("IRONMEM_SHRINKAGE_WORD_BOUNDARY", "1");

    // Verify the suffix-tolerance branch: "suggest" should boost a drawer
    // with "suggested" or "suggesting" (verb-form inflections).
    let mut cs = vec![
        make("she suggested the gitzo tripod last spring", 0.50),
        make("a totally unrelated baking recipe with flour", 0.55),
    ];
    let signals = extract_signals("can you suggest a tripod");
    shrinkage_rerank(&mut cs, &signals);

    cs.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap());
    assert!(
        cs[0].drawer.content.contains("suggested"),
        "inflected-form match should rank the relevant drawer first"
    );

    std::env::remove_var("IRONMEM_SHRINKAGE_WORD_BOUNDARY");
}

#[test]
fn boundary_does_not_inflate_unrelated_substring() {
    let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    std::env::set_var("IRONMEM_SHRINKAGE_WORD_BOUNDARY", "1");

    // The photography-failure pattern: query has "current"; an unrelated
    // drawer says "currently". Under substring matching, "current" hits
    // "currently" and the unrelated drawer scores higher than the gold.
    // Under boundary matching, no hit — relative order preserved.
    let gold = make("Sony A7R IV camera flash and tripod options", 0.50);
    let unrelated = make("we are currently working out at the gym", 0.50);

    let mut cs = vec![unrelated.clone(), gold.clone()];
    let signals = extract_signals("complement my current photography setup");
    shrinkage_rerank(&mut cs, &signals);

    let unrelated_score_after = cs
        .iter()
        .find(|c| c.drawer.content.starts_with("we are currently"))
        .map(|c| c.score)
        .unwrap();
    let gold_score_after = cs
        .iter()
        .find(|c| c.drawer.content.starts_with("Sony A7R IV"))
        .map(|c| c.score)
        .unwrap();

    // 'photography' matches 'photography' word-boundary in gold via the
    // suffix-tolerance pattern (gold has 'photography'). Unrelated has
    // 'currently' but no 'current' word-boundary hit. Gold should not
    // be below unrelated under boundary mode.
    assert!(
        gold_score_after >= unrelated_score_after,
        "gold ({gold_score_after}) must not score below unrelated ({unrelated_score_after}) under boundary mode"
    );

    std::env::remove_var("IRONMEM_SHRINKAGE_WORD_BOUNDARY");
}

#[test]
fn legacy_substring_still_inflates_when_disabled() {
    let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    std::env::set_var("IRONMEM_SHRINKAGE_WORD_BOUNDARY", "0");

    // Same setup as above. Under legacy substring, "current" SHOULD hit
    // "currently" and inflate the unrelated drawer above gold. This proves
    // the tunable wires both paths — bench can A/B cleanly.
    let mut cs = vec![
        make("we are currently working out at the gym", 0.50),
        make("Sony A7R IV camera and tripod options", 0.50),
    ];
    let signals = extract_signals("complement my current photography setup");
    shrinkage_rerank(&mut cs, &signals);

    let unrelated_score = cs
        .iter()
        .find(|c| c.drawer.content.starts_with("we are currently"))
        .map(|c| c.score)
        .unwrap();
    let gold_score = cs
        .iter()
        .find(|c| c.drawer.content.starts_with("Sony A7R IV"))
        .map(|c| c.score)
        .unwrap();

    assert!(
        unrelated_score > gold_score,
        "legacy substring path must inflate 'currently' as a 'current' hit ({unrelated_score} vs {gold_score})"
    );

    std::env::remove_var("IRONMEM_SHRINKAGE_WORD_BOUNDARY");
}
