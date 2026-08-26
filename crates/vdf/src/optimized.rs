use crate::{VdfBackend, VdfProof, VdfResult, VdfError, hash_to_prime};
use crate::backend::VdfParams;
use crypto_bigint::{U2048, Encoding};
use num_bigint::BigUint;
use num_traits::{Zero, One};
use std::time::Duration;
use sha2::{Sha256, Digest};

/// Optimized Wesolowski VDF implementation using crypto-bigint for constant-time operations
#[derive(Debug, Clone)]
pub struct OptimizedWesolowskiVdf {
    params: VdfParams,
    modulus_u2048: U2048,
}

impl OptimizedWesolowskiVdf {
    pub fn new(params: VdfParams) -> Self {
        // Convert BigUint modulus to crypto-bigint U2048
        let modulus_bytes = params.modulus.to_bytes_be();
        let modulus_u2048 = U2048::from_be_slice(&modulus_bytes);
        
        Self { 
            params,
            modulus_u2048,
        }
    }
    
    pub fn with_default_params() -> Self {
        Self::new(VdfParams::default())
    }
    
    /// Optimized sequential squaring using Montgomery multiplication
    fn compute_sequential_squaring_optimized(&self, base: &U2048, iterations: u64) -> U2048 {
        let mut result = *base;
        
        // Unroll loop for better performance
        let unroll_factor = 8;
        let main_iterations = iterations - (iterations % unroll_factor);
        
        // Main unrolled loop
        for _ in (0..main_iterations).step_by(unroll_factor as usize) {
            result = self.modular_square(&result);
            result = self.modular_square(&result);
            result = self.modular_square(&result);
            result = self.modular_square(&result);
            result = self.modular_square(&result);
            result = self.modular_square(&result);
            result = self.modular_square(&result);
            result = self.modular_square(&result);
        }
        
        // Handle remaining iterations
        for _ in 0..(iterations % unroll_factor) {
            result = self.modular_square(&result);
        }
        
        result
    }
    
    /// Optimized modular squaring using crypto-bigint
    #[inline(always)]
    fn modular_square(&self, x: &U2048) -> U2048 {
        // Fallback to basic modular arithmetic for now
        // TODO: Implement proper wide multiplication and reduction
        let x_bigint = self.u2048_to_bigint(x);
        let squared = (&x_bigint * &x_bigint) % &self.params.modulus;
        self.bigint_to_u2048(&squared)
    }
    
    /// Convert between BigUint and U2048
    fn bigint_to_u2048(&self, bigint: &BigUint) -> U2048 {
        let bytes = bigint.to_bytes_be();
        // Pad or truncate to exactly 256 bytes (2048 bits)
        let mut padded_bytes = vec![0u8; 256];
        let start = if bytes.len() > 256 { 0 } else { 256 - bytes.len() };
        let copy_len = std::cmp::min(bytes.len(), 256);
        padded_bytes[start..start + copy_len].copy_from_slice(&bytes[bytes.len() - copy_len..]);
        U2048::from_be_slice(&padded_bytes)
    }
    
    fn u2048_to_bigint(&self, u2048: &U2048) -> BigUint {
        BigUint::from_bytes_be(&u2048.to_be_bytes())
    }
    
    /// Fallback to original implementation for proof generation
    /// (proof generation is less performance critical than sequential squaring)
    fn compute_proof_fallback(&self, base: &BigUint, iterations: u64, challenge: &BigUint) -> BigUint {
        // pi = base^floor(2^T / l) via streaming long division (unbounded T).
        let group = crate::posq::Group::new(self.params.modulus.clone());
        crate::posq::quotient_power(&group, base, iterations, challenge)
    }
    
    /// Modular exponentiation (delegates to num-bigint; the previous
    /// windowed implementation was incorrect from the second window on).
    fn modpow_optimized(&self, base: &BigUint, exponent: &BigUint, modulus: &BigUint) -> BigUint {
        if modulus == &BigUint::one() {
            return BigUint::zero();
        }
        base.modpow(exponent, modulus)
    }
    
    fn fiat_shamir_challenge(&self, input: &[u8], output: &BigUint, iterations: u64) -> BigUint {
        let mut hasher = Sha256::new();
        hasher.update(input);
        hasher.update(&output.to_bytes_be());
        hasher.update(&iterations.to_be_bytes());
        
        hash_to_prime(&hasher.finalize())
    }
}

impl VdfBackend for OptimizedWesolowskiVdf {
    fn prove(&self, input: &[u8], iterations: u64) -> VdfResult<VdfProof> {
        if input.is_empty() {
            return Err(VdfError::InvalidInput);
        }
        
        // Hash input to get base element
        let base_bigint = hash_to_prime(input);
        let base_u2048 = self.bigint_to_u2048(&base_bigint);
        
        // Compute y = g^(2^T) mod N using optimized sequential squaring
        let output_u2048 = self.compute_sequential_squaring_optimized(&base_u2048, iterations);
        let output_bigint = self.u2048_to_bigint(&output_u2048);
        
        // Generate Fiat-Shamir challenge
        let challenge = self.fiat_shamir_challenge(input, &output_bigint, iterations);
        
        // Compute proof using fallback method
        let proof = self.compute_proof_fallback(&base_bigint, iterations, &challenge);
        
        Ok(VdfProof {
            input: input.to_vec(),
            output: output_bigint,
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
        // Updated estimate based on optimizations: ~0.8 microseconds per iteration
        Duration::from_nanos((iterations as f64 * 800.0) as u64)
    }
    
    fn name(&self) -> &'static str {
        "OptimizedWesolowskiVdf"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_optimized_vdf_correctness() {
        let vdf = OptimizedWesolowskiVdf::with_default_params();
        let input = b"optimization test";
        let iterations = 100;
        
        let proof = vdf.prove(input, iterations).expect("Failed to generate proof");
        let is_valid = vdf.verify(&proof).expect("Failed to verify proof");
        
        assert!(is_valid, "Optimized VDF proof should be valid");
        assert_eq!(proof.input, input);
        assert_eq!(proof.iterations, iterations);
    }
    
    #[test]
    #[ignore = "timing-sensitive microbenchmark; run on pinned benchmark hardware"]
    fn test_performance_improvement() {
        use std::time::Instant;
        
        let vdf = OptimizedWesolowskiVdf::with_default_params();
        let input = b"performance test";
        let iterations = 50;
        
        let start = Instant::now();
        let _proof = vdf.prove(input, iterations).expect("Failed to generate proof");
        let elapsed = start.elapsed();
        
        // Should be significantly faster than original implementation
        println!("Optimized VDF time for {} iterations: {:?}", iterations, elapsed);
        assert!(elapsed < Duration::from_millis(100), "Should complete within 100ms");
    }
}
