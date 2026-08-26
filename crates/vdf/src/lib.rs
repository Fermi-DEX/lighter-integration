use num_bigint::BigUint;
use num_traits::{Zero, One};
use sha2::{Sha256, Digest};
use serde::{Deserialize, Serialize};

pub mod backend;
pub mod wesolowski;
pub mod batch;
pub mod optimized;
pub mod optimized_simple;
pub mod montgomery_simple;
pub mod high_performance;
pub mod fpga_backend;
pub mod posq;
pub mod tlk;
pub mod batch_solve;

pub use backend::VdfBackend;
pub use wesolowski::WesolowskiVdf;
pub use batch::{BatchVerifier, BatchProof, ChainProof};
pub use optimized::OptimizedWesolowskiVdf;
pub use optimized_simple::SimpleOptimizedVdf;
pub use high_performance::HighPerformanceVdf;
pub use fpga_backend::FpgaVdfBackend;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VdfProof {
    pub input: Vec<u8>,
    pub output: BigUint,
    pub proof: BigUint,
    pub iterations: u64,
}

#[derive(Debug)]
pub enum VdfError {
    InvalidProof,
    InvalidInput,
    ComputationError(String),
}

impl std::fmt::Display for VdfError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            VdfError::InvalidProof => write!(f, "Invalid VDF proof"),
            VdfError::InvalidInput => write!(f, "Invalid VDF input"),
            VdfError::ComputationError(msg) => write!(f, "VDF computation error: {}", msg),
        }
    }
}

impl std::error::Error for VdfError {}

pub type VdfResult<T> = Result<T, VdfError>;

pub fn hash_to_prime(input: &[u8]) -> BigUint {
    let mut hasher = Sha256::new();
    hasher.update(input);
    let hash = hasher.finalize();
    
    let mut candidate = BigUint::from_bytes_be(&hash);
    
    while !is_probably_prime(&candidate) {
        candidate += BigUint::one();
    }
    
    candidate
}

/// Miller-Rabin probabilistic primality test.
///
/// Uses the first 12 primes as bases (deterministic below 3.3·10^24) plus
/// 28 pseudo-random bases derived from the candidate itself, for a
/// composite-acceptance probability below 4^-40.
pub fn is_probably_prime(n: &BigUint) -> bool {
    let one = BigUint::one();
    let two = BigUint::from(2u32);
    if n <= &one {
        return false;
    }
    for p in [2u32, 3, 5, 7, 11, 13, 17, 19, 23, 29, 31, 37] {
        let p = BigUint::from(p);
        if n == &p {
            return true;
        }
        if (n % &p).is_zero() {
            return false;
        }
    }

    // n - 1 = d * 2^s with d odd
    let n_minus_1 = n - &one;
    let s = n_minus_1.trailing_zeros().unwrap_or(0);
    let d = &n_minus_1 >> s;

    let mut witness = |a: &BigUint| -> bool {
        // returns true if `a` proves n composite
        let mut x = a.modpow(&d, n);
        if x == one || x == n_minus_1 {
            return false;
        }
        for _ in 1..s {
            x = x.modpow(&two, n);
            if x == n_minus_1 {
                return false;
            }
        }
        true
    };

    // Fixed small-prime bases.
    for a in [2u32, 3, 5, 7, 11, 13, 17, 19, 23, 29, 31, 37] {
        let a = BigUint::from(a);
        if &a >= n {
            continue;
        }
        if witness(&a) {
            return false;
        }
    }
    // Candidate-derived bases (Fiat-Shamir style: deterministic, unpredictable
    // without controlling the candidate).
    let n_bytes = n.to_bytes_be();
    for i in 0u32..28 {
        let mut hasher = Sha256::new();
        hasher.update(b"miller-rabin-base");
        hasher.update(i.to_be_bytes());
        hasher.update(&n_bytes);
        let a = BigUint::from_bytes_be(&hasher.finalize()) % n;
        if a <= one {
            continue;
        }
        if witness(&a) {
            return false;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hash_to_prime() {
        let input = b"test input";
        let prime = hash_to_prime(input);
        assert!(prime > BigUint::zero());
    }
}
