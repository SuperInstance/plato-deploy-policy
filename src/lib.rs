//! plato-deploy-policy — Tiered deployment policy
//!
//! Routes changes through three deployment tiers based on belief scores:
//! - Tier 1 (Live): High belief → auto-deploy, A/B test, instant rollback
//! - Tier 2 (Monitored): Medium belief → shadow mode, graduated rollout
//! - Tier 3 (Human-Gated): Low belief → simulation first, human approval
//!
//! Inspired by Oracle1's "Iron Sharpens Iron" tiered trust model.
//! Designed to work with plato-unified-belief scores.

// ── Deployment Tier ─────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Tier {
    /// Auto-deploy, A/B testing, instant rollback. Cost of failure is LOW.
    Live,
    /// Shadow mode, graduated rollout, backtesting. Cost of failure is MEDIUM.
    Monitored,
    /// Simulation first, human approval, hot standby. Cost of failure is HIGH.
    HumanGated,
}

impl Tier {
    pub fn name(&self) -> &'static str {
        match self {
            Tier::Live => "live",
            Tier::Monitored => "monitored",
            Tier::HumanGated => "human-gated",
        }
    }

    pub fn risk_level(&self) -> u8 {
        match self {
            Tier::Live => 1,
            Tier::Monitored => 2,
            Tier::HumanGated => 3,
        }
    }

    /// How long a change must spend at this tier before promotion
    pub fn minimum_observation_ticks(&self) -> u32 {
        match self {
            Tier::Live => 10,
            Tier::Monitored => 50,
            Tier::HumanGated => 100,
        }
    }
}

// ── Deployment Decision ──────────────────────────────────

#[derive(Debug, Clone)]
pub struct DeployDecision {
    pub tier: Tier,
    pub confidence: f32,
    pub trust: f32,
    pub relevance: f32,
    pub reason: String,
    pub requires_human: bool,
    pub rollout_pct: u8, // 0 = not applicable, 5-100 for graduated
}

impl DeployDecision {
    /// Is this decision safe to proceed without human intervention?
    pub fn is_auto(&self) -> bool {
        !self.requires_human
    }

    /// At what percentage should this change be deployed?
    pub fn deployment_percentage(&self) -> u8 {
        match self.tier {
            Tier::Live => 100,
            Tier::Monitored => self.rollout_pct,
            Tier::HumanGated => 0, // No auto-deployment
        }
    }
}

// ── Deployment Policy ────────────────────────────────────

/// Threshold configuration for tier assignment.
/// Belief scores (0.0-1.0) from plato-unified-belief map to tiers.
#[derive(Debug, Clone)]
pub struct DeployPolicy {
    /// Composite belief above this → Tier 1 (Live)
    pub live_threshold: f32,
    /// Composite belief below this → Tier 3 (Human-Gated)
    pub human_threshold: f32,
    /// Between live and human → Tier 2 (Monitored)
    /// Default: live=0.8, human=0.5 → monitored is 0.5-0.8

    /// Graduated rollout starting percentage for monitored tier
    pub monitored_start_pct: u8,
    /// Graduated rollout increment per observation tick
    pub monitored_increment: u8,

    /// Minimum confidence for ANY deployment (floor)
    pub absolute_min_confidence: f32,
    /// Minimum trust for ANY deployment (floor)
    pub absolute_min_trust: f32,
}

impl Default for DeployPolicy {
    fn default() -> Self {
        Self {
            live_threshold: 0.8,
            human_threshold: 0.5,
            monitored_start_pct: 5,
            monitored_increment: 10,
            absolute_min_confidence: 0.3,
            absolute_min_trust: 0.3,
        }
    }
}

impl DeployPolicy {
    /// Create a policy with custom thresholds
    pub fn new(live: f32, human: f32) -> Self {
        Self { live_threshold: live, human_threshold: human, ..Default::default() }
    }

