use vdf::{WesolowskiVdf, VdfBackend};
use std::time::Instant;

fn main() {
    let vdf = WesolowskiVdf::with_default_params();
    let input = b"tick timing test";
    
    println!("VDF Tick Size Analysis");
    println!("======================");
    
    // Test different iteration counts
    let test_iterations = [1, 5, 10, 25, 50, 100];
    
    for &iterations in &test_iterations {
        println!("\nTesting {} iterations:", iterations);
        
        // Run multiple samples
        let mut times = Vec::new();
        for _ in 0..5 {
            let start = Instant::now();
            let _proof = vdf.prove(input, iterations).expect("Proof generation failed");
            let elapsed = start.elapsed();
            times.push(elapsed);
        }
        
        // Calculate statistics
        let avg_time = times.iter().sum::<std::time::Duration>() / times.len() as u32;
        let min_time = times.iter().min().unwrap();
        let max_time = times.iter().max().unwrap();
        
        println!("  Average: {:?}", avg_time);
        println!("  Min: {:?}", min_time);
        println!("  Max: {:?}", max_time);
        println!("  Time per iteration: {:?}", avg_time / iterations as u32);
        
        // Check if we're close to 100µs target
        let target_100us = std::time::Duration::from_micros(100);
        if avg_time <= target_100us {
            println!("  ✓ Within 100µs target");
        } else {
            println!("  ✗ Exceeds 100µs target by {:?}", avg_time - target_100us);
        }
    }
    
    // Find optimal iteration count for 100µs target
    println!("\n======================");
    println!("Finding optimal iteration count for 100µs tick:");
    
    let target = std::time::Duration::from_micros(100);
    let mut best_iterations = 1;
    let mut best_time_diff = std::time::Duration::from_secs(1);
    
    for iterations in 1..=100 {
        let start = Instant::now();
        let _proof = vdf.prove(input, iterations).expect("Proof generation failed");
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
        
        if iterations % 10 == 0 {
            println!("  {} iterations: {:?}", iterations, elapsed);
        }
        
        // Stop if we're way over target
        if elapsed > std::time::Duration::from_millis(1) {
            break;
        }
    }
    
    println!("\nOptimal configuration:");
    println!("  Iterations: {}", best_iterations);
    println!("  Time difference from 100µs target: {:?}", best_time_diff);
    
    // Final verification with optimal iterations
    let start = Instant::now();
    let proof = vdf.prove(input, best_iterations).expect("Final proof generation failed");
    let prove_time = start.elapsed();
    
    let start = Instant::now();
    let is_valid = vdf.verify(&proof).expect("Final verification failed");
    let verify_time = start.elapsed();
    
    println!("\nFinal verification:");
    println!("  Proof time: {:?}", prove_time);
    println!("  Verify time: {:?}", verify_time);
    println!("  Proof valid: {}", is_valid);
    
    if prove_time <= std::time::Duration::from_micros(100) {
        println!("  ✓ Achieves 100µs tick target with {} iterations", best_iterations);
    } else {
        println!("  ⚠ Cannot achieve 100µs tick target on this hardware");
        let achievable_tps = 1_000_000.0 / prove_time.as_micros() as f64;
        println!("    Achievable tick rate: {:.0} ticks/second", achievable_tps);
    }
}
