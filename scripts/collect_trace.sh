#!/bin/bash
# Trace Collector Runner
# Usage: ./collect_trace.sh [count] [output_file]

set -e

COUNT=${1:-100}
OUTPUT=${2:-trace.jsonl}

if [ -z "$DEEPSEEK_API_KEY" ]; then
    echo "Error: DEEPSEEK_API_KEY environment variable not set"
    echo "Please run: export DEEPSEEK_API_KEY=your-api-key"
    exit 1
fi

echo "=== Trace Collection ==="
echo "Requests: $COUNT"
echo "Output: $OUTPUT"
echo ""

# Create test prompts file
PROMPTS_FILE=$(mktemp)
cat > "$PROMPTS_FILE" << 'EOF'
What is the capital of France?
Explain quantum computing in simple terms.
Write a hello world program in Rust.
What are the benefits of caching?
How does a hash table work?
What is machine learning?
Explain the difference between TCP and UDP.
What is a microservice architecture?
How do databases handle concurrent access?
What is the purpose of an API gateway?
EOF

echo "Collecting traces..."
echo ""

# Run the collector (simplified version using curl)
for i in $(seq 1 "$COUNT"); do
    PROMPT=$(sed -n "$(( (i - 1) % 10 + 1 ))p" "$PROMPTS_FILE")
    
    START_TIME=$(date +%s%3N)
    
    RESPONSE=$(curl -s -w "\n%{http_code}" "https://api.deepseek.com/v1/chat/completions" \
        -H "Authorization: Bearer $DEEPSEEK_API_KEY" \
        -H "Content-Type: application/json" \
        -d "{
            \"model\": \"deepseek-chat\",
            \"messages\": [{\"role\": \"user\", \"content\": \"$PROMPT\"}],
            \"max_tokens\": 100,
            \"temperature\": 0.7
        }")
    
    END_TIME=$(date +%s%3N)
    LATENCY=$((END_TIME - START_TIME))
    
    HTTP_CODE=$(echo "$RESPONSE" | tail -n1)
    BODY=$(echo "$RESPONSE" | head -n -1)
    
    if [ "$HTTP_CODE" = "200" ]; then
        # Extract token counts from response
        PROMPT_TOKENS=$(echo "$BODY" | jq -r '.usage.prompt_tokens // 0')
        COMPLETION_TOKENS=$(echo "$BODY" | jq -r '.usage.completion_tokens // 0')
        TOTAL_TOKENS=$(echo "$BODY" | jq -r '.usage.total_tokens // 0')
        REQUEST_ID=$(echo "$BODY" | jq -r '.id // "unknown"')
        
        # Create sanitized log entry (no raw content stored)
        TIMESTAMP=$(date +%s%3N)
        HASH=$(echo -n "$PROMPT" | sha256sum | head -c 16)
        
        echo "{\"timestamp_ms\":$TIMESTAMP,\"request_hash\":\"$HASH\",\"content_length\":${#PROMPT},\"model\":\"deepseek-chat\",\"prompt_tokens\":$PROMPT_TOKENS,\"completion_tokens\":$COMPLETION_TOKENS,\"total_tokens\":$TOTAL_TOKENS,\"latency_ms\":$LATENCY,\"cache_hit\":false}" >> "$OUTPUT"
        
        echo "[$i/$COUNT] ${LATENCY}ms - ${TOTAL_TOKENS} tokens"
    else
        echo "[$i/$COUNT] Error: HTTP $HTTP_CODE"
    fi
    
    # Rate limiting
    sleep 0.1
done

rm -f "$PROMPTS_FILE"

echo ""
echo "=== Collection Complete ==="
echo "Traces saved to: $OUTPUT"
echo "Total entries: $(wc -l < "$OUTPUT")"

# Analyze the trace
echo ""
echo "=== Quick Analysis ==="
echo "Unique requests: $(jq -r '.request_hash' "$OUTPUT" | sort -u | wc -l)"
echo "Avg latency: $(jq -s 'map(.latency_ms) | add / length' "$OUTPUT")ms"
echo "Total tokens: $(jq -s 'map(.total_tokens) | add' "$OUTPUT")"
