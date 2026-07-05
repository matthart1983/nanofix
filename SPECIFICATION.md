# High-Performance FIX Engine — Technical Specification

**Version:** 1.0  
**Classification:** Internal — Confidential  
**Target Environment:** Tier-1 Investment Bank  
**Protocol Support:** FIX 4.2, 4.4, 5.0, 5.0 SP2 (FIXT 1.1)

---

## 1. Executive Summary

This specification defines **nanofix**, a deterministic, ultra-low-latency FIX protocol engine designed for use across all electronic trading desks of a large investment bank. The engine targets **sub-microsecond message parsing**, **single-digit microsecond end-to-end latency** (wire-to-wire), and **sustained throughput exceeding 2 million messages per second** per core — outperforming commercial offerings such as LSEG (Refinitiv) RASH, Onix Solutions OnixS, and Chronicle FIX.

---

## 2. Performance Targets & Industry Benchmarks

### 2.1 Latency Targets

| Metric | nanofix Target | Industry Benchmark (Best Commercial) |
|---|---|---|
| Message parse (NewOrderSingle) | ≤ 300 ns | ~800–1,200 ns |
| Message serialize (ExecutionReport) | ≤ 250 ns | ~700–1,000 ns |
| Wire-to-wire (NIC Rx → NIC Tx) | ≤ 3 µs (median) | ~8–15 µs |
| 99th percentile latency | ≤ 5 µs | ~20–50 µs |
| 99.99th percentile latency | ≤ 8 µs | ~80–200 µs |
| Session logon handshake | ≤ 50 µs | ~200–500 µs |
| Sequence number recovery (gap fill) | ≤ 100 µs per msg | ~500 µs per msg |

### 2.2 Throughput Targets

| Metric | nanofix Target | Industry Benchmark |
|---|---|---|
| Sustained msg/s (single core) | ≥ 2,000,000 | ~500,000–800,000 |
| Burst msg/s (10 ms window) | ≥ 5,000,000 | ~1,500,000 |
| Concurrent sessions (single instance) | ≥ 4,096 | ~500–1,000 |
| Max message size supported | 64 KB | 8–16 KB |

### 2.3 Reliability Targets

| Metric | Target |
|---|---|
| Uptime | 99.999% (five nines) |
| Message loss | Zero (guaranteed delivery) |
| Failover time (active-passive) | ≤ 50 ms |
| State replication lag | ≤ 1 ms |
| MTBF | ≥ 8,760 hours (1 year) |

---

## 3. Architecture

### 3.1 Core Design Principles

1. **Zero-allocation hot path** — No heap allocations after warm-up on the critical message path
2. **Lock-free concurrency** — All inter-thread communication via lock-free ring buffers (SPSC/MPSC)
3. **Kernel bypass networking** — DPDK / Solarflare OpenOnload / Mellanox VMA for NIC access
4. **CPU affinity & isolation** — Pinned threads on isolated cores with `nohz_full` and `rcu_nocbs`
5. **Cache-line aware data structures** — 64-byte aligned, false-sharing-free layouts
6. **Deterministic execution** — No GC, no syscalls on hot path, pre-allocated memory pools
7. **Mechanical sympathy** — Data structures designed for L1/L2 cache residency

### 3.2 Component Architecture

```
┌──────────────────────────────────────────────────────────────────┐
│                        nanofix                       │
├─────────────┬──────────────┬──────────────┬─────────────────────┤
│  Transport  │   Session    │   Message    │    Application      │
│   Layer     │   Layer      │   Layer      │    Gateway          │
├─────────────┼──────────────┼──────────────┼─────────────────────┤
│ • Aeron IPC │ • Session    │ • Zero-copy  │ • Order routing     │
│ • DPDK/     │   state FSM  │   parser     │ • Strategy callbk   │
│   OpenOnload│ • Seq mgmt   │ • Flyweight  │ • Risk checks       │
│ • TCP/UDP   │ • Heartbeat  │   pattern    │ • Drop-copy         │
│ • Multicast │ • Logon/out  │ • FIX dict   │ • Admin interface   │
│ • Unix sock │ • Gap detect │   compiler   │ • Metrics export    │
└─────────────┴──────────────┴──────────────┴─────────────────────┘
         │              │              │               │
    ┌────┴────┐    ┌────┴────┐   ┌────┴────┐    ┌────┴────┐
    │  Ring   │    │  Ring   │   │  Ring   │    │  Ring   │
    │ Buffer  │    │ Buffer  │   │ Buffer  │    │ Buffer  │
    └────┬────┘    └────┬────┘   └────┬────┘    └────┬────┘
         └──────────────┴────────────┴───────────────┘
                         │
                   ┌─────┴─────┐
                   │  Journal  │  (Persistent message store)
                   │  (Aeron/  │
                   │  mmap)    │
                   └───────────┘
```

