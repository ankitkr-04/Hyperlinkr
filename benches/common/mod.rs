use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use sysinfo::{System, Pid};

#[derive(Clone, Debug)]
#[allow(dead_code)]
pub enum MemoryAction {
    Fail,       // Hard fail on memory limits (for CI)
    Warn,       // Log warnings but continue (for dev)
    Skip,       // Skip remaining tests (graceful degradation)
}

#[derive(Clone)]
#[allow(dead_code)]
pub struct BenchConfig {
    pub max_process_memory_mb: u64,
    pub max_concurrent_tasks: usize,
    pub batch_sizes: Vec<usize>,
    pub enable_memory_monitoring: bool,
    pub safety_margin_mb: u64,
    pub memory_action: MemoryAction,
    pub log_memory_stats: bool,
}

impl BenchConfig {
    #[allow(dead_code)]
    pub fn for_system() -> Self {
        let mut sys = System::new_all();
        sys.refresh_all();
        
        let total_memory = sys.total_memory() / 1024 / 1024;
        let available_memory = sys.available_memory() / 1024 / 1024;
        
        // More conservative approach - absolute process memory limits
        let max_process_memory = (available_memory / 2).min(2048);
        let safety_margin = 1024;
        
        let max_concurrent = if total_memory > 8000 { 6 } 
                           else if total_memory > 4000 { 4 } 
                           else { 2 };
        
        let batch_sizes = if available_memory > 4000 {
            vec![10, 50, 100, 500, 1000]
        } else if available_memory > 2000 {
            vec![10, 25, 50, 100, 250]
        } else {
            vec![5, 10, 25, 50]
        };

        Self {
            max_process_memory_mb: max_process_memory,
            max_concurrent_tasks: max_concurrent,
            batch_sizes,
            enable_memory_monitoring: true,
            safety_margin_mb: safety_margin,
            memory_action: MemoryAction::Warn, // Default to warn in dev
            log_memory_stats: true,
        }
    }

    #[allow(dead_code)]
    pub fn aggressive() -> Self {
        let mut sys = System::new_all();
        sys.refresh_all();
        let available_memory = sys.available_memory() / 1024 / 1024;
        
        Self {
            max_process_memory_mb: (available_memory * 3 / 4).min(4096),
            max_concurrent_tasks: 12,
            batch_sizes: vec![10, 50, 100, 500, 1000, 2000],
            enable_memory_monitoring: true,
            safety_margin_mb: 512,
            memory_action: MemoryAction::Fail,
            log_memory_stats: true,
        }
    }

    #[allow(dead_code)]
    pub fn permissive() -> Self {
        Self {
            max_process_memory_mb: u64::MAX,
            max_concurrent_tasks: 16,
            batch_sizes: vec![10, 50, 100, 500, 1000, 5000],
            enable_memory_monitoring: false,
            safety_margin_mb: 0,
            memory_action: MemoryAction::Warn,
            log_memory_stats: false,
        }
    }

    #[allow(dead_code)]
    pub fn ci() -> Self {
        let mut config = Self::for_system();
        config.memory_action = MemoryAction::Fail; // Strict in CI
        config.log_memory_stats = true;
        config
    }

    #[allow(dead_code)]
    pub fn dev() -> Self {
        Self::for_system()
    }

    #[allow(dead_code)]
    pub fn conservative() -> Self {
        Self {
            max_process_memory_mb: 512, // 512MB limit
            max_concurrent_tasks: 2,
            batch_sizes: vec![5, 10, 25],
            enable_memory_monitoring: true,
            safety_margin_mb: 256,
            memory_action: MemoryAction::Warn,
            log_memory_stats: true,
        }
    }
}

#[derive(Debug)]
#[allow(dead_code)]
pub struct MemoryStats {
    pub available_mb: u64,
    pub process_memory_mb: u64,
    pub memory_growth_mb: u64,
    pub within_limits: bool,
}

pub struct MemoryGuard {
    pub config: BenchConfig,
    monitoring: Arc<AtomicBool>,
    initial_memory: u64,
    pid: Pid,
    // Reuse system instance for efficiency
    sys: System,
}

impl MemoryGuard {
    pub fn new(config: BenchConfig) -> Self {
        let pid = Pid::from(std::process::id() as usize);
        let mut sys = System::new();
        sys.refresh_all();

        let initial_memory = sys.process(pid)
            .map(|p| p.memory() / 1024 / 1024)
            .unwrap_or(0);
        
        Self {
            config,
            monitoring: Arc::new(AtomicBool::new(true)),
            initial_memory,
            pid,
            sys,
        }
    }

