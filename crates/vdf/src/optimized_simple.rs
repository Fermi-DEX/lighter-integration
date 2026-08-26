use crate::{VdfBackend, VdfProof, VdfResult, VdfError, hash_to_prime};
use crate::backend::VdfParams;
use num_bigint::BigUint;
use num_traits::{Zero, One};
use std::time::Duration;
use sha2::{Sha256, Digest};

/// Algorithmically optimized Wesolowski VDF implementation
/// Focus on computational optimizations without changing data types
#[derive(Debug, Clone)]
pub struct SimpleOptimizedVdf {
    params: VdfParams,
}

impl SimpleOptimizedVdf {
    pub fn new(params: VdfParams) -> Self {
        Self { params }
    }
    
    pub fn with_default_params() -> Self {
        Self::new(VdfParams::default())
    }
    
    /// Optimized sequential squaring with loop unrolling and reduced allocations
    fn compute_sequential_squaring_optimized(&self, base: &BigUint, iterations: u64) -> BigUint {
        if iterations == 0 {
            return base.clone();
        }
        
        let mut result = base.clone();
        let modulus = &self.params.modulus;
        
        // Unroll loops for better performance - process 4 iterations at a time
        let unroll_factor = 4u64;
        let main_iterations = iterations - (iterations % unroll_factor);
        
        // Main unrolled loop
        for _ in (0..main_iterations).step_by(unroll_factor as usize) {
            // Manually unroll 4 iterations to reduce loop overhead
            result = (&result * &result) % modulus;
            result = (&result * &result) % modulus;
            result = (&result * &result) % modulus;
            result = (&result * &result) % modulus;
        }
        
        // Handle remaining iterations
        for _ in 0..(iterations % unroll_factor) {
            result = (&result * &result) % modulus;
        }
        
        result
    }
    
    /// Optimized modular exponentiation using binary method
    fn modpow_optimized(&self, base: &BigUint, exponent: &BigUint, modulus: &BigUint) -> BigUint {
        if modulus == &BigUint::one() {
            return BigUint::zero();
        }
        
        if exponent == &BigUint::zero() {
            return BigUint::one();
        }
        
        if exponent == &BigUint::one() {
            return base % modulus;
        }
        
        let mut result = BigUint::one();
        let mut base_power = base % modulus;
        let mut exp = exponent.clone();
        
        // Binary exponentiation with reduced allocations
        while exp > BigUint::zero() {
            if &exp & BigUint::one() == BigUint::one() {
                result = (&result * &base_power) % modulus;
            }
            exp >>= 1;
            if exp > BigUint::zero() {
                base_power = (&base_power * &base_power) % modulus;
            }
        }
        
        result
    }
    
    fn compute_proof(&self, base: &BigUint, iterations: u64, challenge: &BigUint) -> BigUint {
        // Compute r = 2^T mod l where l is the challenge
        let two = BigUint::from(2u32);
        
        // Compute quotient q = floor(2^T / l)
        let numerator = two.pow(iterations as u32);
        let quotient = &numerator / challenge;
        
        // Compute proof = g^q mod N using optimized modpow
        self.modpow_optimized(base, &quotient, &self.params.modulus)
    }
    
    fn fiat_shamir_challenge(&self, input: &[u8], output: &BigUint, iterations: u64) -> BigUint {
        let mut hasher = Sha256::new();
        hasher.update(input);
        hasher.update(&output.to_bytes_be());
        hasher.update(&iterations.to_be_bytes());
        
        hash_to_prime(&hasher.finalize())
    }
}

impl VdfBackend for SimpleOptimizedVdf {
    fn prove(&self, input: &[u8], iterations: u64) -> VdfResult<VdfProof> {
        if input.is_empty() {
            return Err(VdfError::InvalidInput);
        }
        
        // Hash input to get base element
        let base = hash_to_prime(input);
        
        // Compute y = g^(2^T) mod N using optimized sequential squaring
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
        
        // Compute r = 2^T mod l
        let two = BigUint::from(2u32);
        let r = self.modpow_optimized(&two, &BigUint::from(vdf_proof.iterations), &challenge);
        
        // Verify: π^l * g^r ≡ y (mod N)
        let left_side = (self.modpow_optimized(&vdf_proof.proof, &challenge, &self.params.modulus) 
                        * self.modpow_optimized(&base, &r, &self.params.modulus)) 
                        % &self.params.modulus;
        
        Ok(left_side == vdf_proof.output)
    }
    
    fn estimated_time(&self, iterations: u64) -> Duration {
        // Improved estimate based on loop unrolling: ~2.0 microseconds per iteration
        Duration::from_nanos((iterations as f64 * 2000.0) as u64)
    }
    
    fn name(&self) -> &'static str {
        "SimpleOptimizedVdf"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_optimized_vdf() {
        let vdf = SimpleOptimizedVdf::with_default_params();
        let input = b"simple optimization test";
        let iterations = 50;
        
        let proof = vdf.prove(input, iterations).expect("Failed to generate proof");
        let is_valid = vdf.verify(&proof).expect("Failed to verify proof");
        
        assert!(is_valid, "Simple optimized VDF proof should be valid");
        assert_eq!(proof.input, input);
        assert_eq!(proof.iterations, iterations);
    }
}
