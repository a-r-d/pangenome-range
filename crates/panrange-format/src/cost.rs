use crate::TraceSummary;

/// Parameters for a deliberately simple remote-read cost model.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NetworkProfile {
    pub name: &'static str,
    pub rtt_ms: f64,
    pub bandwidth_mbps: f64,
    pub max_parallel_requests: u32,
    pub per_request_overhead_ms: f64,
}

impl NetworkProfile {
    pub const LOCAL_SSD: Self = Self {
        name: "Local SSD",
        rtt_ms: 0.0,
        bandwidth_mbps: 8_000.0,
        max_parallel_requests: 1,
        per_request_overhead_ms: 0.02,
    };

    pub const GOOD_CDN: Self = Self {
        name: "Good CDN / broadband",
        rtt_ms: 20.0,
        bandwidth_mbps: 300.0,
        max_parallel_requests: 6,
        per_request_overhead_ms: 0.5,
    };

    pub const MODERATE_INTERNET: Self = Self {
        name: "Moderate internet",
        rtt_ms: 50.0,
        bandwidth_mbps: 100.0,
        max_parallel_requests: 6,
        per_request_overhead_ms: 1.0,
    };

    pub const POOR_MOBILE: Self = Self {
        name: "Poor / mobile",
        rtt_ms: 100.0,
        bandwidth_mbps: 30.0,
        max_parallel_requests: 4,
        per_request_overhead_ms: 2.0,
    };

    pub const STANDARD: [Self; 4] = [
        Self::LOCAL_SSD,
        Self::GOOD_CDN,
        Self::MODERATE_INTERNET,
        Self::POOR_MOBILE,
    ];

    /// Estimates idealized latency from request waves plus payload transfer time.
    ///
    /// This assumes all reads are known up front and distributed perfectly over
    /// the parallel slots. It is not a transport or browser benchmark.
    #[must_use]
    #[allow(clippy::cast_precision_loss)] // Millisecond simulation intentionally uses f64.
    pub fn estimate(&self, trace: &TraceSummary) -> NetworkCost {
        let parallel = u64::from(self.max_parallel_requests.max(1));
        let rounds = trace.read_operations.div_ceil(parallel);
        let request_latency_ms = rounds as f64 * (self.rtt_ms + self.per_request_overhead_ms);
        let transfer_ms = if self.bandwidth_mbps > 0.0 {
            trace.total_bytes_requested as f64 * 8.0 / (self.bandwidth_mbps * 1_000.0)
        } else {
            f64::INFINITY
        };
        NetworkCost {
            request_rounds: rounds,
            request_latency_ms,
            transfer_ms,
            estimated_total_ms: request_latency_ms + transfer_ms,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NetworkCost {
    pub request_rounds: u64,
    pub request_latency_ms: f64,
    pub transfer_ms: f64,
    pub estimated_total_ms: f64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn estimate_accounts_for_parallel_request_waves() {
        let trace = TraceSummary {
            read_operations: 7,
            total_bytes_requested: 1_000_000,
            ..TraceSummary::default()
        };
        let profile = NetworkProfile {
            name: "test",
            rtt_ms: 10.0,
            bandwidth_mbps: 100.0,
            max_parallel_requests: 3,
            per_request_overhead_ms: 1.0,
        };
        let cost = profile.estimate(&trace);
        assert_eq!(cost.request_rounds, 3);
        assert!((cost.request_latency_ms - 33.0).abs() < f64::EPSILON);
        assert!((cost.transfer_ms - 80.0).abs() < f64::EPSILON);
        assert!((cost.estimated_total_ms - 113.0).abs() < f64::EPSILON);
    }
}