### 3.3 Thread Model

| Thread | Core Affinity | Role |
|---|---|---|
| `io-rx-{n}` | Isolated core | Kernel-bypass NIC poll, TCP reassembly |
| `session-{n}` | Isolated core | FIX session state machine, heartbeat, sequencing |
| `parser-{n}` | Isolated core | Zero-copy FIX message parsing |
| `app-{n}` | Isolated core | Aeron ingress/egress dispatch |
| `io-tx-{n}` | Isolated core | Serialization, NIC transmit |
| `journal` | Isolated core | Asynchronous message persistence |
| `admin` | Shared core | Configuration, monitoring, REST API |
| `gc-housekeep` | Shared core | Pool reclamation, timer wheel |

### 3.4 Memory Architecture

```
Pre-allocated Memory Pools
├── Message Pool        : 256 MB (pre-warmed, 64-byte aligned)
│   ├── Small (≤256B)   : 1,048,576 slots
│   ├── Medium (≤4KB)   : 262,144 slots
│   └── Large (≤64KB)   : 16,384 slots
├── Session State Pool  : 64 MB (4,096 sessions × 16 KB)
├── Ring Buffers         : 128 MB (per-stage, power-of-2 sized)
├── String Intern Pool   : 32 MB (tag values, CompIDs)
└── Journal mmap         : 4 GB (circular, memory-mapped file)
```

---

## 4. Transport Layer

### 4.1 Kernel Bypass Stack

- **Standard/default colocated integration:** Aeron IPC / UDP transport
- **Primary venue transport:** DPDK (Data Plane Development Kit) with poll-mode drivers
- **Fallback venue transport:** Solarflare OpenOnload / ef_vi for Solarflare NICs
- **Compatibility transport:** POSIX TCP/UDP for non-latency-sensitive venue connections
- Support for Mellanox ConnectX-6 Dx / Intel E810 NICs

### 4.2 TCP Implementation

- Custom user-space TCP stack on DPDK for FIX-over-TCP
- Nagle disabled (`TCP_NODELAY` equivalent)
- Custom congestion control tuned for datacenter (DCTCP-variant)
- Pre-built SYN cookies for rapid connection acceptance
- Receive-side coalescing disabled for latency

### 4.3 Connection Management

- Acceptor and Initiator modes
- Configurable per-session venue transport plus Aeron for colocated integration
- Connection pooling with pre-established backup connections
- Automatic reconnection with exponential backoff (configurable)

---

## 5. Session Layer

### 5.1 Session State Machine

```
              ┌────────────┐
              │ DISCONNECT │◄────────────────────────┐
              └─────┬──────┘                         │
                    │ connect()                      │ error/logout
              ┌─────▼──────┐                         │
              │ CONNECTING │                         │
              └─────┬──────┘                         │
                    │ TCP established                │
              ┌─────▼──────┐                         │
              │ LOGON_SENT │                         │
              └─────┬──────┘                         │
                    │ Logon received                 │
              ┌─────▼──────┐     gap detected   ┌───┴────────┐
              │   ACTIVE   ├────────────────────►│ RESENDING  │
              └─────┬──────┘                    └───┬────────┘
                    │ logout()                      │ gap filled
                    │          ◄────────────────────┘
              ┌─────▼──────┐
              │LOGOUT_SENT │
              └─────┬──────┘
                    │ Logout confirmed
              ┌─────▼──────┐
              │ DISCONNECT │
              └────────────┘
```

