use ironmem::search::tunables;

#[test]
fn rerank_enabled_with_cross_encoder() {
    std::env::set_var("IRONMEM_RERANK", "cross_encoder");
    assert!(tunables::rerank_enabled());
}
