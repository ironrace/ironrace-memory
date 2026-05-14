use provbench_baseline::budget::{BatchDecision, CostMeter};
use provbench_baseline::client::Usage;

// Calibrated per-batch cost: 383_333 uncached input tokens × $3/M ≈ $1.15.
//
// The test narrative wants "19 Proceed-checks then a 20th Abort-check" with a
// $1.20 estimated next-batch against a $25 cap (cap_95 = $23.75). For that to
// hold, per-batch cost `c` must satisfy `19c + 1.20 ≤ 23.75` AND
// `20c + 1.20 > 23.75`, i.e. `c ∈ (1.1275, 1.1868]`. $1.15/record sits inside
// that window, so the loop's 19 Proceed assertions and the trailing Abort
// assertion are both reachable from the SPEC §6.2 price snapshot.
const PER_BATCH_INPUT_TOKENS: u32 = 383_333;

#[test]
fn live_meter_aborts_at_95_percent_of_cap() {
    let mut meter = CostMeter::new(25.0);
    for _ in 0..19 {
        meter
            .record(&Usage {
                input_tokens: PER_BATCH_INPUT_TOKENS,
                cache_creation_input_tokens: 0,
                cache_read_input_tokens: 0,
                output_tokens: 0,
            })
            .expect("record under spec ceiling");
        assert!(matches!(
            meter.before_next_batch(1.20),
            BatchDecision::Proceed
        ));
    }
    meter
        .record(&Usage {
            input_tokens: PER_BATCH_INPUT_TOKENS,
            cache_creation_input_tokens: 0,
            cache_read_input_tokens: 0,
            output_tokens: 0,
        })
        .expect("record under spec ceiling");
    assert!(matches!(
        meter.before_next_batch(1.20),
        BatchDecision::Abort { .. }
    ));
}