### 5.2 Sequence Number Management

- 64-bit sequence numbers (no rollover concern)
- Memory-mapped sequence file for crash recovery
- Configurable reset policy: `ALWAYS`, `DAILY`, `WEEKLY`, `NEVER`
- Gap detection with ResendRequest / SequenceReset-GapFill
- PossDup / PossResend flag management

### 5.3 Heartbeat & Timeout

- Configurable HeartBtInt (default 30s, minimum 1s)
- TestRequest probe with configurable grace period
- Sub-millisecond timer resolution via `CLOCK_MONOTONIC_RAW`
- Hierarchical timer wheel (O(1) timer management)

---

## 6. Message Layer

### 6.1 Zero-Copy Parser

The parser uses a **flyweight pattern** — no copying of field values from the wire buffer. Field accessors return slices into the original receive buffer.

**Parsing Strategy:**
1. Scan for SOH delimiters using SIMD (SSE4.2 / AVX2 / NEON)
2. Tag number extraction via SWAR (SIMD Within A Register) integer parsing
3. Field index built in-place (tag → offset/length pairs in pre-allocated array)
4. Checksum validation using vectorized XOR accumulation
5. Body length validated against actual content

### 6.2 Message Representation

```
MessageView (Flyweight — no copies)
├── buffer: *const u8          // Points to wire buffer
├── length: u32                // Total message length
├── field_count: u16           // Number of fields
├── fields: [FieldEntry; 256]  // Pre-allocated index
│   ├── tag: u32
│   ├── offset: u32
│   └── length: u16
├── msg_type_offset: u32       // Quick access to tag 35
└── checksum_valid: bool
```

### 6.3 Message Builder (Serializer)

- Pre-computed header templates for common message types
- Tag numbers serialized as compile-time constants where possible
- Integer-to-ASCII conversion using lookup tables (no `sprintf`)
- BodyLength and CheckSum computed in single pass
- Output directly into NIC TX buffer (true zero-copy send)

### 6.4 FIX Dictionary

- Compiled dictionary (XML → optimized binary at build time)
- Field validation tables (type, required/optional, allowed values)
- Repeating group structure encoded as state machines
- Support for custom tags and user-defined fields
- Hot-swappable dictionaries for protocol upgrades

---

## 7. Persistence & Recovery

### 7.1 Message Journal

- Memory-mapped circular log (default 4 GB)
- Indexed by session ID + sequence number
- Binary format with CRC32 integrity checks
- Configurable fsync policy: `NONE`, `BATCH` (every N ms), `EVERY_MESSAGE`
- Recovery scan: ≤ 1 second for 10 million messages

### 7.2 Crash Recovery

1. On startup, scan journal for highest persisted sequence per session
2. Compare with counterparty expected sequence
3. Issue ResendRequest or SequenceReset as needed
4. Time to first message after recovery: ≤ 500 ms

### 7.3 High Availability

- **Active-Passive Failover:**
  - State replicated via shared journal (NFS/DRBD) or Aeron cluster
  - Heartbeat monitoring between primary and standby
  - Failover triggered on missed heartbeats (configurable threshold)
  - Virtual IP migration via VRRP/keepalived

- **Active-Active (Stretch):**
  - Aeron Cluster consensus for session state
  - Deterministic replay for state machine synchronization

---

## 8. Application Gateway

### 8.1 Aeron Transport (Standard)

The default application integration is Aeron-based ingress/egress rather than direct in-process callbacks.

```rust
struct AeronGatewayConfig {
    ingress_channel: "aeron:ipc",
    ingress_stream_id: 1001,
    egress_channel: "aeron:ipc",
    egress_stream_id: 1002,
}
```

- Inbound FIX application flow is published onto Aeron ingress streams
- Responses and drop-copy flow are emitted on Aeron egress streams
- One FIX wire message is mapped to one Aeron frame
- TCP client/server wrappers remain available for venue connectivity

### 8.2 Callback Adapter

