// main.rs
use std::{sync::{atomic::{AtomicBool, AtomicU64, Ordering}, Arc}, time::Instant};
use sysinfo::{RefreshKind, System};
use tokio::task::JoinSet;

pub struct SystemUsage {
    system: System,
}

pub struct CpuUsage {
    pub usage: f32,
    pub name: String
}

impl SystemUsage {
    pub fn new() -> Self {
        let mut system = System::new_with_specifics(RefreshKind::everything());
        system.refresh_all();

        Self { system }
    }

    pub fn get_cpu_info(&mut self) -> (usize, Vec<CpuUsage>) {
        self.system.refresh_cpu_all();
        let cpus = self.system.cpus();
        let logical_cores = cpus.len();
        let mut res: Vec<CpuUsage> = Vec::new();

        for cpu in cpus {
            res.push(
                CpuUsage {
                    usage: cpu.cpu_usage(),
                    name: cpu.name().to_owned()
                }
            );
        }
        
        (logical_cores, res)
    }

    pub fn get_ram_info(&mut self) -> (u64, u64) {
        self.system.refresh_memory();
        (self.system.used_memory(), self.system.total_memory())
    }
}

const SCORE_UNIT: u64 = 1000000;
#[derive(Clone)]
pub struct CpuExplosion {
    pub stop_signal: Arc<AtomicBool>
}

impl CpuExplosion {
    pub fn new() -> Self {
        CpuExplosion { stop_signal: Arc::new(AtomicBool::new(false)) }
    }
    pub async fn stress_test_cpu(&self, duration_sec: u64, cpu_cores: usize) -> u64 {
        let mut handles = JoinSet::new();
        let score = Arc::new(AtomicU64::new(0));
        let start_time = Arc::new(Instant::now());

        for _ in 0..cpu_cores {
            let stop_signal_clone = Arc::clone(&self.stop_signal);
            let score_clone = Arc::clone(&score);
            let start_time_clone = Arc::clone(&start_time);

            // Use spawn_blocking for CPU-bound work
            let _ = handles.spawn_blocking(move || {
                fibonnaci_compute_blocking(start_time_clone, duration_sec, score_clone, stop_signal_clone)
            });
        }

        while let Some(res) = handles.join_next().await {
            match res {
                Ok(_) => {},
                Err(e) => eprintln!("A task panicked: {:?}", e),
            }
        }

        let final_score = score.load(Ordering::Relaxed);
        println!("CPU Stress Test Finished. Total Fibonacci computations: {}", final_score);

        return final_score
    }
}

fn fibonnaci_compute_blocking(start_time: Arc<Instant>, duration: u64, score: Arc<AtomicU64>, stop_signal: Arc<AtomicBool>){
    let mut a: u64 = 0;
    let mut b: u64 = 1;
    let mut local_score = 0;
    let mut converted_score = 0;

    loop {
        if stop_signal.load(Ordering::Relaxed){
            break;
        }
        if start_time.elapsed().as_secs() >= duration {
            stop_signal.store(true, Ordering::Relaxed);
            break;
        }

        let next = a.checked_add(b);

        match next {
            Some(n) => {
                a = b;
                b = n;
            },
            None => {
                a = 0;
                b = 1;
            }
        }
        local_score += 1;
        if local_score >= SCORE_UNIT {
            converted_score += 1;
            local_score = 0;
        }
    }

    let cur_value = score.load(Ordering::Relaxed);
    score.store(cur_value + converted_score, Ordering::Relaxed);
}
