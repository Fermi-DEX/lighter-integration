use num_bigint::BigUint;
use num_traits::{Zero, One};

/// Simplified Montgomery-inspired optimizations for VDF sequential squaring
#[derive(Debug, Clone)]
pub struct OptimizedArithmetic {
    modulus: BigUint,
    modulus_bits: usize,
}

impl OptimizedArithmetic {
    pub fn new(modulus: BigUint) -> Self {
        let modulus_bits = modulus.bits() as usize;
        Self { modulus, modulus_bits }
    }
    
    /// Optimized modular squaring with reduced allocations
    #[inline(always)]
    pub fn mod_square(&self, x: &BigUint) -> BigUint {
        // Use built-in optimization for squaring
        let squared = x * x;
        squared % &self.modulus
    }
    
    /// Highly optimized sequential squaring for VDF
    pub fn sequential_squaring(&self, base: &BigUint, iterations: u64) -> BigUint {
        if iterations == 0 {
            return base.clone();
        }
        
        let mut result = base % &self.modulus;
        
        // Use different strategies based on iteration count
        if iterations <= 16 {
            // For small iterations, unroll completely
            for _ in 0..iterations {
                result = self.mod_square(&result);
            }
        } else {
            // For larger iterations, use chunked processing with unrolling
            let chunk_size = 8;
            let full_chunks = iterations / chunk_size;
            let remainder = iterations % chunk_size;
            
            // Process full chunks with unrolling
            for _ in 0..full_chunks {
                result = self.mod_square(&result);
                result = self.mod_square(&result);
                result = self.mod_square(&result);
                result = self.mod_square(&result);
                result = self.mod_square(&result);
                result = self.mod_square(&result);
                result = self.mod_square(&result);
                result = self.mod_square(&result);
            }
            
            // Handle remaining iterations
            for _ in 0..remainder {
                result = self.mod_square(&result);
            }
        }
        
        result
    }
    
    /// Optimized modular exponentiation using windowing
    pub fn mod_pow(&self, base: &BigUint, exponent: &BigUint) -> BigUint {
        if exponent == &BigUint::zero() {
            return BigUint::one();
        }
        
        if exponent == &BigUint::one() {
            return base % &self.modulus;
        }
        
        // Use precomputed table for small exponents
        if exponent.bits() <= 16 {
            return self.mod_pow_small(base, exponent);
        }
        
        // For larger exponents, use sliding window
        self.mod_pow_sliding_window(base, exponent)
    }
    
    /// Optimized for small exponents
    fn mod_pow_small(&self, base: &BigUint, exponent: &BigUint) -> BigUint {
        let mut result = BigUint::one();
        let mut base_power = base % &self.modulus;
        let mut exp = exponent.clone();
        
        while exp > BigUint::zero() {
            if &exp & BigUint::one() == BigUint::one() {
                result = (&result * &base_power) % &self.modulus;
            }
            exp >>= 1;
            if exp > BigUint::zero() {
                base_power = self.mod_square(&base_power);
            }
        }
        
        result
    }
    
    /// Sliding window method for larger exponents
    fn mod_pow_sliding_window(&self, base: &BigUint, exponent: &BigUint) -> BigUint {
        let window_size = 4;
        let table_size = 1 << (window_size - 1);
        
        // Precompute odd powers: base^1, base^3, base^5, ..., base^(2*table_size-1)
        let mut table = vec![BigUint::one(); table_size];
        let base_mod = base % &self.modulus;
        table[0] = base_mod.clone();
        
        if table_size > 1 {
            let base_squared = self.mod_square(&base_mod);
            for i in 1..table_size {
                table[i] = (&table[i - 1] * &base_squared) % &self.modulus;
            }
        }
        
        let mut result = BigUint::one();
        let exp_bits = exponent.bits() as usize;
        let mut i = exp_bits;
        
        while i > 0 {
            // Find the next window
            if exponent.bit((i - 1) as u64) {
                let mut window = 1u32;
                let mut window_size_actual = 1;
                
                // Extend window as much as possible
                while window_size_actual < window_size && i > window_size_actual {
                    window <<= 1;
                    if exponent.bit((i - window_size_actual - 1) as u64) {
                        window |= 1;
                    }
                    window_size_actual += 1;
                }
                
                // If window is even, make it odd
                while window % 2 == 0 {
                    window >>= 1;
                    window_size_actual -= 1;
                }
                
                // Square result for window_size_actual bits
                for _ in 0..window_size_actual {
                    result = self.mod_square(&result);
                }
                
                // Multiply by precomputed power
                let table_index = (window / 2) as usize;
                if table_index < table.len() {
                    result = (&result * &table[table_index]) % &self.modulus;
                }
                
                i -= window_size_actual;
            } else {
                result = self.mod_square(&result);
                i -= 1;
            }
        }
        
        result
    }
}

/// Extension trait for BigUint to access individual bits
trait BigUintBitAccess {
    fn bit(&self, index: usize) -> bool;
}

impl BigUintBitAccess for BigUint {
    fn bit(&self, index: usize) -> bool {
        let word_index = index / 32;
        let bit_index = index % 32;
        
        let digits = self.to_u32_digits();
        if word_index >= digits.len() {
            false
        } else {
            (digits[word_index] >> bit_index) & 1 == 1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_sequential_squaring() {
        let modulus = BigUint::from(97u32);
        let arith = OptimizedArithmetic::new(modulus.clone());
        
        let base = BigUint::from(5u32);
        let iterations = 3;
        
        let result = arith.sequential_squaring(&base, iterations);
        
        // Manually compute expected result
        let mut expected = base.clone();
        for _ in 0..iterations {
            expected = (&expected * &expected) % &modulus;
        }
        
        assert_eq!(result, expected);
    }
    
    #[test]
    fn test_mod_pow() {
        let modulus = BigUint::from(97u32);
        let arith = OptimizedArithmetic::new(modulus.clone());
        
        let base = BigUint::from(5u32);
        let exponent = BigUint::from(10u32);
        
        let result = arith.mod_pow(&base, &exponent);
        let expected = base.modpow(&exponent, &modulus);
        
        assert_eq!(result, expected);
    }
}
