use bm_a2a::A2aBridge;

pub fn bridge(bridge_id: &str) -> A2aBridge {
    A2aBridge::new(bridge_id)
}
