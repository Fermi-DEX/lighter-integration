use crate::{VdfBackend, VdfProof, VdfResult, VdfError, hash_to_prime};
use crate::backend::VdfParams;
use crate::montgomery_simple::OptimizedArithmetic;
use num_bigint::BigUint;
use num_traits::One;
use std::time::Duration;
use sha2::{Sha256, Digest};

/// High-performance Wesolowski VDF implementation with aggressive optimizations
#[derive(Debug, Clone)]
pub struct HighPerformanceVdf {
    params: VdfParams,
    arithmetic: OptimizedArithmetic,
}

impl HighPerformanceVdf {
    pub fn new(params: VdfParams) -> Self {
        let arithmetic = OptimizedArithmetic::new(params.modulus.clone());
        Self { params, arithmetic }
    }
    
    pub fn with_default_params() -> Self {
        Self::new(VdfParams::default())
    }
    
    /// Highly optimized sequential squaring - the core VDF computation
    fn compute_sequential_squaring_optimized(&self, base: &BigUint, iterations: u64) -> BigUint {
        self.arithmetic.sequential_squaring(base, iterations)
    }
    
    fn compute_proof(&self, base: &BigUint, iterations: u64, challenge: &BigUint) -> BigUint {
        // pi = base^floor(2^T / l) via streaming long division. (The previous
        // implementation used a modular inverse mod N for T > 32, which does
        // not compute the integer quotient and is invalid for composite N.)
        let group = crate::posq::Group::new(self.params.modulus.clone());
        crate::posq::quotient_power(&group, base, iterations, challenge)
    }

    fn fiat_shamir_challenge(&self, input: &[u8], output: &BigUint, iterations: u64) -> BigUint {
        let mut hasher = Sha256::new();
        hasher.update(input);
        hasher.update(&output.to_bytes_be());
        hasher.update(&iterations.to_be_bytes());
        
        hash_to_prime(&hasher.finalize())
    }
}

impl VdfBackend for HighPerformanceVdf {
    fn prove(&self, input: &[u8], iterations: u64) -> VdfResult<VdfProof> {
        if input.is_empty() {
            return Err(VdfError::InvalidInput);
        }
        
        // Hash input to get base element
        let base = hash_to_prime(input);
        
        // Compute y = g^(2^T) mod N using highly optimized sequential squaring
        let output = self.compute_sequential_squaring_optimized(&base, iterations);
        
        // Generate Fiat-Shamir challenge
        let challenge = self.fiat_shamir_challenge(input, &output, iterations);
        
        // Compute proof
        let proof = self.compute_proof(&base, iterations, &challenge);
        
        Ok(VdfProof {
            input: input.to_vec(),
            output,
            proof,
            iterations,
        })
    }
    
    fn verify(&self, vdf_proof: &VdfProof) -> VdfResult<bool> {
        if vdf_proof.input.is_empty() {
            return Err(VdfError::InvalidInput);
        }
        
        // Reconstruct base element
        let base = hash_to_prime(&vdf_proof.input);
        
        // Generate Fiat-Shamir challenge
        let challenge = self.fiat_shamir_challenge(
            &vdf_proof.input, 
            &vdf_proof.output, 
            vdf_proof.iterations
        );
        
        // r = 2^T mod l, computed with modulus l (not mod N then mod l,
        // which diverges once 2^T >= N).
        let r_mod_challenge = crate::posq::two_pow_mod(vdf_proof.iterations, &challenge);
        
        // Verify: π^l * g^r ≡ y (mod N)
        let left_side = (self.arithmetic.mod_pow(&vdf_proof.proof, &challenge) 
                        * self.arithmetic.mod_pow(&base, &r_mod_challenge)) 
                        % &self.params.modulus;
        
        Ok(left_side == vdf_proof.output)
    }
    
    fn estimated_time(&self, iterations: u64) -> Duration {
        // More aggressive estimate based on heavy optimizations: ~1.8 microseconds per iteration
        Duration::from_nanos((iterations as f64 * 1800.0) as u64)
    }
    
    fn name(&self) -> &'static str {
        "HighPerformanceVdf"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_high_performance_vdf() {
        let vdf = HighPerformanceVdf::with_default_params();
        let input = b"high performance test";
        let iterations = 25;
        
        let proof = vdf.prove(input, iterations).expect("Failed to generate proof");
        let is_valid = vdf.verify(&proof).expect("Failed to verify proof");
        
        assert!(is_valid, "High performance VDF proof should be valid");
        assert_eq!(proof.input, input);
        assert_eq!(proof.iterations, iterations);
    }
    
    #[test]
    fn test_high_performance_vs_original() {
        use crate::WesolowskiVdf;
        use std::time::Instant;
        
        let original_vdf = WesolowskiVdf::with_default_params();
        let hp_vdf = HighPerformanceVdf::with_default_params();
        let input = b"performance comparison";
        let iterations = 50;
        
        // Test original
        let start = Instant::now();
        let original_proof = original_vdf.prove(input, iterations).expect("Original proof failed");
        let original_time = start.elapsed();
        
        // Test high performance
        let start = Instant::now();
        let hp_proof = hp_vdf.prove(input, iterations).expect("HP proof failed");
        let hp_time = start.elapsed();
        
        // Both should be valid
        assert!(original_vdf.verify(&original_proof).expect("Original verify failed"));
        assert!(hp_vdf.verify(&hp_proof).expect("HP verify failed"));
        
        println!("Original: {:?}, High Performance: {:?}", original_time, hp_time);
        
        // High performance should be faster (or at least not slower)
        // Note: This might not always pass due to system variations
        // assert!(hp_time <= original_time);
    }
}
