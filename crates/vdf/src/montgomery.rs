use num_bigint::BigUint;
use num_traits::{Zero, One};

/// Montgomery multiplication context for efficient modular arithmetic
#[derive(Debug, Clone)]
pub struct MontgomeryContext {
    /// The modulus N
    modulus: BigUint,
    /// R = 2^k where k is the bit length of modulus
    r: BigUint,
    /// R^2 mod N for conversion to Montgomery form
    r_squared: BigUint,
    /// N' such that R * R^(-1) - N * N' = 1
    n_prime: BigUint,
    /// Bit length of modulus
    k: usize,
}

impl MontgomeryContext {
    /// Create a new Montgomery context for the given modulus
    pub fn new(modulus: BigUint) -> Self {
        let k = modulus.bits() as usize;
        let r = BigUint::one() << k;
        
        // Compute R^2 mod N
        let r_squared = (&r * &r) % &modulus;
        
        // Compute N' using extended Euclidean algorithm
        let n_prime = Self::compute_n_prime(&modulus, &r);
        
        Self {
            modulus,
            r,
            r_squared,
            n_prime,
            k,
        }
    }
    
    /// Convert a number to Montgomery form
    pub fn to_montgomery(&self, x: &BigUint) -> BigUint {
        (x * &self.r_squared) % &self.modulus
    }
    
    /// Convert from Montgomery form back to normal form
    pub fn from_montgomery(&self, x_mont: &BigUint) -> BigUint {
        self.montgomery_reduce(x_mont)
    }
    
    /// Montgomery multiplication: REDC(x_mont * y_mont)
    pub fn montgomery_mul(&self, x_mont: &BigUint, y_mont: &BigUint) -> BigUint {
        let t = x_mont * y_mont;
        self.montgomery_reduce(&t)
    }
    
    /// Montgomery squaring: REDC(x_mont^2)
    pub fn montgomery_square(&self, x_mont: &BigUint) -> BigUint {
        let t = x_mont * x_mont;
        self.montgomery_reduce(&t)
    }
    
    /// Montgomery reduction (REDC algorithm)
    fn montgomery_reduce(&self, t: &BigUint) -> BigUint {
        // m = (t mod R) * N' mod R
        let t_mod_r = t & ((BigUint::one() << self.k) - BigUint::one());
        let m = (&t_mod_r * &self.n_prime) & ((BigUint::one() << self.k) - BigUint::one());
        
        // result = (t + m * N) / R
        let numerator = t + &m * &self.modulus;
        let result = &numerator >> self.k;
        
        // Conditional subtraction
        if result >= self.modulus {
            result - &self.modulus
        } else {
            result
        }
    }
    
    /// Compute N' such that R * R^(-1) - N * N' = 1 
    /// For VDF, we can use a simpler approach since we know R = 2^k
    fn compute_n_prime(modulus: &BigUint, r: &BigUint) -> BigUint {
        // For Montgomery multiplication, we need N' such that N * N' ≡ -1 (mod R)
        // Since R = 2^k, we can compute this more efficiently
        let mut n_prime = BigUint::one();
        let mut n_mod_r = modulus % r;
        
        // Use binary method to find modular inverse
        for _ in 0..r.bits() {
            if (&n_mod_r * &n_prime) & BigUint::one() == BigUint::one() {
                n_prime = n_prime + r;
            }
            n_prime >>= 1;
        }
        
        // N' = -N^(-1) mod R = R - N^(-1) mod R
        r - (n_prime % r)
    }
    
    /// Montgomery modular exponentiation
    pub fn montgomery_modpow(&self, base: &BigUint, exponent: &BigUint) -> BigUint {
        if exponent == &BigUint::zero() {
            return BigUint::one();
        }
        
        // Convert base to Montgomery form
        let base_mont = self.to_montgomery(base);
        let mut result_mont = self.to_montgomery(&BigUint::one());
        let mut base_power_mont = base_mont;
        let mut exp = exponent.clone();
        
        // Binary exponentiation in Montgomery form
        while exp > BigUint::zero() {
            if &exp & BigUint::one() == BigUint::one() {
                result_mont = self.montgomery_mul(&result_mont, &base_power_mont);
            }
            exp >>= 1;
            if exp > BigUint::zero() {
                base_power_mont = self.montgomery_square(&base_power_mont);
            }
        }
        
        // Convert result back to normal form
        self.from_montgomery(&result_mont)
    }
    
    /// Sequential squaring in Montgomery form (optimized for VDF)
    pub fn montgomery_sequential_squaring(&self, base: &BigUint, iterations: u64) -> BigUint {
        if iterations == 0 {
            return base.clone();
        }
        
        // Convert to Montgomery form once
        let mut result_mont = self.to_montgomery(base);
        
        // Perform iterations in Montgomery form (much faster)
        for _ in 0..iterations {
            result_mont = self.montgomery_square(&result_mont);
        }
        
        // Convert back to normal form
        self.from_montgomery(&result_mont)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_montgomery_basic() {
        let modulus = BigUint::from(97u32); // Small prime for testing
        let mont_ctx = MontgomeryContext::new(modulus.clone());
        
        let a = BigUint::from(15u32);
        let b = BigUint::from(23u32);
        
        // Test Montgomery multiplication
        let a_mont = mont_ctx.to_montgomery(&a);
        let b_mont = mont_ctx.to_montgomery(&b);
        let result_mont = mont_ctx.montgomery_mul(&a_mont, &b_mont);
        let result = mont_ctx.from_montgomery(&result_mont);
        
        let expected = (&a * &b) % &modulus;
        assert_eq!(result, expected);
    }
    
    #[test]
    fn test_montgomery_exponentiation() {
        let modulus = BigUint::from(97u32);
        let mont_ctx = MontgomeryContext::new(modulus.clone());
        
        let base = BigUint::from(5u32);
        let exponent = BigUint::from(10u32);
        
        let result = mont_ctx.montgomery_modpow(&base, &exponent);
        let expected = base.modpow(&exponent, &modulus);
        
        assert_eq!(result, expected);
    }
}
