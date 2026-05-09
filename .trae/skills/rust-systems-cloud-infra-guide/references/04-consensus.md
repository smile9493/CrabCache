---
title: "Distributed Consensus & State Machine Consistency"
description: "Deterministic Raft/Paxos state machines with zero-cost serialization"
category: "Infrastructure"
priority: "P0-P1"
applies_to: ["strict"]
prerequisites: ["rust-architecture-guide/references/12-async-internals.md"]
dependents: []
---

# Skill: Distributed Consensus & State Machine Consistency

## 👤 Profile

* **Domain**: Raft/Paxos implementations, distributed databases, coordination services.
* **Environment**: Multi-node cluster, network partition possible.
* **Philosophy**:
    * **Determinism is Consensus Lifeline**: Same input log sequence → Same output state.
    * **Zero-Cost Serialization**: Eliminate serialization CPU bottleneck in high-frequency RPC.

---

## ⚔️ Core Directives

### Action 1: Absolute State Machine Determinism
* **Scenario**: Implement Raft or Paxos Apply logic.
* **Red Line**: State machine **absolutely prohibits** any non-determinism.
* **Prohibited List**:
    * `Instant::now()` / `SystemTime::now()`
    * `rand::random()`
    * `HashMap` / `HashSet` iteration order
    * Local disk inode order
    * Thread scheduling order
* **Alternative**: Leader-proposed timestamp, `BTreeMap` / `IndexMap`.

### Action 2: State Fingerprint Verification
* **Scenario**: Verify state consistency after replaying same logs on different nodes.
* **Execution**: Compute SHA-256 fingerprint of final state; must be identical across nodes.

### Action 3: Zero-Cost Serialization Choice
* **Scenario**: High-frequency inter-node RPC communication.
* **Performance Comparison**:
    | Library | Zero-Copy | Latency | Use Case |
    |---------|-----------|---------|----------|
    | serde_json | ❌ | High | Debug/config |
    | bincode | ❌ | Medium | General RPC |
    | rkyv | ✅ | Very Low | High-freq internal |
    | FlatBuffers | ✅ | Very Low | Cross-language |

---

## 💻 Code Paradigms

### Paradigm A: Deterministic State Machine

```rust
use indexmap::IndexMap;
use std::collections::BTreeMap;

struct DeterministicStateMachine {
    data: BTreeMap<String, Vec<u8>>,
    ordered_keys: IndexMap<String, u64>,
    applied_index: u64,
}

impl DeterministicStateMachine {
    fn apply(&mut self, entry: LogEntry) -> Result<Vec<u8>, Error> {
        // ❌ Prohibited: Instant::now()
        // ❌ Prohibited: rand::random()
        
        match entry.command {
            Command::Set { key, value, timestamp } => {
                // timestamp from Leader proposal
                self.data.insert(key.clone(), value.clone());
                self.ordered_keys.insert(key, timestamp);
                self.applied_index = entry.index;
                Ok(value)
            }
            Command::Delete { key } => {
                let old = self.data.remove(&key);
                self.ordered_keys.remove(&key);
                self.applied_index = entry.index;
                Ok(old.unwrap_or_default())
            }
        }
    }
}
```

### Paradigm B: State Fingerprint Verification

```rust
use sha2::{Sha256, Digest};

impl DeterministicStateMachine {
    fn fingerprint(&self) -> [u8; 32] {
        let mut hasher = Sha256::new();
        
        // BTreeMap guarantees deterministic iteration order
        for (k, v) in &self.data {
            hasher.update(k.as_bytes());
            hasher.update(v);
        }
        
        hasher.update(&self.applied_index.to_le_bytes());
        
        let mut result = [0u8; 32];
        result.copy_from_slice(&hasher.finalize());
        result
    }
}

#[test]
fn test_determinism() {
    let entries = vec![/* same log sequence */];
    
    let mut sm1 = DeterministicStateMachine::new();
    let mut sm2 = DeterministicStateMachine::new();
    
    for entry in &entries {
        sm1.apply(entry.clone()).unwrap();
        sm2.apply(entry.clone()).unwrap();
    }
    
    assert_eq!(sm1.fingerprint(), sm2.fingerprint());
}
```

### Paradigm C: rkyv Zero-Copy Serialization

```rust
use rkyv::{Archive, Serialize, Deserialize};

#[derive(Archive, Serialize, Deserialize)]
struct RaftMessage {
    term: u64,
    leader_id: u64,
    prev_log_index: u64,
    prev_log_term: u64,
    entries: Vec<LogEntry>,
    leader_commit: u64,
}

fn serialize_zero_copy(msg: &RaftMessage) -> Vec<u8> {
    rkyv::to_bytes::<_, 1024>(msg).unwrap().to_vec()
}

fn deserialize_zero_copy(bytes: &[u8]) -> &ArchivedRaftMessage {
    rkyv::check_archived_root::<RaftMessage>(bytes).unwrap()
}
```

---

## 📊 Raft Apply Flow

```
┌─────────────────────────────────────────┐
│           Leader                         │
│  1. Receive client request              │
│  2. Assign index + term                 │
│  3. Propose timestamp (determinism src) │
│  4. Replicate to Followers              │
└─────────────────────────────────────────┘
                 │
                 ▼
┌─────────────────────────────────────────┐
│           All Nodes                      │
│  1. Receive committed log entries       │
│  2. Apply to state machine in order     │
│  3. Guarantee: same in → same out       │
│  4. Update last_applied                 │
└─────────────────────────────────────────┘
```
