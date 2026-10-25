//! Reads one short component-power sample, only after the root policy permits it.
pub(super) fn args(_pid: &str) -> Vec<&str> {
    vec!["-n", "1", "-i", "100", "--samplers", "cpu_power"]
}
