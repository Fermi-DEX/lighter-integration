use criterion::{black_box, criterion_group, criterion_main, Criterion, BenchmarkId};
use vdf::{WesolowskiVdf, VdfBackend};
use std::time::Duration;

fn benchmark_vdf_proof(c: &mut Criterion) {
    let vdf = WesolowskiVdf::with_default_params();
    let input = b"benchmark input for VDF timing";
    
    let mut group = c.benchmark_group("vdf_proof");
    group.measurement_time(Duration::from_secs(30));
    
    // Test different iteration counts to find optimal tick size
    for iterations in [10, 50, 100, 500, 1000, 2000, 5000].iter() {
        group.bench_with_input(
            BenchmarkId::new("prove", iterations),
            iterations,
            |b, &iterations| {
                b.iter(|| {
                    vdf.prove(black_box(input), black_box(iterations))
                        .expect("VDF proof generation failed")
                });
            },
        );
    }
    group.finish();
}

fn benchmark_vdf_verify(c: &mut Criterion) {
    let vdf = WesolowskiVdf::with_default_params();
    let input = b"benchmark input for VDF verification";
    
    let mut group = c.benchmark_group("vdf_verify");
    group.measurement_time(Duration::from_secs(15));
    
    // Pre-generate proofs for verification benchmarks
    let test_cases: Vec<_> = [100, 500, 1000, 2000, 5000]
        .iter()
        .map(|&iterations| {
            let proof = vdf.prove(input, iterations)
                .expect("Failed to generate proof for benchmark");
            (iterations, proof)
        })
        .collect();
    
    for (iterations, proof) in test_cases {
        group.bench_with_input(
            BenchmarkId::new("verify", iterations),
            &proof,
            |b, proof| {
                b.iter(|| {
                    vdf.verify(black_box(proof))
                        .expect("VDF verification failed")
                });
            },
        );
    }
    group.finish();
}

fn benchmark_tick_targets(c: &mut Criterion) {
    let vdf = WesolowskiVdf::with_default_params();
    let input = b"tick benchmark input";
    
    let mut group = c.benchmark_group("tick_timing");
    group.measurement_time(Duration::from_secs(20));
    
    // Target tick times: aim for 100µs as per spec
    let target_100_micros = find_iterations_for_target_time(&vdf, input, Duration::from_micros(100));
    let target_50_micros = find_iterations_for_target_time(&vdf, input, Duration::from_micros(50));
    let target_200_micros = find_iterations_for_target_time(&vdf, input, Duration::from_micros(200));
    
    println!("Estimated iterations for 50µs: {}", target_50_micros);
    println!("Estimated iterations for 100µs: {}", target_100_micros);
    println!("Estimated iterations for 200µs: {}", target_200_micros);
    
    for (target_name, iterations) in [
        ("50µs_target", target_50_micros),
        ("100µs_target", target_100_micros),
        ("200µs_target", target_200_micros),
    ] {
        if iterations > 0 {
            group.bench_with_input(
                BenchmarkId::new("tick", target_name),
                &iterations,
                |b, &iterations| {
                    b.iter(|| {
                        vdf.prove(black_box(input), black_box(iterations))
                            .expect("VDF proof generation failed")
                    });
                },
            );
        }
    }
    group.finish();
}

fn find_iterations_for_target_time(vdf: &WesolowskiVdf, input: &[u8], target: Duration) -> u64 {
    // Quick calibration run
    let start = std::time::Instant::now();
    let _ = vdf.prove(input, 100).expect("Calibration proof failed");
    let elapsed = start.elapsed();
    
    // Estimate iterations needed for target time
    let ratio = target.as_nanos() as f64 / elapsed.as_nanos() as f64;
    let estimated_iterations = (100.0 * ratio) as u64;
    
    // Clamp to reasonable bounds
    estimated_iterations.max(10).min(10000)
}

criterion_group!(
    benches,
    benchmark_vdf_proof,
    benchmark_vdf_verify,
    benchmark_tick_targets
);
criterion_main!(benches);
