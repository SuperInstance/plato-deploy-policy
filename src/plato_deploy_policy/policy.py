"""Deploy policy with P0/P1/P2 deadband classification."""

from dataclasses import dataclass
from enum import Enum

class DeployDecision(Enum):
    DEPLOY = "deploy"
    HOLD = "hold"
    REJECT = "reject"
    DEFER = "defer"

@dataclass
class DeployResult:
    decision: DeployDecision
    priority: str
    reason: str
    score: float

class DeployPolicy:
    def __init__(self, p0_threshold: float = 0.9, p1_threshold: float = 0.7):
        self.p0_threshold = p0_threshold
        self.p1_threshold = p1_threshold

    def classify(self, confidence: float, content: str = "", tags: list[str] = None) -> DeployResult:
        tags = tags or []
        content_lower = content.lower()

        # P0: critical, must deploy immediately
        if confidence >= self.p0_threshold:
            return DeployResult(DeployDecision.DEPLOY, "P0", "high confidence critical", confidence)
        if any(t in tags for t in ["critical", "security", "p0"]):
            return DeployResult(DeployDecision.DEPLOY, "P0", "critical tag", confidence)
        if any(k in content_lower for k in ["security", "critical", "emergency", "outage"]):
            return DeployResult(DeployDecision.DEPLOY, "P0", "critical keyword", confidence)

        # P1: important, deploy within channel
        if confidence >= self.p1_threshold:
            return DeployResult(DeployDecision.DEPLOY, "P1", "above threshold", confidence)
        if any(t in tags for t in ["important", "p1", "high"]):
            return DeployResult(DeployDecision.HOLD, "P1", "important but below threshold", confidence)

        # P2: optimize
        if confidence >= 0.3:
            return DeployResult(DeployDecision.DEFER, "P2", "below optimization threshold", confidence)
        if len(content) < 10:
            return DeployResult(DeployDecision.REJECT, "P2", "content too short", confidence)
        return DeployResult(DeployDecision.DEFER, "P2", "low confidence, defer", confidence)

    def classify_batch(self, items: list[dict]) -> list[DeployResult]:
        return [self.classify(i.get("confidence", 0.5), i.get("content", ""), i.get("tags", []))
                for i in items]

    def summary(self, results: list[DeployResult]) -> dict:
        counts = {"P0": 0, "P1": 0, "P2": 0}
        decisions = {}
        for r in results:
            counts[r.priority] = counts.get(r.priority, 0) + 1
            decisions[r.decision.value] = decisions.get(r.decision.value, 0) + 1
        return {"by_priority": counts, "by_decision": decisions, "total": len(results)}
