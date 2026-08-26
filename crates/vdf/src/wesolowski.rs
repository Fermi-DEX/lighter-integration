use crate::{VdfBackend, VdfProof, VdfResult, VdfError, hash_to_prime};
use crate::backend::VdfParams;
use num_bigint::BigUint;
use num_traits::{Zero, One, ToPrimitive};
use std::time::{Duration, Instant};
use sha2::{Sha256, Digest};

#[derive(Debug, Clone)]
pub struct WesolowskiVdf {
    params: VdfParams,
}

impl WesolowskiVdf {
    pub fn new(params: VdfParams) -> Self {
        Self { params }
    }
    
    pub fn with_default_params() -> Self {
        Self::new(VdfParams::default())
    }
    
    fn compute_sequential_squaring(&self, base: &BigUint, iterations: u64) -> BigUint {
        let mut result = base.clone();
        for _ in 0..iterations {
            result = (&result * &result) % &self.params.modulus;
        }
        result
    }
    
    fn fiat_shamir_challenge(&self, input: &[u8], output: &BigUint, iterations: u64) -> BigUint {
        let mut hasher = Sha256::new();
        hasher.update(input);
        hasher.update(&output.to_bytes_be());
        hasher.update(&iterations.to_be_bytes());
        
        hash_to_prime(&hasher.finalize())
    }
    
    fn compute_proof(&self, base: &BigUint, iterations: u64, challenge: &BigUint) -> BigUint {
        // Compute r = 2^T mod l where l is the challenge
        let two = BigUint::from(2u32);
        let exponent = modpow(&two, &BigUint::from(iterations), challenge);
        
        // Compute quotient q = floor(2^T / l)
        let numerator = two.pow(iterations as u32);
        let quotient = &numerator / challenge;
        
        // Compute proof = g^q mod N
        modpow(base, &quotient, &self.params.modulus)
    }
}

impl VdfBackend for WesolowskiVdf {
    fn prove(&self, input: &[u8], iterations: u64) -> VdfResult<VdfProof> {
        if input.is_empty() {
            return Err(VdfError::InvalidInput);
        }
        
        // Hash input to get base element
        let base = hash_to_prime(input);
        
        // Compute y = g^(2^T) mod N
        let output = self.compute_sequential_squaring(&base, iterations);
        
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
        let r = modpow(&two, &BigUint::from(vdf_proof.iterations), &challenge);
        
        // Verify: π^l * g^r ≡ y (mod N)
        let left_side = (modpow(&vdf_proof.proof, &challenge, &self.params.modulus) 
                        * modpow(&base, &r, &self.params.modulus)) 
                        % &self.params.modulus;
        
        Ok(left_side == vdf_proof.output)
    }
    
    fn estimated_time(&self, iterations: u64) -> Duration {
        // Rough estimate: 1 microsecond per iteration on modern CPU
        Duration::from_micros(iterations)
    }
    
    fn name(&self) -> &'static str {
        "WesolowskiVdf"
    }
}

// Helper function for modular exponentiation
fn modpow(base: &BigUint, exponent: &BigUint, modulus: &BigUint) -> BigUint {
    if modulus == &BigUint::one() {
        return BigUint::zero();
    }
    
    let mut result = BigUint::one();
    let mut base = base % modulus;
    let mut exp = exponent.clone();
    
    while exp > BigUint::zero() {
        if &exp & BigUint::one() == BigUint::one() {
            result = (result * &base) % modulus;
        }
        exp >>= 1;
        base = (&base * &base) % modulus;
    }
    
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vdf_proof_and_verify() {
        let vdf = WesolowskiVdf::with_default_params();
        let input = b"test input for VDF";
        let iterations = 1000;
        
        let proof = vdf.prove(input, iterations).expect("Failed to generate proof");
        let is_valid = vdf.verify(&proof).expect("Failed to verify proof");
        
        assert!(is_valid, "VDF proof should be valid");
        assert_eq!(proof.input, input);
        assert_eq!(proof.iterations, iterations);
    }
    
    #[test]
    fn test_invalid_proof_rejected() {
        let vdf = WesolowskiVdf::with_default_params();
        let input = b"test input";
        let iterations = 100;
        
        let mut proof = vdf.prove(input, iterations).expect("Failed to generate proof");
        
        // Tamper with the proof
        proof.output += BigUint::one();
        
        let is_valid = vdf.verify(&proof).expect("Failed to verify proof");
        assert!(!is_valid, "Tampered proof should be invalid");
    }
}
