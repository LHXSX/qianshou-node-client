//! 节点基准探针 (移植自 super_engine_v2/verifier/benchmark.py · Rust 实现)
//!
//! 目的:
//!   - 节点启动时跑一次 micro-bench · 量化算力能力
//!   - 把结果塞 capabilities 上报 backend
//!   - planner 用 capability_score 做调度优先级
//!
//! 三项 bench:
//!   1. cpu_sha256_mb_per_sec    SHA256 50MB 吞吐
//!   2. memory_gb_per_sec        内存读写吞吐 (1GB)
//!   3. disk_write_mb_per_sec    临时文件写 100MB 吞吐
//!
//! 综合 capability_score = (cpu * 0.5 + mem * 0.3 + disk * 0.2) · 归一化到 0-100

use std::io::Write;
use std::time::Instant;

use serde::Serialize;
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, Serialize, Default)]
pub struct BenchmarkResult {
    pub cpu_sha256_mb_per_sec: f64,
    pub memory_gb_per_sec: f64,
    pub disk_write_mb_per_sec: f64,
    pub capability_score: f64,         // 0-100
    pub bench_elapsed_ms: u64,
}

pub fn run_bench() -> BenchmarkResult {
    let t0 = Instant::now();

    let cpu = bench_cpu_sha256();
    let memory = bench_memory();
    let disk = bench_disk();

    // 归一化 score (经验值 · M4 ≈ 80-90 · 老 i5 ≈ 30-40)
    // cpu: ≈ 1000 MB/s = 50 分 · 2000 MB/s = 100 分
    let cpu_score = (cpu / 20.0).min(100.0);
    // memory: ≈ 10 GB/s = 50 分 · 20 GB/s = 100 分
    let mem_score = (memory * 5.0).min(100.0);
    // disk: ≈ 500 MB/s = 50 分 · 1000 MB/s = 100 分 (SSD ≈ 满分)
    let disk_score = (disk / 10.0).min(100.0);

    let capability_score = cpu_score * 0.5 + mem_score * 0.3 + disk_score * 0.2;

    BenchmarkResult {
        cpu_sha256_mb_per_sec: round2(cpu),
        memory_gb_per_sec: round2(memory),
        disk_write_mb_per_sec: round2(disk),
        capability_score: round2(capability_score),
        bench_elapsed_ms: t0.elapsed().as_millis() as u64,
    }
}

fn round2(x: f64) -> f64 {
    (x * 100.0).round() / 100.0
}

/// CPU bench · 50MB 随机数据 SHA256 → 算 MB/s
fn bench_cpu_sha256() -> f64 {
    const MB: usize = 1024 * 1024;
    const TOTAL: usize = 50 * MB;
    // 用固定模式数据 (避免随机数生成消耗时间)
    let chunk: Vec<u8> = (0..MB).map(|i| (i % 256) as u8).collect();

    let t0 = Instant::now();
    let mut hasher = Sha256::new();
    let mut bytes_done = 0usize;
    while bytes_done < TOTAL {
        hasher.update(&chunk);
        bytes_done += MB;
    }
    let _digest = hasher.finalize();
    let elapsed_s = t0.elapsed().as_secs_f64().max(0.001);
    (TOTAL as f64 / MB as f64) / elapsed_s
}

/// Memory bench · 1GB 分配 + 顺序写 → 算 GB/s
fn bench_memory() -> f64 {
    const SIZE: usize = 256 * 1024 * 1024;  // 256MB · 1GB 在 8GB 机上可能 OOM
    let t0 = Instant::now();
    let mut buf: Vec<u8> = vec![0u8; SIZE];
    // 顺序写 (强制 CPU 真接触每个字节 · 避免 lazy alloc)
    for chunk in buf.chunks_mut(4096) {
        chunk[0] = 0xff;
    }
    let elapsed_s = t0.elapsed().as_secs_f64().max(0.001);
    drop(buf);
    (SIZE as f64 / (1024.0 * 1024.0 * 1024.0)) / elapsed_s
}

/// Disk bench · 写 100MB 到临时文件 → 算 MB/s
fn bench_disk() -> f64 {
    const MB: usize = 1024 * 1024;
    const TOTAL: usize = 100 * MB;
    let chunk: Vec<u8> = vec![0u8; MB];

    let tmp = match tempfile::Builder::new().prefix("ec-bench-").tempfile() {
        Ok(t) => t,
        Err(_) => return 0.0,
    };

    let t0 = Instant::now();
    {
        let mut file = tmp.as_file();
        let mut bytes_done = 0usize;
        while bytes_done < TOTAL {
            if file.write_all(&chunk).is_err() {
                return 0.0;
            }
            bytes_done += MB;
        }
        let _ = file.sync_all();
    }
    let elapsed_s = t0.elapsed().as_secs_f64().max(0.001);
    (TOTAL as f64 / MB as f64) / elapsed_s
}