    fn refresh_memory_info(&mut self) {
        // Only refresh what we need - more efficient
        self.sys.refresh_memory();
        self.sys.refresh_all();
    }

    pub fn check_memory_usage(&mut self) -> Result<(), String> {
        if !self.config.enable_memory_monitoring {
            return Ok(());
        }

        self.refresh_memory_info();
        let stats = self.get_current_stats();

        // Check available memory first (critical)
        if stats.available_mb < self.config.safety_margin_mb {
            let msg = format!(
                "Available memory too low: {}MB available (need {}MB margin)", 
                stats.available_mb, self.config.safety_margin_mb
            );
            
            return match self.config.memory_action {
                MemoryAction::Fail => Err(msg),
                MemoryAction::Warn => {
                    eprintln!("⚠️ {}", msg);
                    Ok(())
                },
                MemoryAction::Skip => Err(format!("SKIP: {}", msg)),
            };
        }
        
        // Check absolute process memory usage
        if stats.process_memory_mb > self.config.max_process_memory_mb {
            let msg = format!(
                "Process memory {}MB exceeded limit {}MB (growth: {}MB)", 
                stats.process_memory_mb, 
                self.config.max_process_memory_mb,
                stats.memory_growth_mb
            );
            
            return match self.config.memory_action {
                MemoryAction::Fail => Err(msg),
                MemoryAction::Warn => {
                    eprintln!("⚠️ {}", msg);
                    Ok(())
                },
                MemoryAction::Skip => Err(format!("SKIP: {}", msg)),
            };
        }

        if self.config.log_memory_stats {
            println!("📊 Memory: {}MB process, {}MB available, {}MB growth", 
                stats.process_memory_mb, stats.available_mb, stats.memory_growth_mb);
        }
        
        Ok(())
    }

    fn get_current_stats(&self) -> MemoryStats {
        let available_mb = self.sys.available_memory() / 1024 / 1024;
        let process_memory_mb = self.sys.process(self.pid)
            .map(|p| p.memory() / 1024 / 1024)
            .unwrap_or(0);
        let memory_growth_mb = process_memory_mb.saturating_sub(self.initial_memory);
        
        let within_limits = available_mb >= self.config.safety_margin_mb 
            && process_memory_mb <= self.config.max_process_memory_mb;
        
        MemoryStats {
            available_mb,
            process_memory_mb,
            memory_growth_mb,
            within_limits,
        }
    }

    #[allow(dead_code)]
    pub fn get_memory_stats(&mut self) -> MemoryStats {
        self.refresh_memory_info();
        self.get_current_stats()
    }

    // For Criterion custom measurements
    #[allow(dead_code)]
    pub fn memory_measurement(&mut self) -> f64 {
        let stats = self.get_memory_stats();
        stats.process_memory_mb as f64
    }

    #[allow(dead_code)]
    pub fn log_memory_summary(&mut self) {
        let stats = self.get_memory_stats();
        println!("\n📈 Memory Summary:");
        println!("   Process: {}MB (started at {}MB, grew {}MB)", 
            stats.process_memory_mb, self.initial_memory, stats.memory_growth_mb);
        println!("   Available: {}MB", stats.available_mb);
        println!("   Limit: {}MB", self.config.max_process_memory_mb);
        println!("   Within limits: {}", if stats.within_limits { "✅" } else { "❌" });
    }

    pub fn start_monitoring(&self) -> Arc<AtomicBool> {
        // Return a handle that can be used to stop monitoring
        Arc::clone(&self.monitoring)
    }

    pub fn stop_monitoring(&self) {
        // Stop monitoring by setting the flag to false
        self.monitoring.store(false, Ordering::Relaxed);
    }
}

// Helper for Criterion integration
impl MemoryGuard {
    #[allow(dead_code)]
    pub fn with_memory_tracking<F, R>(&mut self, f: F) -> Result<R, String> 
    where F: FnOnce() -> R 
    {
        self.check_memory_usage()?;
        let result = f();
        
        if self.config.log_memory_stats {
            let stats = self.get_memory_stats();
            println!("   → Memory after: {}MB", stats.process_memory_mb);
        }
        
        Ok(result)
    }
}

// Usage example with environment detection
impl BenchConfig {
    #[allow(dead_code)]
    pub fn auto() -> Self {
        // Detect environment
        if std::env::var("CI").is_ok() || std::env::var("GITHUB_ACTIONS").is_ok() {
            Self::ci()
        } else if std::env::var("BENCH_MODE").as_deref() == Ok("aggressive") {
            Self::aggressive()
        } else if std::env::var("BENCH_MODE").as_deref() == Ok("permissive") {
            Self::permissive()
        } else {
            Self::dev() // Default to dev-friendly config
        }
    }
}