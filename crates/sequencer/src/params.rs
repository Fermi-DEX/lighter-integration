//! Deployment parameters (whitepaper Table 1) and the no-look inequality.
//!
//! Deployment rule 1: every T_k is derived from Equation (1), in work units,
//! from the measured rate — never in milliseconds. `assert_no_look` is a
//! startup invariant: the node refuses to serve a delay menu that violates
//! it.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PosqParams {
    /// Epoch identifier (bumped on operator replacement / re-genesis).
    pub epoch: u64,
    /// Target tick duration in microseconds (∆; reference 100).
    pub delta_micros: u64,
    /// Per-tick sequential work in squarings (q; measured, ≥90% of frontier).
    pub q_squarings: u64,
    /// Segment length in ticks (F; reference 256).
    pub segment_ticks: u64,
    /// Default inclusion-window length in ticks (W; reference 50).
    pub window_ticks: u16,
    /// Publication slack in ticks (Γ; reference 3).
    pub gamma_ticks: u64,
    /// Observation/verification margins plus implementation buffer, in
    /// microseconds (ξ + µ; reference 3000).
    pub xi_mu_micros: u64,
    /// Adversary squaring-rate advantage bound α, as a rational (num/den);
    /// reference 2/1 at launch.
    pub alpha_num: u64,
    pub alpha_den: u64,
    /// Delay menu, in squarings (T_k; reference 10^6, 2·10^6, 10^7).
    pub delay_classes: Vec<u64>,
    /// Unlock grace G, in segments (reference 1).
    pub grace_segments: u64,
    /// Fixed envelope size S in bytes (reference 1024).
    pub envelope_bytes: usize,
    /// Per-tick, per-bucket admission capacity (public deterministic
    /// capacity rule backing full-window responses).
    pub capacity_per_tick_per_bucket: u32,
    /// Anchor cadence, in segments.
    pub anchor_every_segments: u64,
}

impl PosqParams {
    /// Reference profile (Table 1). `q` must still be calibrated to the
    /// operator's hardware; the value here assumes FPGA-class 25 ns/sq.
    pub fn reference() -> Self {
        Self {
            epoch: 0,
            delta_micros: 100,
            q_squarings: 3_600,
            segment_ticks: 256,
            window_ticks: 50,
            gamma_ticks: 3,
            xi_mu_micros: 3_000,
            alpha_num: 2,
            alpha_den: 1,
            delay_classes: vec![1_000_000, 2_000_000, 10_000_000],
            grace_segments: 1,
            envelope_bytes: 1024,
            capacity_per_tick_per_bucket: 64,
            anchor_every_segments: 8,
        }
    }

    /// A small-parameter profile for tests and local development: q sized
    /// so a tick takes single-digit milliseconds on a laptop, delays derived
    /// just above the no-look bound so waves mature within a short run. The
    /// inequality (1) invariant holds by construction.
    pub fn dev(q: u64, segment_ticks: u64) -> Self {
        let window: u16 = 12;
        let gamma = 1u64;
        // min T = α·q·(W+Γ) with α=2 and no extra margins; add q of headroom.
        let t1 = 2 * q * (window as u64 + gamma) + q;
        Self {
            epoch: 0,
            delta_micros: 2_000,
            q_squarings: q,
            segment_ticks,
            window_ticks: window,
            gamma_ticks: gamma,
            xi_mu_micros: 0,
            alpha_num: 2,
            alpha_den: 1,
            delay_classes: vec![t1, 2 * t1],
            // The public solver runs at ~the clock rate, so opening T_1 takes
            // ~T_1/q ticks of clock time plus the proof pass; grace must
            // cover it (the reference profile's G=1 works because production
            // solvers are parallel rigs at frontier rate, F=256, and
            // T_1/q ≈ 278 ticks ≈ 1.1 segments of slack behind maturity).
            grace_segments: 12,
            envelope_bytes: 1024,
            capacity_per_tick_per_bucket: 16,
            anchor_every_segments: 2,
        }
    }

    /// Mandated public rate R_req in squarings per second, implied by (q, ∆).
    pub fn mandated_rate(&self) -> f64 {
        self.q_squarings as f64 / (self.delta_micros as f64 / 1e6)
    }

    /// Delay class k expressed in ticks at the reference rate: D_k = ⌈T_k/q⌉.
    pub fn delay_ticks(&self, class: u8) -> Option<u64> {
        self.delay_classes
            .get(class as usize)
            .map(|t| t.div_ceil(self.q_squarings))
    }

    /// Grace G in ticks.
    pub fn grace_ticks(&self) -> u64 {
        self.grace_segments * self.segment_ticks
    }

    /// Equation (1): T_k > α · R_req · ((W + Γ)·∆ + ξ + µ).
    /// Returns the minimum admissible T for the given window length.
    pub fn min_delay_squarings(&self, window_ticks: u16) -> u64 {
        let exposure_micros =
            (window_ticks as u64 + self.gamma_ticks) * self.delta_micros + self.xi_mu_micros;
        // α · R_req · exposure = α · q/∆ · exposure  (all in µs)
        // = α · q · exposure / ∆
        (self.alpha_num * self.q_squarings * exposure_micros)
            .div_ceil(self.alpha_den * self.delta_micros)
    }

    /// Startup invariant: every delay class satisfies Equation (1) for the
    /// default window. Returns the violating class if any.
    pub fn check_no_look(&self) -> Result<(), (u8, u64, u64)> {
        let min_t = self.min_delay_squarings(self.window_ticks);
        for (k, &t) in self.delay_classes.iter().enumerate() {
            if t <= min_t {
                return Err((k as u8, t, min_t));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reference_profile_satisfies_no_look() {
        // Worked example, Appendix C: T1 = 10^6 vs required ≈ 6.0·10^5.
        let p = PosqParams::reference();
        let min_t = p.min_delay_squarings(p.window_ticks);
        assert!(min_t > 590_000 && min_t < 610_000, "min_t = {min_t}");
        assert!(p.check_no_look().is_ok());
    }

    #[test]
    fn violating_menu_rejected() {
        let mut p = PosqParams::reference();
        p.delay_classes = vec![100_000];
        assert!(p.check_no_look().is_err());
    }

    #[test]
    fn dev_profile_satisfies_no_look() {
        let p = PosqParams::dev(8, 16);
        assert!(p.check_no_look().is_ok());
    }

    #[test]
    fn delay_ticks_ceil() {
        let p = PosqParams::dev(8, 16);
        assert_eq!(p.delay_ticks(0).unwrap(), p.delay_classes[0].div_ceil(8));
    }
}