    /// Classify a change into a deployment tier based on belief scores.
    /// Returns DeployDecision with tier, rollout info, and reasoning.
    pub fn classify(&self, confidence: f32, trust: f32, relevance: f32) -> DeployDecision {
        let composite = (confidence * trust * relevance).powf(1.0 / 3.0);

        // Absolute floor check — if any dimension is too low, block
        if confidence < self.absolute_min_confidence {
            return DeployDecision {
                tier: Tier::HumanGated, confidence, trust, relevance,
                reason: format!("confidence {:.2} below floor {:.2}", confidence, self.absolute_min_confidence),
                requires_human: true, rollout_pct: 0,
            };
        }
        if trust < self.absolute_min_trust {
            return DeployDecision {
                tier: Tier::HumanGated, confidence, trust, relevance,
                reason: format!("trust {:.2} below floor {:.2}", trust, self.absolute_min_trust),
                requires_human: true, rollout_pct: 0,
            };
        }

        // Tier classification based on composite belief
        if composite >= self.live_threshold {
            DeployDecision {
                tier: Tier::Live, confidence, trust, relevance,
                reason: format!("composite {:.3} ≥ live threshold {:.1}", composite, self.live_threshold),
                requires_human: false, rollout_pct: 100,
            }
        } else if composite >= self.human_threshold {
            DeployDecision {
                tier: Tier::Monitored, confidence, trust, relevance,
                reason: format!("composite {:.3} in monitored range [{:.1}, {:.1})",
                    composite, self.human_threshold, self.live_threshold),
                requires_human: false,
                rollout_pct: self.monitored_start_pct,
            }
        } else {
            DeployDecision {
                tier: Tier::HumanGated, confidence, trust, relevance,
                reason: format!("composite {:.3} < human threshold {:.1}", composite, self.human_threshold),
                requires_human: true, rollout_pct: 0,
            }
        }
    }

    /// Promote a monitored change: increase rollout percentage.
    /// Returns new percentage (capped at 100).
    /// Returns None if not in monitored tier.
    pub fn promote(&self, current_pct: u8, observation_ticks: u32) -> Option<u8> {
        if observation_ticks < Tier::Monitored.minimum_observation_ticks() {
            return None; // Not enough observation yet
        }
        let new_pct = (current_pct as u32 + self.monitored_increment as u32).min(100) as u8;
        if new_pct >= 100 {
            // Promoted to live
            Some(100)
        } else {
            Some(new_pct)
        }
    }

    /// Demote: reduce rollout percentage after regression detected.
    /// Returns new percentage (floors at monitored_start_pct).
    /// Returns None if at minimum already.
    pub fn demote(&self, current_pct: u8) -> Option<u8> {
        if current_pct <= self.monitored_start_pct {
            None
        } else {
            Some(self.monitored_start_pct) // Reset to start
        }
    }
}

// ── Deployment Record ────────────────────────────────────

/// Track deployment history for a change.
#[derive(Debug, Clone)]
pub struct DeployRecord {
    pub id: u64,
    pub tier: Tier,
    pub rollout_pct: u8,
    pub ticks_observed: u32,
    pub deployed_at: u64, // nanosecond timestamp
    pub promoted_at: Option<u64>,
    pub rolled_back: bool,
}

// ── Deployment Ledger ────────────────────────────────────

/// Persistent deployment tracking with promotion/demotion logic.
pub struct DeployLedger {
    records: std::collections::HashMap<u64, DeployRecord>,
    next_id: u64,
    policy: DeployPolicy,
}

impl DeployLedger {
    pub fn new(policy: DeployPolicy) -> Self {
        Self { records: std::collections::HashMap::new(), next_id: 1, policy }
    }

    /// Submit a new change for deployment classification.
    /// Returns (id, DeployDecision).
    pub fn submit(&mut self, confidence: f32, trust: f32, relevance: f32) -> (u64, DeployDecision) {
        let decision = self.policy.classify(confidence, trust, relevance);
        let id = self.next_id;
        self.next_id += 1;

        let now = nanos_now();
        self.records.insert(id, DeployRecord {
            id, tier: decision.tier, rollout_pct: decision.rollout_pct,
            ticks_observed: 0, deployed_at: now, promoted_at: None, rolled_back: false,
        });

        (id, decision)
    }

    /// Tick observation for a deployment. Returns new rollout % if promoted, None if not ready.
    pub fn tick(&mut self, id: u64) -> Option<u8> {
        let record = self.records.get_mut(&id)?;
        if record.rolled_back { return None; }
        record.ticks_observed += 1;

        if record.tier == Tier::Monitored {
            if let Some(new_pct) = self.policy.promote(record.rollout_pct, record.ticks_observed) {
                if new_pct >= 100 {
                    record.tier = Tier::Live;
                    record.rollout_pct = 100;
                    record.promoted_at = Some(nanos_now());
                } else {
                    record.rollout_pct = new_pct;
                }
                return Some(new_pct);
            }
        }
        None
    }

