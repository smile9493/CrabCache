import json
from collections import defaultdict

def analyze(file_path):
    print(f"Analyzing: {file_path}")
    
    mimo_records = []
    with open(file_path, 'r') as f:
        for line in f:
            line = line.strip()
            if not line:
                continue
            try:
                record = json.loads(line)
                model = record.get("model", "")
                # Only analyze mimo models
                if "mimo" in model.lower():
                    mimo_records.append(record)
            except Exception as e:
                continue
                
    total_mimo = len(mimo_records)
    print(f"Total MIMO model records found: {total_mimo}")
    if total_mimo == 0:
        return
        
    # 1. Inspect unique API keys and session combinations
    # Group by key fingerprint
    key_groups = defaultdict(list)
    for r in mimo_records:
        key = r.get("client_key_fingerprint") or "unknown_key"
        key_groups[key].append(r)
        
    print("\n=== Key Fingerprints ===")
    for key, records in key_groups.items():
        # Get unique sessions for this key
        sessions = set()
        convs = set()
        for r in records:
            sfp = r.get("session_fingerprint")
            cid = r.get("conversation_id")
            if sfp:
                sessions.add(sfp)
            if cid:
                convs.add(cid)
        print(f"Key: {key}")
        print(f"  Total Requests: {len(records)}")
        print(f"  Unique Session Fingerprints (sfp): {len(sessions)} {list(sessions)[:3]}")
        print(f"  Unique Conversation IDs (conv): {len(convs)} {list(convs)[:3]}")

    # 2. Distinguishing same-key different sessions
    # Group by (key, session_fingerprint)
    sess_groups = defaultdict(list)
    for r in mimo_records:
        key = r.get("client_key_fingerprint") or "unknown_key"
        sfp = r.get("session_fingerprint") or "unknown_sfp"
        sess_groups[(key, sfp)].append(r)
        
    print("\n=== Top Sessions by Request Count ===")
    sorted_sess = sorted(sess_groups.items(), key=lambda x: len(x[1]), reverse=True)
    for (key, sfp), records in sorted_sess[:5]:
        print(f"Key: {key} | Session FP: {sfp} | Requests: {len(records)}")

    # 3. Analyze Parallelism / Concurrency
    # We can detect overlaps in [start_time, end_time]
    # For each key, we sort requests by start_time and count active requests at any moment
    print("\n=== Concurrency Analysis (Overlapping Requests) ===")
    for key, records in key_groups.items():
        # Events: list of (time, type, request_id, session_fingerprint)
        # type: +1 for start, -1 for end
        events = []
        for r in records:
            start = r.get("timestamp_ms")
            dur = r.get("duration_ms") or 0
            end = start + dur
            req_id = r.get("request_id", "unknown")
            sfp = r.get("session_fingerprint", "unknown")
            events.append((start, 1, req_id, sfp))
            events.append((end, -1, req_id, sfp))
            
        # Sort events: first by time. If times are equal, end events first (-1 before 1) to be conservative
        events.sort(key=lambda x: (x[0], x[1]))
        
        current_concurrent = 0
        max_concurrent = 0
        active_requests = {} # req_id -> sfp
        
        overlap_details = []
        
        for time_ms, ev_type, req_id, sfp in events:
            if ev_type == 1:
                current_concurrent += 1
                active_requests[req_id] = sfp
                if current_concurrent > max_concurrent:
                    max_concurrent = current_concurrent
                if current_concurrent > 1:
                    # Capture snapshot of concurrent requests and their sessions
                    overlap_details.append({
                        "time": time_ms,
                        "concurrency": current_concurrent,
                        "active": list(active_requests.values())
                    })
            else:
                current_concurrent -= 1
                active_requests.pop(req_id, None)
                
        print(f"Key: {key}")
        print(f"  Max Concurrent In-Flight Requests: {max_concurrent}")
        
        # Check if parallel requests belong to the same session or different sessions
        same_sess_overlap = 0
        diff_sess_overlap = 0
        for detail in overlap_details:
            sessions_in_overlap = detail["active"]
            if len(sessions_in_overlap) > 1:
                unique_sess = set(sessions_in_overlap)
                if len(unique_sess) == 1:
                    same_sess_overlap += 1
                else:
                    diff_sess_overlap += 1
                    
        print(f"  Instances of Parallel Requests (>1 concurrent): {len(overlap_details)}")
        print(f"    - Same session parallel: {same_sess_overlap}")
        print(f"    - Different sessions parallel: {diff_sess_overlap}")

    # 4. Request Coalescing (Request Merging)
    print("\n=== Request Coalescing ===")
    coalesced_leaders = sum(1 for r in mimo_records if r.get("coalesce_leader") is True)
    coalesced_followers = sum(1 for r in mimo_records if r.get("coalesced_follower") is True)
    cache_hits = sum(1 for r in mimo_records if r.get("cache_hit") is True)
    print(f"Coalesced Leaders: {coalesced_leaders}")
    print(f"Coalesced Followers (Multiplexed): {coalesced_followers}")
    print(f"Cache Hits: {cache_hits}")

if __name__ == "__main__":
    import sys
    file_path = sys.argv[1] if len(sys.argv) > 1 else "index.jsonl"
    analyze(file_path)