```
trait FixApplicationHandler {
    fn on_logon(session: &Session, msg: &MessageView) -> Action;
    fn on_logout(session: &Session, msg: &MessageView);
    fn on_message_received(session: &Session, msg: &MessageView) -> Action;
    fn on_message_sent(session: &Session, msg: &MessageView);
    fn on_heartbeat_timeout(session: &Session) -> Action;
    fn on_error(session: &Session, error: FixError);
    fn on_gap_detected(session: &Session, begin: u64, end: u64) -> GapAction;
}

enum Action { Accept, Reject(String), Disconnect }
enum GapAction { RequestResend, Reset, Disconnect }
```

Direct callback handlers remain supported as a convenience adapter for tests and single-process embeddings.

### 8.3 Pre-Trade Risk Checks (Inline)

- **Fat-finger check:** Notional value threshold per instrument
- **Rate limiting:** Messages per second per session/CompID
- **Duplicate detection:** ClOrdID bloom filter (lock-free)
- **Kill switch:** Atomic flag to halt all outbound order flow

### 8.4 Message Routing

- Content-based routing (route by symbol, desk, strategy)
- Round-robin and weighted distribution across downstream sessions
- Priority queues for cancel/replace over new orders

---

## 9. Observability & Operations

### 9.1 Metrics (Real-Time)

All metrics exported via:
- **Prometheus** endpoint (pull)
- **StatsD/Carbon** (push)
- **Shared memory** counters (for co-located dashboards)

| Metric | Type |
|---|---|
| `fix.msg.parsed.count` | Counter (by MsgType) |
| `fix.msg.sent.count` | Counter (by MsgType) |
| `fix.latency.parse_ns` | HDR Histogram |
| `fix.latency.wire_to_wire_ns` | HDR Histogram |
| `fix.session.active` | Gauge |
| `fix.session.gap_fills` | Counter |
| `fix.journal.write_ns` | HDR Histogram |
| `fix.reject.count` | Counter (by reason) |
| `fix.risk.block.count` | Counter (by check) |

### 9.2 Logging

- Structured binary logging on hot path (decode offline)
- Human-readable logging on admin/cold path only
- Per-session FIX message audit log (regulatory requirement)
- Log levels: `TRACE`, `DEBUG`, `INFO`, `WARN`, `ERROR`, `FATAL`

### 9.3 Administration

- REST API for session management (create/destroy/reset)
- CLI tool for live session inspection
- Web dashboard for real-time latency heatmaps
- SNMP traps for critical alerts

---

## 10. Security

### 10.1 Authentication

- FIX Logon Username/Password (tag 553/554)
- TLS 1.3 mutual authentication (optional, adds ~2 µs latency)
- IP whitelist per CompID
- LDAP/Kerberos integration for admin API

### 10.2 Encryption

- TLS 1.3 for WAN connections (AES-256-GCM, ChaCha20-Poly1305)
- No encryption for co-located connections (datacenter-internal)
- Hardware AES-NI acceleration

### 10.3 Audit & Compliance

- Complete message audit trail (immutable, timestamped)
- Regulatory timestamps (nanosecond precision, PTP-synchronized)
- MiFID II / Reg NMS / CAT compliant timestamp fields
- WORM storage integration for regulatory retention

---

## 11. Configuration

### 11.1 Session Configuration (YAML)

```yaml
engine:
  transport: dpdk            # dpdk | openonload | kernel
  cores: [2, 3, 4, 5, 6, 7] # isolated cores for engine threads
  journal_path: /mnt/nvme/fix-journal
  journal_size_gb: 4
  
sessions:
  - session_id: "VENUE-NYSE-1"
    fix_version: "FIX.4.4"
    sender_comp_id: "BANK_OMS"
    target_comp_id: "NYSE"
    role: initiator
    host: "10.0.1.50"
    port: 9876
    heartbeat_interval_sec: 30
    reconnect_interval_ms: 1000
    max_reconnect_attempts: 0  # unlimited
    sequence_reset_policy: daily
    tls_enabled: false
    risk:
      max_msg_rate: 50000
      max_notional_usd: 10000000
      kill_switch_enabled: true
    dictionary: "FIX44-NYSE-Custom.xml"
    
  - session_id: "DROP-COPY-1"
    fix_version: "FIX.5.0SP2"
    sender_comp_id: "BANK_DC"
    target_comp_id: "BANK_SURV"
    role: acceptor
    bind_port: 9877
    heartbeat_interval_sec: 60
    sequence_reset_policy: never
    dictionary: "FIX50SP2.xml"
```