    /// Rollback a deployment.
    pub fn rollback(&mut self, id: u64) -> bool {
        if let Some(record) = self.records.get_mut(&id) {
            record.rolled_back = true;
            true
        } else {
            false
        }
    }

    /// Get deployment status.
    pub fn status(&self, id: u64) -> Option<&DeployRecord> {
        self.records.get(&id)
    }

    /// Get all deployments in a tier.
    pub fn by_tier(&self, tier: Tier) -> Vec<&DeployRecord> {
        self.records.values().filter(|r| r.tier == tier && !r.rolled_back).collect()
    }

    /// Total active deployments
    pub fn active_count(&self) -> usize {
        self.records.values().filter(|r| !r.rolled_back).count()
    }

    /// Total rolled back
    pub fn rollback_count(&self) -> usize {
        self.records.values().filter(|r| r.rolled_back).count()
    }
}

fn nanos_now() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now().duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0)
}

// ── Tests ────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tier_properties() {
        assert_eq!(Tier::Live.risk_level(), 1);
        assert_eq!(Tier::Monitored.risk_level(), 2);
        assert_eq!(Tier::HumanGated.risk_level(), 3);
        assert!(Tier::Live.minimum_observation_ticks() < Tier::Monitored.minimum_observation_ticks());
    }

    #[test]
    fn test_classify_live() {
        let policy = DeployPolicy::default();
        let d = policy.classify(0.9, 0.9, 0.9);
        assert_eq!(d.tier, Tier::Live);
        assert!(!d.requires_human);
        assert_eq!(d.rollout_pct, 100);
    }

    #[test]
    fn test_classify_monitored() {
        let policy = DeployPolicy::default();
        let d = policy.classify(0.7, 0.7, 0.7);
        assert_eq!(d.tier, Tier::Monitored);
        assert!(!d.requires_human);
        assert_eq!(d.rollout_pct, 5); // monitored_start_pct
    }

    #[test]
    fn test_classify_human_gated() {
        let policy = DeployPolicy::default();
        let d = policy.classify(0.3, 0.4, 0.3);
        assert_eq!(d.tier, Tier::HumanGated);
        assert!(d.requires_human);
        assert_eq!(d.rollout_pct, 0);
    }

    #[test]
    fn test_confidence_floor() {
        let policy = DeployPolicy::default();
        let d = policy.classify(0.2, 0.9, 0.9); // confidence below 0.3 floor
        assert_eq!(d.tier, Tier::HumanGated);
        assert!(d.requires_human);
    }

    #[test]
    fn test_trust_floor() {
        let policy = DeployPolicy::default();
        let d = policy.classify(0.9, 0.2, 0.9); // trust below 0.3 floor
        assert_eq!(d.tier, Tier::HumanGated);
        assert!(d.requires_human);
    }

    #[test]
    fn test_custom_thresholds() {
        let policy = DeployPolicy::new(0.6, 0.3); // easier to get live
        let d = policy.classify(0.7, 0.7, 0.7);
        assert_eq!(d.tier, Tier::Live);
    }

    #[test]
    fn test_decision_auto() {
        let policy = DeployPolicy::default();
        let live = policy.classify(0.9, 0.9, 0.9);
        assert!(live.is_auto());

        let human = policy.classify(0.2, 0.2, 0.2);
        assert!(!human.is_auto());
    }

    #[test]
    fn test_decision_deployment_pct() {
        let policy = DeployPolicy::default();
        let live = policy.classify(0.9, 0.9, 0.9);
        assert_eq!(live.deployment_percentage(), 100);

        let monitored = policy.classify(0.7, 0.7, 0.7);
        assert_eq!(monitored.deployment_percentage(), 5);

        let human = policy.classify(0.2, 0.2, 0.2);
        assert_eq!(human.deployment_percentage(), 0);
    }

    #[test]
    fn test_promote_not_enough_ticks() {
        let policy = DeployPolicy::default();
        let result = policy.promote(5, 10); // Need 50 ticks, only have 10
        assert!(result.is_none());
    }

    #[test]
    fn test_promote_ready() {
        let policy = DeployPolicy::default();
        let result = policy.promote(5, 50); // Exactly at threshold
        assert_eq!(result, Some(15)); // 5 + 10
    }

    #[test]
    fn test_promote_to_live() {
        let policy = DeployPolicy::default();
        let result = policy.promote(95, 100);
        assert_eq!(result, Some(100));
    }

    #[test]
    fn test_demote() {
        let policy = DeployPolicy::default();
        let result = policy.demote(50);
        assert_eq!(result, Some(5)); // Reset to start

        let at_floor = policy.demote(5);
        assert!(at_floor.is_none()); // Already at minimum
    }

    #[test]
    fn test_ledger_submit() {
        let mut ledger = DeployLedger::new(DeployPolicy::default());
        let (id, decision) = ledger.submit(0.9, 0.9, 0.9);
        assert_eq!(id, 1);
        assert_eq!(decision.tier, Tier::Live);
        assert_eq!(ledger.active_count(), 1);
    }

    #[test]
    fn test_ledger_tick_promotion() {
        let mut ledger = DeployLedger::new(DeployPolicy::default());
        let (id, _) = ledger.submit(0.7, 0.7, 0.7); // Monitored

        // Tick 49 times — not enough
        for _ in 0..49 { assert!(ledger.tick(id).is_none()); }

        // Tick 50 — should promote
        let result = ledger.tick(id);
        assert_eq!(result, Some(15)); // 5 + 10
    }

    #[test]
    fn test_ledger_rollback() {
        let mut ledger = DeployLedger::new(DeployPolicy::default());
        let (id, _) = ledger.submit(0.9, 0.9, 0.9);
        assert!(ledger.rollback(id));
        assert_eq!(ledger.rollback_count(), 1);
        assert_eq!(ledger.active_count(), 0);
    }

    #[test]
    fn test_ledger_by_tier() {
        let mut ledger = DeployLedger::new(DeployPolicy::default());
        let (id1, _) = ledger.submit(0.9, 0.9, 0.9);
        let (id2, _) = ledger.submit(0.7, 0.7, 0.7);
        let (id3, _) = ledger.submit(0.2, 0.2, 0.2);

        assert_eq!(ledger.by_tier(Tier::Live).len(), 1);
        assert_eq!(ledger.by_tier(Tier::Monitored).len(), 1);
        assert_eq!(ledger.by_tier(Tier::HumanGated).len(), 1);

        // Rollback one
        ledger.rollback(id1);
        assert_eq!(ledger.by_tier(Tier::Live).len(), 0);
    }

    #[test]
    fn test_ledger_status() {
        let mut ledger = DeployLedger::new(DeployPolicy::default());
        let (id, _) = ledger.submit(0.5, 0.5, 0.5);
        let status = ledger.status(id).unwrap();
        assert_eq!(status.id, id);
        assert_eq!(status.ticks_observed, 0);
        assert!(!status.rolled_back);
    }

    #[test]
    fn test_ledger_multiple_submits() {
        let mut ledger = DeployLedger::new(DeployPolicy::default());
        let (id1, _) = ledger.submit(0.9, 0.9, 0.9);
        let (id2, _) = ledger.submit(0.7, 0.7, 0.7);
        assert_ne!(id1, id2);
        assert_eq!(ledger.active_count(), 2);
    }

    #[test]
    fn test_nanosecond_ids() {
        let mut ledger = DeployLedger::new(DeployPolicy::default());
        let (_, d1) = ledger.submit(0.9, 0.9, 0.9);
        let (_, d2) = ledger.submit(0.9, 0.9, 0.9);
        assert!(d1.confidence > 0.0);
        assert!(d2.confidence > 0.0);
    }

    #[test]
    fn test_edge_case_equal_thresholds() {
        // Exactly at threshold should classify as the higher tier
        let policy = DeployPolicy::default();
        let d = policy.classify(0.8, 0.8, 0.8); // composite = 0.8 = live_threshold
        assert_eq!(d.tier, Tier::Live);

        let d2 = policy.classify(0.5, 0.5, 0.5); // composite = 0.5 = human_threshold
        assert_eq!(d2.tier, Tier::Monitored); // >= human_threshold
    }
}
