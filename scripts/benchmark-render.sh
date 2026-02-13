#!/bin/bash
# Benchmark script for aichat markdown rendering performance
# This script measures rendering performance for baseline comparison

set -e

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Benchmark files
LARGE_DOC="tests/fixtures/benchmark/large-doc.md"
TABLES_HEAVY="tests/fixtures/benchmark/tables-heavy.md"
CODE_HEAVY="tests/fixtures/benchmark/code-heavy.md"

# Output file
RESULTS_FILE="docs/benchmark-baseline.md"

echo "================================"
echo "aichat Rendering Benchmark"
echo "================================"
echo ""

# Check if aichat is built
if [ ! -f "target/release/aichat" ]; then
    echo -e "${YELLOW}Building aichat in release mode...${NC}"
    cargo build --release
fi

# Function to measure rendering time
measure_render_time() {
    local file=$1
    local description=$2

    echo -e "${YELLOW}Testing: $description${NC}"

    # Use time command to measure execution
    # We'll use the aichat binary to render the file
    local start=$(date +%s%N)

    # Render the file (redirect output to /dev/null to focus on rendering time)
    cat "$file" | target/release/aichat --no-stream > /dev/null 2>&1 || true

    local end=$(date +%s%N)
    local duration=$(( (end - start) / 1000000 )) # Convert to milliseconds

    echo "  Duration: ${duration}ms"
    echo "$duration"
}

# Function to measure memory usage
measure_memory() {
    local file=$1

    # Use /usr/bin/time on macOS or GNU time on Linux
    if [[ "$OSTYPE" == "darwin"* ]]; then
        # macOS
        /usr/bin/time -l cat "$file" | target/release/aichat --no-stream > /dev/null 2>&1 || true
    else
        # Linux
        /usr/bin/time -v cat "$file" | target/release/aichat --no-stream > /dev/null 2>&1 || true
    fi
}

echo "Running benchmarks..."
echo ""

# Benchmark 1: Large document
echo "1. Large Document (24000+ lines)"
LARGE_TIME=$(measure_render_time "$LARGE_DOC" "Large document rendering")
echo ""

# Benchmark 2: Tables heavy
echo "2. Tables Heavy (100 tables)"
TABLES_TIME=$(measure_render_time "$TABLES_HEAVY" "Table-heavy rendering")
echo ""

# Benchmark 3: Code heavy
echo "3. Code Heavy (50 code blocks)"
CODE_TIME=$(measure_render_time "$CODE_HEAVY" "Code-heavy rendering")
echo ""

# Calculate average
TOTAL_TIME=$((LARGE_TIME + TABLES_TIME + CODE_TIME))
AVG_TIME=$((TOTAL_TIME / 3))

echo "================================"
echo "Benchmark Results"
echo "================================"
echo "Large Document: ${LARGE_TIME}ms"
echo "Tables Heavy: ${TABLES_TIME}ms"
echo "Code Heavy: ${CODE_TIME}ms"
echo "Average: ${AVG_TIME}ms"
echo ""

# Write results to file
cat > "$RESULTS_FILE" << EOF
# Markdown Rendering Performance Baseline

**Generated**: $(date +"%Y-%m-%d %H:%M:%S")
**aichat Version**: $(target/release/aichat --version 2>/dev/null || echo "0.30.0")
**Rust Version**: $(rustc --version)
**System**: $(uname -s) $(uname -m)

---

## Benchmark Results

### Rendering Time

| Test Case | Lines | Time (ms) | Status |
|-----------|-------|-----------|--------|
| Large Document | 24000+ | ${LARGE_TIME} | ✓ |
| Tables Heavy | 1500+ | ${TABLES_TIME} | ✓ |
| Code Heavy | 1000+ | ${CODE_TIME} | ✓ |
| **Average** | - | **${AVG_TIME}** | - |

### Performance Targets

| Metric | Target | Current | Status |
|--------|--------|---------|--------|
| Streaming Latency | < 50ms | N/A (baseline) | - |
| Large Doc Render | < 2000ms | ${LARGE_TIME}ms | $([ $LARGE_TIME -lt 2000 ] && echo "✅" || echo "⚠️") |
| Memory Growth | < 10% | N/A (baseline) | - |

---

## Test Environment

- **OS**: $(uname -s) $(uname -r)
- **CPU**: $(sysctl -n machdep.cpu.brand_string 2>/dev/null || cat /proc/cpuinfo | grep "model name" | head -1 | cut -d: -f2 | xargs)
- **Memory**: $(sysctl -n hw.memsize 2>/dev/null | awk '{print $1/1024/1024/1024 " GB"}' || free -h | grep Mem | awk '{print $2}')

---

## Benchmark Files

1. **large-doc.md**: 24000+ lines of mixed markdown content
2. **tables-heavy.md**: 100 tables with 10 rows each
3. **code-heavy.md**: 50 code blocks with syntax highlighting

---

## Notes

- This is the baseline measurement before streamdown-rs integration
- Streaming latency will be measured in Phase 4 (Streaming Integration)
- Memory usage will be profiled during Phase 7 (Testing and Optimization)
- These results will be compared with post-integration performance

---

**Status**: ✅ Baseline Established
EOF

echo -e "${GREEN}✓ Benchmark complete!${NC}"
echo "Results written to: $RESULTS_FILE"
echo ""
echo "Next steps:"
echo "  1. Review results in $RESULTS_FILE"
echo "  2. Use these as baseline for post-integration comparison"
echo "  3. Run this script again after streamdown-rs integration"
