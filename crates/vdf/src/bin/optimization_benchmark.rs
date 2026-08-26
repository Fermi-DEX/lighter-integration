use vdf::{WesolowskiVdf, SimpleOptimizedVdf, HighPerformanceVdf, VdfBackend};
use std::time::Instant;

fn benchmark_implementation<T: VdfBackend>(vdf: &T, name: &str, input: &[u8], iterations: u64) -> std::time::Duration {
    println!("\n{} Benchmark:", name);
    println!("================================");
    
    let mut times = Vec::new();
    
    // Warm up
    for _ in 0..3 {
        let _ = vdf.prove(input, iterations);
    }
    
    // Actual benchmark
    for i in 0..10 {
        let start = Instant::now();
        let proof = vdf.prove(input, iterations).expect("Proof generation failed");
        let prove_time = start.elapsed();
        
        let start = Instant::now();
        let is_valid = vdf.verify(&proof).expect("Verification failed");
        let verify_time = start.elapsed();
        
        if !is_valid {
            panic!("Generated invalid proof in run {}", i);
        }
        
        times.push(prove_time);
        
        if i < 3 {
            println!("  Run {}: Prove: {:?}, Verify: {:?}", i + 1, prove_time, verify_time);
        }
    }
    
    // Calculate statistics
    let avg_time = times.iter().sum::<std::time::Duration>() / times.len() as u32;
    let min_time = *times.iter().min().unwrap();
    let max_time = *times.iter().max().unwrap();
    
    // Calculate standard deviation
    let variance = times.iter()
        .map(|&t| {
            let diff = t.as_nanos() as f64 - avg_time.as_nanos() as f64;
            diff * diff
        })
        .sum::<f64>() / times.len() as f64;
    let std_dev = variance.sqrt();
    
    println!("\nStatistics for {} iterations:", iterations);
    println!("  Average: {:?}", avg_time);
    println!("  Min: {:?}", min_time);
    println!("  Max: {:?}", max_time);
    println!("  Std Dev: {:.2}µs", std_dev / 1000.0);
    println!("  Time per iteration: {:?}", avg_time / iterations as u32);
    
    // Calculate theoretical max iterations for 100µs
    let target_100us = std::time::Duration::from_micros(100);
    let theoretical_max = (target_100us.as_nanos() as f64 / avg_time.as_nanos() as f64 * iterations as f64) as u64;
    println!("  Theoretical max iterations (100µs): {}", theoretical_max);
    
    avg_time
}

fn main() {
    println!("VDF Implementation Comparison Benchmark");
    println!("=======================================");
    
    let input = b"optimization benchmark test";
    let test_iterations = [10, 25, 50, 100];
    
    let original_vdf = WesolowskiVdf::with_default_params();
    let optimized_vdf = SimpleOptimizedVdf::with_default_params();
    let hp_vdf = HighPerformanceVdf::with_default_params();
    
    for &iterations in &test_iterations {
        println!("\n{}", "=".repeat(60));
        println!("TESTING {} ITERATIONS", iterations);
        println!("{}", "=".repeat(60));
        
        let original_time = benchmark_implementation(&original_vdf, "Original WesolowskiVdf", input, iterations);
        let optimized_time = benchmark_implementation(&optimized_vdf, "Simple Optimized VDF", input, iterations);
        let hp_time = benchmark_implementation(&hp_vdf, "High Performance VDF", input, iterations);
        
        let speedup_simple = original_time.as_nanos() as f64 / optimized_time.as_nanos() as f64;
        let speedup_hp = original_time.as_nanos() as f64 / hp_time.as_nanos() as f64;
        
        println!("\nCOMPARISON:");
        println!("  Original:     {:?}", original_time);
        println!("  Simple Opt:   {:?} ({:.2}x speedup)", optimized_time, speedup_simple);
        println!("  High Perf:    {:?} ({:.2}x speedup)", hp_time, speedup_hp);
        
        if speedup_hp > speedup_simple && speedup_simple > 1.0 {
            println!("  ✓ High Performance VDF is fastest!");
        } else if speedup_simple > 1.0 {
            println!("  ✓ Simple optimization successful!");
        } else {
            println!("  ⚠ No significant performance improvement");
        }
    }
    
    println!("\n{}", "=".repeat(60));
    println!("FINDING OPTIMAL ITERATION COUNT FOR 100µs TARGET");
    println!("{}", "=".repeat(60));
    
    let target = std::time::Duration::from_micros(100);
    
    // Test with high performance version
    println!("\nHigh Performance VDF - Finding optimal iterations:");
    let mut best_iterations = 1;
    let mut best_time_diff = std::time::Duration::from_secs(1);
    
    for iterations in (10..=200).step_by(10) {
        let start = Instant::now();
        let _proof = hp_vdf.prove(input, iterations).expect("Proof generation failed");
        let elapsed = start.elapsed();
        
        let time_diff = if elapsed > target {
            elapsed - target
        } else {
            target - elapsed
        };
        
        if time_diff < best_time_diff {
            best_time_diff = time_diff;
            best_iterations = iterations;
        }
        
        println!("  {} iterations: {:?} (diff: {:?})", iterations, elapsed, time_diff);
        
        if elapsed > std::time::Duration::from_millis(1) {
            break;
        }
    }
    
    println!("\nOptimal configuration for Optimized VDF:");
    println!("  Iterations: {}", best_iterations);
    println!("  Time difference from 100µs target: {:?}", best_time_diff);
    
    // Final performance summary
    let final_start = Instant::now();
    let final_proof = hp_vdf.prove(input, best_iterations).expect("Final proof failed");
    let final_prove_time = final_start.elapsed();
    
    let final_start = Instant::now();
    let is_valid = hp_vdf.verify(&final_proof).expect("Final verification failed");
    let final_verify_time = final_start.elapsed();
    
    println!("\nFinal Performance Summary:");
    println!("  Optimal iterations: {}", best_iterations);
    println!("  Prove time: {:?}", final_prove_time);
    println!("  Verify time: {:?}", final_verify_time);
    println!("  Proof valid: {}", is_valid);
    
    if final_prove_time <= target {
        println!("  ✓ Achieves 100µs target with {} iterations", best_iterations);
        let tps = 1_000_000.0 / final_prove_time.as_micros() as f64;
        println!("  Tick rate: {:.0} ticks/second", tps);
    } else {
        println!("  ⚠ Exceeds 100µs target");
        let achievable_tps = 1_000_000.0 / final_prove_time.as_micros() as f64;
        println!("  Achievable tick rate: {:.0} ticks/second", achievable_tps);
    }
    
    // Performance vs original baseline
    let baseline_38_iterations = std::time::Duration::from_micros(99); // Previous baseline
    let improvement_factor = baseline_38_iterations.as_nanos() as f64 / 
                           (final_prove_time.as_nanos() as f64 * 38.0 / best_iterations as f64);
    
    println!("\nImprovement vs Previous Baseline (38 iterations in 99µs):");
    println!("  Improvement factor: {:.2}x", improvement_factor);
}
