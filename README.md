# Perpetra

A high-performance, multithreaded perpetual futures exchange engine written in Rust.

> **Warning**: 🚧 This project is a Work in Progress. The core matching engine is functional and exceptionally performant, but the frontend is not yet implemented.

## ⚡ Performance Benchmark

The core strength of this exchange is its raw performance, achieved through a custom, lock-free-inspired architecture. The following benchmark was conducted on a **MacBook Air with an M2 chip**:

*   **Duration**: 2 minutes
*   **Requests**: 578,349
*   **Throughput**: **~4,818 requests per second**
*   **Avg. Latency**: **0.19 ms**
*   **Failures**: **0**
*   **Server CPU Usage**: ~70%
*   **Server Memory Usage**: ~10-12 MB

This demonstrates the engine's capability to handle significant load with minimal resource consumption and sub-millisecond latency, making it suitable for high-frequency trading scenarios.

## 🏗 Architecture & Concurrency

Perpetra is built for maximum performance on modern multi-core systems:

*   **Multithreaded Core**: The engine is designed around a thread-per-core model.
*   **Pinned Threads**: Critical threads are system level threads which tokio has no control over which ensures determinism.
*   **Efficient Communication**: Inter-thread communication is handled via **Tokio's MPSC channels** for high-throughput message passing and **oneshot channels** for request-response patterns, minimizing lock contention.
*   **Lock-Free Design**: The architecture prioritizes shared-nothing principles and message passing over traditional mutexes for the critical path, leading to phenomenal latency figures.

## 🧩 Current Implementation

The following core components are implemented and functional:

*   **Matching Engine**: A multithreaded engine built on efficient `BTreeMap` structures for maintaining order books.
*   **Liquidation Engine**: Automatically liquidates positions that fall below their maintenance margin.
*   **Funding Rate Payments**: Periodically settles funding between long and short positions.
*   **High-Performance Networking**: A custom HTTP/API server built for low-latency order ingestion.

## 🚧 What's Left (Contributions Welcome!)

As a backend enthusiast, I've prioritized the engine's performance and reliability. The frontend is the primary missing piece.

*   [ ] **Frontend UI**: A web-based interface for interacting with the exchange.
*   [ ] **Advanced Order Types**: (e.g., Stop-Loss, Take-Profit).
*   [ ] **Persistent Storage**: Logging trades and user balances to a database.
*   [ ] **Authentication & Authorization**: User accounts and API key management.

## 🚀 Getting Started

### Prerequisites

*   Rust and Cargo (latest stable version)

### Installation & Running

1.  Clone the repository:
    ```bash
    git clone https://github.com/atharvparlikar/perpetra
    cd backend-rs
    ```

2.  Run the server in release mode for maximum performance:
    ```bash
    cargo run --release
    ```

### Running the Load Test

A load testing client is included to verify the performance metrics.
```bash
# Navigate to the load test directory
cd bot-swarm
go run main.go
```

## 🛠 Tech Stack

*   **Language**: Rust
*   **Concurrency**: Tokio runtime with custom thread management, MPSC, and oneshot channels.
*   **Data Structures**: Standard Library `BTreeMap`
*   **Networking**: Axum (Tokio)

## 🤝 Contributing

This project is open to contributions! This is especially true for:
*   Frontend development.
*   Implementing additional features like advanced order types.
*   Improving documentation and tests.

Please feel free to fork the repo and submit a Pull Request.

