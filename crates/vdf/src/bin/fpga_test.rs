use vdf::{FpgaVdfBackend, WesolowskiVdf, VdfBackend};
use std::time::Instant;

fn main() {
    println!("FPGA VDF Backend Test");
    println!("=====================");
    
    // Try to create FPGA backend
    let fpga_backend = match FpgaVdfBackend::with_default_params() {
        Ok(backend) => {
            println!("✓ FPGA backend created successfully");
            println!("  Backend name: {}", backend.name());
            backend
        }
        Err(e) => {
            println!("✗ Failed to create FPGA backend: {}", e);
            return;
        }
    };
    
    // Create CPU backend for comparison
    let cpu_backend = WesolowskiVdf::with_default_params();
    println!("✓ CPU backend created for comparison");
    
    let input = b"fpga vs cpu performance test";
    let test_iterations = [10, 25, 50, 100];
    
    println!("\nPerformance Comparison:");
    println!("{}", "=".repeat(50));
    
    for &iterations in &test_iterations {
        println!("\nTesting {} iterations:", iterations);
        
        // Test CPU backend
        let start = Instant::now();
        let cpu_proof = cpu_backend.prove(input, iterations).expect("CPU proof failed");
        let cpu_time = start.elapsed();
        
        let start = Instant::now();
        let cpu_valid = cpu_backend.verify(&cpu_proof).expect("CPU verify failed");
        let cpu_verify_time = start.elapsed();
        
        // Test FPGA backend
        let start = Instant::now();
        let fpga_proof = fpga_backend.prove(input, iterations).expect("FPGA proof failed");
        let fpga_time = start.elapsed();
        
        let start = Instant::now();
        let fpga_valid = fpga_backend.verify(&fpga_proof).expect("FPGA verify failed");
        let fpga_verify_time = start.elapsed();
        
        // Results
        println!("  CPU:   Prove: {:?}, Verify: {:?}, Valid: {}", 
                cpu_time, cpu_verify_time, cpu_valid);
        println!("  FPGA:  Prove: {:?}, Verify: {:?}, Valid: {}", 
                fpga_time, fpga_verify_time, fpga_valid);
        
        if fpga_time < cpu_time {
            let speedup = cpu_time.as_nanos() as f64 / fpga_time.as_nanos() as f64;
            println!("  ✓ FPGA is {:.2}x faster!", speedup);
        } else {
            println!("  → FPGA fallback to CPU (no hardware acceleration)");
        }
    }
    
    // Estimate maximum iterations for 100µs
    println!("\n{}", "=".repeat(50));
    println!("Finding optimal iterations for 100µs target:");
    
    let target = std::time::Duration::from_micros(100);
    let mut best_iterations = 1;
    let mut best_time_diff = std::time::Duration::from_secs(1);
    
    for iterations in (10..=1000).step_by(10) {
        let start = Instant::now();
        let _proof = fpga_backend.prove(input, iterations).expect("Proof failed");
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
        
        if iterations % 50 == 0 {
            println!("  {} iterations: {:?}", iterations, elapsed);
        }
        
        // Stop if we're way over target
        if elapsed > std::time::Duration::from_millis(5) {
            break;
        }
    }
    
    println!("\nOptimal Configuration:");
    println!("  Backend: {}", fpga_backend.name());
    println!("  Optimal iterations: {}", best_iterations);
    println!("  Time difference from 100µs: {:?}", best_time_diff);
    
    // Final performance test
    let start = Instant::now();
    let final_proof = fpga_backend.prove(input, best_iterations).expect("Final proof failed");
    let final_prove_time = start.elapsed();
    
    println!("\nFinal Performance:");
    println!("  Prove time: {:?}", final_prove_time);
    println!("  Achievable tick rate: {:.0} ticks/second", 
             1_000_000.0 / final_prove_time.as_micros() as f64);
    
    // Theoretical FPGA performance
    let estimated_fpga_time = fpga_backend.estimated_time(best_iterations);
    if estimated_fpga_time < final_prove_time {
        let theoretical_speedup = final_prove_time.as_nanos() as f64 / estimated_fpga_time.as_nanos() as f64;
        println!("  Theoretical FPGA speedup: {:.0}x", theoretical_speedup);
        println!("  Theoretical tick rate: {:.0} ticks/second", 
                 1_000_000.0 / estimated_fpga_time.as_micros() as f64);
        println!("  Theoretical max iterations (100µs): {}", 
                 100_000 / estimated_fpga_time.as_nanos() * best_iterations as u128);
    }
    
    println!("\nNext Steps:");
    if fpga_backend.name().contains("CPU_Fallback") {
        println!("  1. Install AWS FPGA Developer Kit:");
        println!("     git clone https://github.com/aws/aws-fpga.git");
        println!("     cd aws-fpga && source hdk_setup.sh");
        println!("  2. Load VDF acceleration AFI (Amazon FPGA Image)");
        println!("  3. Rebuild with --features fpga-hardware");
        println!("  4. Expected performance: 50-100x speedup");
    } else {
        println!("  1. FPGA acceleration is active!");
        println!("  2. Consider further optimizations:");
        println!("     - Parallel verification");
        println!("     - Batch processing");
        println!("     - Custom ASIC development");
    }
}