---

## 12. Supported Message Types

### 12.1 Session Level
| MsgType | Tag 35 | Direction |
|---|---|---|
| Logon | A | Both |
| Logout | 5 | Both |
| Heartbeat | 0 | Both |
| TestRequest | 1 | Both |
| ResendRequest | 2 | Both |
| SequenceReset | 4 | Both |
| Reject | 3 | Both |

### 12.2 Application Level (Pre-Built)
| MsgType | Tag 35 | Direction |
|---|---|---|
| NewOrderSingle | D | Outbound |
| ExecutionReport | 8 | Inbound |
| OrderCancelRequest | F | Outbound |
| OrderCancelReject | 9 | Inbound |
| OrderCancelReplaceRequest | G | Outbound |
| OrderStatusRequest | H | Outbound |
| MarketDataRequest | V | Outbound |
| MarketDataSnapshotFullRefresh | W | Inbound |
| MarketDataIncrementalRefresh | X | Inbound |
| SecurityDefinitionRequest | c | Outbound |
| SecurityDefinition | d | Inbound |
| TradeCaptureReport | AE | Both |
| AllocationInstruction | J | Outbound |
| Confirmation | AK | Inbound |
| QuoteRequest | R | Both |
| Quote | S | Both |

### 12.3 Extensibility
- Any MsgType supported via dictionary configuration
- Custom tags via dictionary extension
- Repeating groups fully supported (nested to arbitrary depth)

---

## 13. Deployment Requirements

### 13.1 Hardware (Per Instance)

| Component | Specification |
|---|---|
| CPU | Intel Xeon W-3400 / AMD EPYC 9004 (≥ 16 cores) |
| RAM | 128 GB DDR5-4800 ECC |
| NIC | Solarflare X2522 / Mellanox ConnectX-6 Dx (25/100 GbE) |
| Storage | 2× NVMe SSD (Intel Optane P5800X preferred) |
| OS | RHEL 9 / Ubuntu 22.04 (RT kernel optional) |

### 13.2 OS Tuning

- `isolcpus` / `nohz_full` / `rcu_nocbs` for engine cores
- Transparent Huge Pages disabled (`madvise` mode)
- NUMA-aware memory allocation
- IRQ affinity set to non-engine cores
- `vm.swappiness=0`
- `net.core.busy_poll` enabled (for kernel TCP fallback)

---

## 14. Regulatory Compliance

| Regulation | Requirement | Implementation |
|---|---|---|
| MiFID II RTS 25 | Clock sync ≤ 100 µs, timestamp granularity 1 µs | PTP with Timekeeper NIC, ns-precision timestamps |
| Reg NMS | Order protection, locked/crossed market detection | Application layer callbacks |
| CAT | Reportable event timestamps | Nanosecond audit trail |
| SEC Rule 15c3-5 | Pre-trade risk controls | Inline risk checks (§8.2) |
| FCA | Transaction reporting | Drop-copy session support |
| ESMA | Algo identification | Tag 1003 (TradeReportingIndicator) |

---

## 15. Development & Build

### 15.1 Language & Toolchain

- **Primary language:** Rust (no `unsafe` outside transport/SIMD modules)
- **Build system:** Cargo with feature flags per transport
- **SIMD:** `std::arch` intrinsics with runtime feature detection
- **Minimum Rust version:** 1.75 (stable)

### 15.2 Feature Flags

```toml
[features]
aeron = []
default = ["aeron"]
dpdk = ["dep:dpdk-sys"]
openonload = ["dep:onload-sys"]
kernel-tcp = []
tls = ["dep:rustls"]
journal-fsync = []
simd-avx2 = []
simd-neon = []          # Apple Silicon / ARM
prometheus = ["dep:prometheus"]
```

---
