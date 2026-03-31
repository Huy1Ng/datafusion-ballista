#!/bin/bash
# export DATAFUSION_EXECUTION_PARQUET_SCHEMA_FORCE_VIEW_TYPES=true
# Exit immediately if a command exits with a non-zero status (e.g., if build fails)
set -e
# Step 0 - generate test file
# cargo install tpchgen-cli 
# tpchgen-cli -s 1 --format=parquet --output-dir data 

echo "--- Step 1: Building debug Binaries ---"
cargo build

# Function to kill processes on script exit (success or failure)
cleanup() {
    echo ""
    echo "--- Step 4: Shutting down Ballista processes ---"
    
    if [ -n "$EXECUTOR2_PID" ]; then kill $EXECUTOR2_PID; fi
    if [ -n "$EXECUTOR1_PID" ]; then kill $EXECUTOR1_PID; fi
    if [ -n "$SCHEDULER_PID" ]; then kill $SCHEDULER_PID; fi
    
    echo "Done."
}

# Register the cleanup function to run on EXIT, SIGINT (Ctrl+C), or SIGTERM
trap cleanup EXIT SIGINT SIGTERM

echo "--- Step 2: Starting Scheduler and Executors ---"

# Start Scheduler
RUST_LOG=info ./target/debug/ballista-scheduler > /dev/null 2>&1 &
SCHEDULER_PID=$!
echo "Scheduler started with PID $SCHEDULER_PID"

# Give the scheduler a moment to bind to the port
sleep 1

# Start Executor 1
RUST_LOG=info ./target/debug/ballista-executor -c 2 -p 50051 > /dev/null 2>&1 &
EXECUTOR1_PID=$!
echo "Executor 1 started with PID $EXECUTOR1_PID"

# Start Executor 2
RUST_LOG=info ./target/debug/ballista-executor -c 2 -p 50052 > /dev/null 2>&1 &
EXECUTOR2_PID=$!
echo "Executor 2 started with PID $EXECUTOR2_PID"

# Give executors a moment to register with the scheduler
sleep 3

echo "--- Step 3: Running TPC-H Benchmark ---"
cargo run --bin tpch -- benchmark ballista \
  --host localhost \
  --port 50050 \
  --path "$(pwd)/data" \
  --format parquet \
  --query 10

# The script ends here, triggering the 'cleanup' trap automatically.

